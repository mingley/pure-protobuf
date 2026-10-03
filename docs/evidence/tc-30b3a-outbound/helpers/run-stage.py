"""Run one prepared correctness gate only after the parent grants its lease.

Adapted capture-command.py preserves native exits. This driver classifies only
executed baseline test failures, never Cargo build errors, as semantic reds.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

here = Path(__file__).resolve().parent
plan = json.loads((here / 'plan.json').read_text())
if len(sys.argv) != 2:
    raise SystemExit('usage: run-stage.py STAGE; inspect plan.json without launching')
assert os.environ.get('TC30B_OUTBOUND_ORDINARY_LEASE') == 'granted', 'parent ordinary lease required'
stage = next(item for item in plan['stages'] if item['name'] == sys.argv[1])
record = here / (stage['name'] + '.json')
assert not record.exists(), 'preserve prior attempt: use a new stage/attempt name'
root = Path(stage['cwd'])
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip() == stage['expected_source']
assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=root), 'tracked source must remain clean'
assert os.environ.get('RUSTUP_HOME') == '/workspace/pure-protobuf/work/toolchain/rustup', 'source pinned toolchain env.sh first'
env = os.environ.copy()
env.update(plan['environment'])
env.update(TC30B_OUTBOUND_CWD=str(root), TC30B_OUTBOUND_RECORDS=str(here), TC30B_OUTBOUND_OWNED_TARGET_ROOTS=json.dumps(plan['resources']['owned_target_roots']), RUST_TEST_THREADS='4')
if stage.get('rustdoc'):
    env['RUSTDOCFLAGS'] = '-D warnings'
else:
    env.pop('RUSTDOCFLAGS', None)
protoc = Path(env['PROTOC'])
assert protoc.is_file(), 'genuine pinned protoc missing; preserve this as setup failure'
assert hashlib.sha256(protoc.read_bytes()).hexdigest() == 'ba5165ada96fc34d1295b2056ab8ea99756f7896a0ef0449d07f721595702b28', 'pinned protoc hash changed'
argv = list(stage['argv'])
if argv[0].startswith('@artifact:'):
    _, build, target = argv[0].split(':')
    build_record = json.loads((here / (build + '.json')).read_text())
    assert build_record['exit'] == 0 and build_record['source_before'] == stage['expected_source'], 'build failures are not semantic reds'
    artifacts = []
    for line in (here / (build + '.log')).read_text().splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if value.get('reason') == 'compiler-artifact' and value.get('target', {}).get('name') == target and value.get('executable'):
            artifacts.append(value['executable'])
    assert len(set(artifacts)) == 1, artifacts
    argv[0] = artifacts[0]
if hasattr(os, 'sched_getaffinity'):
    cpus = sorted(os.sched_getaffinity(0))[:plan['resources']['CPU']]
    os.sched_setaffinity(0, cpus)
result = subprocess.run([sys.executable, str(here / 'capture-command.py'), stage['name'], *argv], env=env)
value = json.loads(record.read_text())
log = (here / (stage['name'] + '.log')).read_text()
assert value['source_before'] == value['source_after'] == stage['expected_source']
assert not value['tracked_changes_before'] and not value['tracked_changes_after']
assert value['maximum_owned_cache'] <= plan['resources']['maximum_aggregate_owned_target_bytes']
assert value['minimum_free'] >= plan['resources']['minimum_global_free_bytes']
assert result.returncode == stage['expected_exit'], 'unexpected native exit: original log and record retained'
if stage.get('semantic_red'):
    assert 'running 1 test' in log and 'test result: FAILED. 0 passed; 1 failed;' in log
    assert stage['red_evidence'] in log, 'failure differs from baseline fail-closed oracle'
    disposition = 'executed_baseline_semantic_red'
else:
    disposition = 'passed_correctness_gate'
for package in stage.get('graph_excludes', []):
    assert not re.search(r'\b' + re.escape(package) + r' v', log), package
(here / (stage['name'] + '.disposition.json')).write_text(json.dumps({'stage':stage['name'], 'disposition':disposition, 'native_exit':result.returncode, 'source':stage['expected_source'], 'performance':'not_run'}, indent=2) + '\n')
print(disposition)
