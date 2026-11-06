#!/usr/bin/env python3
"""Validate the committed capture without compiling or rerunning a benchmark."""
import argparse
import hashlib
import io
import json
import pathlib
import statistics
import subprocess
import tarfile

HERE = pathlib.Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--repo', type=pathlib.Path, default=HERE.parents[2])
ROOT = parser.parse_args().repo.resolve()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def unpack(data):
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as packed:
        assert all(member.isfile() for member in packed.getmembers())
        return {member.name: packed.extractfile(member).read() for member in packed.getmembers()}


manifest = json.loads((HERE / 'manifest.json').read_text())
for name, digest in manifest['artifact_sha256'].items():
    assert sha((HERE / name).read_bytes()) == digest, name
assert {path.name for path in HERE.iterdir() if path.is_file()} == {
    'manifest.json', *manifest['artifact_sha256']
}
source = json.loads((HERE / 'source-manifest.json').read_text())
portable = json.loads((HERE / 'portable-baseline.json').read_text())
assert portable['recorded_baseline_commit'] == source['baseline_commit']
assert portable['recorded_source_manifest_sha256'] == sha((HERE / 'source-manifest.json').read_bytes())
assert portable['recorded_raw_archive_sha256'] == sha((HERE / 'raw.tar.gz').read_bytes())
assert set(portable['per_input']) == set(source['baseline_git_inputs'])
public_base = portable['portable_baseline_commit']
assert public_base == '0ae8a43d3d08617c249c84a3f0d30f614494d70e'
assert subprocess.check_output(['git', 'rev-parse', f'{public_base}^{{commit}}'], cwd=ROOT, text=True).strip() == public_base
public_paths = subprocess.check_output(
    ['git', 'ls-tree', '-r', '--name-only', '-z', public_base, '--',
     'Cargo.toml', 'Cargo.lock', 'build.rs', 'clippy.toml', 'rustfmt.toml', 'src',
     'vendor/google/conformance_fds.bin', 'pbrs-grpc'], cwd=ROOT,
).decode().split('\0')
assert sorted(path for path in public_paths if path) == sorted(source['baseline_git_inputs'])
assert len(portable['per_input']) == 407
for name, digest in source['baseline_git_inputs'].items():
    assert portable['per_input'][name] == {'recorded_sha256': digest, 'portable_sha256': digest}
    data = subprocess.check_output(['git', 'show', f'{public_base}:{name}'], cwd=ROOT)
    assert sha(data) == digest, ('public baseline input differs', name)
replay = json.loads((HERE / 'portable-replay-validation.json').read_text())
assert replay['public_commit_present'] == public_base
assert replay['recorded_commit_absent'] == source['baseline_commit']
assert replay['selected_git_inputs'] == 407 and replay['candidate_overlays'] == 10
assert replay['reconstruction']['recorded_baseline_commit'] == source['baseline_commit']
assert replay['reconstruction']['git_baseline_commit'] == public_base
assert replay['reconstruction']['portable_alias_manifest_sha256'] == sha((HERE / 'portable-baseline.json').read_bytes())
assert replay['commands'][-2]['exit_code'] == replay['commands'][-1]['exit_code'] == 0
replayed_inputs = json.loads(replay['commands'][-1]['stdout'])
assert replayed_inputs == {'git_inputs': 407, 'overlays': 10,
                           'recorded_baseline': source['baseline_commit'],
                           'reconstruction_baseline': public_base}
absent = [command for command in replay['commands']
          if command['command'] == ['git', 'cat-file', '-e', f'{source["baseline_commit"]}^{{commit}}']]
assert len(absent) == 1 and absent[0]['exit_code'] != 0
overlays = unpack((HERE / 'source-overlays.tar.gz').read_bytes())
assert set(overlays) == set(source['candidate_overlays'])
assert len(overlays) == 10
for name, data in overlays.items():
    assert sha(data) == source['candidate_overlays'][name], name

raw = unpack((HERE / 'raw.tar.gz').read_bytes())
assert json.loads(raw['source-manifest.json']) == source
capture = json.loads(raw['measurements/manifest.json'])
results = json.loads((HERE / 'results.json').read_text())
assert capture['inputs_before'] == capture['inputs_after']
assert len(capture['runs']) == results['processes'] == 72
assert results['baseline_commit'] == source['baseline_commit']
assert results['scope'] == capture['scope']
for key in ['started_utc', 'finished_utc', 'cpu_affinity']:
    assert results[key] == capture[key]

for variant in ['baseline', 'candidate']:
    for kind in ['cpu', 'allocations']:
        name = f'{variant}-{kind}'
        build = json.loads(raw[f'builds/{name}.json'])
        assert build['exit_code'] == 0 and build['guard'] is None
        assert build['source_inputs_before'] == build['source_inputs_after']
        assert build['driver_inputs_before'] == build['driver_inputs_after']
        assert build['driver_inputs_before']['src/main.rs'] == sha((HERE / 'main.rs').read_bytes())
        assert build['source_inputs_before'] == source['variants'][variant]['source_files']
        assert build['binary_sha256'] == capture['binaries'][f'{name}-bin']
        assert build['binary_bytes'] == results['binary_bytes'][name]
        assert sha(raw[f'builds/{name}.stdout']) == build['stdout_sha256']
        assert sha(raw[f'builds/{name}.stderr']) == build['stderr_sha256']
        for mode in ['stock', 'deadline', 'origin-http']:
            precheck = json.loads(raw[f'builds/{name}-{mode}-precheck.stdout'])
            assert precheck['mode'] == mode
            assert precheck['successful'] == precheck['iterations'] == 8
            assert precheck['channel_local_bytes'] == results['layout_bytes'][variant]['channel_local']
            assert precheck['channel_handle_bytes'] == results['layout_bytes'][variant]['channel']

rows = []
for run in capture['runs']:
    assert run['exit_code'] == 0
    for stream in ['stdout', 'stderr']:
        assert sha(raw[f'measurements/{run[stream]}']) == run[f'{stream}_sha256']
    data = json.loads(raw[f'measurements/{run["stdout"]}'])
    assert data['schema'] == 'pbrs.origin-default-diagnostic.v1'
    assert data['mode'] == run['mode']
    assert data['successful'] == data['iterations']
    assert data['checksum'] == sum(b'payload1') * data['iterations']
    assert data['request_scheme'] == 'http'
    assert data['request_authority'] == ('benchmark.local' if run['mode'] == 'origin-http' else 'benchmark.local:80')
    assert data['request_path'] == '/origin.Bench/Call'
    assert data['wire_payload_bytes'] == 10
    assert data['framed_bytes_per_direction'] == 15
    assert data['process_cpu_ns'] > 0 and data['wall_ns'] > 0
    assert data['allocation_instrumentation'] == (run['kind'] == 'allocations')
    assert data['channel_local_bytes'] == results['layout_bytes'][run['variant']]['channel_local']
    assert data['channel_handle_bytes'] == results['layout_bytes'][run['variant']]['channel']
    if run['kind'] == 'cpu':
        assert data['iterations'] == 20_000 and data['warmup'] == 1_000
        assert data['allocation_calls'] is None and data['requested_bytes'] is None
    else:
        assert run['kind'] == 'allocations'
        assert data['iterations'] == data['warmup'] == 128
    rows.append((run, data))

recomputed = []
for mode in ['stock', 'deadline', 'origin-http']:
    ratios, baseline, candidate = [], [], []
    for pair in range(9):
        selected = [(run, data) for run, data in rows
                    if run['kind'] == 'cpu' and run['mode'] == mode and run['pair'] == pair]
        assert len(selected) == 2 and {run['sequence'] for run, _ in selected} == {0, 1}
        paired = {run['variant']: data for run, data in selected}
        assert set(paired) == {'baseline', 'candidate'}
        old = paired['baseline']['process_cpu_ns'] / 20_000
        new = paired['candidate']['process_cpu_ns'] / 20_000
        ratios.append(new / old)
        baseline.append(old)
        candidate.append(new)
    allocations = {}
    for variant in ['baseline', 'candidate']:
        selected = [(run, data) for run, data in rows
                    if run['kind'] == 'allocations' and run['mode'] == mode and run['variant'] == variant]
        assert len(selected) == 3 and {run['pair'] for run, _ in selected} == {0, 1, 2}
        allocations[variant] = [{'calls_per_rpc': data['allocation_calls'] / data['iterations'],
                                 'bytes_per_rpc': data['requested_bytes'] / data['iterations']}
                                for _, data in selected]
    recomputed.append({'mode': mode, 'paired_new_old_cpu': ratios,
                       'median_ratio': statistics.median(ratios),
                       'range': [min(ratios), max(ratios)],
                       'baseline_cpu_ns': statistics.median(baseline),
                       'candidate_cpu_ns': statistics.median(candidate),
                       'allocations': allocations})
assert recomputed == json.loads(raw['measurements/summary.json']) == results['modes']
guard = json.loads(raw['builds/candidate-target-cap-attempt/observation.json'])
assert guard['exit_code'] == -15 and guard['guard'] == 'target_cap'
assert guard['limit_bytes'] == 600 * 1024**2
cleanup = json.loads(raw['builds/candidate-target-cap-attempt/cleanup.json'])
assert cleanup['exit_code'] == 0
assert b'r-efi v5.3.0' in raw['builds-resolve-failure-01/baseline-resolve.stderr']
print('PASS: capsule hashes, 407 public Git baseline inputs, 4 builds, 12 prechecks, 72 measured processes, paired summaries, layout, and retained resource failures')
