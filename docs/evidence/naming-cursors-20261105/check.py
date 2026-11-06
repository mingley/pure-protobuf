#!/usr/bin/env python3
"""Recompute naming-allocator observations and reconstruct selected Git inputs."""
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
assert manifest['schema'] == 'pbrs.naming-cursors-evidence.v1'
assert manifest['baseline_commit'] == '0ae8a43d3d08617c249c84a3f0d30f614494d70e'
assert manifest['qualified'] is False
raw = (HERE / 'raw.tar.gz').read_bytes()
assert sha(raw) == manifest['raw_sha256']
with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as archive:
    members = archive.getmembers()
    assert all(m.isfile() and not Path(m.name).is_absolute() and '..' not in Path(m.name).parts for m in members)
    assert len({m.name for m in members}) == len(members)
    data = {m.name: archive.extractfile(m).read() for m in members}
assert set(data) == set(manifest['files'])
for name, content in data.items():
    assert {'sha256': sha(content), 'bytes': len(content)} == manifest['files'][name], name
j = lambda name: json.loads(data[name])
source = j('source/source-plan-before-patch.json')
plan = j('source/allocator-source-plan.json')
assert source['baseline_commit'] == plan['baseline_commit'] == manifest['baseline_commit']
baseline = {name: subprocess.check_output(['git', 'show', f"{manifest['baseline_commit']}:{name}"], cwd=ROOT)
            for name in source['selected_source_sha256']}
assert {name: sha(content) for name, content in baseline.items()} == source['selected_source_sha256']
text = baseline['Cargo.toml'].decode()
baseline['Cargo.toml'] = ('[workspace]\nresolver = "2"\n\n' + text[text.index('[package]'):]).encode()
assert {name: sha(content) for name, content in baseline.items()} == plan['before']
assert plan['changed'] == ['src/codegen/naming.rs']
candidate = dict(baseline)
candidate['src/codegen/naming.rs'] = data['source/overlay/src/codegen/naming.rs']
assert {name: sha(content) for name, content in candidate.items()} == plan['after']
builds = {}
for variant, selected in [('baseline', baseline), ('candidate', candidate)]:
    build = j(f'source/{variant}-build.json')
    assert build['status'] == 0 and build['guard'] is None
    assert build['source_sha256'] == {name: sha(content) for name, content in selected.items()}
    assert build['log_sha256'] == sha(data[f'source/{variant}-build.jsonl'])
    for path, field in [('Cargo.toml', 'manifest_sha256'), ('Cargo.lock', 'lock_sha256'), ('src/main.rs', 'harness_sha256')]:
        assert sha(data[f'source/{variant}-harness/{path}']) == build[field]
    assert build['harness_sha256'] == source['harness_sha256']
    assert build['command'][:5] == ['cargo', 'build', '--release', '--locked', '--offline']
    builds[variant] = build
assert builds['baseline']['rustc'] == builds['candidate']['rustc']
assert builds['baseline']['binary_sha256'] == source['baseline_artifact_sha256']
assert data['source/baseline-harness/Cargo.lock'] == data['source/candidate-harness/Cargo.lock']
unit = j('source/candidate-unit.json')
assert unit['status'] == 0 and unit['guard'] is None and unit['source_inputs_unchanged'] is True
assert unit['source_sha256'] == plan['after']
assert unit['log_sha256'] == sha(data['source/candidate-unit.log'])
assert b'1 passed; 0 failed' in data['source/candidate-unit.log']

combined_plan = j('source/combined-source-plan.json')
combined_source = dict(baseline)
for name in manifest['combined_overlays']:
    combined_source[name] = data['source/combined-overlay/' + name]
assert {name: sha(content) for name, content in combined_source.items()} == combined_plan['adapted_sha256']
assert combined_plan['original_sha256']['Cargo.toml'] == source['selected_source_sha256']['Cargo.toml']
assert {name: digest for name, digest in combined_plan['original_sha256'].items() if name != 'Cargo.toml'} == {
    name: sha(content) for name, content in combined_source.items() if name != 'Cargo.toml'}
assert set(manifest['combined_overlays']) == {name for name in combined_source if baseline.get(name) != combined_source[name]}
combined_build = j('source/combined-build.json')
assert combined_build['status'] == 0 and combined_build['guard'] is None
assert combined_build['source_sha256'] == combined_plan['adapted_sha256']
assert combined_build['log_sha256'] == sha(data['source/combined-build.jsonl'])
assert combined_build['rustc'] == builds['baseline']['rustc']
for path, field in [('Cargo.toml', 'manifest_sha256'), ('Cargo.lock', 'lock_sha256'), ('src/main.rs', 'harness_sha256')]:
    assert sha(data[f'source/combined-harness/{path}']) == combined_build[field]
assert data['source/combined-harness/Cargo.lock'] == data['source/baseline-harness/Cargo.lock']

successful_processes = 0
all_rows = {}
observed_trees = {}
for dataset, pairs, captures_expected in [('initial', 5, 70), ('focused', 9, 36), ('combined', 5, 30)]:
    run = j(f'{dataset}/manifest.json')
    is_combined = dataset == 'combined'
    assert run['schema'] == ('pbrs.combined-codegen-diagnostic.v1' if is_combined else 'pbrs.naming-cursor-diagnostic.v1')
    assert run['qualified'] is False and run['state'] == 'complete'
    assert run['pairs'] == pairs and len(run['captures']) == len(run['plan']) == captures_expected
    assert run['successful_processes'] == captures_expected * 2
    assert run['controller_sha256'] == sha(data['controllers/measure_combined.py' if is_combined else 'controllers/measure.py'])
    assert run['helper_sha256'] == sha(data['controllers/canonical_codegen_blocking.py'])
    expected_builds = {'baseline': builds['baseline'], 'candidate': combined_build} if is_combined else builds
    assert run['binary_sha256'] == {v: b['binary_sha256'] for v, b in expected_builds.items()}
    for case, files in run['corpus_sha256'].items():
        assert files == {name.removeprefix(f'corpora/{case}/'): sha(content)
                         for name, content in data.items() if name.startswith(f'corpora/{case}/')}
    seen = set()
    for index, capture in enumerate(run['captures']):
        assert capture['index'] == index
        assert all(capture.get(k) == v for k, v in run['plan'][index].items())
        assert j(f'{dataset}/logs/{index:03d}/capture.json') == capture
        identity = (capture['case'], capture['repeat'], capture['variant'])
        assert identity not in seen
        seen.add(identity)
        generated = capture['generated']
        observed = {name: (entry['sha256'], entry['bytes']) for name, entry in generated.items()}
        digest = sha(json.dumps(observed, sort_keys=True).encode())
        assert capture['output_tree_sha256'] == digest
        assert capture['rust_files'] == len(generated) and generated
        assert capture['rust_bytes'] == sum(entry['bytes'] for entry in generated.values())
        tree_key = ('combined', capture['case'], capture['variant']) if is_combined else ('allocator', capture['case'])
        observed_trees.setdefault(tree_key, set()).add(digest)
        assert set(capture['phases']) == {'generation', 'generation_unchanged'}
        for phase in capture['phases'].values():
            assert phase['exit_code'] == 0 and isinstance(phase['elapsed_ns'], int) and phase['elapsed_ns'] > 0
            assert phase['completion_wait'] == 'blocking with independent deadline watchdog'
            assert phase['elapsed_scope'] == 'command completion before RSS sampler teardown'
            assert dataset + '/' + phase['stdout_log'] in data
            stderr = data[dataset + '/' + phase['stderr_log']]
            assert b'Exit status: 0' in stderr
        if is_combined:
            after = capture['unchanged_generated']
            assert capture['unchanged_mtimes_preserved'] == len(generated)
            assert after == generated
        successful_processes += 2
    assert seen == {(case, repeat, variant) for case in run['cases'] for repeat in range(pairs)
                    for variant in ['baseline', 'candidate']}
    rows = j(f'{dataset}/summary.json')
    assert len(rows) == len(run['cases']) * 2
    assert {(row['case'], row['operation']) for row in rows} == {
        (case, operation) for case in run['cases'] for operation in ['generation', 'generation_unchanged']}
    for row in rows:
        matching = [c for c in run['captures'] if c['case'] == row['case']]
        values = {(c['repeat'], c['variant']): c['phases'][row['operation']]['elapsed_ns'] for c in matching}
        ratios = [values[i, 'candidate'] / values[i, 'baseline'] for i in range(pairs)]
        assert row['new_over_old'] == {'median': statistics.median(ratios),
            'range': [min(ratios), max(ratios)], 'pairs': ratios}
        assert row['median_ns'] == {v: statistics.median(values[i, v] for i in range(pairs))
                                    for v in ['baseline', 'candidate']}
        identical = len({c['output_tree_sha256'] for c in matching}) == 1
        assert row['output_identical'] == identical
        if is_combined:
            assert row['rust_bytes'] == {v: next(c['rust_bytes'] for c in matching if c['variant'] == v)
                                         for v in ['baseline', 'candidate']}
        else:
            assert identical
            assert row['rust_bytes'] == matching[0]['rust_bytes']
    all_rows[dataset] = rows
assert successful_processes == manifest['successful_processes'] == 272
assert all(len(trees) == 1 for trees in observed_trees.values())
assert all_rows == manifest['rows']
print(json.dumps({'passed': True, 'archive_files': len(data), 'selected_inputs_per_build': len(baseline),
                  'successful_processes': successful_processes,
                  'scope': 'Source reconstruction and observation arithmetic; no executable/output regeneration or host qualification.'}))
