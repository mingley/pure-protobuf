#!/usr/bin/env python3
"""Check archived source hashes and recompute every equality measurement."""
import argparse
from collections import defaultdict
import hashlib
import itertools
import json
import math
from pathlib import Path
import statistics
import subprocess
import tarfile


def digest(data):
    return hashlib.sha256(data).hexdigest()


def same(actual, expected):
    assert math.isfinite(actual) and math.isfinite(expected)
    assert math.isclose(actual, expected, rel_tol=1e-12, abs_tol=1e-12), (actual, expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--packet', type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[3])
    args = parser.parse_args()
    manifest = json.loads((args.packet / 'manifest.json').read_text())
    archive = args.packet / 'raw.tar.gz'
    assert digest(archive.read_bytes()) == manifest['archive_sha256']
    files = {}
    with tarfile.open(archive, 'r:gz') as tar:
        for member in tar:
            assert member.isfile() and member.name not in files
            assert not member.name.startswith('/') and '..' not in Path(member.name).parts
            assert member.size <= 16 * 1024 * 1024
            stream = tar.extractfile(member)
            assert stream is not None
            files[member.name] = stream.read()
    assert set(files) == set(manifest['files'])
    for name, data in files.items():
        assert digest(data) == manifest['files'][name], name
    def read(name):
        return json.loads(files[name])

    plan = read('source/source-plan.json')
    assert plan['source_commit'] == manifest['source_commit']
    assert len(plan['source_files']) == 70
    base = {}
    for name in plan['source_files']:
        data = subprocess.check_output(['git', 'show', f"{manifest['source_commit']}:{name}"], cwd=args.repo)
        assert digest(data) == plan['git_source_sha256'][name], name
        if name == 'Cargo.toml':
            text = data.decode()
            data = ('[workspace]\nresolver = "2"\n\n' + text[text.index('[package]'):]).encode()
        base[name] = data
    assert files['source/baseline/src/map.rs'] == base['src/map.rs']
    source_inputs = {}
    for variant in ['baseline', 'candidate', 'sorted', 'outlined']:
        source = dict(base)
        source['src/map.rs'] = files[f'source/{variant}/src/map.rs']
        assert files[f'source/{variant}/Cargo.toml'] == source['Cargo.toml']
        source_inputs[variant] = {name: digest(data) for name, data in source.items()}
    assert source_inputs['baseline'] == plan['overlays']['baseline']
    assert source_inputs['candidate'] == plan['overlays']['candidate']
    for variant in ['sorted', 'outlined']:
        assert source_inputs[variant] == read(f'source/{variant}-source-plan.json')['source_sha256']
    binaries = {}
    compilers = set()
    builds = []
    for variant in ['baseline', 'candidate', 'sorted', 'outlined']:
        builds.append((variant, f'{variant}-build', f'{variant}-harness', f'{variant}-bin'))
        for mode in ['cpu', 'allocations']:
            builds.append((variant, f'{variant}-cold-{mode}-build', f'{variant}-cold-harness', f'{variant}-cold-{mode}-bin'))
    locks = {}
    for variant, stem, harness, binary in builds:
        build = read(f'build/{stem}.json')
        assert build['variant'] == variant and build['source_sha256'] == source_inputs[variant]
        compilers.add(build['rustc'])
        assert digest(files[f'build/{stem}.jsonl']) == build['build_log_sha256']
        for suffix, field in [('src/main.rs', 'harness_sha256'), ('Cargo.toml', 'manifest_sha256'), ('Cargo.lock', 'lock_sha256')]:
            assert digest(files[f'harness/{harness}/{suffix}']) == build[field]
        family = 'cold' if '-cold-' in stem else 'warm'
        locks.setdefault(family, files[f'harness/{harness}/Cargo.lock'])
        assert locks[family] == files[f'harness/{harness}/Cargo.lock']
        messages = [json.loads(line) for line in files[f'build/{stem}.jsonl'].splitlines() if line.startswith(b'{')]
        pbrs = [m for m in messages if m.get('reason') == 'compiler-artifact' and m.get('target', {}).get('name') == 'pbrs']
        assert len(pbrs) == 1 and pbrs[0]['features'] == []
        assert any(m.get('reason') == 'build-finished' and m.get('success') is True for m in messages)
        binaries[binary] = build['binary_sha256']
    assert len(compilers) == 1 and 'LLVM version: 23.1.1' in next(iter(compilers))

    standard = set((k, n, d, r, 'reverse') for k, n, d, r in itertools.product(
        ['int', 'proto', 'lazy'], [8, 64, 256], ['distinct', 'duplicate'], ['equal', 'value', 'key', 'size']))
    boundary = set((k, n, d, 'equal', o) for k, n, d, o in itertools.product(
        ['int', 'proto', 'lazy'], [1, 15, 16, 17], ['distinct', 'duplicate'], ['forward', 'reverse']))
    total_pairs = total_processes = 0
    datasets = {}
    for label, kind, pairs in [('screen', 'simple', 5), ('confirm', 'simple', 9), ('same-source', 'drift', 9),
                               ('indexed-screen', 'indexed', 5), ('outlined-screen', 'final', 5),
                               ('outlined-holdout', 'final', 9), ('cold', 'final', 5), ('allocations', 'final', 1)]:
        summary = read(f'data/{label}-summary.json')
        datasets[label] = summary
        raw_name = f'data/{label}-raw.jsonl'
        assert digest(files[raw_name]) == summary['raw_sha256']
        rows = [json.loads(line) for line in files[raw_name].splitlines()]
        if label == 'screen':
            expected_cells = standard
        elif label == 'confirm':
            expected_cells = {tuple(r['cell']) for r in datasets['screen']['cells'] if r['median_new_old'] > 1.02 or (r['cell'][3] == 'equal' and r['cell'][1] in [8, 256])}
        elif label == 'same-source':
            expected_cells = {tuple(r['cell']) for r in datasets['screen']['cells'] if r['median_new_old'] > 1.02}
        elif label == 'indexed-screen':
            expected_cells = standard | boundary
        elif label == 'outlined-screen':
            expected_cells = {(*c, 'warm') for c in standard | boundary}
        elif label == 'outlined-holdout':
            expected_cells = {tuple(r['cell']) for r in datasets['outlined-screen']['cells'] if r['median_outlined_baseline'] > 1.02 or (r['cell'][3] == 'equal' and r['cell'][1] in [8, 16, 256] and r['cell'][4] == 'reverse')}
        else:
            expected_cells = set((k, n, d, r, 'reverse', phase) for k, n, d, r, phase in itertools.product(
                ['int', 'proto', 'lazy'], [8, 16, 256], ['distinct', 'duplicate'], ['equal', 'key'],
                ['warm', 'cold'] if label == 'allocations' else ['cold']))
        grouped = defaultdict(list)
        for row in rows:
            cell = tuple(row['cell'])
            assert cell in expected_cells
            captures = row['captures'].copy()
            if 'hash_reference' in row:
                captures.append(dict(row['hash_reference'], variant='hash'))
            variants = {s['variant'] for s in captures}
            expected_variants = {'baseline', 'candidate', 'hash'} if label == 'screen' else {'baseline', 'candidate'}
            if kind == 'drift': expected_variants = {'first', 'second'}
            elif kind == 'indexed': expected_variants = {'baseline', 'candidate', 'sorted', 'hash'}
            elif kind == 'final': expected_variants = {'baseline', 'outlined'} if label == 'outlined-holdout' else {'baseline', 'candidate', 'sorted', 'outlined', 'hash'}
            assert variants == expected_variants and len(captures) == len(variants)
            values = {}
            counts = set()
            iterations = set()
            for capture in captures:
                assert capture['returncode'] == 0 and capture['stderr'] == ''
                sample = capture['sample']; command = capture['command']
                assert command[:3] == ['taskset', '-c', '0']
                binary = Path(command[3]).name
                assert binary in binaries
                actual_variant = capture['variant']
                expected_variant = 'baseline' if actual_variant in ['hash', 'first', 'second'] else actual_variant
                expected_binary = f'{expected_variant}-bin'
                if label in ['cold', 'allocations']:
                    expected_binary = f'{expected_variant}-cold-{"cpu" if label == "cold" else "allocations"}-bin'
                assert binary == expected_binary
                assert command[5:9] == [cell[0], str(cell[1]), cell[2], cell[3]]
                assert command[10] == cell[4]
                assert command[4] == ('hash' if capture['variant'] == 'hash' else 'map')
                assert sample['kind'] == cell[0] and sample['n'] == cell[1] and sample['dist'] == cell[2] and sample['relationship'] == cell[3]
                assert sample['order'] == cell[4] and sample['peer'] == command[4]
                assert sample['cardinality'] == ((cell[1]+3)//4 if cell[2] == 'duplicate' else cell[1])
                assert sample['expected'] is (cell[3] == 'equal')
                assert sample['iterations'] == int(command[9]) and sample['iterations'] > 0
                iterations.add(sample['iterations'])
                assert sample['matches'] == (sample['iterations'] if sample['expected'] else 0)
                assert sample['cpu_ns'] > 0
                same(sample['cpu_ns_per_eq'], sample['cpu_ns']/sample['iterations'])
                values[capture['variant']] = sample['cpu_ns_per_eq']
                if label in ['cold', 'allocations']:
                    assert command[11] == cell[5] and sample['phase'] == cell[5]
                    assert sample['allocation_instrumented'] is (label == 'allocations')
                    if label == 'allocations':
                        assert sample['allocation_calls'] % sample['iterations'] == 0
                        assert sample['allocation_bytes'] % sample['iterations'] == 0
                        if capture['variant'] != 'hash': counts.add((sample['allocation_calls'], sample['allocation_bytes']))
                        if cell[5] == 'warm' or capture['variant'] == 'hash': assert sample['allocation_calls'] == sample['allocation_bytes'] == 0
            assert len(iterations) == 1
            if label == 'allocations': assert len(counts) == 1
            if kind == 'simple':
                same(row['candidate_baseline_ratio'], values['candidate']/values['baseline'])
                if label == 'screen': same(row['candidate_hash_ratio'], values['candidate']/values['hash'])
            elif kind == 'drift': same(row['second_first_ratio'], values['second']/values['first'])
            elif kind == 'indexed':
                for peer in ['baseline', 'candidate', 'hash']: same(row[f'sorted_{peer}_ratio'], values['sorted']/values[peer])
            else:
                same(row['outlined_baseline_ratio'], values['outlined']/values['baseline'])
                if 'hash' in values: same(row['outlined_hash_ratio'], values['outlined']/values['hash'])
            grouped[cell].append((row, captures, values))
            total_processes += len(captures)
        if label == 'same-source':
            assert summary['binary_sha256'] == binaries['baseline-bin']
        else:
            for variant, recorded in summary['binary_sha256'].items():
                name = f'{variant}-bin'
                if label in ['cold', 'allocations']: name = f'{variant}-cold-{"cpu" if label == "cold" else "allocations"}-bin'
                assert recorded == binaries[name]
        assert set(grouped) == expected_cells
        assert {tuple(r['cell']) for r in summary['cells']} == expected_cells
        for result in summary['cells']:
            records = grouped[tuple(result['cell'])]
            assert len(records) == pairs and {r[0]['pair'] for r in records} == set(range(pairs))
            if kind == 'simple': numerator, denominator, field, span = 'candidate', 'baseline', 'median_new_old', 'range_new_old'
            elif kind == 'drift': numerator, denominator, field, span = 'second', 'first', 'median_second_first', 'range_second_first'
            elif kind == 'indexed': numerator, denominator, field, span = 'sorted', 'baseline', 'median_sorted_baseline', 'range_sorted_baseline'
            else: numerator, denominator, field, span = 'outlined', 'baseline', 'median_outlined_baseline', 'range_outlined_baseline'
            ratios = [v[numerator]/v[denominator] for _, _, v in records]
            same(result[field], statistics.median(ratios))
            for actual, expected in zip(result[span], [min(ratios), max(ratios)]): same(actual, expected)
            for variant, actual in result.get('ns_medians', {}).items(): same(actual, statistics.median(v[variant] for _, _, v in records))
            extra = {'simple': [('median_new_hash', 'candidate', 'hash')], 'indexed': [('median_sorted_candidate', 'sorted', 'candidate'), ('median_sorted_hash', 'sorted', 'hash')], 'final': [('median_outlined_hash', 'outlined', 'hash')], 'drift': []}[kind]
            for key, a, b in extra:
                if key in result: same(result[key], statistics.median(v[a]/v[b] for _, _, v in records))
            for variant, allocation in result.get('allocation_per_eq', {}).items():
                for _, captures, _ in records:
                    sample = next(s['sample'] for s in captures if s['variant'] == variant)
                    same(allocation['calls'], sample['allocation_calls']/sample['iterations'])
                    same(allocation['bytes'], sample['allocation_bytes']/sample['iterations'])
        total_pairs += len(rows)
    assert total_pairs == manifest['process_groups'] == 2316
    assert total_processes == manifest['processes'] == 8748
    assert b'test result: ok. 14 passed; 0 failed' in files['tests/map-tests.log']
    print(f'OK: {len(files)} files; 70 Git source inputs; 12 builds; {total_pairs} groups; {total_processes} processes; all ratios and allocation counts verified')


if __name__ == '__main__':
    main()
