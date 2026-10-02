#!/usr/bin/env python3
"""CL08 allocator-only capture using unchanged source-pinned run-cell commands."""
import argparse
import hashlib
import json
import random
import statistics
import subprocess
import time
from pathlib import Path

BASE = 'cf3eee22c6b06324e299f81b76915651471d8025'
CANDIDATE = '2321d736c0e9ed1b3323d24d99b2cc437c594774'
BASE_HASH = '73fa59c733f0637b22ce8d58f035aa45cb13f0b7efc4c8e9bb4cdddcef54e0b9'
PILOT = {'query.d3.v0', 'query.d8.v7', 'entities.n1000', 'sparse.v9'}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def parse_line(stderr, prefix):
    records = [json.loads(line[len(prefix):]) for line in stderr.splitlines() if line.startswith(prefix)]
    if len(records) != 1:
        raise ValueError(f'expected one {prefix} record, got {len(records)}')
    return records[0]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--oracle', type=Path, required=True)
    parser.add_argument('--rows', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--pilot', action='store_true')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    oracle = {r['specimen']: r for r in json.loads(args.oracle.read_text())}
    if len(oracle) != 64:
        raise ValueError('incomplete 64-specimen oracle')
    rows = [r['id'] for r in json.loads(args.rows.read_text()) if r['state'] == 'eligible']
    if len(rows) != 244 or len(set(rows)) != 244:
        raise ValueError('incomplete or repeated eligible ledger rows')
    if args.pilot:
        rows = [row for row in rows if row.split('.', 3)[3].rsplit('.', 1)[0] in PILOT]
        if len(rows) != 16:
            raise ValueError('incomplete pilot')
    paths = {'before': args.before.resolve(), 'after': args.after.resolve()}
    hashes = {key: sha(path) for key, path in paths.items()}
    if hashes['before'] != BASE_HASH:
        raise ValueError('baseline binary differs from approved immutable cf3 executable')
    metadata = {
        'schema': 'cl08-ledger-allocator/1', 'sources': {'before': BASE, 'after': CANDIDATE},
        'binaries': {key: {'path': str(path), 'sha256': hashes[key]} for key, path in paths.items()},
        'oracle_sha256': sha(args.oracle), 'rows_sha256': sha(args.rows),
        'capture_sha256': sha(Path(__file__)), 'warmup': 100, 'prepare_iters': 128,
        'iters': [64] if args.pilot else [64, 128], 'repeats': 1 if args.pilot else 3,
        'instructions': {'status': 'not_run', 'reason': 'allocator-only unchanged run-cell path; no CPU claim'},
        'wall': 'shared-host noisy diagnostic; no comparative latency or throughput claim',
        'selected_rows': rows, 'blocked_maps': 12, 'completed': False,
    }
    (args.out / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    rng = random.Random(807)
    totals = []
    start = time.monotonic()
    with (args.out / 'raw.jsonl').open('w') as raw:
        for row in rows:
            profile, specimen_shape = row.removeprefix('rpc.adoption.').split('.', 1)
            specimen, shape = specimen_shape.rsplit('.', 1)
            expected = oracle[specimen]
            if expected['state'] != 'eligible' or not expected['full_decoded_equality'] or not expected['equal_wire_work']:
                raise ValueError('selected row bypasses global preflight')
            if not expected['input_wire_fingerprint'].startswith('fnv1a64:'):
                raise ValueError('selected row lacks source-bound input fingerprint')
            for n in metadata['iters']:
                for repeat in range(metadata['repeats']):
                    revisions = ['before', 'after']
                    rng.shuffle(revisions)
                    for revision in revisions:
                        path = paths[revision]
                        command = [str(path), 'run-cell', row, '--iters', str(n), '--prepare-iters', '128', '--warmup', '100']
                        record = {'revision': revision, 'source': metadata['sources'][revision], 'row': row,
                                  'profile': profile, 'shape': shape, 'iters': n, 'repeat': repeat,
                                  'command': command, 'oracle_input_wire_fingerprint': expected['input_wire_fingerprint']}
                        record['launch_sha256'] = sha(path)
                        began = time.monotonic()
                        try:
                            if record['launch_sha256'] != hashes[revision]:
                                raise ValueError('executable changed before launch')
                            result = subprocess.run(command, capture_output=True, text=True, timeout=180, check=False)
                            record.update(returncode=result.returncode, stdout=result.stdout, stderr=result.stderr)
                            record['after_sha256'] = sha(path)
                            if record['after_sha256'] != record['launch_sha256']:
                                raise ValueError('executable changed during launch')
                            if result.returncode:
                                raise ValueError(f'child exited {result.returncode}')
                            child = parse_line(result.stderr, '__CHILD__ ')
                            qualification = parse_line(result.stderr, '__QUALIFICATION__ ')
                            want = {'request_bytes': expected['request_bytes'], 'response_bytes': expected['response_bytes_per_message'],
                                    'checksum': expected['request_read_checksum'], 'stream_replies': expected['stream_responses']}
                            if qualification != want or child['cell'] != row or child['iters'] != n:
                                raise ValueError('wrong qualification, row or N')
                            if any(type(child[key]) is not int or child[key] < 0 for key in ['allocs', 'alloc_bytes', 'wall_ns']):
                                raise ValueError('invalid child counters')
                            record.update(child=child, qualification=qualification, validated=True)
                        except subprocess.TimeoutExpired as error:
                            record.update(validated=False, error=str(error), returncode=None,
                                stdout=(error.stdout or b'').decode('utf-8', errors='replace'),
                                stderr=(error.stderr or b'').decode('utf-8', errors='replace'))
                        except Exception as error:
                            record.update(validated=False, error=str(error))
                        record['elapsed_seconds_diagnostic'] = time.monotonic() - began
                        raw.write(json.dumps(record) + '\n')
                        raw.flush()
                        if not record['validated']:
                            raise ValueError(record['error'])
                        totals.append(record)
            print(f'completed {row}', flush=True)
    summaries = []
    for row in rows:
        for n in metadata['iters']:
            for revision in ['before', 'after']:
                records = [r for r in totals if r['row'] == row and r['iters'] == n and r['revision'] == revision]
                summaries.append({'row': row, 'iters': n, 'revision': revision, 'repeats': len(records),
                    'allocs_per_rpc': statistics.median(r['child']['allocs'] / n for r in records),
                    'allocated_bytes_per_rpc': statistics.median(r['child']['alloc_bytes'] / n for r in records),
                    'wall_ns_per_rpc_diagnostic': statistics.median(r['child']['wall_ns'] / n for r in records),
                    'elapsed_seconds_diagnostic': sum(r['elapsed_seconds_diagnostic'] for r in records)})
    metadata.update(completed=True, raw_children=len(totals), elapsed_seconds_diagnostic=time.monotonic() - start,
                    summaries=summaries, raw_sha256=sha(args.out / 'raw.jsonl'))
    (args.out / 'report.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print(f'validated {len(totals)} raw children in {metadata["elapsed_seconds_diagnostic"]:.3f}s', flush=True)


if __name__ == '__main__':
    main()
