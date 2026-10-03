#!/usr/bin/env python3
"""Read-only capsule audit; optional local payload readback, never extraction."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import stat
import tarfile
import tomllib

HERE = Path(__file__).resolve().parent
BOUND = 2147483648


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as file:
        for data in iter(lambda: file.read(1048576), b''):
            value.update(data)
    return value.hexdigest()


def payload_check(path, manifest, oracle):
    require(digest(path) == oracle['archive_sha256'], 'local archive hash')
    rows = {('cache' if name == '.' else 'cache/' + name): row for name, row in manifest.items()}
    visited, contents, inodes = set(), {}, {}
    with tarfile.open(path, 'r|gz') as archive:
        for member in archive:
            require(member.name in rows and member.name not in visited, 'payload member set')
            row = rows[member.name]
            identity = row['identity']
            require(member.mode == stat.S_IMODE(identity['st_mode']), 'payload mode')
            require((member.uid, member.gid) == (identity['st_uid'], identity['st_gid']), 'payload ownership')
            if row['kind'] == 'directory':
                require(member.isdir(), 'directory type')
            elif row['kind'] == 'symlink':
                require(member.issym() and member.linkname == row['link'], 'symlink metadata')
            else:
                key = identity['st_dev'], identity['st_ino']
                if key in inodes:
                    require(member.islnk() and member.linkname == inodes[key], 'hardlink topology')
                    value = contents[member.linkname]
                else:
                    require(member.isfile() and member.size == identity['st_size'], 'regular type/size')
                    inodes[key] = member.name
                    value = hashlib.sha256()
                    stream = archive.extractfile(member)
                    for chunk in iter(lambda: stream.read(1048576), b''):
                        value.update(chunk)
                    value = value.hexdigest()
                require(value == row['sha256'], 'payload bytes')
                contents[member.name] = value
            visited.add(member.name)
    require(visited == set(rows), 'missing payload members')
    return {'members': len(visited), 'regular_paths': len(contents), 'content_mode_ownership_links': 'pass'}


def audit_retirements(index, blobs, payload_root):
    results = {}
    obj = lambda name: json.loads(blobs[name])
    for stage in index['retirements']:
        prefix = stage['prefix']
        record, manifest = obj(prefix + 'record.json'), obj(prefix + 'manifest.json')
        require(record['status'] == 'verified_retired' and record['retired'] and record['exit'] == 0, 'retirement result')
        require(record['cache'] == stage['cache'] and record['archive'] == stage['archive'], 'stage identity')
        require(hashlib.sha256(blobs[prefix + 'manifest.json']).hexdigest() == record['manifest_sha256'], 'manifest hash')
        require(not obj(prefix + 'hash-only-paths.json'), 'full payload disposition')
        require(all(row['disposition'] == 'archived' for row in manifest.values()), 'all paths preserved')
        removed = obj(prefix + 'removed-paths.json')
        require(len(removed) == len(manifest) and set(removed) == set(manifest), 'exact removed path set')
        before, after = obj(prefix + 'source-before.json'), obj(prefix + 'source-after.json')
        require(before == after and not before['status'], 'isolated source before/after identity')
        require(obj(prefix + 'fresh-before-retirement-verification.json')['complete_path_inode_mode_hashes_pass'], 'fresh inode/hash check')
        for name in ('users-before-locks.json', 'users-after-locks.json', 'users-immediately-before-retirement.json'):
            require(not obj(prefix + name)['matches'], 'visible live cache user')
        locks = obj(prefix + 'held-locks.json')['locks']
        require(locks, 'at least one Cargo lock')
        expected_locks = {str(Path(record['cache']) / name) for name in manifest if Path(name).name == '.cargo-lock'}
        require({lock['path'] for lock in locks} == expected_locks and len(locks) == len(expected_locks), 'all Cargo lock paths')
        for lock in locks:
            if stage['lock_scope'] == 'dedicated OFD plus flock':
                require(lock['flock'] == 'exclusive nonblocking held on dedicated fd'
                        and lock['linux_ofd_write_lock']['command'] == 37
                        and lock['POSIX_lockf'] == 'not used', 'OFD lock record')
            else:
                require(lock['flock'] == 'exclusive nonblocking held'
                        and lock['fcntl'] == 'exclusive nonblocking held', 'historical acquisition record')
        samples = [json.loads(line) for line in blobs[prefix + 'resources.jsonl'].splitlines()]
        require(samples and all(row['free_bytes'] >= BOUND and row['owned_total'] <= BOUND for row in samples), 'retirement resource samples')
        require(record['observed_min_free'] == min(row['free_bytes'] for row in samples), 'recorded free minimum')
        require(record['observed_max_owned_cache'] == max(row['owned_total'] for row in samples), 'recorded cache maximum')
        files = [row for row in manifest.values() if row['kind'] == 'file']
        require(len(files) == record['regular_paths'], 'regular count')
        require(sum(row['identity']['st_size'] for row in files) == record['logical_namespace_bytes_with_aliases'], 'logical byte sum')
        require(record['post_retirement_archive_sha256'] == record['archive']['archive_sha256'], 'post-retirement archive hash')
        result = {'regular_paths': len(files), 'elf_paths': sum(row['elf'] for row in files),
                  'lock_scope': stage['lock_scope'], 'payload': 'indexed; not reread by this metadata-only invocation'}
        if payload_root:
            result['payload'] = payload_check(payload_root / stage['payload_relative'], manifest, stage['archive'])
        results[stage['stage']] = result
    return results


def audit_rx(index, blobs):
    summary = json.loads(blobs[index['summary_member']])
    require(summary['status'] == 'bounded_slice_qualified_parent_open', 'qualification scope')
    failures, tests = {}, {}
    for label in index['captures']:
        prefix = 'qualification/records/' + label
        meta = json.loads(blobs[prefix + '.meta.json'])
        require(meta['before'] == meta['after'] and meta['pins_unchanged'] and not meta['before']['status'], 'capture source/tool identity')
        require(meta['process_launched'] and not meta['stop_reason'] and not meta.get('helper_error'), 'genuine command capture')
        require(not meta['cleanup']['live_after_cleanup'], 'owned process cleanup')
        require(meta['env']['CARGO_BUILD_JOBS'] == '1' and meta['env']['CARGO_INCREMENTAL'] == '0', 'fixed compiler env')
        require(meta['exit'] == int(blobs[prefix + '.exit']), 'raw exit')
        for stream in ('stdout', 'stderr'):
            require(hashlib.sha256(blobs[prefix + '.' + stream + '.log']).hexdigest() == meta[stream + '_sha256'], 'raw stream hash')
        samples = [json.loads(line) for line in blobs[prefix + '.resources.jsonl'].splitlines()]
        require(samples and all(row['owned_total'] <= BOUND and row['global_free'] >= BOUND for row in samples), 'compiler resource samples')
        require(meta['observed_max_owned_total'] == max(row['owned_total'] for row in samples), 'compiler max sample')
        require(meta['observed_min_global_free'] == min(row['global_free'] for row in samples), 'compiler min sample')
        expected = summary['captures'][label]
        require((meta['before']['head'], meta['toolchain'], meta['exit'], meta['argv']) ==
                (expected['source'], expected['toolchain'], expected['exit'], expected['argv']), 'source-labelled command')
        if meta['exit']:
            failures[label] = expected
        match = re.findall(rb'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', blobs[prefix + '.stdout.log'], re.M)
        if match:
            require(len(match) == 1 and match[0][1:] == (b'0', b'0') and meta['exit'] == 0, 'test result')
            tests[label] = int(match[0][0])
    require(failures == summary['genuine_failed_captures'] and len(failures) == 7, 'all seven genuine reds retained')
    require(all(row['exit'] == 101 for row in failures.values()), 'genuine Cargo failure exits')
    require(sum(n for name, n in tests.items() if name.startswith('native185_')) == summary['native185_executions'] == 107, 'actual 1.85 executions')
    require(sum(tests[name] for name in ('native188_rpc_unit_001', 'native188_worker_002', 'native188_fairness_002')) == summary['registered_rpc_executions'] == 172, 'registered RPC executions')
    require(tests['bench_final_stable_units_001'] == summary['benchmark_unit_executions'] == 18, 'stable unit executions')
    symbols = blobs['qualification/records/native185_default_symbols_001.stdout.log']
    for symbol in (b'SchedulerCounts', b'SpawnSite', b'scheduler_state', b'scheduler_counts', b'reset_scheduler_counts', b'note_spawn'):
        require(symbol not in symbols, 'default scheduler symbol absence')
    source = lambda version, path: blobs['sources/' + version + '/' + path]
    for path in ('server/connection.rs', 'client/pool.rs', 'client/streaming.rs', 'wire/send.rs'):
        path = 'pbrs-grpc/src/' + path
        stripped, count = re.subn(rb'[ \t]*#\[cfg\(feature = "copy-counts"\)\]\n[ \t]*crate::copy_counts::note_spawn\(crate::copy_counts::SpawnSite::[A-Za-z]+\);\n', b'', source('candidate', path))
        require(count == 1 and stripped == source('base', path) == source('baseline', path), 'unchanged spawn expression/source')
        require(source('format_only', path) == source('candidate', path), 'final runtime identity')
    extra = b'#[cfg(feature = "copy-counts")]\npub use copy_counts::{SchedulerCounts, reset_scheduler_counts, scheduler_counts};\n'
    lib = source('candidate', 'pbrs-grpc/src/lib.rs')
    require(lib.count(extra) == 1 and lib.replace(extra, b'') == source('base', 'pbrs-grpc/src/lib.rs'), 'cfg-only API addition')
    marker = b'/// Snapshot of the process-wide copy counters.'
    require(source('candidate', 'pbrs-grpc/src/copy_counts.rs').split(marker, 1)[1]
            == source('base', 'pbrs-grpc/src/copy_counts.rs').split(marker, 1)[1], 'existing CopyCounts unchanged')
    for path in ('pbrs-grpc/src/lib.rs', 'pbrs-grpc/src/copy_counts.rs'):
        require(source('candidate', path) == source('format_only', path), 'final diagnostic library source identity')
    main = source('candidate', 'bench/devloop/src/main.rs')
    require(main == source('baseline', 'bench/devloop/src/main.rs'), 'identical diagnostic command wiring')
    main = main.replace(b'mod scheduler_rpc;\n', b'').replace(b'        "scheduler-check" => scheduler_rpc::run(),\n', b'')
    main = main.replace(b'     \\x20 devloop sizes\\n\\\n     \\x20 devloop scheduler-check\\n"', b'     \\x20 devloop sizes\\n"')
    require(main == source('base', 'bench/devloop/src/main.rs'), 'original20 registry/worker/helper/comparator source recovered')
    for path in ('Cargo.toml','Cargo.lock','pbrs-grpc/Cargo.toml','bench/devloop/Cargo.toml','bench/devloop/Cargo.lock','rpc-bench/Cargo.toml'):
        require(source('base', path) == source('candidate', path) == source('format_only', path), 'unchanged manifest/provider lock')
    old_format = b'assert_eq!(wire, prost_wire, "pbrs/prost wire mismatch for {:?}", size);'
    new_format = b'assert_eq!(wire, prost_wire, "pbrs/prost wire mismatch for {size:?}");'
    old_blob, new_blob = source('generated_owner','bench/devloop/src/blob.rs'), source('format_only','bench/devloop/src/blob.rs')
    require(old_blob.count(old_format) == 1 and old_blob.replace(old_format, new_format) == new_blob, 'one format-only correction')
    locks = [tomllib.loads(source(version, 'rpc-bench/Cargo.lock').decode()) for version in ('base','seven_edges','inactive_h2_removed')]
    identify = lambda p: (p['name'], p['version'], p.get('source'), p.get('checksum'))
    packages = [{identify(p): p for p in lock['package']} for lock in locks]
    require(len(packages[0]) == 140 and set(packages[0]) == set(packages[1]) == set(packages[2]), 'all140 package tuples unchanged')
    require(sum(bool(p.get('source')) for p in packages[0].values()) == 136, 'all136 registry tuples unchanged')
    additions = {'atomic-waker','fnv','futures-sink','indexmap','slab','tokio-util','tracing'}
    for key in packages[0]:
        before, seven, final = [p[key] for p in packages]
        if key[0] == 'pbrs-grpc':
            a, b, c = [set(p['dependencies']) for p in (before,seven,final)]
            require(b-a == additions and not a-b and b-c == {'h2'} and not c-b, 'exact seven local additions and one removal')
            require({k:v for k,v in before.items() if k != 'dependencies'} ==
                    {k:v for k,v in seven.items() if k != 'dependencies'} ==
                    {k:v for k,v in final.items() if k != 'dependencies'}, 'local node fields unchanged')
        else:
            require(before == seven == final, 'every other package/edge unchanged')
    require(source('inactive_h2_removed','rpc-bench/Cargo.lock') == source('format_only','rpc-bench/Cargo.lock'), 'final consumer lock identity')
    generated_before = json.loads(blobs['qualification/registered-wkt/generated-before.json'])
    generated_after = json.loads(blobs['qualification/registered-wkt/generated-after.json'])
    require(generated_before['generated_files_sha256'] == generated_after['generated_files_sha256']
            and len(generated_after['generated_files_sha256']) == 15, 'all15 generated outputs byte-identical')
    generated_archive = blobs['qualification/registered-wkt/generated-before.tar.gz']
    require(hashlib.sha256(generated_archive).hexdigest() == generated_before['archive_sha256'], 'actual generated archive hash')
    wkt = source('format_only', 'rpc-bench/src/generated_wkt.rs')
    require(b'include!(concat!(env!("OUT_DIR"), "/google/protobuf/timestamp.rs"));' in wkt, 'actual Timestamp owner inclusion')
    oracle = [('unary','echo',1,1,0),('unary','terminal',1,0,0),('unary','cancel',1,0,0),
              ('server_stream','echo',1,2,1),('server_stream','terminal',1,1,1),('server_stream','cancel',1,1,1),
              ('client_stream','echo',2,1,0),('client_stream','terminal',2,0,0),('client_stream','cancel',1,0,0),('client_stream','early',1,1,0),
              ('bidi','echo',2,2,1),('bidi','terminal',1,1,1),('bidi','cancel',1,1,1),('bidi','early',1,1,1)]
    keys = ('client_connection_driver', 'client_bidi_upload', 'client_server_stream_cancel', 'server_rpc_dispatch')
    for variant in ('baseline', 'candidate'):
        report = json.loads(blobs['qualification/records/' + variant + '_scheduler_001.stdout.log'])
        require(report['schema'] == 'scheduler-spawns/1' and report['runtime'] == 'current_thread'
                and report['transport'] == 'duplex' and report['warmup_calls'] == 4
                and report['quiescent_alive_tasks'] == 0 and len(report['rows']) == 14, 'bounded diagnostic input/cleanup')
        setup = report['setup']
        require(setup['harness_connection_tasks'] == 1, 'setup attribution')
        if variant == 'candidate':
            require([setup['library_spawns'][k] for k in keys] == [1,0,0,0], 'driver setup count')
        else:
            require(setup['library_spawns']['status'] == 'not_run', 'semantic baseline declines instrumentation')
        for row, expected in zip(report['rows'], oracle):
            shape, mode, bodies, replies, producers = expected
            require(tuple(row[k] for k in ('shape','mode','body_messages','reply_messages','application_producer_spawns')) == expected, 'independent shape/body/reply oracle')
            require(row['terminal'] == {'terminal':'FAILED_PRECONDITION','cancel':'CANCELLED'}.get(mode,'OK')
                    and row['steady_alive_tasks'] == 2 and row['rpc_and_producer_completion_acknowledged'], 'terminal/drop/quiescence')
            if variant == 'candidate':
                require(row['library_spawns']['status'] == 'measured'
                        and [row['library_spawns'][k] for k in keys] == [0,int(shape == 'bidi'),int(shape == 'server_stream'),1], 'site count oracle')
            else:
                require(row['library_spawns']['status'] == 'not_run', 'baseline status')
        for missing in ('wakeups','channel_sends','cross_thread_handoffs','context_switches','instruction_attribution'):
            require(report[missing]['status'] == 'not_run', 'explicit unmeasured attribution')
    for name in [x for x in blobs if x.endswith('.upstream-before.json')]:
        after = name.replace('.upstream-before.json', '.upstream-after.json')
        require(after in blobs and blobs[name] == blobs[after], 'direct upstream pre/post guard')
    return {'ordinary_captures': len(index['captures']), 'genuine_Cargo_reds': len(failures),
            'native185_executions': 107, 'registered_RPC_executions': 172, 'stable_unit_executions': 18,
            'diagnostic_cases_each_variant': 14, 'parent_RX10': 'open', 'performance': 'not_run'}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--payload-root', type=Path)
    args = parser.parse_args()
    index = json.loads((HERE / 'index.json').read_text())
    capsule = HERE / index['capsule']['file']
    require(index['owned_cap_and_global_floor'] == BOUND, 'fixed bounds')
    require(digest(capsule) == index['capsule']['sha256'], 'capsule hash')
    blobs = {}
    with tarfile.open(capsule, 'r:gz') as archive:
        for member in archive:
            require(member.isfile() and member.name in index['members'] and member.name not in blobs, 'metadata member set/type')
            data = archive.extractfile(member).read()
            expected = index['members'][member.name]
            require(len(data) == expected['bytes'] and hashlib.sha256(data).hexdigest() == expected['sha256'], 'metadata bytes/hash')
            blobs[member.name] = data
    require(set(blobs) == set(index['members']), 'complete capsule')
    result = {'status': 'pass', 'metadata_members': len(blobs), 'limits': index['limits'],
              'retirements': audit_retirements(index, blobs, args.payload_root)}
    if 'summary_member' in index:
        result['ordinary_qualification'] = audit_rx(index, blobs)
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
