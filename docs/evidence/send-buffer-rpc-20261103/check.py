#!/usr/bin/env python3
"""Check retained send-loop RPC diagnostics against Git and raw captures."""
import hashlib
import io
import itertools
import json
import math
from pathlib import Path, PurePosixPath
import random
import statistics
import subprocess
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = next(path for path in HERE.parents if (path / '.git').exists())
COMMIT = '5d280bfbced620bc63a761741fcac06b3c8a359d'
OLD = 'a35701582fb9e6964c855386b0d9c991c50345ba'
SEND = 'pbrs-grpc/src/wire/send.rs'
PATHS = ['Cargo.toml', 'Cargo.lock', 'build.rs', 'README.md', 'LICENSE-MIT', 'LICENSE-APACHE',
         'src', 'pbrs-grpc', 'protobuf-tonic', 'rpc-bench', 'vendor/google', 'examples', 'proto']
CAPTURE_FILES = {'client.stderr', 'client.stdout', 'server.stderr', 'server.stdout', 'metrics.json', 'run.json'}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def safe(name):
    path = PurePosixPath(name)
    assert not path.is_absolute() and '..' not in path.parts and str(path) == name, name
    return name


def parse(files, name):
    return json.loads(files[safe(name)])


def archive_files():
    manifest = json.loads((HERE / 'manifest.json').read_text())
    assert manifest['schema'] == 'pbrs.split-rpc-evidence.v1'
    assert manifest['source_commit'] == COMMIT and manifest['qualified'] is False
    assert sha((HERE / 'check.py').read_bytes()) == manifest['checker_sha256']
    assert sha((HERE / 'README.md').read_bytes()) == manifest['readme_sha256']
    data = (HERE / 'raw.tar.gz').read_bytes()
    assert sha(data) == manifest['archive_sha256'], 'archive digest differs'
    files = {}
    total = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
        for entry in archive.getmembers():
            name = safe(entry.name)
            assert entry.isfile() and name not in files and 0 <= entry.size < 8 * 1024 * 1024, name
            total += entry.size
            assert total < 128 * 1024 * 1024, 'archive too large'
            files[name] = archive.extractfile(entry).read()
    assert set(files) == set(manifest['files']), 'archive members differ'
    for name, raw in files.items():
        assert manifest['files'][name] == {'bytes': len(raw), 'sha256': sha(raw)}, name
    return manifest, files


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)


def source_checks(files):
    source = parse(files, 'source-manifest.json')
    assert source['commit'] == COMMIT and source['old_send_commit'] == OLD
    assert source['paths'] == PATHS and source['only_difference'] == SEND
    assert source['source_file_count'] == 596
    source_archive = git('archive', '--format=tar', COMMIT, '--', *PATHS)
    old_send = git('show', OLD + ':' + SEND)
    with tempfile.TemporaryDirectory(prefix='pbrs-send-loop-source-') as tmp:
        dest = Path(tmp)
        # Match prepare.py: Git archive can contain symlinks. The data filter
        # rejects links escaping this temporary directory before extraction.
        with tarfile.open(fileobj=io.BytesIO(source_archive), mode='r:') as tar:
            tar.extractall(dest, filter='data')
        def snapshot():
            return {str(p.relative_to(dest)): sha(p.read_bytes()) for p in sorted(dest.rglob('*')) if p.is_file()}
        after = snapshot()
        assert b'frame.split_to(n)' in (dest / SEND).read_bytes()
        assert b'frame.slice(..n)' in old_send
        (dest / SEND).write_bytes(old_send)
        before = snapshot()
    assert before == source['source_file_sha256_before']
    assert after == source['source_file_sha256_after']
    assert len(before) == len(after) == 596 and before.keys() == after.keys()
    assert {name for name in before if before[name] != after[name]} == {SEND}
    return {'before': before, 'after': after}


def build_checks(files, sources):
    digests = {}
    for side in ('before', 'after'):
        prefix = 'built/' + side + '/'
        record = parse(files, prefix + 'rpc-shipping-build.json')
        aggregate = parse(files, prefix + 'source.json')
        assert record['schema'] == 'pbrs.frozen-probe-build.v1'
        assert record['exit_code'] == 0 and record['source_unchanged'] is True and record['harness_unchanged'] is True
        assert record['source_files'] == sources[side] and record['mode'] == 'shipping'
        assert record['features'] == ['default'] and record['message_storage'] == 'lazy'
        command = record['command']
        assert command[:4] == ['cargo', 'build', '--locked', '--offline']
        assert '--release' in command and '--features' not in command and '--message-format=json' in command
        assert record['cargo_lock_sha256'] == sources[side]['rpc-bench/Cargo.lock']
        expected_harness = {p.removeprefix('rpc-bench/'): h for p, h in sources[side].items()
                            if p.startswith('rpc-bench/') and p != 'rpc-bench/Cargo.lock'}
        assert record['harness_files'] == expected_harness
        for name, digest in record['logs'].items():
            assert sha(files[prefix + safe(name)]) == digest
        messages = [json.loads(line) for line in files[prefix + 'rpc-shipping.jsonl'].splitlines()]
        artifacts = [m for m in messages if m.get('reason') == 'compiler-artifact'
                     and m.get('target', {}).get('name') == 'rpc-bench' and m.get('executable')]
        assert len(artifacts) == 1 and artifacts[0]['executable'] == record['build_executable']
        assert artifacts[0]['features'] == record['features']
        assert artifacts[0]['profile']['opt_level'] == '3' and not artifacts[0]['profile']['debug_assertions']
        binary = aggregate['binaries']['rpc-shipping']
        assert binary['path'] == record['binary'] and binary['build_record'] == 'rpc-shipping-build.json'
        assert binary['sha256'] == record['binary_sha256']
        assert len(binary['sha256']) == 64
        digests[side] = binary['sha256']
    return digests


def execution_order(plan):
    rng = random.Random(plan['seed'])
    groups = list(itertools.product(range(plan['repetitions']['shipping']), plan['cells']))
    rng.shuffle(groups)
    result = []
    for repeat, cell in groups:
        pairs = list(itertools.product(plan['pairs'].items(), cell['counts']))
        rng.shuffle(pairs)
        for (name, pair), count in pairs:
            versions = ['before', 'after']
            rng.shuffle(versions)
            result.extend(dict(repeat=repeat, cell_name=cell['name'], cell=cell, pair_name=name,
                               pair=pair, count=count, source=side) for side in versions)
    return result


def approximate(left, right, path='result'):
    if isinstance(left, dict) and isinstance(right, dict):
        assert left.keys() == right.keys(), path
        for key in left:
            approximate(left[key], right[key], path + '.' + key)
    elif isinstance(left, list) and isinstance(right, list):
        assert len(left) == len(right), path
        for index, (a, b) in enumerate(zip(left, right)):
            approximate(a, b, f'{path}[{index}]')
    elif type(left) in (int, float) and type(right) in (int, float):
        assert math.isfinite(left) and math.isfinite(right) and math.isclose(left, right, rel_tol=1e-12, abs_tol=1e-9), path
    else:
        assert left == right, path


def verify_summary(plan, records, captured):
    def interval(values):
        return {'median': statistics.median(values), 'range': [min(values), max(values)], 'pairs': values}
    rows = []
    repeats = plan['repetitions']['shipping']
    for cell in plan['cells']:
        n, nn = cell['counts']
        for native_pair, side in [('native_client', 'client'), ('native_server', 'server')]:
            slopes = {s: {peer: [] for peer in ('native', 'tonic')} for s in ('before', 'after')}
            key = 'client_lifetime_cpu_seconds' if side == 'client' else 'server_cpu_during_client_seconds'
            for source, repeat in itertools.product(('before', 'after'), range(repeats)):
                for peer, pair in [('native', native_pair), ('tonic', 'control')]:
                    low = records[(source, repeat, cell['name'], pair, n)][key] * 1e6
                    high = records[(source, repeat, cell['name'], pair, nn)][key] * 1e6
                    slopes[source][peer].append((high - low) / (nn - n))
            positive = all(x > 0 for source in slopes.values() for values in source.values() for x in values)
            ratios = None
            if positive:
                ratios = {k: interval(v) for k, v in {
                    'after_over_before_native': [a / b for a, b in zip(slopes['after']['native'], slopes['before']['native'])],
                    'after_native_over_tonic': [a / b for a, b in zip(slopes['after']['native'], slopes['after']['tonic'])],
                    'control_after_over_before': [a / b for a, b in zip(slopes['after']['tonic'], slopes['before']['tonic'])],
                }.items()}
            rows.append({'cell': cell['name'], 'endpoint': side, 'metric': 'cpu_us_per_rpc', 'repeats': repeats,
                         'all_slopes_positive': positive, 'paired_slopes': slopes,
                         'medians': {s: {p: statistics.median(v) for p, v in peers.items()} for s, peers in slopes.items()},
                         'ratios': ratios})
    approximate(rows, captured, 'summary')
    return rows


def flag(command, expected):
    assert expected in command, (expected, command)


def campaign(files, name, plan_name, digests, expected_captures, expected_calls):
    prefix = name + '/'
    manifest = parse(files, prefix + 'manifest.json')
    plan = parse(files, prefix + 'plan.json')
    assert plan == parse(files, plan_name) == manifest['plan']
    assert manifest['schema'] == 'pbrs.compressed-rpc-comparison.v1' and manifest['mode'] == 'shipping'
    assert manifest['qualified'] is False and manifest['state'] == 'complete' and manifest['passed'] is True
    assert manifest['binaries'] == digests
    assert sha(files[prefix + 'controller.py']) == manifest['controller_sha256']
    assert sha(files[prefix + 'helper.py']) == manifest['helper_sha256']
    for side in ('before', 'after'):
        assert manifest['build_records'][side] == parse(files, 'built/' + side + '/source.json')
    order = execution_order(plan)
    assert order == manifest['execution_order']
    assert len(order) == len(manifest['runs']) == manifest['captures'] == expected_captures
    assert manifest['successful_rpcs'] == expected_calls
    records, total, tick_rates = {}, 0, []
    actual_capture_files = {n for n in files if n.startswith(prefix + 'capture-')}
    expected_files = {f'{prefix}capture-{i:03d}/{part}' for i in range(len(order)) for part in CAPTURE_FILES}
    assert actual_capture_files == expected_files, 'capture member set differs'
    for i, (entry, row) in enumerate(zip(order, manifest['runs'])):
        path = f'capture-{i:03d}/run.json'
        assert row == dict(source=entry['source'], repeat=entry['repeat'], name=entry['cell_name'],
                           pair=entry['pair_name'], count=entry['count'], path=path, passed=True)
        run = parse(files, prefix + path)
        assert run['passed'] is True and run['client_exit_code'] == 0 and run['server_exit_before_teardown'] is None
        assert run['environment'] == {'TOKIO_WORKER_THREADS': '2'}
        cell = {k: v for k, v in entry['cell'].items() if k not in ('name', 'counts')}
        cell['pair'] = entry['pair']
        assert run['cell'] == cell and not cell['tls'] and cell['compression'] == 'identity'
        metrics = parse(files, prefix + f'capture-{i:03d}/metrics.json')
        assert run['metrics'] == metrics
        for field in ('offered_rpcs', 'attempted_rpcs', 'dispatched_rpcs', 'completed_rpcs', 'successful_rpcs'):
            assert metrics[field] == entry['count'], (name, i, field)
        for field in ('failed_rpcs', 'timeouts', 'queue_overflows', 'unstarted_rpcs', 'unfinished_rpcs'):
            assert metrics[field] == 0, (name, i, field)
        assert metrics['status_errors'] == {} and metrics['payload_pattern'] == 'zero'
        assert metrics['payload_validation'] == 'length-only'
        assert metrics['payload_request_bytes'] == metrics['payload_response_bytes'] == cell['payload_bytes']
        client, server = run['commands']['client'], run['commands']['server']
        binary = parse(files, 'built/' + entry['source'] + '/rpc-shipping-build.json')['binary']
        assert client[0] == server[0] == binary and client[1] == 'load' and server[1] == 'load-server'
        flag(client, '--transport=' + entry['pair'][0]); flag(client, '--codec=' + entry['pair'][1])
        flag(server, '--transport=' + entry['pair'][2]); flag(server, '--codec=' + entry['pair'][3])
        for command in (client, server):
            for option in ('--compression=identity', '--payload-pattern=zero', '--payload-seed=260027',
                           '--req-bytes=' + str(cell['payload_bytes']), '--resp-bytes=' + str(cell['payload_bytes']),
                           '--max-message-size=' + str(cell['max_message_size'])):
                flag(command, option)
        for option in ('--shape=unary', '--connections=1', '--max-in-flight=1', '--duration-secs=60',
                       '--rpc-count=' + str(entry['count'])):
            flag(client, option)
        assert any(x.startswith('--server_addr=127.0.0.1:') for x in client)
        samples = run['resource_samples']
        assert samples and samples[-1]['client']['state'] == 'Z'
        first_server, last = run['server_cpu_before'], samples[-1]
        assert first_server['pid'] == last['server']['pid']
        for process in ('client', 'server'):
            values = [sample[process] for sample in samples]
            assert len({x['pid'] for x in values}) == 1
            for previous, current in zip(values, values[1:]):
                assert current['monotonic_ns'] >= previous['monotonic_ns']
                assert current['user_ticks'] >= previous['user_ticks']
                assert current['system_ticks'] >= previous['system_ticks']
        client_ticks = last['client']['user_ticks'] + last['client']['system_ticks']
        server_ticks = sum(last['server'][key] - first_server[key] for key in ('user_ticks', 'system_ticks'))
        for ticks, seconds in ((client_ticks, run['client_lifetime_cpu_seconds']),
                               (server_ticks, run['server_cpu_during_client_seconds'])):
            assert ticks >= 0 and seconds >= 0 and math.isfinite(seconds)
            if ticks:
                assert seconds > 0
                tick_rates.append(ticks / seconds)
            else:
                assert seconds == 0
        key = (row['source'], row['repeat'], row['name'], row['pair'], row['count'])
        assert key not in records
        records[key] = run
        total += metrics['successful_rpcs']
    assert total == expected_calls and tick_rates
    inferred = statistics.median(tick_rates)
    assert all(math.isclose(rate, inferred, rel_tol=1e-12) for rate in tick_rates)
    rows = verify_summary(plan, records, parse(files, prefix + 'summary.json'))
    return {'captures': len(order), 'successful_rpcs': total, 'summary_rows': len(rows),
            'implied_ticks_per_second': inferred}


def main():
    manifest, files = archive_files()
    sources = source_checks(files)
    binaries = build_checks(files, sources)
    results = campaign(files, 'results', 'plan.json', binaries, 72, 64800)
    longer = campaign(files, 'long-results', 'long-plan.json', binaries, 60, 45000)
    assert manifest['captures'] == 132 and manifest['successful_rpcs'] == 109800
    print(json.dumps({'source_commit': COMMIT, 'source_files_per_side': 596, 'archive_files': len(files),
                      'initial': results, 'longer': longer, 'qualified': False}))


if __name__ == '__main__':
    main()
