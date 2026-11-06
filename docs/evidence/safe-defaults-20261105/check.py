#!/usr/bin/env python3
"""Check archived cost observations and optionally reconstruct their sources."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import statistics
import subprocess
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sha = lambda value: hashlib.sha256(value).hexdigest()
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--reconstruct', type=Path, help='write selected sources to a new directory')
args = parser.parse_args()
manifest = json.loads((HERE / 'manifest.json').read_text())
results = json.loads((HERE / 'results.json').read_text())
assert manifest['schema'] == 'pbrs.safe-defaults-evidence.v1'
assert results['schema'] == 'pbrs.safe-defaults-costs.v1'
assert manifest['qualified'] is results['qualified'] is False
assert sha((HERE / 'results.json').read_bytes()) == manifest['results_sha256']
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
    assert {'bytes': len(content), 'sha256': sha(content)} == manifest['files'][name], name
j = lambda name: json.loads(data[name])
plan = j('sources/actual-plan.json')
assert plan['base_commit'] == manifest['source_base_commit']

def git_sources(names):
    return {name: subprocess.check_output(['git', 'show', f"{plan['base_commit']}:{name}"], cwd=ROOT)
            for name in names}

def hashes(source):
    return {name: sha(content) for name, content in source.items()}

def write_sources(folder, source):
    for name, content in source.items():
        path = folder / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)

baseline = git_sources(plan['baseline_inputs_sha256'])
assert hashes(baseline) == plan['baseline_inputs_sha256']
with tempfile.TemporaryDirectory(prefix='pbrs-safe-defaults-check-') as tmp:
    folder = Path(tmp)
    write_sources(folder, baseline)
    subprocess.run(['git', 'apply', '-p1', '-'], cwd=folder,
                   input=data['sources/actual-final.patch'], check=True)
    final_sources = {name: (folder / name).read_bytes() for name in plan['final_inputs_sha256']}
    assert hashes(final_sources) == plan['final_inputs_sha256']
    subprocess.run(['git', 'apply', '-p1', '-'], cwd=folder,
                   input=data['sources/initial-adjustment.patch'], check=True)
    initial_sources = {name: (folder / name).read_bytes() for name in plan['initial_inputs_sha256']}
    assert hashes(initial_sources) == plan['initial_inputs_sha256']

builds = j('actual/final-builds/report.json')
initial = j('actual/initial-builds/report.json')
for prefix, report, expected in [('final', builds, plan['final_inputs_sha256']),
                                 ('initial', initial, plan['initial_inputs_sha256'])]:
    assert report['inputs_sha256'] == expected
    assert report['source_inputs_unchanged'] and report['changed_source_files'] == []
    assert len(report['builds']) == 4
    for row in report['builds']:
        assert row['exit_code'] == 0 and row['precheck'].count('PASS ') == 4
        logs = [json.loads(line) for line in data[f"actual/{prefix}-builds/{row['name']}.stdout"].splitlines()
                if line.startswith(b'{')]
        assert any(record.get('executable') and record.get('target', {}).get('name') == 'pbrs-safe-defaults-proof'
                   for record in logs)
assert builds['rustc'] == initial['rustc']
for old, new in zip(initial['builds'], builds['builds']):
    for field in ['name', 'binary_sha256', 'binary_bytes', 'generated']:
        assert old[field] == new[field]
sizes = [{'name': row['name'], 'generated_bytes': sum(v['bytes'] for v in row['generated'].values()),
          'binary_bytes': row['binary_bytes'], 'binary_sha256': row['binary_sha256'],
          'generated': row['generated']} for row in builds['builds']]
assert sizes == results['sizes']

runtime = j('actual/runtime/report.json')
assert runtime['build_report_sha256'] == sha(data['actual/final-builds/report.json'])
assert runtime['runner_sha256'] == sha(data['controllers/measure-generated-final.py'])
assert len(runtime['captures']) == 80
identities = set()
values = {}
for capture in runtime['captures']:
    stem = 'actual/runtime/' + capture['id']
    assert sha(data[stem + '.stdout']) == capture['stdout_sha256']
    assert sha(data[stem + '.stderr']) == capture['stderr_sha256']
    value = json.loads(data[stem + '.stdout'])
    assert value == capture['value'] and capture['exit_code'] == 0
    assert value['case'] == capture['case'] and value['wall_ns'] > 0 and value['iterations'] > 0
    assert value['ordinary'] == (capture['variant'] == 'ordinary')
    assert Path(capture['command'][0]).name == f"{capture['variant']}-{capture['profile']}"
    key = capture['profile'], capture['case'], capture['repeat'], capture['variant']
    assert key not in values
    values[key] = value['wall_ns'] / value['iterations']
    identities.add((value['input_wire_hex'], value['expected_checksum'], value['record_size'], value['presence_size']))
assert len(identities) == 1
for row in runtime['rows']:
    old = [values[row['profile'], row['case'], i, 'ordinary'] for i in range(5)]
    new = [values[row['profile'], row['case'], i, 'safe'] for i in range(5)]
    ratios = [n / o for n, o in zip(new, old)]
    assert row['safe_ordinary_ratios'] == ratios
    assert row['median_safe_ordinary'] == statistics.median(ratios)
    assert row['range_safe_ordinary'] == [min(ratios), max(ratios)]
    assert row['median_ns_ordinary'] == statistics.median(old)
    assert row['median_ns_safe'] == statistics.median(new)
assert runtime['rows'] == results['actual_runtime_rows'] and len(runtime['rows']) == 8

warm = j('actual/warm-builds/report.json')
assert warm['runner_sha256'] == sha(data['controllers/measure-builds.py'])
assert warm['proof_inputs_unchanged'] and len(warm['captures']) == 12
assert warm['proof_inputs_sha256'] == {name: digest for name, digest in plan['initial_inputs_sha256'].items()
                                       if name.startswith('tests/proof/safe_defaults/')}
for capture in warm['captures']:
    stem = f"actual/warm-builds/{capture['profile']}-r{capture['repeat']}-{capture['variant']}"
    assert capture['clean_exit_code'] == capture['exit_code'] == 0 and capture['elapsed_seconds'] > 0
    logs = [json.loads(line) for line in data[stem + '.stdout'].splitlines() if line.startswith(b'{')]
    rebuilt = [item.get('target', {}).get('name') for item in logs
               if item.get('reason') == 'compiler-artifact' and not item.get('fresh')]
    assert rebuilt == capture['rebuilt_targets']
    assert set(rebuilt) <= {'pbrs-safe-defaults-proof', 'build-script-build'}
for row in warm['rows']:
    values = {(c['repeat'], c['variant']): c['elapsed_seconds'] for c in warm['captures'] if c['profile'] == row['profile']}
    ratios = [values[i, 'safe'] / values[i, 'ordinary'] for i in range(3)]
    assert row['ratios_safe_ordinary'] == ratios
    assert row['median_safe_ordinary'] == statistics.median(ratios)
    assert row['range_safe_ordinary'] == [min(ratios), max(ratios)]
assert warm['rows'] == results['warm_consumer_build_rows'] and len(warm['rows']) == 2

synthetic_plan = j('synthetic/source-plan.json')
synthetic_sources = git_sources(synthetic_plan['original_sha256'])
assert hashes(synthetic_sources) == synthetic_plan['original_sha256']
synthetic_sources['Cargo.toml'] = data['synthetic/adapted-Cargo.toml']
synthetic_sources['Cargo.lock'] = data['synthetic/adapted-Cargo.lock']
assert hashes(synthetic_sources) == synthetic_plan['adapted_sha256']
runtime_build = j('synthetic/runtime-build-release.json')
assert runtime_build['source_unchanged'] and runtime_build['exit_code'] == 0
assert runtime_build['inputs_sha256'] == synthetic_plan['adapted_sha256']
layout_builds = j('synthetic/layout-cache-builds.json')
synthetic = j('synthetic/runtime/report.json')
assert layout_builds['runtime_build_sha256'] == sha(data['synthetic/runtime-build-release.json'])
assert layout_builds['source_sha256'] == synthetic['source_sha256'] == sha(data['synthetic/layouts-cached.rs'])
assert synthetic['builds_sha256'] == sha(data['synthetic/layout-cache-builds.json'])
assert synthetic['runner_sha256'] == sha(data['synthetic/measure-layouts.py'])
assert len(layout_builds['builds']) == 4 and len({b['layout'] for b in layout_builds['builds']}) == 1
for build in layout_builds['builds']:
    assert build['exit_code'] == 0 and build['precheck'] == 'CHECK default field semantics and drop passed'
    assert build['binary_sha256'] == synthetic['binaries_sha256'][f"{build['variant']}-o{build['opt_level']}-cache"]
values = {}
assert len(synthetic['captures']) == 220
for capture in synthetic['captures']:
    stem = 'synthetic/runtime/' + capture['id']
    assert sha(data[stem + '.stdout']) == capture['stdout_sha256']
    assert sha(data[stem + '.stderr']) == capture['stderr_sha256']
    value = json.loads(data[stem + '.stdout'])
    assert capture['exit_code'] == 0 and value['wall_ns'] > 0
    assert value['case'] == capture['case'] and value['safe_defaults'] == (capture['variant'] == 'derived')
    assert value['wall_ns'] / value['iterations'] == capture['ns_per_op']
    key = capture['opt_level'], capture['case'], capture['repeat'], capture['variant']
    assert key not in values
    values[key] = capture['ns_per_op']
for row in synthetic['rows']:
    old = [values[row['opt_level'], row['case'], i, 'zeroed'] for i in range(5)]
    new = [values[row['opt_level'], row['case'], i, 'derived'] for i in range(5)]
    ratios = [n / o for n, o in zip(new, old)]
    assert row['paired_new_old'] == ratios and row['median_new_old'] == statistics.median(ratios)
    assert row['range_new_old'] == [min(ratios), max(ratios)]
    assert row['median_ns_zeroed'] == statistics.median(old) and row['median_ns_derived'] == statistics.median(new)
assert synthetic['rows'] == results['synthetic_runtime_rows'] and len(synthetic['rows']) == 22
if args.reconstruct:
    destination = args.reconstruct.resolve()
    destination.mkdir(parents=True, exist_ok=False)
    write_sources(destination / 'actual', final_sources)
    write_sources(destination / 'synthetic/runtime', synthetic_sources)
    write_sources(destination / 'records', data)
print(json.dumps({'passed': True, 'archive_files': len(data), 'actual_processes': 80,
                  'paired_consumer_builds': 12, 'synthetic_processes': 220,
                  'scope': 'Archive/source consistency and recomputed observations; no executable reproduction or host qualification.'}))
