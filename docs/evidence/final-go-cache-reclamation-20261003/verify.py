#!/usr/bin/env python3
"""Portable read-only proof checker; never compiles or reclaims anything."""
from pathlib import Path,PurePosixPath
import argparse,hashlib,json,io,tarfile,zipfile,base64,stat

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('directory',nargs='?',type=Path,default=Path(__file__).resolve().parent)
 root=parser.parse_args().directory.resolve();sha=lambda raw:hashlib.sha256(raw).hexdigest()
 read=lambda n:json.loads((root/n).read_bytes())
 pins=read('artifact-manifest.json')['files'];actual={str(p.relative_to(root)) for p in root.rglob('*') if p.is_file() and p.name!='artifact-manifest.json'}
 assert actual==set(pins)
 for name,pin in pins.items():
  path=PurePosixPath(name);assert not path.is_absolute() and '..' not in path.parts
  raw=(root/name).read_bytes();assert len(raw)==pin['bytes'] and sha(raw)==pin['sha256'],name
 before={};totals={};allfiles=0
 for label in ['cache','modcache']:
  a=read('inventory-before-'+label+'.json');b=read('inventory-pre-delete-'+label+'.json');q=read('inventory-quarantine-'+label+'.json')
  assert a==b and set(a['entries'])==set(q['entries'])
  for name,row in a['entries'].items():
   x=dict(row);y=dict(q['entries'][name])
   if name=='.':x.pop('ctime_ns');y.pop('ctime_ns')
   assert x==y,(label,name)
  entries=a['entries'];unique={}
  for row in entries.values():
   assert row['kind'] in ['file','directory']
   assert stat.S_ISREG(row['mode']) if row['kind']=='file' else stat.S_ISDIR(row['mode'])
   k=(row['dev'],row['ino'])
   if k in unique:assert row==unique[k]
   unique[k]=row
  calculated={'paths':len(entries),'file_paths':sum(r['kind']=='file' for r in entries.values()),'directory_paths':sum(r['kind']=='directory' for r in entries.values()),'unique_file_inodes':sum(r['kind']=='file' for r in unique.values()),'apparent_file_path_bytes':sum(r['size'] for r in entries.values() if r['kind']=='file'),'unique_regular_apparent_bytes':sum(r['size'] for r in unique.values() if r['kind']=='file'),'unique_regular_allocated_bytes':sum(r['blocks']*512 for r in unique.values() if r['kind']=='file'),'unique_directory_allocated_bytes':sum(r['blocks']*512 for r in unique.values() if r['kind']=='directory')}
  assert all(a['totals'][k]==v for k,v in calculated.items())
  before[label]=entries;totals[label]=calculated;allfiles+=calculated['file_paths']
 assert totals['cache']['file_paths']==3449 and totals['cache']['directory_paths']==257
 assert totals['modcache']['file_paths']==10150 and totals['modcache']['directory_paths']==3727
 assert sum(t['unique_regular_allocated_bytes']+t['unique_directory_allocated_bytes'] for t in totals.values())==1035022336
 selected=read('selected-payloads.json')['files'];expected={}
 for label,entries in before.items():
  for name,row in entries.items():
   if row['kind']=='file' and ((label=='cache' and not name.endswith('-d')) or (label=='modcache' and name.startswith('cache/download/'))):expected[label+'/'+name]={'bytes':row['size'],'sha256':row['sha256']}
 protected=read('protected-inputs.json')
 for name,row in protected['files'].items():
  p=Path(name)
  if p.suffix in ['.json','.txt','.stderr','.stdout','.jsonl','.py','.mod','.sum']:
   rel='external/'+str(p.relative_to('/workspace/pure-protobuf'));expected[rel]=row
 assert expected==selected
 parts=read('archive-parts.json');assert len(parts['parts'])==4
 stream=[]
 for row in parts['parts']:
  raw=(root/row['path']).read_bytes();assert len(raw)==row['bytes']<=25*1024**2 and sha(raw)==row['sha256'];stream.append(raw)
 archive=b''.join(stream);pin=read('archive.json')
 assert len(archive)==pin['bytes']==78821045 and sha(archive)==pin['sha256']=='f8735053b0315fb437997a4cc00d21fac25779cd1f2feb41891a209097f2c2e8'
 saved={};rows=[]
 with tarfile.open(fileobj=io.BytesIO(archive),mode='r|gz') as t:
  for m in t:
   path=PurePosixPath(m.name);assert not path.is_absolute() and '..' not in path.parts
   assert m.isfile() and not m.issym() and not m.islnk() and m.name in selected and m.name not in saved
   raw=t.extractfile(m).read();assert {'bytes':len(raw),'sha256':sha(raw)}==selected[m.name]
   rows.append({'name':m.name,'bytes':len(raw),'sha256':sha(raw)})
   saved[m.name]=raw
 assert set(saved)==set(selected) and len(saved)==2281 and rows==read('archive-members.json')
 frozen=saved['external/tests/interop/go/go.sum'].decode();module=read('runtime-module-zip-proof.json');tuples={tuple(x) for x in module['tuples']}
 assert len(tuples)==module['runtime_module_tuple_count']==32
 from_infos=set()
 for name in ['final-go-peer/server-module-info.txt','final-go-peer-retry1/server-module-info.txt','final-go-peer-retry1/client-module-info.txt']:
  for line in saved['external/work/'+name].decode().splitlines():
   fields=line.split('\t')
   if len(fields)>=5 and fields[1] in ['mod','dep']:from_infos.add(tuple(fields[2:5]))
 assert from_infos==tuples
 historical=json.loads(saved['external/work/final-go-peer-retry1/module-verification.json'])
 assert tuples=={tuple(x) for x in historical['unique_runtime_module_pins']}
 zip_members=0
 for row in module['zip_proofs']:
  assert (row['path'],row['version'],row['go_sum_h1']) in tuples
  assert f"{row['path']} {row['version']} {row['go_sum_h1']}\n" in frozen
  raw=saved['modcache/'+row['zip']];assert sha(raw)==row['zip_sha256'] and len(raw)==row['zip_bytes']
  assert saved['modcache/'+row['zip'].removesuffix('.zip')+'.ziphash'].decode().strip()==row['go_sum_h1']
  h=hashlib.sha256();members=[];escape=lambda s:''.join('!'+c.lower() if 'A'<=c<='Z' else c for c in s)
  with zipfile.ZipFile(io.BytesIO(raw)) as z:
   names=z.namelist();assert len(names)==len(set(names))
   for name in sorted(names):
    p=PurePosixPath(name);assert not p.is_absolute() and '..' not in p.parts and not name.endswith('/')
    payload=z.read(name);digest=sha(payload);h.update((digest+'  '+name+'\n').encode())
    members.append({'name':name,'bytes':len(payload),'sha256':digest})
    expanded=escape(row['path'])+'@'+escape(row['version'])+'/'+name.removeprefix(row['path']+'@'+row['version']+'/')
    assert before['modcache'][expanded]['sha256']==digest
   assert members==row['members'];zip_members+=len(members)
  assert 'h1:'+base64.b64encode(h.digest()).decode()==row['computed_h1']==row['go_sum_h1']
 rename=read('rename.json');completed=read('completed.json')
 assert completed['status']=='completed' and completed['removed_only']==rename
 assert completed['both_original_nodes_absent'] and completed['both_quarantine_nodes_absent'] and completed['protected_inputs_unchanged']
 assert not completed['new_compilation_run'] and not completed['source_or_peer_qualification_changed']
 for label,row in rename.items():
  assert row['from']=='/workspace/pure-protobuf/work/final-go-peer/'+label
  assert row['to']=='/workspace/pure-protobuf/work/final-go-cache-reclamation-20261003/quarantine-'+label
  assert (row['dev'],row['ino'])==(before[label]['.']['dev'],before[label]['.']['ino'])
 assert (rename['cache']['dev'],rename['cache']['ino'])==(27,1981700)
 assert (rename['modcache']['dev'],rename['modcache']['ino'])==(27,1981701)
 changes=read('quarantine-directory-permission-changes.json');expected_changes={}
 for label,entries in before.items():
  for name,r in entries.items():
   if r['kind']=='directory' and not r['mode']&stat.S_IWUSR:expected_changes[(label,name)]=r
 assert len(changes)==len(expected_changes)==completed['permission_changes_only_owned_quarantine_directories']==3594
 for c in changes:
  old=expected_changes.pop((c['cache'],c['relative']))
  assert (c['dev'],c['ino'],c['old_mode'])==(old['dev'],old['ino'],old['mode'])
  assert c['new_mode']==c['old_mode']|stat.S_IWUSR
 assert not expected_changes
 scans={}
 for phase in ['before-preservation','after-preservation','immediately-before-rename','before-exact-node-rename','after-exact-node-rename']:
  scan=read('proc/'+phase+'.json');assert not scan['matches']
  for view in scan['views']:
   assert set(view['surfaces'])==set(['exe','cwd','cmdline','environ','fd','maps'])
   for r in view['surfaces'].values():
    if r['state']=='read':assert not r['cache_references'] and len(r['sha256'])==64
  for r in scan['permission_limits']:
   s=r['status'];assert s.get('State','').startswith('Z') or (s.get('Uid','').split()[:1]==['0'] and s.get('Name') in ['dockerd','containerd','containerd-shim','containerd-shim-runc-v2','runc'])
  scans[phase]={'views':len(scan['views']),'permission':len(scan['permission_limits']),'transient':len(scan['transient'])}
 assert completed['global_available_change']==read('global-after.json')['free']-read('global-pre-delete.json')['free']==1026002944
 old=json.loads(saved['external/work/final-go-peer/build-record.json']);retry=json.loads(saved['external/work/final-go-peer-retry1/build-record.json'])
 assert old['status']=='failed_or_stopped' and old['phases'][1]['exit']==1
 assert retry['status']=='pass' and all(p['exit']==0 for p in retry['phases'])
 for record in [old,retry]:
  assert record['input_sha256_before']==record['input_sha256_after']
  for name,pin in record['input_sha256_before'].items():assert protected['files']['/workspace/pure-protobuf/'+name]['sha256']==pin
 print(json.dumps({'result':'passed','inventoried_file_paths':allfiles,'archive_payloads':len(saved),'runtime_module_tuples':len(tuples),'ZIP_source_members_verified':zip_members,'directory_permission_changes_verified':len(changes),'process_scans':scans,'new_compilation_run':False,'scope':'two exact completed Go cache inodes only; hash-only outputs and permission/race/physical-allocation limits retained'},indent=2))
if __name__=='__main__':main()
