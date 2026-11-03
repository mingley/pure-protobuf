#!/usr/bin/env python3
"""Audit local rehearsal archives against an explicit committed Git snapshot.

This script does not build, publish, query a registry, or create attestations.
It can additionally compare another package directory. With an explicit
earlier VCS identity, this checks source equivalence rather than archive or
executed-binary identity. Adapters' pbrs lock provenance is checked separately.
"""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile
import tomllib


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git_blob(root, commit, path):
    return subprocess.check_output(['git', 'show', f'{commit}:{path}'], cwd=root)


def load_archive(path, prefix):
    members = {}
    with tarfile.open(path, 'r:gz') as archive:
        for member in archive.getmembers():
            name = PurePosixPath(member.name)
            assert not name.is_absolute() and '..' not in name.parts, member.name
            assert name.parts and name.parts[0] == prefix, member.name
            if member.isdir():
                continue
            assert member.isfile(), ('non-regular package member', member.name)
            rel = name.relative_to(prefix).as_posix()
            assert rel not in members, ('duplicate package member', rel)
            assert not ({'.git', 'work', 'target'} & set(PurePosixPath(rel).parts)), rel
            file = archive.extractfile(member)
            assert file is not None
            members[rel] = file.read()
    assert members, ('empty archive', str(path))
    return members


def compare_archives(reference, alternate, name, core_version, core_digests,
                     expected_alternate_vcs):
    assert reference.keys() == alternate.keys(), ('package member set differs', name)
    different = sorted(p for p in reference if reference[p] != alternate[p])
    actual_vcs = json.loads(alternate['.cargo_vcs_info.json'])
    assert actual_vcs == expected_alternate_vcs, ('unexpected comparison VCS', name, actual_vcs)
    remaining = [p for p in different if p != '.cargo_vcs_info.json']
    if name == 'pbrs':
        assert not remaining, ('core archive members differ', remaining)
    elif remaining:
        assert remaining == ['Cargo.lock'], ('adapter source difference', name, remaining)
        left = tomllib.loads(reference['Cargo.lock'].decode())
        right = tomllib.loads(alternate['Cargo.lock'].decode())
        for lock, core_digest in zip((left, right), core_digests):
            expected = {'source': 'registry+https://github.com/rust-lang/crates.io-index',
                        'checksum': core_digest}
            entries = [p for p in lock['package'] if p['name'] == 'pbrs'
                       and p['version'] == core_version]
            assert len(entries) == 1, ('expected one local pbrs lock entry', name)
            entry = entries[0]
            provenance = {k: entry[k] for k in ('source', 'checksum') if k in entry}
            assert provenance in ({}, expected), ('unexpected core provenance', name, provenance)
            for key in ('source', 'checksum'):
                entry.pop(key, None)
        assert left == right, ('adapter lock differs beyond pbrs provenance', name)
    return {'different_members': different,
            'comparison_vcs': actual_vcs,
            'allowed_differences': ['explicit recorded VCS identity',
                                    'adapter Cargo.lock pbrs source/checksum provenance'],
            'scope': 'all package source and normalized manifests identical; not archive or executable identity'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path.cwd())
    parser.add_argument('--commit', required=True)
    parser.add_argument('--packages', type=Path, required=True)
    parser.add_argument('--compare-packages', type=Path)
    parser.add_argument('--compare-commit', help='explicit expected VCS SHA for comparison packages')
    parser.add_argument('--compare-dirty-crates', nargs='*', default=[],
                        choices=['pbrs', 'protobuf-tonic', 'pbrs-grpc'])
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    commit = subprocess.check_output(
        ['git', 'rev-parse', '--verify', args.commit + '^{commit}'], cwd=root, text=True).strip()
    assert commit == args.commit and len(commit) == 40, 'provide the full source commit SHA'
    manifests = [('pbrs', ''), ('protobuf-tonic', 'protobuf-tonic'), ('pbrs-grpc', 'pbrs-grpc')]
    originals = {name: tomllib.loads(git_blob(root, commit, str(PurePosixPath(base)/'Cargo.toml')).decode())
                 for name, base in manifests}
    core_version = originals['pbrs']['package']['version']
    core_archive = args.packages/f'pbrs-{core_version}.crate'
    core_digest = sha(core_archive.read_bytes())
    compare_commit = args.compare_commit or commit
    if args.compare_packages:
        assert len(compare_commit) == 40
        assert subprocess.check_output(['git', 'rev-parse', '--verify', compare_commit+'^{commit}'],
                                       cwd=root, text=True).strip() == compare_commit
        compare_core_digest = sha((args.compare_packages/core_archive.name).read_bytes())
    else:
        assert args.compare_commit is None and not args.compare_dirty_crates
    report = {'schema': 'pbrs.local-package-audit.v1', 'source_commit': commit,
              'scope': 'local package contents and committed source only; no build or hosted provenance attestation',
              'auditor_sha256': sha(Path(__file__).read_bytes()), 'archives': []}
    for name, base in manifests:
        original = originals[name]
        version = original['package']['version']
        assert original['package']['name'] == name
        archive_path = args.packages/f'{name}-{version}.crate'
        archive_bytes = archive_path.read_bytes()
        members = load_archive(archive_path, f'{name}-{version}')
        assert {'Cargo.toml', 'Cargo.toml.orig', 'Cargo.lock', '.cargo_vcs_info.json',
                'LICENSE-APACHE', 'LICENSE-MIT'} <= members.keys()
        vcs = json.loads(members['.cargo_vcs_info.json'])
        assert vcs == {'git': {'sha1': commit}, 'path_in_vcs': base}, ('unexpected VCS record', name, vcs)
        normalized = tomllib.loads(members['Cargo.toml'].decode())
        assert normalized['package']['name'] == name
        assert normalized['package']['version'] == version
        for field in ['edition', 'rust-version']:
            assert normalized['package'].get(field) == original['package'].get(field), (name, field)
        dependencies = {}
        if name != 'pbrs':
            for section in ('dependencies', 'build-dependencies'):
                dep = normalized[section]['pbrs']
                assert dep['version'] == core_version and 'path' not in dep, (name, section, dep)
                source_dep = original[section]['pbrs']
                assert source_dep['version'] == core_version and source_dep['path'] == '..'
                dependencies[section] = dep
        if name == 'pbrs':
            assert not any(PurePosixPath(path).parts[0] in
                           {'docs', 'tests', 'third_party', 'benches', 'benchmarks'} for path in members)
        source_checks = []
        for path, data in sorted(members.items()):
            if path in {'Cargo.toml', 'Cargo.lock', '.cargo_vcs_info.json'}:
                continue
            git_path = str(PurePosixPath(base)/('Cargo.toml' if path == 'Cargo.toml.orig' else path))
            committed = git_blob(root, commit, git_path)
            assert data == committed, ('package source differs from commit', name, path, git_path)
            source_checks.append({'archive_path': path, 'git_path': git_path, 'sha256': sha(data)})
        row = {'name': name, 'version': version, 'archive': str(archive_path),
               'sha256': sha(archive_bytes), 'bytes': len(archive_bytes), 'members': len(members),
               'vcs': vcs, 'normalized_pbrs_dependencies': dependencies,
               'source_file_checks': source_checks}
        if args.compare_packages:
            other = args.compare_packages/archive_path.name
            other_members = load_archive(other, f'{name}-{version}')
            expected_compare_git = {'sha1': compare_commit}
            if name in args.compare_dirty_crates:
                expected_compare_git['dirty'] = True
            row['comparison'] = dict(
                archive=str(other), sha256=sha(other.read_bytes()),
                **compare_archives(members, other_members, name, core_version,
                                   (core_digest, compare_core_digest),
                                   {'git': expected_compare_git, 'path_in_vcs': base}))
        report['archives'].append(row)
    assert not args.output.exists(), 'preserve previous audit output'
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({'source_commit': commit, 'archives':
                     [{k: row[k] for k in ('name', 'version', 'sha256', 'bytes', 'members')}
                      for row in report['archives']], 'output': str(args.output)}, indent=2))


if __name__ == '__main__':
    main()
