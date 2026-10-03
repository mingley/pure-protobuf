#!/usr/bin/env python3
"""Authorized completed Go caches only; phases preserve, then exact-node delete."""
from pathlib import Path,PurePosixPath
import os,stat,json,hashlib,subprocess,sys,datetime,collections,errno,re,tarfile,gzip,io,zipfile,base64,shutil,fcntl
R=Path('/workspace/pure-protobuf');W=R/'work/final-go-cache-reclamation-20261003'
BASE='a4f012ab7aae9ba579dac3b0133e3dc18a1251ff'
NODES={'cache':R/'work/final-go-peer/cache','modcache':R/'work/final-go-peer/modcache'}
QUAR={name:W/('quarantine-'+name) for name in NODES}
FLOOR=2*1024**3
FIELDS=('dev','ino','mode','uid','gid','nlink','size','blocks','mtime_ns','ctime_ns')
sha=lambda raw:hashlib.sha256(raw).hexdigest()
def utc():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def dump(name,j):
 p=W/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(j,indent=2,sort_keys=True)+'\n')
def load(name):return json.loads((W/name).read_text())
def meta(s):return {k:getattr(s,'st_'+k) for k in FIELDS}
def filehash(p):
 before=p.lstat();assert stat.S_ISREG(before.st_mode),str(p)
 fd=os.open(p,os.O_RDONLY|os.O_NOFOLLOW)
 try:
  assert meta(os.fstat(fd))==meta(before)
  h=hashlib.sha256()
  while b:=os.read(fd,1024**2):h.update(b)
  assert meta(os.fstat(fd))==meta(before)
 finally:os.close(fd)
 assert meta(p.lstat())==meta(before)
 return h.hexdigest()
def inventory(root):
 assert root.resolve()==root and root.is_dir()
 entries={}
 def visit(p):
  s=p.lstat();rel=str(p.relative_to(root));assert s.st_uid==os.getuid()
  if stat.S_ISDIR(s.st_mode):
   entries[rel]={'kind':'directory',**meta(s)}
   for c in sorted(p.iterdir()):visit(c)
  else:
   assert stat.S_ISREG(s.st_mode),str(p)
   entries[rel]={'kind':'file',**meta(s),'sha256':filehash(p)}
 visit(root)
 unique={}
 for v in entries.values():unique.setdefault((v['dev'],v['ino']),v)
 totals={'paths':len(entries),'file_paths':sum(v['kind']=='file' for v in entries.values()),'directory_paths':sum(v['kind']=='directory' for v in entries.values()),'unique_file_inodes':sum(v['kind']=='file' for v in unique.values()),'apparent_file_path_bytes':sum(v['size'] for v in entries.values() if v['kind']=='file'),'unique_regular_apparent_bytes':sum(v['size'] for v in unique.values() if v['kind']=='file'),'unique_regular_allocated_bytes':sum(v['blocks']*512 for v in unique.values() if v['kind']=='file'),'unique_directory_allocated_bytes':sum(v['blocks']*512 for v in unique.values() if v['kind']=='directory'),'scope':'stat st_blocks by device/inode, not exclusive physical/COW extents; free-space observation may change concurrently'}
 return {'root':str(root),'entries':entries,'totals':totals}
def space():
 d=shutil.disk_usage(R);return {'utc':utc(),'free':d.free,'used':d.used,'total':d.total}
def git(*args):return subprocess.check_output(['git','-C',str(R),*args])
def status(p):
 try:return {k:v.strip() for line in (p/'status').read_text().splitlines() for k,s,v in [line.partition(':')] if k in ['Name','State','Uid','Gid','NSpid','Threads']}
 except OSError as e:return {'error':e.errno}
def procscan(label,paths):
 start=utc();views=[];matches=[];permission=[];transient=[]
 patterns=[re.compile(re.escape(str(p))+r'(?=/|\x00|\s|$|[;"\'])') for p in paths]
 for leader in sorted(Path('/proc').iterdir()):
  if not leader.name.isdigit():continue
  try:tasks=list((leader/'task').iterdir())
  except OSError as e:tasks=[];transient.append({'pid':leader.name,'surface':'task','errno':e.errno})
  for p in [leader,*[x for x in tasks if x.name!=leader.name]]:
   row={'pid':int(leader.name),'view':str(p),'status':status(p),'surfaces':{}}
   for surface in ['exe','cwd','cmdline','environ','fd','maps']:
    try:
     if surface in ['exe','cwd']:raw=os.readlink(p/surface).encode()
     elif surface=='fd':
      links=[]
      for f in (p/'fd').iterdir():
       try:links.append(os.readlink(f))
       except FileNotFoundError:pass
      raw='\n'.join(links).encode()
     else:raw=(p/surface).read_bytes()
     text=raw.decode(errors='replace');refs=[str(path) for path,pattern in zip(paths,patterns) if pattern.search(text)]
     row['surfaces'][surface]={'state':'read','bytes':len(raw),'sha256':sha(raw),'cache_references':refs}
     if refs:matches.append({'pid':row['pid'],'surface':surface,'view':str(p),'references':refs})
    except OSError as e:
     rec={'pid':row['pid'],'view':str(p),'surface':surface,'errno':e.errno,'status':row['status']}
     row['surfaces'][surface]={'state':'unreadable',**rec}
     (transient if e.errno in [errno.ENOENT,errno.ESRCH] else permission).append(rec)
   views.append(row)
 result={'phase':label,'started_utc':start,'ended_utc':utc(),'views':views,'matches':matches,'permission_limits':permission,'transient':transient,'limits':'Readable surfaces in current container namespace only; permissions, enumeration races, PID reuse and future writers remain. Raw argv/environment values never retained. Work-only advisory owner lock is not respected by Go itself.'}
 dump('proc/'+label+'.json',result);assert not matches,(label,matches)
 unexpected=[x for x in permission if not x['status'].get('State','').startswith('Z') and not(x['status'].get('Uid','').split()[:1]==['0'] and x['status'].get('Name') in ['dockerd','containerd','containerd-shim','containerd-shim-runc-v2','runc'])]
 assert not unexpected,unexpected
 print(json.dumps({'phase':label,'views':len(views),'matches':len(matches),'permission':len(permission),'transient':len(transient)}),flush=True)
 return result
class GuardedWriter:
 def __init__(self,f):self.f=f;self.next=0
 def write(self,b):
  if self.f.tell()>=self.next:assert space()['free']>=FLOOR+len(b);self.next=self.f.tell()+1024**2
  return self.f.write(b)
 def flush(self):return self.f.flush()
def pin_external():
 paths=[]
 for root in [R/'work/final-go-peer',R/'work/final-go-peer-retry1']:
  for p in root.iterdir():
   if p.is_file():paths.append(p)
 paths += [R/'work/build-final-go-peer.py',R/'work/run-final-native-go.py',R/'tests/interop/go/go.mod',R/'tests/interop/go/go.sum']
 for x in ['bin/go','pkg/tool/linux_amd64/compile','pkg/tool/linux_amd64/link','pkg/tool/linux_amd64/asm']:paths.append(R/'work/toolchain/go1253/go'/x)
 roots=[R/'work/toolchain/go1253',R/'work/toolchain/go-mod-cache',R/'work/final-go-peer/gopath',R/'work/final-go-peer/tmp',R/'work/final-go-peer-retry1/tmp']
 return {'files':{str(p):{'sha256':filehash(p),'bytes':p.stat().st_size} for p in sorted(set(paths))},'protected_roots':{str(p):{'dev':p.stat().st_dev,'ino':p.stat().st_ino} for p in roots},'scope':'All existing top-level first-fail/retry binaries/moduleinfos/records/raw/task scripts/frozen mod+sum and four Go tool bytes are pinned. Protected SDK/shared module-cache root identities remain; their entire contents are not independently inventoried by this cleanup.'}
def verify_external(j):
 for name,v in j['files'].items():assert Path(name).stat().st_size==v['bytes'] and filehash(Path(name))==v['sha256'],name
 for name,v in j['protected_roots'].items():assert Path(name).is_dir() and (Path(name).stat().st_dev,Path(name).stat().st_ino)==(v['dev'],v['ino']),name
def archive_verify(expected):
 seen={};rows=[]
 with tarfile.open(W/'selected-payloads.tar.gz','r|gz') as archive:
  for m in archive:
   assert m.isfile() and not m.islnk() and not m.issym()
   p=PurePosixPath(m.name);assert not p.is_absolute() and '..' not in p.parts
   assert m.name in expected and m.name not in seen
   h=hashlib.sha256();size=0
   with archive.extractfile(m) as f:
    while b:=f.read(1024**2):h.update(b);size+=len(b)
   assert expected[m.name]=={'sha256':h.hexdigest(),'bytes':size},m.name
   seen[m.name]=h.hexdigest();rows.append({'name':m.name,'sha256':h.hexdigest(),'bytes':size})
 assert set(seen)==set(expected);return rows

def preserve():
 assert load('owner-clearance.json')['cache_readers_idle_confirmed']
 assert not (W/'preserved.json').exists() and all(not q.exists() for q in QUAR.values())
 head=git('rev-parse','HEAD').decode().strip();assert subprocess.run(['git','-C',str(R),'merge-base','--is-ancestor',BASE,head]).returncode==0
 dump('main-observation.json',{'requested_parent':BASE,'observed_descendant':head,'direct_source_mutation':False})
 dump('global-before.json',space());assert space()['free']>=FLOOR
 procscan('before-preservation',list(NODES.values()))
 inv={name:inventory(root) for name,root in NODES.items()}
 for name,j in inv.items():dump('inventory-before-'+name+'.json',j)
 external=pin_external();dump('protected-inputs.json',external)
 frozen=(R/'tests/interop/go/go.sum').read_text();runtime=set()
 for name in ['work/final-go-peer/server-module-info.txt','work/final-go-peer-retry1/server-module-info.txt','work/final-go-peer-retry1/client-module-info.txt']:
  for line in (R/name).read_text().splitlines():
   p=line.split('\t')
   if len(p)>=5 and p[1] in ['mod','dep']:runtime.add(tuple(p[2:5]))
 verified=json.loads((R/'work/final-go-peer-retry1/module-verification.json').read_text())
 assert runtime=={tuple(x) for x in verified['unique_runtime_module_pins']} and len(runtime)==32
 selected={};inputs={};zips=[]
 for label,j in inv.items():
  for rel,row in j['entries'].items():
   if row['kind']!='file':continue
   keep=(label=='cache' and not rel.endswith('-d')) or (label=='modcache' and rel.startswith('cache/download/'))
   if keep: name=label+'/'+rel;selected[name]={'sha256':row['sha256'],'bytes':row['size']};inputs[name]=NODES[label]/rel
 for path,version,h1 in sorted(runtime):
  assert f'{path} {version} {h1}\n' in frozen
  escape=lambda s:''.join('!'+c.lower() if 'A'<=c<='Z' else c for c in s)
  prefix='cache/download/'+escape(path)+'/@v/'+escape(version)
  zp=NODES['modcache']/(prefix+'.zip'); zh=NODES['modcache']/(prefix+'.ziphash')
  assert zh.read_text().strip()==h1
  h=hashlib.sha256();members=[]
  with zipfile.ZipFile(zp) as archive:
   names=archive.namelist();assert len(names)==len(set(names))
   for n in sorted(names):
    assert not n.endswith('/'),n
    rel=PurePosixPath(n);assert not rel.is_absolute() and '..' not in rel.parts
    raw=archive.read(n);digest=sha(raw);h.update((digest+'  '+n+'\n').encode())
    expanded=escape(path)+'@'+escape(version)+'/'+n.removeprefix(path+'@'+version+'/')
    assert inv['modcache']['entries'][expanded]['sha256']==digest,(path,n)
    members.append({'name':n,'bytes':len(raw),'sha256':digest})
  actual='h1:'+base64.b64encode(h.digest()).decode();assert actual==h1
  zips.append({'path':path,'version':version,'go_sum_h1':h1,'computed_h1':actual,'zip':prefix+'.zip','zip_sha256':filehash(zp),'zip_bytes':zp.stat().st_size,'members':members})
 dump('runtime-module-zip-proof.json',{'runtime_module_tuple_count':32,'tuples':sorted(runtime),'zip_proofs':zips,'expanded_source_files_match_zip_payloads':True,'no_go_process_run':True})
 # Retain top-level build inputs and records. Binaries remain at protected paths
 # and in the peer owner's sealed F2 proof, avoiding redundant binary copies.
 for name in external['files']:
  p=Path(name)
  if p.suffix in ['.json','.txt','.stderr','.stdout','.jsonl','.py','.mod','.sum']:
   arc='external/'+str(p.relative_to(R));selected[arc]={'sha256':external['files'][name]['sha256'],'bytes':external['files'][name]['bytes']};inputs[arc]=p
 dump('selected-payloads.json',{'files':selected,'coverage':'All source32ZIPs and all module-download metadata lossless; full build action-index metadata. Extracted module trees and -d compiled build outputs have complete hashes/metadata only; Go binaries and SDK remain protected outside this archive.'})
 with (W/'selected-payloads.tar.gz').open('xb') as raw:
  with gzip.GzipFile(fileobj=GuardedWriter(raw),mode='wb',compresslevel=6,mtime=0) as zipped:
   with tarfile.open(fileobj=zipped,mode='w|',format=tarfile.PAX_FORMAT) as archive:
    for name,p in sorted(inputs.items()):
     assert filehash(p)==selected[name]['sha256']
     info=tarfile.TarInfo(name);info.size=selected[name]['bytes'];info.mode=stat.S_IMODE(p.stat().st_mode)
     with p.open('rb') as f:archive.addfile(info,f)
  raw.flush();os.fsync(raw.fileno())
 rows=archive_verify(selected);dump('archive-members.json',rows)
 dump('archive.json',{'sha256':filehash(W/'selected-payloads.tar.gz'),'bytes':(W/'selected-payloads.tar.gz').stat().st_size,'members':len(rows)})
 verify_external(external);procscan('after-preservation',list(NODES.values()))
 dump('preserved.json',{'status':'payloads_verified','at':utc(),'archive':load('archive.json'),'cache_inventories':{label:j['totals'] for label,j in inv.items()},'protected_inputs_verified':True,'new_compilation_run':False,'not_deleted_yet':True})
 print(json.dumps(load('preserved.json')),flush=True)

def reclaim():
 assert load('preserved.json')['status']=='payloads_verified'
 assert not (W/'completed.json').exists() and all(not q.exists() for q in QUAR.values())
 external=load('protected-inputs.json');verify_external(external)
 archive_verify(load('selected-payloads.json')['files'])
 procscan('immediately-before-rename',list(NODES.values()))
 inv={}
 for label,node in NODES.items():
  fresh=inventory(node);dump('inventory-pre-delete-'+label+'.json',fresh)
  assert fresh==load('inventory-before-'+label+'.json'),label
  inv[label]=fresh
 procscan('before-exact-node-rename',list(NODES.values()))
 rename={};dump('global-pre-delete.json',space());assert space()['free']>=FLOOR
 for label,node in NODES.items():
  assert node.resolve()==node and node.parent==R/'work/final-go-peer'
  assert meta(node.lstat())=={k:inv[label]['entries']['.'][k] for k in FIELDS}
  os.rename(node,QUAR[label]);s=QUAR[label].lstat();expected=inv[label]['entries']['.']
  assert (s.st_dev,s.st_ino)==(expected['dev'],expected['ino'])
  assert not node.exists()
  rename[label]={'from':str(node),'to':str(QUAR[label]),'dev':s.st_dev,'ino':s.st_ino,'at':utc()}
 dump('rename.json',rename)
 procscan('after-exact-node-rename',[*NODES.values(),*QUAR.values()])
 permissions=[]
 for label,q in QUAR.items():
  fresh=inventory(q);dump('inventory-quarantine-'+label+'.json',fresh)
  before=inv[label]['entries'];assert fresh['entries'].keys()==before.keys()
  for name,x in before.items():
   a=dict(x);b=dict(fresh['entries'][name])
   if name=='.':a.pop('ctime_ns');b.pop('ctime_ns')
   assert a==b,(label,name)
  # Only owned read-only directories inside the exact quarantined inode receive
  # owner-write to permit unlink/rmdir. File modes never change; full old modes
  # and identities are already retained above.
  for name,row in before.items():
   if row['kind']=='directory' and not row['mode']&stat.S_IWUSR:
    p=q/name;s=p.lstat();assert (s.st_dev,s.st_ino,s.st_uid)==(row['dev'],row['ino'],os.getuid())
    assert p.resolve().is_relative_to(q) and stat.S_ISDIR(s.st_mode)
    os.chmod(p,stat.S_IMODE(row['mode'])|stat.S_IWUSR,follow_symlinks=False)
    permissions.append({'cache':label,'relative':name,'dev':row['dev'],'ino':row['ino'],'old_mode':row['mode'],'new_mode':p.lstat().st_mode})
 dump('quarantine-directory-permission-changes.json',permissions)
 assert getattr(shutil.rmtree,'avoids_symlink_attacks',False)
 for label,q in QUAR.items():
  s=q.lstat();assert (s.st_dev,s.st_ino)==(rename[label]['dev'],rename[label]['ino'])
  shutil.rmtree(q)
  assert not q.exists()
 verify_external(external)
 dump('global-after.json',space())
 dump('completed.json',{'status':'completed','finished_utc':utc(),'removed_only':rename,'both_original_nodes_absent':all(not p.exists() for p in NODES.values()),'both_quarantine_nodes_absent':all(not p.exists() for p in QUAR.values()),'protected_inputs_unchanged':True,'permission_changes_only_owned_quarantine_directories':len(permissions),'new_compilation_run':False,'source_or_peer_qualification_changed':False,'global_available_change':load('global-after.json')['free']-load('global-pre-delete.json')['free'],'limits':'Coordinated idle owner plus instantaneous readable proc surfaces and exact fresh hashes/inodes. Permissions/races/future writers remain; nominal allocations distinct from concurrent global space.'})
 print(json.dumps(load('completed.json')),flush=True)

if __name__=='__main__':
 fd=os.open(W/'owner.lock',os.O_CREAT|os.O_RDWR,0o600);fcntl.flock(fd,fcntl.LOCK_EX|fcntl.LOCK_NB)
 try:
  if sys.argv[1:]==['preserve']:preserve()
  elif sys.argv[1:]==['reclaim']:reclaim()
  else:raise ValueError('explicit phase required')
 except Exception as e:
  dump('failure-'+datetime.datetime.now(datetime.timezone.utc).strftime('%H%M%S')+'.json',{'at':utc(),'phase':sys.argv[1:],'type':type(e).__name__,'message':str(e),'original_nodes_exist':{n:p.exists() for n,p in NODES.items()},'quarantines_exist':{n:p.exists() for n,p in QUAR.items()}})
  raise
 finally:os.close(fd)
