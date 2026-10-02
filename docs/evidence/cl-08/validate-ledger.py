#!/usr/bin/env python3
"""Independent CL08 raw allocator evidence validation and exact parity accounting."""
import hashlib
import json
import statistics
from pathlib import Path

root = Path('work/profiles/allocator-ledger')
report = json.loads((root / 'report.json').read_text())
oracle = {r['specimen']: r for r in json.loads(Path('work/qualification/inputs-first.json').read_text())}
rows = {r['id'] for r in json.loads(Path('work/cl08-ledger-rows.json').read_text()) if r['state'] == 'eligible'}
assert len(rows) == 244 and set(report['selected_rows']) == rows
raw_path = root / 'raw.jsonl'
assert hashlib.sha256(raw_path.read_bytes()).hexdigest() == report['raw_sha256']
records = [json.loads(line) for line in raw_path.read_text().splitlines()]
assert len(records) == 2928 and report['raw_children'] == len(records)
seen = set()
for record in records:
    row, revision, n, repeat = (record[k] for k in ['row', 'revision', 'iters', 'repeat'])
    key = (row, revision, n, repeat)
    assert key not in seen
    seen.add(key)
    assert row in rows and revision in ['before', 'after'] and n in [64, 128] and repeat in range(3)
    assert record['validated'] and record['returncode'] == 0
    assert record['launch_sha256'] == record['after_sha256'] == report['binaries'][revision]['sha256']
    assert record['source'] == report['sources'][revision]
    assert record['command'] == [report['binaries'][revision]['path'], 'run-cell', row, '--iters', str(n), '--prepare-iters', '128', '--warmup', '100']
    _, _, profile, *parts = row.split('.')
    specimen, shape = '.'.join(parts[:-1]), parts[-1]
    expected = oracle[specimen]
    assert record['profile'] == profile and record['shape'] == shape
    assert expected['state'] == 'eligible' and expected['equal_wire_work'] and expected['full_decoded_equality']
    assert record['oracle_input_wire_fingerprint'] == expected['input_wire_fingerprint']
    actual = [json.loads(line.removeprefix('__QUALIFICATION__ ')) for line in record['stderr'].splitlines() if line.startswith('__QUALIFICATION__ ')]
    assert actual == [{'request_bytes': expected['request_bytes'], 'response_bytes': expected['response_bytes_per_message'], 'checksum': expected['request_read_checksum'], 'stream_replies': 4}]
    children = [json.loads(line.removeprefix('__CHILD__ ')) for line in record['stderr'].splitlines() if line.startswith('__CHILD__ ')]
    assert len(children) == 1 and children[0] == record['child']
    assert children[0]['cell'] == row and children[0]['iters'] == n
assert len(seen) == len(rows) * 2 * 2 * 3
index = {}
for row in rows:
    for n in [64, 128]:
        for revision in ['before', 'after']:
            children = [r['child'] for r in records if (r['row'], r['iters'], r['revision']) == (row, n, revision)]
            index[row, n, revision] = {
                'allocs': statistics.median(r['allocs'] / n for r in children),
                'bytes': statistics.median(r['alloc_bytes'] / n for r in children),
            }
comparisons = []
for row in sorted(rows):
    if '.native_prost.' not in row:
        continue
    tonic_row = row.replace('.native_prost.', '.tonic_prost.')
    for n in [64, 128]:
        before = index[row, n, 'before']
        after = index[row, n, 'after']
        tonic = index[tonic_row, n, 'after']
        comparisons.append({'row': row, 'iters': n, 'before': before, 'after': after, 'tonic_after': tonic,
            'bytes_at_or_below_tonic': after['bytes'] <= tonic['bytes'],
            'no_allocation_count_regression': after['allocs'] <= before['allocs'],
            'bytes_saved_from_before': before['bytes'] - after['bytes'],
            'allocs_saved_from_before': before['allocs'] - after['allocs']})
summary = {
    'schema': 'cl08-allocator-summary/1', 'raw_children': len(records), 'eligible_native_rows': 122,
    'blocked_native_map_rows': 6, 'blocked_total_profile_rows': 12,
    'instructions': {'status': 'not_run', 'reason': 'allocator-only ledger; no per-ledger CPU claim'},
    'by_iters': {str(n): {
        'at_or_below_tonic_bytes': sum(r['bytes_at_or_below_tonic'] for r in comparisons if r['iters'] == n),
        'above_tonic_bytes': sum(not r['bytes_at_or_below_tonic'] for r in comparisons if r['iters'] == n),
        'allocation_count_regressions': sum(not r['no_allocation_count_regression'] for r in comparisons if r['iters'] == n),
        'lower_bytes_than_own_baseline': sum(r['bytes_saved_from_before'] > 0 for r in comparisons if r['iters'] == n),
        'lower_allocs_than_own_baseline': sum(r['allocs_saved_from_before'] > 0 for r in comparisons if r['iters'] == n),
    } for n in [64, 128]},
    'comparisons': comparisons,
}
Path('work/profiles/cl08-allocator-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps({k: v for k, v in summary.items() if k != 'comparisons'}, indent=2))
