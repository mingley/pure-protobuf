#!/usr/bin/env python3
"""Verify source, proof and timing records for borrowed recursive validation."""
import hashlib
import io
import itertools
import json
import math
from pathlib import Path, PurePosixPath
import random
import re
import statistics
import subprocess
import tarfile
import tempfile
import tomllib

HERE = Path(__file__).resolve().parent
ROOT = next(p for p in HERE.parents if (p / '.git').exists())
BASE = '41612b32e6e3466406ff63138a1e2d65c0d5d22e'
RUNS = {'safe-screen': (320, 5, 3), 'safe-confirmation': (448, 7, 5),
        'safe-focused-confirmation': (360, 9, 5), 'partial-screen': (40, 5, 3),
        'partial-confirmation': (56, 7, 5)}

def sha(data):
    return hashlib.sha256(data).hexdigest()

def safe(name):
    p = PurePosixPath(name)
    assert not p.is_absolute() and '..' not in p.parts and str(p) == name, name
    return name

def load(files, path):
    return json.loads(files[safe(path)])

def git_blob(path):
    safe(path)
    return subprocess.check_output(['git', 'show', BASE + ':' + path], cwd=ROOT)

def open_archive():
    manifest = json.loads((HERE / 'manifest.json').read_text())
    assert manifest['schema'] == 'pbrs.borrowed-validator-evidence.v1'
    assert manifest['base_commit'] == BASE and manifest['qualified'] is False
    assert sha((HERE / 'check.py').read_bytes()) == manifest['checker_sha256']
    assert sha((HERE / 'README.md').read_bytes()) == manifest['readme_sha256']
    archive_bytes = (HERE / 'raw.tar.gz').read_bytes()
    assert sha(archive_bytes) == manifest['archive_sha256']
    files, total = {}, 0
    with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode='r:gz') as tar:
        for member in tar.getmembers():
            name = safe(member.name)
            assert member.isfile() and name not in files and 0 <= member.size < 4 * 1024 * 1024
            total += member.size
            assert total < 32 * 1024 * 1024
            data = tar.extractfile(member).read()
            assert not data.startswith(b'\x7fELF'), name
            files[name] = data
    assert set(files) == set(manifest['files'])
    for name, data in files.items():
        assert manifest['files'][name] == {'bytes': len(data), 'sha256': sha(data)}, name
    return files

def source_and_proofs(files):
    pin = load(files, 'integration-review/source-hashes.json')
    assert pin['baseline_commit'] == BASE and len(pin['files']) == 5
    audit = load(files, 'integration-review/source-audit.json')
    assert audit['base_commit'] == BASE and audit['scope'].startswith('post-hoc')
    assert audit['total_files'] == len(audit['files']) == 168
    assert len({row['path'] for row in audit['files']}) == 168
    tree = {}
    for entry in subprocess.check_output(['git', 'ls-tree', '-rz', '--full-tree', BASE], cwd=ROOT).split(b'\0'):
        if entry:
            head, path = entry.split(b'\t', 1); mode, kind, oid = head.split(b' ')
            tree[path.decode()] = (mode.decode(), oid.decode())
    changed = set()
    for row in audit['files']:
        path = safe(row['path']); data = files['source/' + path]
        oid = hashlib.sha1(f'blob {len(data)}\0'.encode() + data).hexdigest()
        assert sha(data) == row['sha256'] and oid == row['snapshot_oid']
        mode, expected_oid = tree[path]
        assert (row['git_mode'], row['git_oid']) == (mode, expected_oid)
        assert row['snapshot_mode'] == mode
        status = 'unchanged' if oid == expected_oid else 'content-changed'
        assert row['status'] == status
        if status == 'unchanged': assert data == git_blob(path)
        else: changed.add(path)
    assert changed == {'Cargo.toml', 'Cargo.lock', 'src/codegen/parse.rs',
                       'tests/proof/validated_lazy/build.rs', 'tests/proof/validated_lazy/strict.proto',
                       'tests/proof/validated_lazy/src/main.rs', 'tests/proof/validated_lazy/README.md'}
    assert {name.removeprefix('source/') for name in files if name.startswith('source/')} == {row['path'] for row in audit['files']}
    patch = files['integration-review/candidate.patch']
    assert sha(patch) == pin['patch_sha256']
    paths = {row['path'] for row in pin['files']}
    assert paths == {'src/codegen/parse.rs', 'tests/proof/validated_lazy/build.rs',
                     'tests/proof/validated_lazy/strict.proto', 'tests/proof/validated_lazy/src/main.rs',
                     'tests/proof/validated_lazy/README.md'}
    with tempfile.TemporaryDirectory(prefix='pbrs-validator-source-') as temp:
        tmp = Path(temp)
        subprocess.run(['git', 'init', '--quiet', str(tmp)], check=True)
        for row in pin['files']:
            p = safe(row['path']); data = git_blob(p)
            assert sha(data) == row['baseline_sha256']
            dst = tmp / p; dst.parent.mkdir(parents=True, exist_ok=True); dst.write_bytes(data)
        patch_path = tmp / 'candidate.patch'; patch_path.write_bytes(patch)
        subprocess.run(['git', 'apply', '--check', str(patch_path)], cwd=tmp, check=True)
        subprocess.run(['git', 'apply', str(patch_path)], cwd=tmp, check=True)
        for row in pin['files']:
            assert sha((tmp / row['path']).read_bytes()) == row['candidate_sha256']
            assert (tmp / row['path']).read_bytes() == files['source/' + row['path']]
    cfg = files['source/Cargo.toml']
    assert tomllib.loads(cfg.decode())['workspace']['members'] == ['.']
    assert cfg.split(b'[package]', 1)[1] == git_blob('Cargo.toml').split(b'[package]', 1)[1]
    for p in ['src/lazy.rs', 'tests/proof/codec_benchmark/Cargo.toml',
              'tests/proof/codec_benchmark/Cargo.lock', 'tests/proof/codec_benchmark/src/main.rs',
              'proto/codec_cases.proto']:
        assert files['source/' + p] == git_blob(p), p
    before = files['snapshot-baseline-pbrs/codec_cases.rs'].decode()
    after = files['candidate-safe-pbrs/codec_cases.rs'].decode()
    def region(text):
        start = text.index('    pub fn validate_until(', text.index('impl Node {'))
        end = text.index('    pub fn compute_size(', start)
        return start, end
    a, b = region(before); c, d = region(after)
    assert before[:a] == after[:c] and before[b:] == after[d:]
    assert 'fn __pbrs_validate_self(' not in before[a:b]
    assert after[c:d].count('fn __pbrs_validate_self(') == 1
    assert files['snapshot-baseline-prost/cases.rs'] == files['candidate-safe-prost/cases.rs']
    check = load(files, 'generated-fixture-check.json')
    assert check['status'] == 'passed'
    assert check['before_sha256'] == sha(before.encode()) and check['after_sha256'] == sha(after.encode())
    assert check['old_validator_sha256'] == sha(before[a:b].encode())
    assert check['new_validator_sha256'] == sha(after[c:d].encode())
    assert check['prost_sha256'] == sha(files['snapshot-baseline-prost/cases.rs'])
    partial_before = files['partial-baseline-pbrs/partial-getter.rs'].decode()
    partial_after = files['partial-candidate-pbrs/partial-getter.rs'].decode()
    a, b = region(partial_before); c, d = region(partial_after)
    assert partial_before[:a] == partial_after[:c] and partial_before[b:] == partial_after[d:]
    assert 'fn __pbrs_validate_self(' not in partial_before[a:b]
    assert partial_after[c:d].count('fn __pbrs_validate_self(') == 1
    assert files['partial-baseline-prost/partial.rs'] == files['partial-candidate-prost/partial.rs']
    shape = load(files, 'proof-shapes.json')
    expectations = {'proof.rs': {'Node': True, 'Mixed': False, 'WithRequired': False,
                                'FreeChoice': False, 'RequiredCollections': False},
                    'strict.rs': {'StrictNode': True, 'CompactNode': True, 'RecursiveChoice': True,
                                  'RecursiveSequence': True, 'Blob': False, 'BlobRoot': False}}
    found = {}
    for filename, names in expectations.items():
        text = files['integration-review/proof-generated/' + filename].decode()
        for name, expected in names.items():
            start = text.index(f'impl {name} {{'); end = text.index(f'pbrs::impl_typed_message!({name},', start)
            part = text[start:end]; marker = '\n        fn __pbrs_validate_self('
            assert (marker in part) is expected, name
            if expected:
                assert 'crate::GENERATED_VALIDATIONS.fetch_add' in part[part.index(marker):][:400]
            found[name] = expected
    assert shape['status'] == 'passed' and shape['selected'] == found
    assert b'passed 12433 differential inputs in borrowed/shared parse and two validator modes' in files['differential-final.log']
    assert b'checked 13 bundled bindings' in files['regen-check-final.log']
    assert b'1 passed; 0 failed' in files['selector-tests-final.log']
    log = files['validated-lazy-collision.log']
    for expected in [b'PASS recursive self oneof/repeated validation, unknown groups, UTF-8 and recursion bounds',
                     b'PASS clone before/after materialization and concurrent shared getters',
                     b'PASS legacy MergeBytes default fallback']:
        assert expected in log
    for name in ('selector', 'consumer', 'consumer-clippy'):
        report = load(files, 'integration-review/' + name + '.json')
        output = files['integration-review/' + name + '.log']
        assert report['exit_code'] == 0 and report['source_unchanged'] is True and not report['disk_guard']
        assert sha(output) == report['log_sha256']
        for p in paths - {'tests/proof/validated_lazy/README.md'}:
            candidate = next(row['candidate_sha256'] for row in pin['files'] if row['path'] == p)
            assert report['source_sha256'][p] == candidate
        for p in ('src/codegen/config.rs', 'src/codegen/messages.rs'):
            assert report['source_sha256'][p] == sha(files['integration-review/integrated-codegen/' + Path(p).name])
        for p in ('tests/proof/validated_lazy/proof.proto', 'tests/proof/validated_lazy/Cargo.toml',
                  'tests/proof/validated_lazy/Cargo.lock', 'Cargo.lock'):
            assert report['source_sha256'][p] == sha(git_blob(p))
    assert b'1 passed; 0 failed' in files['integration-review/selector.log']
    assert b'PASS recursive self oneof/repeated validation, unknown groups, UTF-8 and recursion bounds' in files['integration-review/consumer.log']
    return pin

def interval(values):
    return {'median': statistics.median(values), 'range': [min(values), max(values)], 'pairs': values}

def equal(a, b):
    if isinstance(a, dict) and isinstance(b, dict):
        assert a.keys() == b.keys()
        for k in a: equal(a[k], b[k])
    elif isinstance(a, list) and isinstance(b, list):
        assert len(a) == len(b)
        for x, y in zip(a, b): equal(x, y)
    elif type(a) in (float, int) and type(b) in (float, int):
        assert math.isfinite(a) and math.isfinite(b) and math.isclose(a, b, rel_tol=1e-12, abs_tol=1e-12), (a, b)
    else: assert a == b, (a, b)

def check_run(files, label, expected, observation):
    count, pairs, samples = expected
    prefix = label + '-results/'
    manifest = load(files, prefix + 'manifest.json')
    assert manifest['state'] == 'complete' and manifest['passed'] is True and manifest['qualified'] is False
    assert manifest['before_git_commit'] == BASE and (manifest['processes'], manifest['pairs'], manifest['samples']) == expected
    partial = label.startswith('partial-')
    names = ('partial-baseline', 'partial-candidate') if partial else ('snapshot-baseline', 'candidate-safe')
    for side, name in zip(('before', 'after'), names):
        observed = next(item for item in observation['binaries'] if item['path'] == name)
        assert manifest['binary_sha256'][side] == observed['binary_sha256']
        assert 'rustc version 1.99.1 (58cf8f9c3 2026-10-13)' in observed['readelf_comment']
    for path, expected_hash in manifest['candidate_source_sha256'].items():
        source = path if path.startswith('partial/') else 'source/' + path
        assert sha(files[source]) == expected_hash, path
    operations = ('read_first_child',) if partial else ('parse_owned', 'read_all')
    rng = random.Random(manifest['seed']); order = []
    for pair in range(pairs):
        cells = list(itertools.product(manifest['iterations'], operations)); rng.shuffle(cells)
        for case, op in cells:
            versions = list(itertools.product(('before', 'after'), ('pbrs', 'prost'))); rng.shuffle(versions)
            order.extend({'pair': pair, 'case': case, 'operation': op, 'source': src, 'backend': peer}
                         for src, peer in versions)
    assert order == manifest['plan'] and len(order) == count
    raw = [json.loads(line) for line in files[prefix + 'raw.jsonl'].splitlines()]
    assert len(raw) == count
    fixtures, values = {}, {}
    for entry, row in zip(order, raw):
        assert all(row[k] == v for k, v in entry.items()) and row['returncode'] == 0 and row['stderr'] == ''
        cmd = row['command']; exp_name = names[0 if entry['source'] == 'before' else 1]
        assert Path(cmd[0]).name == exp_name
        assert cmd[1:] == [entry['case'], entry['backend'], entry['operation'], str(manifest['iterations'][entry['case']]), str(samples)]
        result = json.loads(row['stdout'])
        assert all(result[k] == entry[k] for k in ('case', 'backend', 'operation'))
        assert result['semantic_prechecks'] is True and result['allocation_instrumentation'] is False
        assert result.get('message_storage', 'lazy') == 'lazy' and len(result['samples']) == samples
        assert all(x['iterations'] == manifest['iterations'][entry['case']] and x['cpu_ns'] > 0 for x in result['samples'])
        signature = [result['wire_bytes'], result['wire_fnv1a64'], result['checksum'], result.get('full_fixture_checksum')]
        assert fixtures.setdefault(entry['case'], signature) == signature
        key = (entry['pair'], entry['case'], entry['operation'], entry['source'], entry['backend'])
        assert key not in values
        values[key] = statistics.mean(x['cpu_ns'] / x['iterations'] for x in result['samples'])
    expected_rows = []
    for case, op in itertools.product(manifest['iterations'], operations):
        def ratio(s1, b1, s2, b2):
            return interval([values[(p, case, op, s1, b1)] / values[(p, case, op, s2, b2)] for p in range(pairs)])
        expected_rows.append({'case': case, 'operation': op,
            'candidate_over_baseline_pbrs': ratio('after', 'pbrs', 'before', 'pbrs'),
            'candidate_pbrs_over_prost': ratio('after', 'pbrs', 'after', 'prost'),
            'baseline_pbrs_over_prost': ratio('before', 'pbrs', 'before', 'prost'),
            'candidate_over_baseline_prost': ratio('after', 'prost', 'before', 'prost'), 'fixture': fixtures[case]})
    equal(expected_rows, load(files, prefix + 'summary.json'))
    return {'processes': count, 'pairs': pairs, 'host': manifest['host']['node']}

def main():
    files = open_archive(); source_and_proofs(files)
    observation = load(files, 'integration-review/build-observation.json')
    assert observation['scope'].startswith('post-build') and len(observation['binaries']) == 4
    for build, target in [('snapshot-baseline-build.jsonl', 'pbrs-codec-proof'),
                          ('candidate-safe-final-build.jsonl', 'pbrs-codec-proof'),
                          ('partial-baseline-build.jsonl', 'pbrs-partial-getter-proof'),
                          ('partial-candidate-build-retry.jsonl', 'pbrs-partial-getter-proof')]:
        entries = [json.loads(line) for line in files[build].splitlines() if line.startswith(b'{')]
        artifacts = [x for x in entries if x.get('reason') == 'compiler-artifact'
                     and x.get('target', {}).get('name') == target and x.get('executable')]
        assert len(artifacts) == 1 and artifacts[0]['profile']['opt_level'] == '3'
        assert 'allocations' not in artifacts[0]['features']
    results = {name: check_run(files, name, expected, observation) for name, expected in RUNS.items()}
    assert sum(x['processes'] for x in results.values()) == 1224
    print(json.dumps({'base_commit': BASE, 'runs': results, 'processes': 1224, 'qualified': False}))

if __name__ == '__main__':
    main()
