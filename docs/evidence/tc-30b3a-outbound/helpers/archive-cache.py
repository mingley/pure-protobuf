"""Preserve completed cache ELF/generated/fingerprint inputs before retirement."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys

here=Path(__file__).resolve().parent
assert os.environ.get('TC30B_OUTBOUND_ORDINARY_LEASE')=='granted'
plan=json.loads((here/'plan.json').read_text())
cache=Path(plan['environment']['CARGO_TARGET_DIR'])
name=sys.argv[1]
output=here/'artifacts'/name
assert not output.exists(), output
active=[]
for proc in Path('/proc').glob('[0-9]*'):
    try:
        exe=(proc/'exe').resolve(strict=True)
        argv=(proc/'cmdline').read_bytes().replace(b'\0',b' ').decode(errors='replace')
        uses=str(exe).startswith(str(cache)+'/')
        if exe.name in {'cargo','rustc','rustdoc','clippy-driver'}:
            env=(proc/'environ').read_bytes().split(b'\0')
            uses|=any(x==b'CARGO_TARGET_DIR='+str(cache).encode() for x in env) or str(cache) in argv
        if uses: active.append({'pid':int(proc.name),'exe':str(exe),'argv':argv})
    except (FileNotFoundError,ProcessLookupError,PermissionError): pass
assert not active,active
output.mkdir(parents=True)
artifacts=[]
preserved={}
for manifest in (here/'artifacts').glob('*/manifest.json'):
    for item in json.loads(manifest.read_text())['artifacts']:
        other=manifest.parent/item['path']
        if other.is_file(): preserved[(item['sha256'],stat.S_IMODE(other.stat().st_mode))]=other
for path in sorted(cache.rglob('*')):
    if not path.is_file(): continue
    rel=path.relative_to(cache)
    generated=path.suffix=='.rs' and 'out' in rel.parts
    metadata='.fingerprint' in rel.parts or path.suffix=='.d' or ('build' in rel.parts and path.name in {'output','stderr','root-output','invoked.timestamp'})
    elf=False
    if path.suffix in {'', '.so'}:
        with path.open('rb') as stream: elf=stream.read(4)==b'\x7fELF'
    if not (generated or metadata or elf): continue
    size=path.stat().st_size
    with path.open('rb') as stream: digest=hashlib.file_digest(stream,'sha256').hexdigest()
    target=output/rel
    target.parent.mkdir(parents=True,exist_ok=True)
    key=(digest,stat.S_IMODE(path.stat().st_mode))
    if key in preserved:
        os.link(preserved[key],target)
    else:
        assert shutil.disk_usage(here).free-size>=2*1024**3, 'reserve must remain during artifact preservation'
        shutil.copy2(path,target)
        preserved[key]=target
    with target.open('rb') as stream: assert hashlib.file_digest(stream,'sha256').hexdigest()==digest
    artifacts.append({'path':str(rel),'kind':'elf' if elf else 'generated_rust' if generated else 'fingerprint_or_dependency','bytes':size,'sha256':digest})
record={'cache':str(cache),'archived_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'active_cache_consumers':active,'cache_bytes':int(subprocess.check_output(['du','-sb',str(cache)],text=True).split()[0]),'artifacts':artifacts,'stage_records':{p.name:json.loads(p.read_text()).get('source_before') for p in here.glob('*.json') if 'source_before' in json.loads(p.read_text())},'performance':'not_run'}
(output/'manifest.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps({'archive':str(output),'elf':sum(x['kind']=='elf' for x in artifacts),'generated':sum(x['kind']=='generated_rust' for x in artifacts),'metadata':sum(x['kind']=='fingerprint_or_dependency' for x in artifacts),'bytes':sum(x['bytes'] for x in artifacts),'cache_bytes':record['cache_bytes']}))
if len(sys.argv)>2 and sys.argv[2]=='--retire':
    assert cache==Path('/workspace/scratch/work/tc30b-outbound/target/qualification')
    shutil.rmtree(cache)
    (output/'retired.json').write_text(json.dumps({'cache':str(cache),'retired_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'free':shutil.disk_usage(here).free})+'\n')
