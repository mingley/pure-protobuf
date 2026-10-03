#!/usr/bin/env python3
"""Root-run, read-only source preparation after shipping source is frozen.

Writes only a new work directory. Does not invoke Cargo, a compiler, binaries,
benchmark tools, or mutate any source/lock/cache. Does not create a BUILD record.
"""
import argparse
import ast
from datetime import datetime, timezone
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tomllib

ROOT = Path('/workspace/pure-protobuf')
PLAN = ROOT / 'work/shared-parent-release-plan-20261003'
SUPPORT = {'atomic-waker', 'fnv', 'futures-sink', 'indexmap', 'slab', 'tokio-util', 'tracing'}
EXTRA = (
    'scripts/measure-tc32-bridges.py', 'scripts/audit-tc32-bridges.py',
    'tests/bench/test_tc32_bridge_collectors.py',
    'docs/evidence/sb-32/preflight-inventory.json',
    'docs/evidence/tc32a-bridge-qualification-20261002/bridge-inventory.json',
    'docs/evidence/tc32b-collector-20261002/default-registry.txt',
    'bench/devloop/adoption/evidence/rpc-inventory.json',
    'bench/devloop/adoption/capture-callgrind.py',
    'docs/evidence/cl-08/callgrind-smoke-cells.json',
    'docs/evidence/cl-08/run-callgrind.sh',
    'docs/evidence/cl-08/validate-callgrind.py', 'scripts/devloop.sh',
)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def tuples(data):
    return sorted(({'name': p['name'], 'version': p['version'],
                    'source': p.get('source'), 'checksum': p.get('checksum')}
                   for p in tomllib.loads(data.decode())['package']),
                  key=lambda p: (p['name'], p['version'], p['source'] or ''))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    assert len(args.source) == 40 and all(c in '0123456789abcdef' for c in args.source)
    out = args.out.resolve()
    assert out.is_relative_to(ROOT / 'work')
    out.mkdir(parents=True, exist_ok=False)
    commands = []

    def write(name, data):
        target = out / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)

    def dump(name, value):
        write(name, (json.dumps(value, indent=2, sort_keys=True) + '\n').encode())

    def git(*argv, root=ROOT):
        command = ['git', '-C', str(root), *argv]
        started = datetime.now(timezone.utc).isoformat()
        process = subprocess.run(command, capture_output=True, check=False)
        index = len(commands)
        write(f'raw/git-{index}.stdout', process.stdout)
        write(f'raw/git-{index}.stderr', process.stderr)
        commands.append({'argv': command, 'started_utc': started,
                         'finished_utc': datetime.now(timezone.utc).isoformat(),
                         'exit_code': process.returncode,
                         'stdout': f'raw/git-{index}.stdout',
                         'stderr': f'raw/git-{index}.stderr'})
        dump('git-commands.json', commands)
        process.check_returncode()
        return process.stdout

    assert git('rev-parse', 'HEAD').decode().strip() == args.source
    assert not git('status', '--porcelain', '--untracked-files=normal')
    upstream = ROOT / 'third_party/protobuf'
    upstream_commit = git('rev-parse', 'HEAD', root=upstream).decode().strip()
    assert upstream_commit == '35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03'
    assert not git('status', '--porcelain', root=upstream)
    driver = ast.parse(git('show', args.source + ':bench/devloop/measure-tonic-transport.py'))
    constants = {n.targets[0].id: ast.literal_eval(n.value) for n in driver.body
                 if isinstance(n, ast.Assign) and isinstance(n.targets[0], ast.Name)
                 and n.targets[0].id in ('SOURCE_ROOTS', 'SCHEMAS')}
    auditor = ast.parse(git('show', args.source + ':scripts/audit-tc32-bridges.py'))
    expression = next(n.value for n in auditor.body if isinstance(n, ast.Assign)
                      and isinstance(n.targets[0], ast.Name) and n.targets[0].id == 'PINNED_SOURCES')
    required = tuple(ast.literal_eval(expression.left)) + tuple(
        f'bench/devloop/adoption/proto/options/part_{i:02}.proto' for i in range(20))
    assert len(required) == 41 and len(constants['SCHEMAS']) == 14
    roots = tuple(dict.fromkeys((*constants['SOURCE_ROOTS'], *required, *EXTRA)))
    archive = git('archive', '--format=tar', args.source, *roots)
    write('source-inputs.tar', archive)
    files, schemas = {}, {}
    with tarfile.open(fileobj=io.BytesIO(archive)) as stream:
        for member in stream.getmembers():
            if not member.isfile():
                continue
            data = stream.extractfile(member).read()
            assert (ROOT / member.name).read_bytes() == data, member.name
            files[member.name] = {'sha256': sha(data), 'size': len(data)}
            if member.name.endswith('.proto'):
                schemas[member.name] = sha(data)
                write('schemas/' + member.name, data)
    assert set(required) <= files.keys()
    for name in constants['SCHEMAS']:
        relative = str(Path(name).relative_to('third_party/protobuf'))
        data = git('show', upstream_commit + ':' + relative, root=upstream)
        assert (ROOT / name).read_bytes() == data, name
        schemas[name] = sha(data)
        write('schemas/' + name, data)
    lock_review = {}
    for name in ('Cargo.lock', 'bench/devloop/Cargo.lock', 'bench/devloop/adoption/Cargo.lock'):
        data = git('show', args.source + ':' + name)
        before = (PLAN / 'locks/before' / name).read_bytes()
        assert tuples(data) == tuples(before), 'package pin tuple drift: ' + name
        packages = tomllib.loads(data.decode())['package']
        grpc = [p for p in packages if p['name'] == 'pbrs-grpc']
        if grpc:
            assert len(grpc) == 1 and SUPPORT <= set(grpc[0]['dependencies'])
            assert data == (PLAN / 'locks/modeled' / name).read_bytes(), 'unexpected lock edge delta: ' + name
        else:
            assert name == 'bench/devloop/adoption/Cargo.lock' and data == before
        lock_review[name] = {'sha256': sha(data), 'packages': len(packages),
                            'all_existing_name_version_source_checksum_tuples_retained': True}
    manifest = tomllib.loads(git('show', args.source + ':pbrs-grpc/Cargo.toml').decode())
    assert manifest['dependencies']['indexmap'] == '2.14.0'
    assert git('rev-parse', 'HEAD').decode().strip() == args.source
    assert not git('status', '--porcelain', '--untracked-files=normal')
    assert git('rev-parse', 'HEAD', root=upstream).decode().strip() == upstream_commit
    assert not git('status', '--porcelain', root=upstream)
    record = {'state': 'source_inputs_only_not_a_completed_build', 'source_commit': args.source,
              'protobuf_source_commit': upstream_commit, 'roots': roots,
              'required_tc32_paths': required, 'files': files,
              'source_sha256': {name: row['sha256'] for name, row in files.items()},
              'schema_sha256': schemas, 'lock_review': lock_review,
              'compiler_build_capture_cache_mutation_invocations': 0}
    dump('input-pin.json', record)
    print(json.dumps({'source': args.source, 'files': len(files), 'schemas': len(schemas),
                      'state': record['state'], 'out': str(out)}, indent=2))

if __name__ == '__main__':
    main()
