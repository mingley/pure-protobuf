"""Check archived opt-in graphs, old registry bytes and actual API oracles."""
import hashlib
import json
from pathlib import Path

here = Path(__file__).resolve().parent
root = next(p for p in here.parents if (p / 'bench/devloop/adoption').is_dir())
sha = lambda path: hashlib.sha256(Path(path).read_bytes()).hexdigest()
index = json.loads((here / 'artifact-sha256.json').read_text())['sha256']
for name, expected in index.items():
    assert sha(here / name) == expected, name
graphs = json.loads((here / 'feature-graphs.json').read_text())
graphs = {label: {tuple(row['package']): row for row in rows} for label, rows in graphs.items()}
assert len(graphs['before']) == len(graphs['default']) == len(graphs['no_default']) == 153
assert len(graphs['bridge']) == 154
assert graphs['before'].keys() == graphs['default'].keys() == graphs['no_default'].keys()
for key, original in graphs['before'].items():
    for label in ('default', 'no_default'):
        current = graphs[label][key]
        assert current['dependencies'] == original['dependencies']
        assert current['features'] == original['features'] or (key[0] == 'devloop' and current['features'] == ['default'] and original['features'] == [])
assert {key[0] for key in graphs['bridge'].keys() - graphs['default'].keys()} == {'protobuf-tonic'}
old = (here / 'default-registry.txt').read_bytes()
new = (here / 'bridge-registry.txt').read_text().splitlines(keepends=True)
assert len(new) == 1970 and len(old.splitlines()) == 1634
assert ''.join(row for row in new if not row.startswith('codec.adoption.bridge.')).encode() == old
rows = json.loads((here / 'parent-bridge-inventory.json').read_text())['cells']
reference = json.loads((root / 'docs/evidence/tc32a-bridge-qualification-20261002/bridge-inventory.json').read_text())['cells']
reference = {row['id']: row for row in reference}
assert len(rows) == 336 and {row['id'] for row in rows} == reference.keys()
for row in rows:
    assert all(row[k] == reference[row['id']][k] for k in ('read_checksum', 'common_wire_bytes', 'common_wire_fingerprint'))
    if row['timing_qualification'] == 'passed':
        assert row == reference[row['id']]
assert sum(row['timing_qualification'] == 'passed' for row in rows) == 324
assert sum(row['timing_qualification'] == 'blocked' for row in rows) == 12
print('33 artifact hashes; 153 default/154 opt-in graph; 1634 old registry rows byte-identical; 336 retained actual API oracles; numeric costs not_run')
