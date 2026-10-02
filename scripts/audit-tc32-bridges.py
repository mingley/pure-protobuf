#!/usr/bin/env python3
"""Independently rebuild absolute TC32 bridge costs from retained actual children."""
import argparse
import collections
from fractions import Fraction
import gzip
import hashlib
import json
import re
import statistics
from pathlib import Path

PREFIX = 'codec.adoption.bridge.'
REFERENCE_SHA256 = 'aa2c7238d08617ea320ba96e3432b675946c171c3ef32d9877f593b26564d9be'


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


PINNED_SOURCES = ('bench/devloop/Cargo.toml', 'bench/devloop/Cargo.lock',
                  'bench/devloop/src/main.rs', 'bench/devloop/adoption/Cargo.toml',
                  'bench/devloop/adoption/generate.py', 'bench/devloop/adoption/src/bridge.rs',
                  'bench/devloop/adoption/src/bridge_cells.rs', 'bench/devloop/adoption/src/bridge_options.rs',
                  'protobuf-tonic/src/lib.rs', 'scripts/measure-tc32-bridges.py',
                  'scripts/audit-tc32-bridges.py', 'bench/devloop/build.rs',
                  'bench/devloop/adoption/build.rs', 'bench/devloop/adoption/src/lib.rs',
                  'bench/devloop/adoption/src/corpus.rs', 'bench/devloop/adoption/src/fresh.rs',
                  'bench/devloop/adoption/src/workloads.rs', 'bench/devloop/adoption/src/sparse_walks.rs',
                  'bench/devloop/adoption/proto/adoption.proto', 'bench/devloop/adoption/proto/sparse.proto',
                  'bench/devloop/adoption/proto/options/definitions.proto') + tuple(
                      f'bench/devloop/adoption/proto/options/part_{i:02}.proto' for i in range(20))


def validate_build_pin(pin):
    require(pin['profile'] == 'release' and 'bridge' in pin['features'], 'wrong build profile/feature')
    require(re.fullmatch(r'[0-9a-f]{40}', pin['protobuf_source_commit']) is not None, 'missing upstream source pin')
    require(set(PINNED_SOURCES) <= pin['source_sha256'].keys(), 'missing source/dependency/generator pins')
    require(all(re.fullmatch(r'[0-9a-f]{64}', value) is not None for value in pin['source_sha256'].values()), 'malformed source pin')
    require({'rustc', 'cargo', 'protoc'} <= pin['tools'].keys(), 'missing compiler/tool pins')
    for tool in pin['tools'].values():
        require(tool['path'] and tool['version'] and re.fullmatch(r'[0-9a-f]{64}', tool['sha256']) is not None, 'malformed tool pin')


def expected_ids():
    specimens = [f'query.d{d}.v{v}' for d in range(3, 9) for v in range(8)]
    specimens += [f'entities.n{n}' for n in (10, 100, 1000)]
    specimens += [f'sparse.v{v}' for v in range(10)]
    specimens += [f'maps.n{n}' for n in (8, 64, 512)]
    specimens += [f'options.part{i:02}' for i in range(20)]
    return {f'{PREFIX}{direction}.{specimen}.{mode}'
            for direction in ('prost_to_pbrs', 'pbrs_to_prost')
            for specimen in specimens for mode in ('api_only', 'read_all')}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def inventory_rows(inventory):
    rows = inventory['cells']
    require(len(rows) == 336 and {r['id'] for r in rows} == expected_ids(), 'incomplete/duplicate bridge inventory')
    eligible = 0
    for row in rows:
        require(row['api'] == 'protobuf_tonic::' + row['direction'], 'substituted API')
        require(all(row[k] == 'passed' for k in ('full_equality', 'complete_reads', 'both_api_roundtrips')), 'failed semantic oracle')
        require(type(row['read_checksum']) is int and row['read_checksum'] >= 0, 'invalid complete-read checksum')
        identity = f"bridge-v1:{row['actual_source_wire_bytes']}:{row['common_wire_fingerprint']}:{row['actual_source_wire_fingerprint']}"
        require(row['wire_fingerprint'] == identity, 'fingerprint omits actual source')
        for k in ('common_wire_fingerprint', 'actual_source_wire_fingerprint', 'target_reencoded_wire_fingerprint'):
            require(re.fullmatch(r'fnv1a64:[0-9a-f]{16}', row[k]) is not None, 'missing/malformed wire fingerprint')
        if '.maps.' in row['id']:
            require(row['timing_qualification'] == 'blocked' and row['timing_blocked_reason'], 'map timing bypass')
            require(not row['source_output_equal_wire'], 'map wire-policy failure hidden')
            require(abs(row['actual_source_wire_bytes'] - row['target_reencoded_wire_bytes']) == 4, 'original map wire difference changed')
        else:
            require(row['timing_qualification'] == 'passed' and row['source_output_equal_wire'], 'non-map wire guard failed')
            require(row['common_wire_bytes'] == row['actual_source_wire_bytes'] == row['target_reencoded_wire_bytes'], 'unequal work bytes')
            require(row['common_wire_fingerprint'] == row['actual_source_wire_fingerprint'] == row['target_reencoded_wire_fingerprint'], 'unequal work wire')
            eligible += 1
    require(eligible == 324, 'wrong eligible/blocked coverage')
    return {r['id']: r for r in rows}


def one_json(stderr, prefix):
    lines = [line[len(prefix):] for line in stderr.splitlines() if line.startswith(prefix)]
    require(len(lines) == 1, 'missing/duplicate ' + prefix)
    return json.loads(lines[0])


def graph_total(path):
    raw = gzip.decompress(Path(path).read_bytes())
    text = raw.decode('utf-8')
    events = re.findall(r'^events:\s*(.+)$', text, re.M)
    summaries = re.findall(r'^summary:\s*(.+)$', text, re.M)
    require(events == ['Ir'] and len(summaries) == 1, 'ambiguous/missing graph instruction count')
    require(re.fullmatch(r'[0-9]+', summaries[0]) is not None, 'invalid graph count')
    return int(summaries[0]), hashlib.sha256(raw).hexdigest()


def rebuild(directory):
    directory = Path(directory)
    meta = json.loads((directory / 'metadata.json').read_text())
    require(meta['schema'] == 'tc32-bridge-cost/1' and meta['completed'], 'unfinished capture')
    require(meta['warmup'] == 100 and meta['prepare_iters'] == 2 * meta['iters'] and meta['repeats'] == 3, 'preparation/repeat contract changed')
    require(meta['iters'] > 0 and re.fullmatch(r'[0-9a-f]{40}', meta['source_commit']) is not None, 'invalid source/N')
    require(re.fullmatch(r'[0-9a-f]{64}', meta['binary']['sha256']) is not None, 'missing binary pin')
    validate_build_pin(meta['build_pin'])
    require(meta['build_pin']['source_commit'] == meta['source_commit'] and meta['build_pin']['binary_sha256'] == meta['binary']['sha256'] and meta['build_pin']['profile'] == 'release', 'source/build provenance mismatch')
    for name, expected in meta['artifact_sha256'].items():
        require(sha(directory / name) == expected, 'artifact hash mismatch: ' + name)
    require(json.loads((directory / 'build-pin.json').read_text()) == meta['build_pin'], 'build pin changed')
    require((directory / 'original-registry.txt').read_text().splitlines() == meta['original_registry_rows'], 'original registry proof changed')
    inventory = inventory_rows(json.loads((directory / 'inventory.json').read_text()))
    require(sha(directory / 'reference-inventory.json') == REFERENCE_SHA256, 'retained TC32a oracle changed')
    reference = inventory_rows(json.loads((directory / 'reference-inventory.json').read_text()))
    for row, qualification in inventory.items():
        require(all(qualification[k] == reference[row][k] for k in ('api', 'specimen', 'direction', 'mode', 'read_checksum', 'common_wire_bytes', 'common_wire_fingerprint')), 'retained complete-work oracle differs')
        if qualification['timing_qualification'] == 'passed':
            require(qualification == reference[row], 'non-map actual API wire/semantic oracle changed')
    selected = meta['selected_cells']
    require(selected and len(set(selected)) == len(selected), 'missing/duplicate selection')
    eligible = {r['id'] for r in inventory.values() if r['timing_qualification'] == 'passed'}
    require(set(selected) <= eligible, 'selected map/unregistered cell')
    require(meta['full_eligible_coverage'] == (set(selected) == eligible), 'selective coverage claim')
    registry = (directory / 'registry.txt').read_text().splitlines()
    require(len(registry) == len(set(registry)), 'duplicate parent registry')
    require({line.split()[0] for line in registry if line.startswith(PREFIX)} == expected_ids(), 'parent registry incomplete')
    require([line for line in registry if not line.startswith(PREFIX)] == meta['original_registry_rows'], 'old registry/order changed')
    records = [json.loads(line) for line in (directory / 'raw.jsonl').read_text().splitlines()]
    groups = {}
    expected_collectors = {'allocator', 'callgrind'} if meta['callgrind_available'] else {'allocator'}
    for record in records:
        require(record['validated'] and record['returncode'] == 0, 'failed raw child')
        row, n, repeat, collector = (record[k] for k in ('cell', 'iters', 'repeat', 'collector'))
        require(row in selected and n in (meta['iters'], 2 * meta['iters']) and repeat in range(3) and collector in expected_collectors, 'unexpected child scope')
        key = (row, n, repeat, collector)
        require(key not in groups, 'duplicate raw child')
        require(record['launch_sha256'] == record['after_sha256'] == meta['binary']['sha256'], 'binary drift')
        command = record['command']
        index = command.index('run-cell')
        require(command[index - 1] == meta['binary']['path'] and command[index + 1:] == [row, '--iters', str(n), '--prepare-iters', str(meta['prepare_iters']), '--warmup', '100'], 'child lifecycle command changed')
        if collector == 'allocator':
            require(index == 1, 'allocator child instrumented')
        else:
            require(command[0] == meta['tools']['valgrind']['path'] and command[1:3] == ['--tool=callgrind', '--cache-sim=no'] and index == 5 and command[3].startswith('--callgrind-out-file='), 'wrong instruction collector')
        child = one_json(record['stderr'], '__CHILD__ ')
        qualification = one_json(record['stderr'], '__QUALIFICATION__ ')
        require(qualification == inventory[row], 'actual child qualification differs from preflight')
        require(child['cell'] == row and child['iters'] == n, 'wrong child ID/N')
        require(child.get('input_wire_fingerprint') == inventory[row]['wire_fingerprint'], 'missing/unequal actual-source fingerprint')
        require(all(type(child[k]) is int and child[k] >= 0 for k in ('allocs', 'alloc_bytes', 'wall_ns')), 'invalid exact counters')
        require(record['child'] == child and record['qualification'] == qualification, 'parsed child changed')
        if collector == 'callgrind':
            graph = directory / record['graph']
            require(sha(graph) == record['graph_sha256'], 'graph hash mismatch')
            total, raw_sha = graph_total(graph)
            counts = re.findall(r'I\s+refs:\s*([0-9,]+)', record['stderr'])
            require(len(counts) == 1 and int(counts[0].replace(',', '')) == total, 'graph/stderr IR disagreement')
            require(total == record['instruction_total'] and raw_sha == record['uncompressed_graph_sha256'], 'raw IR/graph changed')
        groups[key] = record
    require(len(groups) == len(selected) * 6 * len(expected_collectors), 'missing child coverage')
    rows = []
    for row in selected:
        values = {}
        for metric in ('allocs', 'alloc_bytes'):
            per_ops = [Fraction(groups[row, n, r, 'allocator']['child'][metric], n) for n in (meta['iters'], 2 * meta['iters']) for r in range(3)]
            require(len(set(per_ops)) == 1, 'allocation repeat/N2N variation: ' + row + '/' + metric)
            values[metric] = {'status': 'measured', 'value': float(per_ops[0]), 'method': 'uninstrumented exact counting GlobalAlloc; N/2N x3 identical per-op totals'}
        if meta['callgrind_available']:
            differences = []
            for r in range(3):
                first = groups[row, meta['iters'], r, 'callgrind']['instruction_total']
                second = groups[row, 2 * meta['iters'], r, 'callgrind']['instruction_total']
                require(second >= first, 'differential IR underflow')
                differences.append(Fraction(second - first, meta['iters']))
            values['instructions'] = {'status': 'measured', 'value': float(statistics.median(differences)), 'method': 'differential_callgrind_2n_minus_n', 'individual_per_op': [float(v) for v in differences]}
        else:
            values['instructions'] = {'status': 'not_run', 'reason': 'valgrind unavailable; no absolute instruction result'}
        rows.append({'id': row, 'input_wire_fingerprint': inventory[row]['wire_fingerprint'], 'metrics': values})
    return {'schema': 'tc32-bridge-absolute-cost/1', 'source_commit': meta['source_commit'], 'binary_sha256': meta['binary']['sha256'], 'iters': meta['iters'], 'prepare_iters': meta['prepare_iters'], 'warmup': 100, 'repeats': 3, 'full_eligible_coverage': meta['full_eligible_coverage'], 'blocked_map_cells': [r['id'] for r in inventory.values() if r['timing_qualification'] == 'blocked'], 'raw_children': len(records), 'cells': rows, 'wall': 'diagnostic only; no latency/headroom claim', 'performance_qualification': 'pending unchanged-binary replay and existing full controls; parent TC32 open'}


def compare_replays(first, second):
    require(first['source_commit'] == second['source_commit'] and first['binary_sha256'] == second['binary_sha256'], 'replay source/binary changed')
    require(all(first[k] == second[k] for k in ('iters', 'prepare_iters', 'warmup', 'repeats', 'full_eligible_coverage', 'blocked_map_cells')), 'replay lifecycle/coverage changed')
    a = {r['id']: r for r in first['cells']}; b = {r['id']: r for r in second['cells']}
    require(a.keys() == b.keys(), 'replay cells changed')
    failures, missing, unstable, rows = [], [], [], []
    for row in a:
        require(a[row]['input_wire_fingerprint'] == b[row]['input_wire_fingerprint'], 'replay actual-source drift')
        for metric in ('allocs', 'alloc_bytes', 'instructions'):
            x, y = a[row]['metrics'][metric], b[row]['metrics'][metric]
            require(x.get('method') == y.get('method'), 'replay method changed')
            if x['status'] != 'measured' or y['status'] != 'measured':
                missing.append({'id': row, 'metric': metric, 'status': 'not_run'})
                continue
            baseline, current = x['value'], y['value']
            delta = (current - baseline) / baseline if baseline else (0 if current == 0 else None)
            # Codec allocator controls must be exact. Instruction replay retains
            # the existing directional >1% regression rule, plus reports both
            # signed variation and absolute variation without a threshold waiver.
            failed = current != baseline if metric != 'instructions' else (delta is None or delta > 0.01)
            record = {'id': row, 'metric': metric, 'first': baseline, 'second': current, 'relative_delta': delta, 'absolute_relative_variation': abs(delta) if delta is not None else None, 'original_limit': 0 if metric != 'instructions' else 0.01, 'failed': failed}
            rows.append(record)
            if failed:
                failures.append(record)
            if metric == 'instructions' and (delta is None or abs(delta) > 0.01):
                unstable.append(record)
    return {'schema': 'tc32-bridge-unchanged-replay/1', 'same_source_binary': True, 'comparisons': rows, 'failed_rows': failures, 'unavailable_rows': missing, 'unstable_instruction_rows': unstable, 'comparison_passed': not failures and not missing, 'replay_stability_passed': not failures and not missing and not unstable, 'full_performance_qualification': False, 'limit': 'Existing full frozen controls remain separately required; no threshold changes or selected best rows.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--baseline', type=Path)
    args = parser.parse_args()
    result = rebuild(args.directory)
    require(result == json.loads((args.directory / 'report.json').read_text()), 'reported absolute cost differs from raw reconstruction')
    if args.baseline:
        baseline = rebuild(args.baseline)
        result = compare_replays(baseline, result)
    print(json.dumps(result, indent=2))
    if args.baseline and not result['replay_stability_passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
