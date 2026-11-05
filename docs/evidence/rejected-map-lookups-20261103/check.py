#!/usr/bin/env python3
"""Verify the retained borrowed-map lookup experiments against Git."""
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import re
import statistics
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent
ROOT = next(p for p in HERE.parents if (p / '.git').exists())
BASE = '41612b32e6e3466406ff63138a1e2d65c0d5d22e'
VARIANTS = ('baseline', 'candidate', 'candidate2', 'candidate3')
SUITES = {
    'screen': ('candidate', 76, 5, 100000, True),
    'confirm': ('candidate', 32, 7, 1000000, False),
    'holdout': ('candidate', 13, 9, 3000000, False),
    'variant2-screen': ('candidate2', 13, 5, 300000, False),
    'variant2-holdout': ('candidate2', 13, 9, 3000000, False),
    'variant2-full': ('candidate2', 76, 5, 100000, True),
    'variant3-screen': ('candidate3', 13, 5, 300000, False),
    'variant3-holdout': ('candidate3', 13, 9, 3000000, False),
    'variant3-full': ('candidate3', 76, 5, 100000, True),
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git_blob(path):
    return subprocess.check_output(['git', 'show', BASE + ':' + path], cwd=ROOT)


def open_archive():
    manifest = json.loads((HERE / 'manifest.json').read_text())
    assert manifest['schema'] == 'pbrs.borrowed-map-rejected-evidence.v1'
    assert manifest['base_commit'] == BASE and manifest['selected'] is False
    raw = (HERE / 'raw.tar.gz').read_bytes()
    assert sha(raw) == manifest['archive_sha256']
    files = {}
    with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as archive:
        for member in archive.getmembers():
            path = PurePosixPath(member.name)
            assert member.isfile() and not path.is_absolute() and '..' not in path.parts
            assert member.name not in files and member.size < 2 * 1024 * 1024
            files[member.name] = archive.extractfile(member).read()
    assert files.keys() == manifest['files'].keys()
    for path, data in files.items():
        assert manifest['files'][path] == {'bytes': len(data), 'sha256': sha(data)}, path
    return manifest, files


def load(files, path):
    return json.loads(files[path])


def verify_builds(files):
    snapshot = load(files, 'snapshot.json')
    assert snapshot['commit'] == BASE and len(snapshot['files']) == 72
    for path, expected in snapshot['files'].items():
        data = git_blob(path)
        if path == 'Cargo.toml':
            text = data.decode()
            data = ('[workspace]\nresolver = "2"\n\n' + text[text.index('[package]'):]).encode()
        assert sha(data) == expected, path
    builds = {}
    harness = sha(files['bench.rs'])
    rustc, cargo, lock = None, None, None
    for variant in VARIANTS:
        for mode in ('shipping', 'alloc'):
            stem = variant if mode == 'shipping' else variant + '-alloc'
            report = load(files, stem + '-build.json')
            assert report['variant'] == variant and report['mode'] == mode
            assert re.fullmatch('[0-9a-f]{64}', report['retained_sha256'])
            assert report['retained_sha256'] == report['executable_sha256']
            assert report['build_log_sha256'] == sha(files[stem + '-build.jsonl'])
            assert report['harness_sha256'] == harness == sha(files[variant + '-harness/src/main.rs'])
            assert report['manifest_sha256'] == sha(files[variant + '-harness/Cargo.toml'])
            assert report['cargo_lock_sha256'] == sha(files[variant + '-harness/Cargo.lock'])
            assert ('allocations' in report['command']) is (mode == 'alloc')
            assert '--locked' in report['command'] and '--offline' in report['command']
            assert report['input_sha256'].keys() == snapshot['files'].keys()
            if variant == 'baseline':
                assert report['input_sha256'] == snapshot['files']
            else:
                for path, expected in snapshot['files'].items():
                    if path in ('src/map.rs', 'src/lazy.rs'):
                        expected = sha(files[variant + '/' + path])
                    assert report['input_sha256'][path] == expected, (variant, path)
            assert report['rustc'].startswith('rustc 1.99.1 ') and 'LLVM version: 23.1.1' in report['rustc']
            rustc = rustc or report['rustc']
            cargo = cargo or report['cargo']
            lock = lock or report['cargo_lock_sha256']
            assert (report['rustc'], report['cargo'], report['cargo_lock_sha256']) == (rustc, cargo, lock)
            messages = [json.loads(line) for line in files[stem + '-build.jsonl'].splitlines()
                        if line.startswith(b'{')]
            artifacts = [item for item in messages if item.get('reason') == 'compiler-artifact'
                         and item.get('executable') == report['executable']]
            assert len(artifacts) == 1
            assert artifacts[0]['target']['name'] == 'pbrs-borrowed-map-lookup-screen'
            assert artifacts[0]['profile']['opt_level'] == '3'
            builds[stem] = report
    return builds


def expected_input(case, iterations):
    kind, n, distribution, length, pattern, result = case
    assert kind in ('proto', 'lazy', 'int', 'hash')
    assert n > 0 and distribution in ('distinct', 'duplicate') and length >= 8
    assert pattern in ('distinct', 'prefix') and result in ('hit', 'miss')
    count = (n + 3) // 4 if distribution == 'duplicate' else n
    values = {}
    for i in range(n):
        values[(i * 17) % count] = i * 7 + 3
    start = 0 if result == 'hit' else 1_000_000
    queries = [start + (i * 13 % count) for i in range(16)]
    fingerprint = 14695981039346656037
    for query in queries:
        suffix = f'{query:08x}'
        filler = 'x' * (length - 8)
        word = filler + suffix if pattern == 'prefix' else suffix + filler
        for byte in word.encode():
            fingerprint = ((fingerprint ^ byte) * 1099511628211) & ((1 << 64) - 1)
    outputs = [values[x] + 1 if x in values else 0 for x in queries]
    checksum = (sum(outputs) * (iterations // 16) + sum(outputs[:iterations % 16])) & ((1 << 64) - 1)
    return f'{fingerprint:016x}', checksum, count


def verify_sample(raw, case, variant, iteration_count, instrumented):
    assert raw['exit_code'] == 0 and raw['stderr'] == ''
    assert tuple(raw['case']) == case and raw['variant'] == variant
    basename = variant + ('-alloc-bin' if instrumented else '-bin')
    assert Path(raw['command'][0]).name == basename
    assert raw['command'][1:] == [*(str(part) for part in case), str(iteration_count)]
    value = json.loads(raw['stdout'])
    fingerprint, checksum, unique = expected_input(case, iteration_count)
    actual_case = (value['kind'], value['size'], value['distribution'], value['key_length'],
                   value['pattern'], value['result'])
    assert actual_case == case and value['iterations'] == iteration_count and value['unique'] == unique
    assert value['query_fingerprint'] == fingerprint and value['checksum'] == checksum
    assert value['semantic_preflight'] is True and value['cpu_ns'] > 0 and value['wall_ns'] > 0
    assert value['allocation_instrumentation'] is instrumented
    if instrumented:
        assert type(value['allocation_calls']) is int and type(value['requested_bytes']) is int
    else:
        assert value['allocation_calls'] is value['requested_bytes'] is None
    return value


def verify_suite(files, name, spec, builds):
    variant, case_count, pair_count, iterations, allocations = spec
    result = load(files, name + '-summary.json')
    raw_bytes = files[name + '-raw.jsonl']
    assert result['raw_sha256'] == sha(raw_bytes)
    assert result['iterations'] == iterations and result['pairs_per_case'] == pair_count
    assert len(result['rows']) == case_count
    if 'binary_sha256' in result:
        assert result['binary_sha256'] == {v: builds[v]['retained_sha256'] for v in ('baseline', variant)}
    else:
        assert result['baseline_binary_sha256'] == builds['baseline']['retained_sha256']
        assert result[variant + '_binary_sha256'] == builds[variant]['retained_sha256']
        assert result['allocation_binary_sha256'] == {
            v: builds[v + '-alloc']['retained_sha256'] for v in ('baseline', variant)}
    indexed = {}
    for line in raw_bytes.splitlines():
        raw = json.loads(line)
        case = tuple(raw['case'])
        instrumented = raw.get('mode') == 'allocations'
        key = (case, raw.get('pair'), raw['variant'], instrumented)
        assert key not in indexed
        indexed[key] = raw
    assert len(indexed) == case_count * (pair_count * 2 + (2 if allocations else 0))
    cases = set()
    for row in result['rows']:
        case = tuple(row['case']) if 'case' in row else (
            row['kind'], row['size'], row['distribution'], row['key_length'], row['pattern'], row['result'])
        assert case not in cases
        cases.add(case)
        assert len(row['pairs']) == pair_count
        ratios = []
        for i, pair in enumerate(row['pairs']):
            observed = {}
            for name2 in ('baseline', variant):
                raw = indexed.pop((case, i, name2, False))
                observed[name2] = verify_sample(raw, case, name2, iterations, False)
            a, b = observed['baseline'], observed[variant]
            assert pair['baseline_cpu_ns'] == a['cpu_ns'] and pair['baseline_wall_ns'] == a['wall_ns']
            prefix = variant if variant + '_cpu_ns' in pair else 'candidate'
            assert pair[prefix + '_cpu_ns'] == b['cpu_ns'] and pair[prefix + '_wall_ns'] == b['wall_ns']
            assert pair['query_fingerprint'] == a['query_fingerprint'] == b['query_fingerprint']
            assert pair['checksum'] == a['checksum'] == b['checksum']
            ratio = b['cpu_ns'] / a['cpu_ns']
            assert pair.get('ratio_cpu', pair.get('cpu_ratio')) == ratio
            ratios.append(ratio)
        assert row['median_cpu_ratio'] == statistics.median(ratios)
        assert row['min_cpu_ratio'] == min(ratios) and row['max_cpu_ratio'] == max(ratios)
        if allocations:
            for name2 in ('baseline', variant):
                raw = indexed.pop((case, None, name2, True))
                value = verify_sample(raw, case, name2, 4096, True)
                saved = row['allocations'][name2]
                assert saved == {'calls': value['allocation_calls'], 'requested_bytes': value['requested_bytes'],
                                 'iterations': 4096, 'checksum': value['checksum']}
    assert not indexed
    return result


def case(result, wanted):
    return next(row for row in result['rows'] if row.get('case') == wanted)


def main():
    manifest, files = open_archive()
    builds = verify_builds(files)
    reports = {name: verify_suite(files, name, spec, builds) for name, spec in SUITES.items()}
    first = case(reports['holdout'], ['proto', 8, 'duplicate', 8, 'distinct', 'hit'])
    third = case(reports['variant3-holdout'], ['lazy', 8, 'duplicate', 8, 'distinct', 'hit'])
    assert first['min_cpu_ratio'] > 1.03 and third['min_cpu_ratio'] > 1.02
    assert manifest['potential_contention'] == ['variant2-holdout', 'variant2-full']
    print(json.dumps({'base_commit': BASE, 'members': len(files), 'measured_suites': len(reports),
                      'source_files': 72, 'candidate1_small_ratio': first['median_cpu_ratio'],
                      'candidate3_small_ratio': third['median_cpu_ratio'], 'selected': False}))


if __name__ == '__main__':
    main()
