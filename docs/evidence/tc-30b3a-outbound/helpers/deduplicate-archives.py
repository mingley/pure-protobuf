"""Deduplicate immutable preserved archive copies, never live cache files."""
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys

here=Path(__file__).resolve().parent
assert os.environ.get('TC30B_OUTBOUND_ORDINARY_LEASE')=='granted'
archives=here/'artifacts'
name=sys.argv[1]
record=here/(name+'.json')
assert not record.exists()
def size(): return int(subprocess.check_output(['du','-sb',str(archives)],text=True).split()[0])
before=size()
seen={}
linked=[]
for manifest in sorted(archives.glob('*/manifest.json')):
    for item in json.loads(manifest.read_text())['artifacts']:
        path=manifest.parent/item['path']
        with path.open('rb') as stream: digest=hashlib.file_digest(stream,'sha256').hexdigest()
        assert digest==item['sha256'], path
        info=path.stat()
        key=(digest,stat.S_IMODE(info.st_mode))
        if key in seen and (info.st_dev,info.st_ino)!=(seen[key].stat().st_dev,seen[key].stat().st_ino):
            temporary=path.with_name(path.name+'.deduplicate-temporary')
            assert not temporary.exists()
            os.link(seen[key],temporary)
            os.replace(temporary,path)
            linked.append({'path':str(path.relative_to(archives)),'sha256':digest,'bytes':item['bytes']})
        else: seen[key]=path
record.write_text(json.dumps({'before_unique_bytes':before,'after_unique_bytes':size(),'linked':linked,'scope':'Immutable archive copies only; full original paths and byte hashes preserved; no live cache links'},indent=2)+'\n')
print(json.dumps({'before':before,'after':size(),'archive_paths_deduplicated':len(linked)}))
