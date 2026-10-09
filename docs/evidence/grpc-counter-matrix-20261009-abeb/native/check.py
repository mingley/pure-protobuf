#!/usr/bin/env python3
"""Archive terminal counter campaigns, including failed and partial captures."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import tarfile


def sha(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def archive(source, destination, expected_source, chunk_bytes=32 * 1024 * 1024):
    source = source.resolve(strict=True)
    destination = destination.resolve()
    if source == destination or source in destination.parents:
        raise ValueError('destination must be outside the capture directory')
    campaign_path = source / 'campaign.json'
    campaign_bytes = campaign_path.read_bytes()
    campaign = json.loads(campaign_bytes)
    if not re.fullmatch(r'[0-9a-f]{40}', expected_source):
        raise ValueError('expected source must be a full Git commit')
    if campaign.get('source') != expected_source:
        raise ValueError('campaign source does not match the expected commit')
    if campaign.get('state') not in ['finished', 'failed']:
        raise ValueError('campaign is still active')
    if campaign.get('qualified') is not False:
        raise ValueError('counter campaigns do not establish qualification')
    if chunk_bytes < 1:
        raise ValueError('archive chunk size must be positive')
    files = []
    for path in sorted(source.rglob('*')):
        if path.is_symlink():
            raise ValueError('capture contains a symlink: ' + str(path))
        if path.is_file():
            stat = path.stat()
            if stat.st_size > chunk_bytes:
                raise ValueError('capture exceeds archive chunk size: ' + str(path))
            files.append((path, stat.st_size, stat.st_mtime_ns))
    destination.mkdir(parents=True, exist_ok=False)
    chunks = []
    batch = []
    size = 0

    def write_batch():
        directory = destination / f'part-{len(chunks):04d}'
        directory.mkdir()
        entries = []
        with (directory / 'raw.tar.gz').open('wb') as raw:
            with gzip.GzipFile(fileobj=raw, mode='wb', mtime=0) as zipped:
                with tarfile.open(fileobj=zipped, mode='w') as tar:
                    for path, expected_size, expected_mtime in batch:
                        data = path.read_bytes()
                        stat = path.stat()
                        if len(data) != expected_size or stat.st_mtime_ns != expected_mtime:
                            raise ValueError('capture changed during publication: ' + str(path))
                        name = path.relative_to(source).as_posix()
                        member = tarfile.TarInfo(name)
                        member.size = len(data)
                        tar.addfile(member, io.BytesIO(data))
                        entries.append(dict(path=name, bytes=len(data), sha256=hashlib.sha256(data).hexdigest()))
        manifest = dict(schema='pbrs.counter-capsule.v1',
                        archive=dict(name='raw.tar.gz', sha256=sha(directory / 'raw.tar.gz')),
                        files=entries, logical_bytes=sum(row['bytes'] for row in entries))
        (directory / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
        chunks.append(dict(path=directory.name, files=len(entries), logical_bytes=manifest['logical_bytes'],
                           manifest_sha256=sha(directory / 'manifest.json')))

    for entry in files:
        if batch and size + entry[1] > chunk_bytes:
            write_batch()
            batch = []
            size = 0
        batch.append(entry)
        size += entry[1]
    if batch:
        write_batch()
    if campaign_path.read_bytes() != campaign_bytes:
        raise ValueError('campaign changed during publication')
    current_files = {p.relative_to(source).as_posix() for p in source.rglob('*') if p.is_file()}
    if current_files != {p.relative_to(source).as_posix() for p, _, _ in files}:
        raise ValueError('capture inventory changed during publication')
    for path, expected_size, expected_mtime in files:
        stat = path.stat()
        if stat.st_size != expected_size or stat.st_mtime_ns != expected_mtime:
            raise ValueError('capture changed during publication: ' + str(path))
    summary = dict(source=expected_source, qualified=False, campaign=campaign, parts=chunks,
                   files=sum(row['files'] for row in chunks), logical_bytes=sum(row['logical_bytes'] for row in chunks))
    (destination / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    shutil.copy2(Path(__file__), destination / 'check.py')
    (destination / 'README.md').write_text(
        '# Endpoint counter captures\n\nSource `' + expected_source + '`, phase `' + str(campaign.get('phase'))
        + '`, state `' + campaign['state'] + '`. Failed RPCs and partial captures are retained. '
        'See summary.json and the per-group ledgers for outcomes; absent cells are not wins.\n\n'
        'These are shared-host synthetic workload diagnostics. Read-all corpora, lifecycle, saturation, '
        'task wakeups and dedicated-host timing remain separate. Production qualification remains false. '
        'Run `python3 check.py --check .` to verify every retained file.\n')
    check(destination)
    return summary


def check(directory):
    directory = Path(directory)
    summary = json.loads((directory / 'summary.json').read_text())
    seen = set()
    total = 0
    for part in summary['parts']:
        root = directory / part['path']
        if sha(root / 'manifest.json') != part['manifest_sha256']:
            raise ValueError('manifest checksum mismatch')
        manifest = json.loads((root / 'manifest.json').read_text())
        if sha(root / manifest['archive']['name']) != manifest['archive']['sha256']:
            raise ValueError('archive checksum mismatch')
        expected = {entry['path']: entry for entry in manifest['files']}
        if len(expected) != len(manifest['files']):
            raise ValueError('duplicate manifest path')
        local = set()
        local_bytes = 0
        with tarfile.open(root / manifest['archive']['name'], 'r:gz') as tar:
            for member in tar:
                if not member.isfile() or member.name in seen or member.name not in expected:
                    raise ValueError('unexpected archive member')
                entry = expected[member.name]
                stream = tar.extractfile(member)
                digest = hashlib.sha256()
                for block in iter(lambda: stream.read(1024 * 1024), b''):
                    digest.update(block)
                if member.size != entry['bytes'] or digest.hexdigest() != entry['sha256']:
                    raise ValueError('file checksum mismatch')
                seen.add(member.name)
                local.add(member.name)
                local_bytes += member.size
        if local != set(expected) or len(local) != part['files']:
            raise ValueError('archive inventory mismatch')
        if local_bytes != manifest['logical_bytes'] or local_bytes != part['logical_bytes']:
            raise ValueError('archive byte total mismatch')
        total += local_bytes
    if len(seen) != summary['files'] or total != summary['logical_bytes']:
        raise ValueError('campaign inventory mismatch')
    print(f'verified {len(seen)} files, {total} bytes; no production qualification')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', type=Path)
    parser.add_argument('--source', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--expected-source')
    args = parser.parse_args()
    if args.check is not None:
        check(args.check)
    elif args.source is not None and args.output is not None and args.expected_source is not None:
        archive(args.source, args.output, args.expected_source)
    else:
        parser.error('use --check, or --source, --output and --expected-source')


if __name__ == '__main__':
    main()
