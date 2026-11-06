#!/usr/bin/env python3
"""Verify retained source/receipt arithmetic; do not execute archived code."""
from pathlib import Path
import hashlib
import io
import json
import random
import statistics
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent
def sha(value): return hashlib.sha256(value).hexdigest()
manifest = json.loads((HERE / 'manifest.json').read_text())
raw = (HERE / 'raw.tar.gz').read_bytes()
assert sha(raw) == manifest['archive_sha256']
with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as archive:
    members = archive.getmembers()
    assert len(members) == len({m.name for m in members})
    assert all(m.isfile() for m in members)
    assert {m.name for m in members} == manifest['files'].keys()
    files = {m.name: archive.extractfile(m).read() for m in members}
for name, item in manifest['files'].items():
    assert len(files[name]) == item['bytes'] and sha(files[name]) == item['sha256'], name
def data(name): return json.loads(files[name])
def lines(name): return [json.loads(line) for line in files[name].splitlines()]

git_root = None
for path in [HERE, Path.cwd()]:
    result = subprocess.run(['git', 'rev-parse', '--show-toplevel'], cwd=path, capture_output=True, text=True)
    if result.returncode == 0:
        git_root = result.stdout.strip()
        break
assert git_root, 'Run in a checkout containing the named Git base'
base = manifest['base_commit']
selected = sorted(data('canonical-baseline-cpu.json')['source_before'])
request = ''.join(f'{base}:{name}\n' for name in selected).encode()
packed = subprocess.check_output(['git', 'cat-file', '--batch'], input=request, cwd=git_root)
committed = {}; offset = 0
for name in selected:
    end = packed.index(b'\n', offset); header = packed[offset:end].split()
    assert len(header) == 3 and header[1] == b'blob', name
    length = int(header[2]); offset = end + 1
    committed[name] = packed[offset:offset + length]; offset += length + 1
assert offset == len(packed)
assert committed['src/map.rs'] == files['baseline-map.rs']

def source(overlay):
    values = dict(committed)
    for name in ['Cargo.toml', 'Cargo.lock']: values[name] = files['source/' + name]
    values['src/map.rs'] = files[overlay]
    return {name: sha(value) for name, value in values.items()}
for name, overlay, expected in [
    ('unrelated-red', 'repro-map.rs', 101), ('target-pop-control', 'repro-map.rs', 0),
    ('candidate-unit', 'candidate-map.rs', 0), ('candidate-clippy', 'candidate-map.rs', 0),
    *[(f'candidate-miri-{i}', 'candidate-map.rs', 0) for i in range(3)],
    ('failed-bytes-view-test/candidate-unit', 'failed-bytes-view-test/map.rs', 101),
    ('failed-key-drop-return-value/candidate-unit', 'failed-key-drop-return-value/map.rs', 101),
    ('before-test-lint-fix/candidate-unit', 'before-test-lint-fix/map.rs', 0),
    ('before-test-lint-fix/candidate-clippy', 'before-test-lint-fix/map.rs', 101),
]:
    receipt = data(name + '.json')
    assert receipt['exit_code'] == expected and receipt['guard'] is None
    assert receipt['source_sha256'] == source(overlay), name
assert b'0 passed; 3 failed' in files['unrelated-red.log']
assert b'100 passed; 0 failed' in files['candidate-unit.log']
assert b'Finished' in files['candidate-clippy.log'] and b'error:' not in files['candidate-clippy.log']
for i in range(3):
    assert b'1 passed; 0 failed' in files[f'candidate-miri-{i}.log']
    assert '-Zmiri-strict-provenance' in data(f'candidate-miri-{i}.json')['miri_flags']
# The measured candidate includes the subsequent eight method-documentation lines.
doc_a = b'    /// Remove the last stored entry with this key and return its value.\n    ///\n    /// An earlier parsed duplicate can become visible. Other entries keep\n    /// their relative order. Repeated removals erase every stored version.\n'
doc_b = b'    /// Remove the last stored entry with this key, returning whether it existed.\n    ///\n    /// An earlier parsed duplicate can become visible. Other entries keep\n    /// their relative order. Repeated removals erase every stored version.\n'
assert files['candidate-with-docs-map.rs'].count(doc_a) == files['candidate-with-docs-map.rs'].count(doc_b) == 1
assert files['candidate-with-docs-map.rs'].replace(doc_a, b'').replace(doc_b, b'') == files['candidate-map.rs']

builds = {}
for variant in ['baseline', 'candidate']:
    for mode in ['cpu', 'alloc']:
        stem = f'canonical-{variant}-{mode}'
        build = data(stem + '.json'); builds[variant, mode] = build
        assert build['exit_code'] == 0 and build['guard'] is None
        overlay = 'baseline-map.rs' if variant == 'baseline' else 'candidate-with-docs-map.rs'
        assert build['source_before'] == build['source_after'] == source(overlay)
        assert build['target_bytes'] < 300 * 1024 ** 2 and build['free_bytes'] > 1024 ** 3
        assert build['executable_sha256'] == build['retained_sha256']
        assert sha(files[stem + '.jsonl']) == build['build_log_sha256']
        inputs = build['constant_inputs']
        for name, field in [('src/main.rs', 'harness_source_sha256'), ('Cargo.toml', 'harness_manifest_sha256'), ('Cargo.lock', 'harness_lock_sha256')]:
            assert sha(files['harness/' + name]) == inputs[field]
        assert sha(files['source/Cargo.toml']) == inputs['runtime_manifest_sha256']
        assert sha(files['source/Cargo.lock']) == inputs['runtime_lock_sha256']
        messages = [json.loads(line) for line in files[stem + '.jsonl'].splitlines() if line.startswith(b'{')]
        assert messages[-1] == {'reason': 'build-finished', 'success': True}
        for target, field in [('pbrs', 'library_artifact'), ('pbrs-map-removal-screen', 'binary_artifact')]:
            matches = [m for m in messages if m.get('reason') == 'compiler-artifact' and m['target']['name'] == target]
            assert matches == [build[field]]
for mode in ['cpu', 'alloc']:
    a, b = builds['baseline', mode], builds['candidate', mode]
    assert a['constant_inputs'] == b['constant_inputs']
    assert a['canonical_source_root'] == b['canonical_source_root']
    assert a['canonical_harness_root'] == b['canonical_harness_root']
    assert a['rustc_commands'] == b['rustc_commands']
    assert a['crate_metadata'] == b['crate_metadata']
    for field in ['binary_artifact', 'library_artifact']:
        for key in ['package_id', 'manifest_path', 'profile', 'features', 'filenames']:
            assert a[field][key] == b[field][key]
    assert a['binary_artifact']['profile']['opt_level'] == '3'
    assert a['binary_artifact']['profile']['debuginfo'] == 0
assert b'-C lto=thin -C codegen-units=1' in files['canonical-baseline-cpu.jsonl']

cases = data('canonical-cases.json')['rows']; plan = data('canonical-cpu-plan.json')
assert len(cases) == 7 and plan['cases'] == cases and plan['pairs'] == 9
order = data('canonical-cpu-order.json'); expected = []; rng = random.Random(plan['seed'])
for pair in range(9):
    indices = list(range(7)); rng.shuffle(indices)
    for index in indices:
        variants = ['baseline', 'candidate']; rng.shuffle(variants)
        expected.append({'case_no': index, 'pair': pair, 'variants': variants})
assert order == expected
cpu = lines('canonical-cpu-raw.jsonl'); summary = data('canonical-cpu-summary.json')
assert len(cpu) == summary['successful_processes'] == 126 and len(summary['rows']) == 7
observed = {i: [] for i in range(7)}; statuses = []
def result(record, case, variant, mode):
    assert record['variant'] == variant and record['exit_code'] == 0 and not record['stderr']
    assert Path(record['command'][0]).name == f'canonical-{variant}-{mode}-bin'
    repetitions = case['cpu_repetitions' if mode == 'cpu' else 'allocation_repetitions']
    assert record['command'][1:] == [case['shape'], case['kind'], str(case['entries']), str(case['key_length']), str(repetitions)]
    value = json.loads(record['stdout'])
    assert value['schema'] == 'pbrs.map-remove-screen.v1' and value['semantic_checks']
    for key in ['shape', 'kind', 'entries', 'key_length']: assert value[key] == case[key]
    assert value['repetitions'] == repetitions
    assert value['operations'] == repetitions * (1024 if case['shape'] == 'single' else 1)
    assert value['allocation_instrumentation'] == (mode == 'alloc')
    return value
for index, step in enumerate(order):
    pair = {}; case = cases[step['case_no']]
    for j, variant in enumerate(step['variants']):
        record = cpu[index * 2 + j]
        assert record['case_no'] == step['case_no'] and record['pair'] == step['pair']
        value = result(record, case, variant, 'cpu')
        assert value['cpu_ns'] > 0 and value['wall_ns'] > 0
        assert value['allocation_calls'] is value['requested_bytes'] is None
        assert plan['binary_sha256'][variant] == builds[variant, 'cpu']['retained_sha256']
        pair[variant] = value
        statuses.append({'case_no': step['case_no'], 'pair': step['pair'], 'variant': variant, 'status': 'passed', 'exit_code': 0})
    a, b = pair['baseline'], pair['candidate']
    assert a['checksum'] == b['checksum'] and a['operations'] == b['operations']
    observed[step['case_no']].append({'pair': step['pair'], 'checksum': a['checksum'], 'operations': a['operations'],
        'baseline_cpu_ns': a['cpu_ns'], 'candidate_cpu_ns': b['cpu_ns'], 'ratio': b['cpu_ns'] / a['cpu_ns'],
        'baseline_wall_ns': a['wall_ns'], 'candidate_wall_ns': b['wall_ns']})
for i, row in enumerate(summary['rows']):
    assert row['case_no'] == i and row['case'] == cases[i] and row['pairs'] == observed[i]
    ratios = [pair['ratio'] for pair in observed[i]]
    assert row['median_ratio'] == statistics.median(ratios)
    assert row['min_ratio'] == min(ratios) and row['max_ratio'] == max(ratios)
    assert row['pairs_above_one'] == sum(value > 1 for value in ratios)
terminal = data('canonical-cpu-status.json')
assert terminal['rows'] == statuses and terminal['planned'] == terminal['passed'] == 126
assert terminal['failed'] == terminal['missing'] == 0

allocations = lines('canonical-allocation-raw.jsonl')
rows = data('canonical-allocation-summary.json')['rows']
assert len(allocations) == 14 and len(rows) == 7
found = {}
for record in allocations:
    key = record['case_no'], record['variant']; assert key not in found
    value = result(record, cases[key[0]], key[1], 'alloc')
    assert value['cpu_ns'] is value['wall_ns'] is None
    assert record['binary_sha256'] == builds[key[1], 'alloc']['retained_sha256']
    found[key] = value
for i, row in enumerate(rows):
    assert row['case_no'] == i and row['case'] == cases[i]
    for variant in ['baseline', 'candidate']:
        value = found[i, variant]
        assert row['checksum'] == value['checksum'] and row['operations'] == value['operations']
        assert row[variant + '_allocations'] == value['allocation_calls']
        assert row[variant + '_requested_bytes'] == value['requested_bytes']
print(json.dumps({'passed': True, 'checkout_root': git_root, 'archived_files': len(files),
    'selected_inputs': len(selected), 'baseline_red_tests': 3, 'candidate_minimal_tests': 100,
    'strict_provenance_miri_tests': 3, 'cpu_processes': 126, 'allocation_processes': 14,
    'scope': 'Selected-source/receipt reconstruction and raw arithmetic; no executable replay or host qualification.'}))
