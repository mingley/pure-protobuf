#!/usr/bin/env python3
"""Read-only replay of the observable task-waker test fixture overlay."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
record = json.loads((HERE / 'wake-guard-1.88-overlay.json').read_text())
sha = lambda raw: hashlib.sha256(raw).hexdigest()
assert sha((HERE / 'provenance.json').read_bytes()) == record['original_topology_mapping_sha256']
assert sha((HERE / record['exact_patch_file']).read_bytes()) == record['exact_patch_sha256']
results = []
for entry in record['files']:
    path = entry['file']
    if len(sys.argv) > 1:
        original = (Path(sys.argv[1]) / (Path(path).name + '.before')).read_bytes()
    else:
        original = subprocess.check_output(['git', 'show', record['base_commit'] + ':' + path], cwd=ROOT)
    assert sha(original) == entry['old_sha256']
    lines = original.decode().splitlines(keepends=True)
    for operation in reversed(entry['operations']):
        start, end = operation['start_line_index'], operation['end_line_index']
        assert ''.join(lines[start:end]) == operation['old']
        lines[start:end] = operation['new'].splitlines(keepends=True)
    replayed = ''.join(lines).encode()
    assert sha(replayed) == entry['new_sha256']
    assert (HERE / Path(path).name).read_bytes() == replayed
    results.append({'file': path, 'old_sha256': entry['old_sha256'], 'new_sha256': entry['new_sha256']})
print(json.dumps({'status': 'pass', 'files': results, 'runtime_changed': False,
    'new_compilation_run': False, 'original_mapping_preserved': True}, indent=2))
