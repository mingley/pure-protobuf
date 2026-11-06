#!/usr/bin/env python3
"""Check archived inputs and allocation arithmetic without executing them."""
from pathlib import Path
import hashlib
import io
import json
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent


def digest(value):
    return hashlib.sha256(value).hexdigest()


manifest = json.loads((HERE / 'manifest.json').read_text())
raw = (HERE / 'raw.tar.gz').read_bytes()
assert digest(raw) == manifest['archive_sha256']
with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as archive:
    members = archive.getmembers()
    assert len(members) == len({m.name for m in members})
    assert all(m.isfile() for m in members)
    assert {m.name for m in members} == manifest['files'].keys()
    files = {m.name: archive.extractfile(m).read() for m in members}
for name, record in manifest['files'].items():
    assert len(files[name]) == record['bytes'] and digest(files[name]) == record['sha256'], name


def data(name):
    return json.loads(files[name])


root = subprocess.check_output(['git', 'rev-parse', '--show-toplevel'], cwd=HERE, text=True).strip()
baseline = data('baseline-build.json')
candidate = data('candidate-build.json')
assert baseline['inputs_sha256'].keys() == candidate['inputs_sha256'].keys()
paths = sorted(baseline['inputs_sha256'])
base = manifest['base_commit']
request = ''.join(f'{base}:{path}\n' for path in paths).encode()
packed = subprocess.check_output(['git', 'cat-file', '--batch'], cwd=root, input=request)
committed = {}
offset = 0
for path in paths:
    end = packed.index(b'\n', offset)
    header = packed[offset:end].split()
    assert len(header) == 3 and header[1] == b'blob', path
    length = int(header[2])
    offset = end + 1
    committed[path] = packed[offset:offset + length]
    offset += length + 1
assert offset == len(packed)
assert {p: digest(b) for p, b in committed.items()} == baseline['inputs_sha256']
assert committed['bench/devloop/src/main.rs'] == files['baseline-main.rs']
committed['bench/devloop/src/main.rs'] = files['candidate-main.rs']
assert {p: digest(b) for p, b in committed.items()} == candidate['inputs_sha256']
assert baseline['rustc'] == candidate['rustc']
for phase, receipt in [('baseline', baseline), ('candidate', candidate)]:
    assert receipt['base_commit'] == base
    assert receipt['exit_code'] == receipt['process_returncode'] == 0
    assert receipt['source_inputs_unchanged'] and not receipt['disk_guard']
    assert digest(files[phase + '-build.log']) == receipt['log_sha256']
assert b'25 passed; 0 failed' in files['candidate-build.log']
assert b'5 passed; 0 failed' in files['candidate-build.log']
assert 'cargo clippy' in candidate['command'][-1]

cells = ['codec.pbrs.fresh_encode', 'codec.prost.fresh_encode', 'codec.v4.fresh_encode']
warmups = [0, 1, 4, 8]
observed = {}
for phase in ['baseline', 'candidate']:
    capture = data(phase + '.json')
    assert capture['base_commit'] == base
    assert capture['harness_sha256'] == digest(files[phase + '-main.rs'])
    assert len(capture['binary_sha256']) == 64
    records = capture['records']
    assert len(records) == 12
    for record, (cell, warmup) in zip(records, [(c, w) for c in cells for w in warmups], strict=True):
        assert record['exit_code'] == 0 and not record['stdout']
        assert record['warmup'] == warmup
        assert record['command'][1:] == ['run-cell', cell, '--iters', '4', '--warmup', str(warmup), '--prepare-iters', '8']
        child_lines = [line.removeprefix('__CHILD__ ') for line in record['stderr'].splitlines() if line.startswith('__CHILD__ ')]
        assert len(child_lines) == 1
        child = json.loads(child_lines[0])
        assert child == record['child'] and child['cell'] == cell and child['iters'] == 4
        assert child.get('fresh_encode_profile') == ('independent-warmup-v1' if phase == 'candidate' else None)
        assert child['wall_ns'] > 0
        observed[phase, cell, warmup] = (child['allocs'], child['alloc_bytes'])

for warmup, expected in zip(warmups, [(28, 796), (22, 684), (4, 348), (4, 348)], strict=True):
    assert observed['baseline', cells[0], warmup] == expected
for cell, expected in zip(cells, [(28, 796), (20, 992), (4, 348)], strict=True):
    assert {observed['candidate', cell, w] for w in warmups} == {expected}
for cell in cells[1:]:
    assert {observed['baseline', cell, w] for w in warmups} == {observed['candidate', cell, 0]}

print(json.dumps(dict(passed=True, selected_inputs=len(paths), processes=24,
                     scope='Selected-source reconstruction, retained receipts and allocation arithmetic; no executable replay or CPU claim.')))
