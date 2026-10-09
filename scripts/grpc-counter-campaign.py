#!/usr/bin/env python3
"""Capture the basic client/server counter matrix from one verified benchmark."""
import argparse
import gzip
import hashlib
import importlib.util
import itertools
import json
import os
from pathlib import Path
import platform
import random
import shutil
import subprocess
from types import SimpleNamespace

PAIRS = [
    ('native', 'pbrs', 'tonic', 'prost'),
    ('native', 'prost', 'tonic', 'prost'),
    ('tonic', 'prost', 'native', 'pbrs'),
    ('tonic', 'prost', 'native', 'prost'),
    ('tonic', 'prost', 'tonic', 'prost'),
]
SHAPES = ['unary', 'server_stream', 'client_stream', 'bidi', 'bidi_pipelined']


def digest(path):
    with path.open('rb') as stream:
        sha = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            sha.update(chunk)
    return sha.hexdigest()


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def levels(text):
    result = []
    for value in text.split(','):
        parts = value.split(':')
        if len(parts) != 2:
            raise ValueError('load levels must be connections:total-in-flight')
        connections, in_flight = map(int, parts)
        if not 1 <= connections <= 64 or not connections <= in_flight <= 1024:
            raise ValueError('load outside 1..64 connections and 1..1024 total calls')
        pair = (connections, in_flight)
        if pair in result:
            raise ValueError('duplicate load level')
        result.append(pair)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-checkout', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--build-record', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--phase', choices=['native', 'callgrind', 'syscalls'], required=True)
    parser.add_argument('--valgrind', type=Path)
    parser.add_argument('--strace', type=Path)
    parser.add_argument('--payloads', default='0,1024,65536,1048576')
    parser.add_argument('--load-levels', default='1:1,1:16,64:1024')
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--count', type=int, default=200)
    parser.add_argument('--outer-timeout', type=float, default=60)
    parser.add_argument('--seed', type=int, default=20261009)
    args = parser.parse_args()
    try:
        payloads = [int(p) for p in args.payloads.split(',')]
        loads = levels(args.load_levels)
        if (not payloads or len(set(payloads)) != len(payloads)
                or any(p not in (0, 1024, 65536, 1048576) for p in payloads)
                or not 1 <= args.repeats <= 10 or not 1 <= args.count <= 100000
                or not 0.01 <= args.outer_timeout <= 60):
            raise ValueError('invalid payloads, repeats, count or timeout')
        if args.phase == 'callgrind' and args.valgrind is None:
            raise ValueError('Callgrind needs --valgrind')
        if args.phase == 'syscalls' and args.strace is None:
            raise ValueError('syscalls need --strace')
    except ValueError as error:
        parser.error(str(error))
    source = args.source_checkout.resolve(strict=True)
    binary = args.binary.resolve(strict=True)
    build_record = args.build_record.resolve(strict=True)
    smoke_path = source / 'scripts/grpc-load-smoke.py'
    smoke = load_module('counter_smoke', smoke_path)
    ledger = load_module('counter_ledger', source / 'scripts/dominance-ledger.py')
    build = smoke.verified_build(build_record, binary, source)
    # The build verifier requires a release build with allocation-counts.
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    controller = output / 'controller.py'
    shutil.copy2(__file__, controller)
    binary_hash = digest(binary)
    harness_hash = digest(smoke_path)
    state = dict(schema='pbrs.counter-campaign.v1', source=build['source']['commit'],
                 binary_sha256=binary_hash, controller_sha256=digest(controller),
                 phase=args.phase, qualified=False, state='running', passed=False, groups=[],
                 seed=args.seed, payloads=payloads, load_levels=loads, repeats=args.repeats,
                 expected_groups=len(payloads) * 4 * len(loads),
                 expected_captures=len(payloads) * 4 * len(loads) * 2 * len(PAIRS) * len(SHAPES) * args.repeats,
                 limits=['shared-host diagnostics; wall time is not qualified',
                         'basic synthetic payloads; read-all corpora and lifecycle cells remain separate',
                         'no task wake counters or measured scheduling noise',
                         'per-RPC deadlines remain the benchmark defaults'])

    def save():
        temporary = output / 'campaign.json.tmp'
        temporary.write_text(json.dumps(state, indent=2) + '\n')
        temporary.replace(output / 'campaign.json')

    save()
    try:
        strace_version = None
        if args.phase == 'syscalls':
            strace_version = subprocess.check_output([str(args.strace), '--version'], text=True).splitlines()[0]
        groups = list(itertools.product(payloads, [False, True], ['identity', 'gzip'], loads))
        random.Random(args.seed).shuffle(groups)
        for group_index, (payload, tls, compression, (connections, in_flight)) in enumerate(groups):
            group_path = output / f'group-{group_index:03d}'
            group_path.mkdir()
            # Every declared concurrent slot can receive a request in the N run.
            n = max(args.count, in_flight)
            cells = [dict(pair=list(pair), shape=shape, payload_bytes=payload, tls=tls,
                          compression=compression, connections=connections,
                          in_flight=in_flight, repeat=repeat)
                     for pair, shape, repeat in itertools.product(PAIRS, SHAPES, range(args.repeats))]
            random.Random(args.seed + group_index).shuffle(cells)
            row = dict(path=group_path.name, payload_bytes=payload, tls=tls,
                       compression=compression, connections=connections,
                       in_flight=in_flight, rpc_counts=[n, 2 * n], captures=[])
            state['groups'].append(row)
            save()
            for count in (n, 2 * n):
                directory = group_path / f'n-{count}'
                directory.mkdir()
                record = dict(schema='pbrs.load-smoke.v2', binary_sha256=binary_hash,
                              harness_script_sha256=harness_hash,
                              campaign_controller_sha256=state['controller_sha256'],
                              harness_head=build['source']['commit'], harness_dirty=False,
                              head=build['source']['commit'], dirty=False, source_verified=True,
                              build_record=build, host=platform.uname()._asdict(),
                              cpu_affinity=sorted(os.sched_getaffinity(0)), seed=args.seed,
                              cells=cells, rpc_count=count,
                              duration_per_cell_seconds=args.outer_timeout,
                              allocation_counts=True, context_switches=args.phase == 'native',
                              callgrind=str(args.valgrind) if args.phase == 'callgrind' else None,
                              strace=str(args.strace) if args.phase == 'syscalls' else None,
                              strace_version=strace_version, qualification={'qualified': False}, runs=[])
                (directory / 'manifest.json').write_text(json.dumps(record, indent=2) + '\n')
                shutil.copy2(smoke_path, directory / 'harness.py')
                settings = SimpleNamespace(binary=binary, duration=args.outer_timeout,
                                           rpc_count=count, allocation_counts=True,
                                           context_switches=record['context_switches'],
                                           callgrind=args.valgrind if args.phase == 'callgrind' else None,
                                           strace=args.strace if args.phase == 'syscalls' else None)
                for index, cell in enumerate(cells):
                    child = directory / f'cell-{index:03d}'
                    result = smoke.run_cell(settings, cell, child)
                    record['runs'].append(dict(path=f'{child.name}/run.json', passed=result['passed']))
                    (directory / 'partial-report.json').write_text(json.dumps(record, indent=2) + '\n')
                    for profile in child.glob('*.callgrind'):
                        with profile.open('rb') as original, gzip.open(str(profile) + '.gz', 'wb') as zipped:
                            shutil.copyfileobj(original, zipped)
                        profile.unlink()
                    print(f'{group_path.name} N={count}: {index + 1}/{len(cells)} {result["passed"]}', flush=True)
                smoke.verified_build(build_record, binary, source)
                record['binary_unchanged'] = digest(binary) == binary_hash
                record['source_unchanged'] = True
                record['harness_script_unchanged'] = digest(smoke_path) == harness_hash
                record['campaign_controller_unchanged'] = digest(Path(__file__)) == state['controller_sha256']
                record['harness_script_unchanged'] &= record['campaign_controller_unchanged']
                record['passed'] = (record['binary_unchanged'] and record['harness_script_unchanged']
                                    and record['campaign_controller_unchanged']
                                    and all(run['passed'] for run in record['runs']))
                (directory / 'report.json').write_text(json.dumps(record, indent=2) + '\n')
                row['captures'].append(dict(path=directory.name, passed=record['passed'],
                                            total=len(cells), passed_cells=sum(r['passed'] for r in record['runs'])))
                save()
            compared = ledger.compare(group_path / f'n-{n}', group_path / f'n-{2 * n}', allow_failed=True)
            (group_path / 'ledger.json').write_text(json.dumps(compared, indent=2) + '\n')
            row['ledger_rows'] = len(compared['rows'])
            save()
    except BaseException as error:
        state.update(state='failed', passed=False, error=dict(type=type(error).__name__, message=str(error)))
        save()
        raise
    state['state'] = 'finished'
    state['passed'] = all(capture['passed'] for group in state['groups'] for capture in group['captures'])
    save()
    return 0 if state['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
