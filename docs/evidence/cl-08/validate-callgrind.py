#!/usr/bin/env python3
"""Validate CL08 frozen 20-row CG controls from independent raw N/2N counters."""
import collections
import hashlib
import json
import math
import re
import shutil
import statistics
from pathlib import Path

root = Path('work/profiles/callgrind')
expected = json.loads((root / 'smoke-cells.json').read_text())
pins = json.loads(Path('work/profiles/cl08-pins.json').read_text())
summary = {'schema': 'cl08-callgrind-summary/1', 'scope': expected, 'rows': [], 'primary_regressions': [], 'diagnostic_regressions': []}
reports = {}
for revision in ['before', 'after']:
    source = pins['source_' + revision]
    binary = pins['binary_' + revision]
    assert hashlib.sha256(Path(binary['path']).read_bytes()).hexdigest() == binary['sha256']
    report = json.loads((root / (revision + '.json')).read_text())
    assert report['schema'] == 'devloop/1' and report['devloop_commit'] == source
    assert [r['id'] for r in report['cells']] == expected
    records = [json.loads(line) for line in (root / (revision + '.raw.jsonl')).read_text().splitlines()]
    assert len(records) == 120
    groups = collections.defaultdict(list)
    for record in records:
        assert record['returncode'] == 0
        cmd = record['command']
        row = cmd[cmd.index('run-cell') + 1]
        n = int(cmd[cmd.index('--iters') + 1])
        assert row in expected and n in [200, 400]
        assert cmd[cmd.index('--prepare-iters') + 1] == '400'
        assert Path(cmd[cmd.index('run-cell') - 1]).resolve() == Path(binary['path']).resolve()
        children = [json.loads(line.removeprefix('__CHILD__ ')) for line in record['stderr'].splitlines() if line.startswith('__CHILD__ ')]
        assert len(children) == 1 and children[0]['iters'] == n
        # Frozen tonic aliases intentionally invoke the same original tonic helper.
        expected_child = row.replace('rpc.tonic_prost.', 'rpc.tonic.')
        assert children[0]['cell'] == expected_child
        refs = re.findall(r'I\s+refs:\s*([\d,]+)', record['stderr'])
        assert len(refs) == 1
        groups[row].append((n, int(refs[0].replace(',', '')), children[0], record))
    graphs = root / (revision + '-graphs')
    graphs.mkdir(exist_ok=True)
    for result in report['cells']:
        row = result['id']
        assert result['iters'] == 200 and result['repeats'] == 3
        assert result['instruction_method'] == 'differential_callgrind_2n_minus_n'
        rs = groups[row]
        assert [r[0] for r in rs] == [200, 400] * 3
        calculated = {
            'instructions': statistics.median((rs[i + 1][1] - rs[i][1]) / 200 for i in range(0, 6, 2)),
            'allocs': statistics.median(rs[i][2]['allocs'] / 200 for i in range(0, 6, 2)),
            'alloc_bytes': statistics.median(rs[i][2]['alloc_bytes'] / 200 for i in range(0, 6, 2)),
        }
        for key, value in calculated.items():
            assert result[key]['status'] == 'measured'
            assert math.isfinite(value) and value >= 0
            assert result[key]['data']['value'] == value
        for i, (n, _, _, raw) in enumerate(rs):
            pid = re.search(r'==([0-9]+)==', raw['stderr']).group(1)
            original = Path('/tmp/devloop-callgrind.' + pid)
            assert original.is_file()
            shutil.copyfile(original, graphs / f'{row}.{n}.repeat{i // 2}.callgrind')
    reports[revision] = {r['id']: r for r in report['cells']}
assert reports['before'].keys() == reports['after'].keys()
for row in expected:
    before, after = reports['before'][row], reports['after'][row]
    limit = 0.02 if after['kind'] == 'rpc' else 0.01
    output = {'id': row, 'metrics': {}, 'wall': 'noisy shared-host diagnostic only'}
    for key in ['instructions', 'allocs', 'alloc_bytes', 'syscalls', 'locks']:
        if before[key]['status'] != 'measured' or after[key]['status'] != 'measured':
            output['metrics'][key] = {'status': 'not_run'}
            continue
        b, a = before[key]['data']['value'], after[key]['data']['value']
        delta = (a - b) / b if b else None
        output['metrics'][key] = {'status': 'measured', 'before': b, 'after': a, 'relative_delta': delta}
        if delta is not None and delta > limit:
            summary['primary_regressions' if key in ['instructions', 'allocs', 'alloc_bytes'] else 'diagnostic_regressions'].append({'row': row, 'metric': key, 'delta': delta, 'original_limit': limit})
    summary['rows'].append(output)
summary['targets'] = []
for shape in ['unary', 'server_stream']:
    row = 'rpc.prost.' + shape
    native = reports['after'][row]
    tonic = reports['after']['rpc.tonic_prost.' + shape]
    b, a = reports['before'][row]['allocs']['data']['value'], native['allocs']['data']['value']
    summary['targets'].append({'row': row, 'allocation_improvement_at_least_2_percent': (b - a) / b >= 0.02,
        'bytes_at_or_below_tonic': native['alloc_bytes']['data']['value'] <= tonic['alloc_bytes']['data']['value']})
Path('work/profiles/cl08-callgrind-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps({k: v for k, v in summary.items() if k not in ['rows', 'scope']}, indent=2))
