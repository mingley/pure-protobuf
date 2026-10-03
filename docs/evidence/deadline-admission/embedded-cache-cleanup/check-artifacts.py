#!/usr/bin/env python3
"""Independently verify preserved evidence; never touch the deleted cache."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import tarfile

HERE = Path(__file__).resolve().parent
sha = lambda raw: hashlib.sha256(raw).hexdigest()
proof = json.loads((HERE / 'proof.json').read_text())
pins = json.loads((HERE / 'members-sha256.json').read_text())
archive = HERE / proof['archive']['file']
assert archive.stat().st_size == proof['archive']['bytes']
assert sha(archive.read_bytes()) == proof['archive']['sha256']
assert len(pins) == proof['archive']['regular_members'] == 1258
raws = {}
seen = set()
with tarfile.open(archive, 'r:gz') as entries:
    for member in entries:
        path = PurePosixPath(member.name)
        assert not path.is_absolute() and '..' not in path.parts
        assert path.parts[0] == 'embedded-cache-cleanup'
        assert member.name not in seen
        seen.add(member.name)
        assert member.isdir() or member.isfile(), member.name
        if member.isfile():
            raw = entries.extractfile(member).read()
            assert pins[member.name] == {'bytes': len(raw), 'sha256': sha(raw)}, member.name
            raws[str(path.relative_to('embedded-cache-cleanup'))] = raw
assert len(raws) == len(pins)
load = lambda name: json.loads(raws[name])
snapshot = load('cache-before.json')
old = load('cache-before-metadata-refresh.json')
result = load('reclaim-result.json')
files = snapshot['files']
assert len(files) == proof['artifact_file_count'] == 1615
assert set(old['files']) == set(files)
changed = [name for name in files if old['files'][name] != files[name]]
assert changed == ['.rustc_info.json']
assert snapshot['metadata_refresh'] == proof['metadata_refresh']
assert sha(raws['metadata-refresh-before/.rustc_info.json']) == old['files'][changed[0]]['sha256']
assert sha(raws['metadata-refresh-after/.rustc_info.json']) == files[changed[0]]['sha256']
assert b'sha256' in raws['reclaim-first-red.log']
assert snapshot['cache'] == result['removed_only'] == proof['removed_only']
assert snapshot['cache'].endswith('/h2-embedded/target')
assert result['target_absent'] and result['protected_all_exist']
assert not result['qualification_or_source_mutated'] and not result['new_compilation_run']
assert result['global_free_bytes_after'] == proof['global_free_bytes_after']
assert snapshot['protected'] == proof['protected']
for name in ['process-before.json', 'process-before-reclaim-first.json', 'process-before-reclaim.json']:
    process = load(name)
    assert not process['active_owned_processes']
    assert process['inspection_errors'] == {'PermissionError': 2}
    assert process['excluded_capture_process_and_ancestors']
    assert 'future-writer' in process['limits']
assert snapshot['cache_produced_from_topology_source'] == proof['cache_produced_from_source']
assert snapshot['checkout_commit'] == proof['snapshot_checkout_commit']
assert proof['cache_produced_from_source'] == '604f4c2e5b1e33050bf859a97dc69fb7ae791f3a'
assert proof['global_free_bytes_before'] == snapshot['global_free_bytes_before']
assert sum(pin['bytes'] for pin in files.values()) == proof['apparent_sum_bytes'] == snapshot['apparent_sum_bytes']
assert sum(pin['bytes'] for pin in old['files'].values()) == proof['apparent_sum_bytes_before_metadata_refresh']
unique = {}
for pin in files.values():
    key = (pin['device'], pin['inode'])
    if key in unique:
        assert unique[key]['bytes'] == pin['bytes'] and unique[key]['sha256'] == pin['sha256']
    unique[key] = pin
assert sum(pin['bytes'] for pin in unique.values()) == proof['unique_inode_apparent_bytes']
assert sum(pin['allocated_bytes'] for pin in unique.values()) == proof['unique_inode_allocated_bytes']
assert sum(pin['allocated_bytes'] for pin in snapshot['directories'].values()) == proof['directory_allocated_bytes']
preserved = 0
for name, pin in files.items():
    if name.endswith('.d') or '/.fingerprint/' in '/' + name or ('/build/' in '/' + name and '/out/' in name):
        assert pin['preserved_content'], name
    if pin['preserved_content']:
        rel = 'metadata/' + name
        if name in snapshot['qualified_consumer_elves']:
            elf = snapshot['qualified_consumer_elves'][name]
            rel = 'consumer-elves/' + PurePosixPath(elf['preserved']).name
            assert elf['sha256'] == pin['sha256'] and elf['bytes'] == pin['bytes']
        assert len(raws[rel]) == pin['bytes'] and sha(raws[rel]) == pin['sha256'], name
        preserved += 1
assert preserved == 1244
assert len(snapshot['qualified_consumer_elves']) == 4
# Qualified unit executables were preserved before reclamation in the source-
# qualified topology archive. Verify those bytes there rather than trust paths.
qualified = load('qualified-elves.json')
topology = HERE.parent / 'embedded-topology/raw.tar.gz'
assert sha(topology.read_bytes()) == '8d899c703aabc54ef417a9d08c830f811ca1d15946fe40a2423013dcc8a2c9d6'
expected = {PurePosixPath(pin['preserved']).name: pin for pin in qualified.values()}
found = set()
with tarfile.open(topology, 'r:gz') as entries:
    for member in entries:
        name = PurePosixPath(member.name).name
        if member.isfile() and name in expected:
            raw = entries.extractfile(member).read()
            pin = expected[name]
            assert member.size == pin['bytes'] and sha(raw) == pin['sha256']
            assert pin['source']['source_commit'] == proof['cache_produced_from_source']
            found.add(name)
assert found == set(expected)
if (HERE / 'artifact-sha256.json').exists():
    for name, pin in json.loads((HERE / 'artifact-sha256.json').read_text()).items():
        raw = (HERE / name).read_bytes()
        assert len(raw) == pin['bytes'] and sha(raw) == pin['sha256'], name
print(json.dumps({'status': 'pass', 'regular_archive_members': len(raws),
    'all_cache_file_hash_records': len(files), 'full_content_files_verified': preserved,
    'consumer_elves_verified': len(snapshot['qualified_consumer_elves']),
    'qualified_unit_elves_verified': len(found), 'scope': result['removed_only'],
    'new_compilation_run': False}, indent=2))
