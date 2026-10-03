"""Verify full selected command evidence without extracting archive members."""
from pathlib import Path
import hashlib
import json
import tarfile

ROOT = Path(__file__).resolve().parent

def require(value, message):
    if not value:
        raise RuntimeError(message)

def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for data in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(data)
    return value.hexdigest()

manifest = json.loads((ROOT / 'manifest.json').read_text())
archive = ROOT / 'raw-command-proof.tar.gz'
require(manifest['status'] == 'PASS_FULL_SELECTED_RAW_COMMAND_CAPSULE', 'unqualified manifest')
require(archive.stat().st_size == manifest['archive']['bytes']
        and digest(archive) == manifest['archive']['sha256'], 'archive byte identity differs')
rows = {row['member']: row for row in manifest['rows']}
require(len(rows) == len(manifest['rows']) == 228, 'member coverage differs')
seen = set()
with tarfile.open(archive, 'r:gz') as stream:
    for member in stream:
        require(member.isfile() and member.name in rows and member.name not in seen,
                'unexpected, duplicate, or nonregular member')
        row = rows[member.name]
        require(member.size == row['bytes'] and member.mode == 0o644, 'member size or capsule mode differs')
        value = hashlib.sha256()
        source = stream.extractfile(member)
        count = 0
        for data in iter(lambda: source.read(1024 * 1024), b''):
            count += len(data)
            value.update(data)
        require(count == row['bytes'] and value.hexdigest() == row['sha256'], 'member payload differs')
        seen.add(member.name)
require(seen == set(rows) and manifest['command_records'] == 44
        and manifest['raw_command_files'] == 176 and manifest['hash_only_included_members'] == 0,
        'selected command scope differs')
print('PASS: 228 complete members, 44 command records, 176 raw command files')
