"""Wait for regression checks, build once, then run the unchanged resource campaign."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path('/workspace/pure-protobuf')
SOURCE = ROOT / 'work/gates-candidate-05f5'
PIN = '07e409aed328c9716718eb70ff05c15848fbb664'
OUT = ROOT / 'work/resource-008-prerequisites'
OUT.mkdir(exist_ok=False)
CONTROLLER = ROOT / 'work/campaign-v5-007/controller.py'
CONTROLLER_SHA = 'f3f110905fb62642cf5948869708aa1bf3cca2bc58d394b34c7b611ea7c52cac'
REPORTS = ['native-sequential-gates-2c66', 'native-sequential-gates-2c66-continuation',
           'native-sequential-gates-05f5', 'native-sequential-gates-07e4']
state = dict(source=PIN, state='waiting_for_regressions', qualified=False, passed=False,
             actual_24h_started=False, actual_24h_completed=False, steps=[])
(OUT / 'controller.py').write_bytes(Path(__file__).read_bytes())


def save():
    temporary = OUT / 'status.json.tmp'
    temporary.write_text(json.dumps(state, indent=2) + '\n')
    temporary.replace(OUT / 'status.json')


def wait_report(directory):
    while True:
        try:
            record = json.loads((ROOT / 'work' / directory / 'results.json').read_text())
        except (OSError, json.JSONDecodeError):
            record = {}
        if record.get('state') == 'finished':
            return record
        time.sleep(3)


def run(name, args):
    state['state'] = name
    save()
    directory = OUT / name
    directory.mkdir()
    started = time.monotonic()
    with (directory / 'stdout').open('w') as stdout, (directory / 'stderr').open('w') as stderr:
        result = subprocess.run(args, cwd=SOURCE, stdout=stdout, stderr=stderr)
    state['steps'].append(dict(name=name, argv=args, exit_code=result.returncode,
                               elapsed_seconds=time.monotonic() - started))
    save()
    if result.returncode:
        raise RuntimeError(f'{name} failed: exit {result.returncode}')


save()
try:
    latest = wait_report(REPORTS[-1])
    docs = wait_report('final-doc-gates-07e4')
    if not latest.get('passed') or not docs.get('passed'):
        raise RuntimeError('regression or strict documentation prerequisite failed')
    full = json.loads((ROOT / 'work' / REPORTS[0] / 'results.json').read_text())
    successful = {}
    for directory in REPORTS:
        record = json.loads((ROOT / 'work' / directory / 'results.json').read_text())
        for row in record['results']:
            if row['exit_code'] == 0 and 'artifact_error' not in row:
                name = 'lib' if '--lib' in row['command'] else row['command'][-2]
                successful[name] = dict(source=record['source'], report=directory)
    missing = sorted(set(full['targets']) - successful.keys())
    if missing:
        raise RuntimeError(f'unexecuted native targets: {missing}')
    state['native_target_sources'] = successful
    save()
    assert hashlib.sha256(CONTROLLER.read_bytes()).hexdigest() == CONTROLLER_SHA
    assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=SOURCE,
                                   text=True).strip() == PIN
    assert not subprocess.check_output(['git', 'status', '--porcelain',
                                       '--untracked-files=all'], cwd=SOURCE, text=True)
    os.environ['PROTOC'] = str(ROOT / 'work/toolchain/protoc/bin/protoc')
    os.environ['PATH'] = str(ROOT / 'work/toolchain/protoc/bin') + os.pathsep + os.environ['PATH']
    run('caller_rustls_raw', [str(ROOT / 'work/run-rust'), 'cargo', 'test', '--locked',
                            '--manifest-path', 'pbrs-grpc/tests/caller-rustls/Cargo.toml',
                            '--target-dir', str(ROOT / 'work/target/caller-rustls'),
                            '--message-format=json'])
    # Freeze the benchmark before the resource run so a later matrix needs no
    # compilation while the actual day is running.
    run('benchmark_build', [str(ROOT / 'work/run-rust'), 'python3',
                            str(SOURCE / 'scripts/build-rpc-bench.py'), '--source', PIN,
                            '--output', str(ROOT / 'work/benchmark-07e4')])
    state['state'] = 'resource_controller'
    save()
    state['actual_24h_started'] = None
    state['actual_24h_completed'] = None
    state['live_resource_status'] = 'work/campaign-v5-008/controller-status.json'
    save()
    run('preview_and_day', [str(ROOT / 'work/run-rust'), 'python3', str(CONTROLLER),
                            '--source', PIN, '--source-dir', str(SOURCE), '--root', str(ROOT),
                            '--output', str(ROOT / 'work/campaign-v5-008'),
                            '--build-profile', 'release', '--publish-main',
                            '--evidence-path', 'docs/evidence/grpc-resource-soak-20261009-attempt-008'])
    outcome = json.loads((ROOT / 'work/campaign-v5-008/controller-status.json').read_text())
    days = [row for row in outcome['outcomes'] if row['duration_requested_seconds'] == 86400]
    state['actual_24h_started'] = bool(days)
    state['actual_24h_completed'] = bool(days and (days[0].get('soak_24h') or {}).get('status') == 'completed')
    state['passed'] = state['actual_24h_completed'] and all(row['runner_exit_code'] == 0 and row['validator_exit_code'] == 0 for row in outcome['outcomes'])
    state['resource_outcome'] = outcome
    state['state'] = 'finished'
    # Even a completed day does not qualify features, performance or the full
    # release: the original resource validator records those remaining gates.
    save()
except BaseException as error:
    state.update(state='failed', error=dict(type=type(error).__name__, message=str(error)))
    save()
    raise
