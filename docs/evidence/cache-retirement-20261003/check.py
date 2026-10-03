#!/usr/bin/env python3
"""Read-only portable metadata audit; --payload-root also revalidates local payloads."""
import argparse
import hashlib
import json
import pathlib
import stat
import tarfile

HERE = pathlib.Path(__file__).resolve().parent
CAP = 2147483648


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as file:
        for data in iter(lambda: file.read(1048576), b''):
            value.update(data)
    return value.hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def payload_check(path, manifest, expected):
    require(digest(path) == expected['archive_sha256'], 'local payload archive hash')
    rows = {('cache' if rel == '.' else 'cache/' + rel): row for rel, row in manifest.items()
            if row['disposition'] == 'archived'}
    visited, contents, inode_targets = set(), {}, {}
    with tarfile.open(path, mode='r|gz') as archive:
        for member in archive:
            require(member.name in rows and member.name not in visited, 'payload member set')
            row, visited_member = rows[member.name], member.name
            identity = row['identity']
            require(member.mode == stat.S_IMODE(identity['st_mode']), 'payload mode')
            require((member.uid, member.gid) == (identity['st_uid'], identity['st_gid']), 'payload uid/gid')
            if row['kind'] == 'directory':
                require(member.isdir(), 'payload directory')
            elif row['kind'] == 'symlink':
                require(member.issym() and member.linkname == row['link'], 'payload symlink')
            else:
                key = identity['st_dev'], identity['st_ino']
                if key in inode_targets:
                    require(member.islnk() and member.linkname == inode_targets[key], 'payload hardlink topology')
                    value = contents[member.linkname]
                else:
                    require(member.isfile() and member.size == identity['st_size'], 'payload regular size/type')
                    inode_targets[key] = member.name
                    value = hashlib.sha256()
                    file = archive.extractfile(member)
                    for chunk in iter(lambda: file.read(1048576), b''):
                        value.update(chunk)
                    value = value.hexdigest()
                require(value == row['sha256'], 'payload content')
                contents[member.name] = value
            visited.add(visited_member)
    require(visited == set(rows), 'payload missing member')
    return {'members': len(visited), 'regular_paths': len(contents), 'hash_content_mode_ownership_links': 'pass'}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--payload-root', type=pathlib.Path)
    args = parser.parse_args()
    index = json.loads((HERE / 'index.json').read_text())
    capsule = HERE / index['capsule']['file']
    require(digest(capsule) == index['capsule']['sha256'], 'metadata capsule hash')
    blobs = {}
    with tarfile.open(capsule, 'r:gz') as archive:
        for member in archive:
            require(member.isfile() and member.name in index['members'], 'metadata member type/name')
            require(member.name not in blobs, 'duplicate metadata member')
            data = archive.extractfile(member).read()
            oracle = index['members'][member.name]
            require(len(data) == oracle['bytes'] and hashlib.sha256(data).hexdigest() == oracle['sha256'], 'metadata member bytes')
            blobs[member.name] = data
    require(set(blobs) == set(index['members']), 'missing metadata members')
    obj = lambda name: json.loads(blobs[name])
    stages = {}
    for stage in index['stages']:
        prefix = stage['record_directory'] + '/'
        record = obj(prefix + 'record.json')
        manifest = obj(prefix + 'manifest.json')
        require(record['status'] == 'verified_retired' and record['retired'] and record['exit'] == 0, 'retirement status')
        require(hashlib.sha256(blobs[prefix + 'manifest.json']).hexdigest() == record['manifest_sha256'], 'manifest recorded hash')
        require(record['cache'] == stage['cache'] and record['archive'] == stage['archive'], 'indexed stage identity')
        require(set(obj(prefix + 'removed-paths.json')) == set(manifest), 'retirement path set')
        require(len(obj(prefix + 'removed-paths.json')) == len(manifest), 'duplicate retirement path')
        before, after = obj(prefix + 'source-before.json'), obj(prefix + 'source-after.json')
        require(before['compiler_input_files'] == after['compiler_input_files'] and not before['status'] and not after['status'], 'source compiler-input equality')
        require(record['moving_root_context']['before'] == before['head_context_only'] and record['moving_root_context']['after'] == after['head_context_only'], 'source context labels')
        if stage['stage'] != 'semver':
            require(before['head_context_only'] == after['head_context_only'], 'isolated source HEAD')
        samples = [json.loads(line) for line in blobs[prefix + 'resources.jsonl'].splitlines()]
        require(samples and all(s['free_bytes'] >= CAP and s['owned_total'] <= CAP for s in samples), 'fixed sampled resource guards')
        require(record['observed_min_free'] == min(s['free_bytes'] for s in samples), 'minimum sampled free')
        require(record['observed_max_owned_cache'] == max(s['owned_total'] for s in samples), 'maximum sampled own cache')
        for name in ('users-before-locks.json', 'users-after-locks.json', 'users-immediately-before-retirement.json'):
            require(not obj(prefix + name)['matches'], 'visible cache user')
        locks = obj(prefix + 'held-locks.json')['locks']
        require(len(locks) == stage['cargo_locks'], 'Cargo lock coverage')
        require(all(x['flock'] == 'exclusive nonblocking held' and x['fcntl'] == 'exclusive nonblocking held' for x in locks), 'held lock types')
        require(obj(prefix + 'fresh-before-retirement-verification.json')['complete_path_inode_mode_hashes_pass'], 'fresh live verification')
        files = [row for row in manifest.values() if row['kind'] == 'file']
        omitted = [name for name, row in manifest.items() if row['disposition'] == 'hash_only']
        require(omitted == obj(prefix + 'hash-only-paths.json') or set(omitted) == set(obj(prefix + 'hash-only-paths.json')), 'hash-only exact paths')
        require(len(omitted) == stage['hash_only_paths'] == record['hash_only_paths'], 'hash-only count')
        require(not any(row['elf'] and row['disposition'] != 'archived' for row in files), 'every ELF retained')
        require(len(files) == record['regular_paths'], 'regular count')
        require(sum(row['identity']['st_size'] for row in files) == record['logical_namespace_bytes_with_aliases'], 'logical namespace size')
        require(record['post_retirement_archive_sha256'] == record['archive']['archive_sha256'], 'post-retirement payload hash')
        result = {'hashed_regular_paths': len(files), 'archived_regular_paths': len(files) - len(omitted), 'hash_only_paths': len(omitted),
                  'elf_paths_archived': sum(row['elf'] for row in files), 'payload': 'indexed; not reread by this metadata-only invocation'}
        if args.payload_root:
            result['payload'] = payload_check(args.payload_root / stage['payload_relative'], manifest, record['archive'])
        stages[stage['stage']] = result
    first = obj('semver/first-preflight-invalid.json')
    require(first['exit'] == 1 and not first['cache_deleted'] and not first['archive_created'], 'first preflight red preserved')
    require(blobs['semver.exit'].strip() == b'1', 'first red raw exit')
    second = obj('semver_attempt2/record.json')
    require(second['exit'] == 1 and not second['retired'] and 'benchmark_service.proto' in second['error'], 'second source-symlink red')
    for name, oracle in index['helper_versions'].items():
        require(hashlib.sha256(blobs[name]).hexdigest() == oracle, 'helper version bytes')
    require(obj('tool-symlink-regression/record.json')['status'] == 'pass' and obj('source-symlink-regression/record.json')['status'] == 'pass', 'focused symlink regressions')
    print(json.dumps({'status': 'pass', 'metadata_members': len(blobs), 'stages': stages,
                      'limits': index['limits']}, indent=2))


if __name__ == '__main__':
    main()
