#!/usr/bin/env python3
"""Verify the text-only capsule and its original record bytes without extraction."""
import hashlib
import json
from pathlib import Path, PurePosixPath


def digest(data):
    return hashlib.sha256(data).hexdigest()


def safe(name):
    path = PurePosixPath(name)
    return bool(name) and bool(path.parts) and not path.is_absolute() and str(path) == name and not any(
        part in ('.', '..') for part in path.parts
    ) and '\\' not in name and ':' not in name


def check(root):
    manifest_path = root / 'manifest.json'
    if manifest_path.is_symlink() or not manifest_path.is_file():
        raise ValueError('expected regular manifest')
    manifest = json.loads(manifest_path.read_text())
    if manifest.get('qualified') is not False or manifest.get('production_ready') is not False:
        raise ValueError('unexpected qualification claim')
    expected = {'README.md', 'check.py', 'results.json', 'records.json', 'source.patch.json'}
    names = [row['path'] for row in manifest['files']]
    if len(names) != len(set(names)) or set(names) != expected:
        raise ValueError('incomplete or duplicate publication inventory')
    for row in manifest['files']:
        name = row['path']
        if not safe(name) or len(PurePosixPath(name).parts) != 1:
            raise ValueError('unsafe publication name')
        path = root / name
        if path.is_symlink() or not path.is_file():
            raise ValueError('expected regular publication file')
        data = path.read_bytes()
        if len(data) != row['bytes'] or digest(data) != row['sha256']:
            raise ValueError('publication mismatch: ' + name)
    patch = json.loads((root / 'source.patch.json').read_text())
    raw_patch = patch['patch'].encode('utf-8')
    if len(raw_patch) != patch['bytes'] or digest(raw_patch) != patch['sha256']:
        raise ValueError('decoded source patch mismatch')
    records = json.loads((root / 'records.json').read_text())
    seen = set()
    for row in records['files']:
        name = row['path']
        if not safe(name) or name in seen:
            raise ValueError('unsafe or duplicate record name')
        data = records['contents_by_sha256'][row['sha256']].encode('utf-8')
        if len(data) != row['bytes'] or digest(data) != row['sha256']:
            raise ValueError('record mismatch: ' + name)
        seen.add(name)
    if set(records['contents_by_sha256']) != {row['sha256'] for row in records['files']}:
        raise ValueError('unreferenced record content')
    print(f'verified {len(expected)} publication files and {len(seen)} original records; no production qualification')


if __name__ == '__main__':
    check(Path(__file__).resolve().parent)
