#!/usr/bin/env python3
"""Verify TC-30b2a evidence consistency without executing a benchmark."""
import hashlib
import json
import pathlib
import re
import subprocess
import tarfile

artifact = pathlib.Path(__file__).resolve().parent
root = artifact.parents[2]
manifest = json.loads((artifact / 'archive-manifest.json').read_text())
handoff = json.loads((artifact / 'handoff.json').read_text())
archive = artifact / 'raw-artifacts.tar.gz'
sha = lambda data: hashlib.sha256(data).hexdigest()
assert sha(archive.read_bytes()) == manifest['archive_sha256'] == handoff['archive_sha256']
assert archive.stat().st_size == manifest['archive_bytes']
assert sha((artifact / 'verify.py').read_bytes()) == handoff['verifier_sha256']
expected = {row['path']: row for row in manifest['members']}
with tarfile.open(archive, 'r:gz') as bundle:
    members = bundle.getmembers()
    assert len(members) == len(expected)
    assert len({member.name for member in members}) == len(members)
    data = {}
    for member in members:
        assert member.isfile() and member.name in expected
        parts = pathlib.PurePosixPath(member.name).parts
        assert parts[0] == 'raw' and '..' not in parts and not member.name.startswith('/')
        content = bundle.extractfile(member).read()
        row = expected[member.name]
        assert len(content) == row['bytes'] and sha(content) == row['sha256']
        data[member.name] = content
lines = [f"{expected[path]['sha256']}  {path}" for path in sorted(expected)]
assert (artifact / 'member-sha256.txt').read_text() == '\n'.join(lines) + '\n'
index = json.loads(data['raw/command-records.json'])
assert index['qualified'] is handoff['qualified'] is False
runs = {row['name']: row for row in index['runs']}
assert len(runs) == len(index['runs'])
expected_failures = {'gzip-before-fix': 101, 'native-final': 101, 'consumer-format': 1}
for name, row in runs.items():
    assert int(data[f'raw/{name}.exit']) == row['exit']
    assert row['exit'] == expected_failures.get(name, 0)
    log = data[f'raw/{name}.log'].decode()
    results = [{'passed': int(a), 'failed': int(b)} for a, b in re.findall(r'test result: \w+\. (\d+) passed; (\d+) failed;', log)]
    assert results == row['test_results']
    if 'live_command_record' in row:
        command = json.loads(data['raw/' + row['live_command_record']])
        assert command['exit'] == row['exit']
        assert command['source_before'] == command['source_after'] == row['source_state']['source']
        assert command['tracked_changes_before'] == command['tracked_changes_after'] == ''
counts = {'framing-final': 16, 'native-final-corrected': 117, 'generated-final': 34}
for name, count in counts.items():
    row = runs[name]
    assert sum(result['passed'] for result in row['test_results']) == count
    assert sum(result['failed'] for result in row['test_results']) == 0
    assert row['source_state']['source'] == handoff['stable_gates_source']
assert sum(counts.values()) == index['final_test_execution_count'] == handoff['final_test_execution_count'] == 167
for name, source in [('stable-source', handoff['stable_gates_source']), ('final-source', handoff['final_source'])]:
    snapshot = json.loads(data['raw/' + name + '.json'])
    assert snapshot['source'] == source and snapshot['tree_clean']
    for path, digest in snapshot['files'].items():
        content = subprocess.check_output(['git', 'show', source + ':' + path], cwd=root)
        assert sha(content) == digest
for path in handoff['unchanged_manifests_locks']:
    base = subprocess.check_output(['git', 'show', handoff['base'] + ':' + path], cwd=root)
    final = subprocess.check_output(['git', 'show', handoff['final_source'] + ':' + path], cwd=root)
    assert base == final
comparison = json.loads(data['raw/comment-only-comparison.json'])
assert comparison['exact_comment_replacement_verified']
assert comparison['stable_source'] == handoff['stable_gates_source']
assert comparison['final_source'] == handoff['final_source']
fingerprints = json.loads(data['raw/pre-clean-fingerprints.json'])
assert not fingerprints['active_owned_build_or_executable_processes']
assert len(fingerprints['final_gate_elfs']) == 12 and all(row['elf'] for row in fingerprints['final_gate_elfs'])
assert len(fingerprints['generated_rs']) == 87
assert fingerprints['last_du_sb_unique_target_bytes'] < 2 * 1024**3
for prohibited in ['tonic v', 'tower v', 'axum v', 'http-body v']:
    assert prohibited not in data['raw/default-tree.log'].decode()
print(f"Verified {len(members)} raw members, {len(runs)} command records, 167 final test executions and reachable source/lock pins; qualified=false.")
