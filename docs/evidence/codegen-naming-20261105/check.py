#!/usr/bin/env python3
"""Check archived observations and reconstruct selected sources from Git."""
import hashlib
import io
import json
from pathlib import Path
import statistics
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sha = lambda data: hashlib.sha256(data).hexdigest()
manifest = json.loads((HERE / 'manifest.json').read_text())
assert manifest['schema'] == 'pbrs.codegen-naming-diagnostic.v1'
assert manifest['qualified'] is False
raw = (HERE / 'raw.tar.gz').read_bytes()
assert sha(raw) == manifest['raw_sha256']
with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as archive:
    members = archive.getmembers()
    assert all(member.isfile() and not Path(member.name).is_absolute()
               and '..' not in Path(member.name).parts for member in members)
    assert len({member.name for member in members}) == len(members)
    data = {member.name: archive.extractfile(member).read() for member in members}
assert set(data) == set(manifest['files'])
for name, content in data.items():
    assert {'sha256': sha(content), 'bytes': len(content)} == manifest['files'][name], name
j = lambda name: json.loads(data[name])
plan = j('source-records/source-plan.json')
assert plan['baseline_commit'] == manifest['baseline_commit']
baseline = {name: subprocess.check_output(['git', 'show', f"{manifest['baseline_commit']}:{name}"], cwd=ROOT)
            for name in plan['original_sha256']['baseline']}
assert {name: sha(content) for name, content in baseline.items()} == plan['original_sha256']['baseline']
candidate = dict(baseline)
for name in plan['changed_original_files']:
    candidate[name] = data['source-overlays/' + name]
assert {name: sha(content) for name, content in candidate.items()} == plan['original_sha256']['candidate']
assert {name for name in candidate if candidate[name] != baseline[name]} == set(plan['changed_original_files'])
builds = {}
for variant, original in [('baseline', baseline), ('candidate', candidate)]:
    adapted = dict(original)
    original_manifest = adapted['Cargo.toml'].decode()
    adapted['Cargo.toml'] = ('[workspace]\nresolver = "2"\n\n' + original_manifest[original_manifest.index('[package]'):]).encode()
    hashes = {name: sha(content) for name, content in adapted.items()}
    build = j(f'source-records/{variant}-build.json')
    assert hashes == plan['adapted_sha256'][variant] == build['source_sha256']
    assert build['status'] == 0 and build['guard'] is None
    assert sha(data[f'source-records/{variant}-build.jsonl']) == build['log_sha256']
    for name, field in [('Cargo.toml', 'manifest_sha256'), ('Cargo.lock', 'lock_sha256'), ('src/main.rs', 'harness_sha256')]:
        assert sha(data[f'source-records/{variant}-harness/{name}']) == build[field]
    assert build['harness_sha256'] == plan['harness_sha256']
    assert build['rustc']
    builds[variant] = build
assert builds['baseline']['rustc'] == builds['candidate']['rustc']
assert data['source-records/baseline-harness/Cargo.lock'] == data['source-records/candidate-harness/Cargo.lock']
initial = j('initial/manifest.json')
focused = j('focused/manifest.json')
assert initial['state'] == focused['state'] == 'complete'
assert initial['qualified'] is focused['qualified'] is False
assert initial['successful_processes'] == 120 and focused['successful_processes'] == 36
assert manifest['successful_processes'] == 156
assert len(initial['captures']) == 60 and len(focused['captures']) == 18
assert sha(data['controllers/canonical_codegen_blocking.py']) == initial['helper_sha256'] == focused['helper_sha256']
assert sha(data['controllers/codegen_compare_blocking.py']) == initial['controller_sha256']
assert sha(data['source-records/focused.py']) == focused['controller_sha256']
assert initial['protoc_sha256'] == focused['protoc_sha256']
assert initial['time_executable_sha256'] == focused['time_executable_sha256']
for variant in ('baseline', 'candidate'):
    assert initial['binary_sha256'][variant] == focused['binary_sha256'][variant] == builds[variant]['binary_sha256']
expected_initial = []
for row in j('initial/summary.json'):
    matching = [capture for capture in initial['captures'] if capture['case'] == row['case']]
    values = {(capture['repeat'], capture['source'], capture['backend']): capture['phases'][row['operation']]['elapsed_ns'] for capture in matching}
    count = 5
    expected = {
        'after_over_before_pbrs': [values[i, 'after', 'pbrs'] / values[i, 'before', 'pbrs'] for i in range(count)],
        'after_pbrs_over_prost': [values[i, 'after', 'pbrs'] / values[i, 'after', 'prost'] for i in range(count)],
        'control_after_over_before': [values[i, 'after', 'prost'] / values[i, 'before', 'prost'] for i in range(count)],
    }
    for name, ratios in expected.items():
        assert row['ratios'][name] == {'median': statistics.median(ratios), 'range': [min(ratios), max(ratios)], 'pairs': ratios}
    medians = {source: {backend: statistics.median(values[i, source, backend] for i in range(count)) for backend in ('pbrs', 'prost')} for source in ('before', 'after')}
    assert medians == row['elapsed_ns_medians']
    assert row['baseline_candidate_output_identical']
    pbrs_outputs = {capture['output_tree_sha256'] for capture in matching if capture['backend'] == 'pbrs'}
    assert len(pbrs_outputs) == 1
    assert row['generated_bytes'] == {binary: next(c['rust_bytes'] for c in matching if c['binary'] == binary) for binary in ('baseline', 'candidate', 'prost')}
    expected_initial.append(row)
assert expected_initial == manifest['initial_rows']
assert len(expected_initial) == 6
for capture in initial['captures']:
    assert j(f"initial/logs/{capture['index']:03d}/capture.json") == capture
    observed_tree = {name: (entry['sha256'], entry['bytes']) for name, entry in capture['generated'].items()}
    assert sha(json.dumps(observed_tree, sort_keys=True).encode()) == capture['output_tree_sha256']
    assert sum(entry['bytes'] for entry in capture['generated'].values()) == capture['rust_bytes']
    assert len(capture['generated']) == capture['rust_files']
    if capture['backend'] == 'pbrs':
        assert capture['unchanged_mtimes_preserved'] == capture['rust_files']
    for phase in capture['phases'].values():
        assert phase['exit_code'] == 0 and phase['elapsed_ns'] > 0
        assert phase['completion_wait'] == 'blocking with independent deadline watchdog'
        assert phase['elapsed_scope'] == 'command completion before RSS sampler teardown'
        assert 'initial/' + phase['stdout_log'] in data
        assert 'initial/' + phase['stderr_log'] in data
expected_focused = []
for row in j('focused/summary.json'):
    values = {(capture['repeat'], capture['binary']): capture['phases'][row['operation']]['elapsed_ns'] for capture in focused['captures']}
    ratios = [values[i, 'candidate'] / values[i, 'baseline'] for i in range(9)]
    assert row['candidate_over_baseline'] == {'median': statistics.median(ratios), 'range': [min(ratios), max(ratios)], 'pairs': ratios}
    assert row['pairs_above_one'] == sum(value > 1 for value in ratios)
    assert row['elapsed_ns_medians'] == {label: statistics.median(values[i, label] for i in range(9)) for label in ('baseline', 'candidate')}
    expected_focused.append(row)
assert len(expected_focused) == 2
assert expected_focused == manifest['focused_rows']
assert focused['all_outputs_identical'] is True
assert len({capture['output_tree_sha256'] for capture in focused['captures']}) == 1
for capture in focused['captures']:
    observed_tree = {name: (entry['sha256'], entry['bytes']) for name, entry in capture['generated'].items()}
    assert sha(json.dumps(observed_tree, sort_keys=True).encode()) == capture['output_tree_sha256']
    assert capture['output_tree_sha256'] == next(c['output_tree_sha256'] for c in initial['captures'] if c['case'] == '100' and c['binary'] == capture['binary'])
    for phase in capture['phases'].values():
        assert phase['exit_code'] == 0 and phase['elapsed_ns'] > 0
        assert phase['completion_wait'] == 'blocking with independent deadline watchdog'
        assert phase['elapsed_scope'] == 'command completion before RSS sampler teardown'
        assert 'focused/' + phase['stdout_log'] in data
        assert 'focused/' + phase['stderr_log'] in data
print(json.dumps({'passed': True, 'archive_files': len(data), 'source_inputs_per_build': len(baseline), 'successful_processes': 156,
                  'scope': 'Archive consistency, reconstructed selected sources and recomputed observations; no executable/output reproduction or host qualification.'}))
