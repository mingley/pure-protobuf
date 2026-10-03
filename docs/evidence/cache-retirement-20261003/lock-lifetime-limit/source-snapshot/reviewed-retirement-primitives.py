#!/usr/bin/env python3
"""Authorized cache-only preservation/retirement. No compiler or performance launch."""
import argparse, collections, contextlib, datetime, fcntl, gzip, hashlib, json, os, pathlib, shutil, stat, subprocess, sys, tarfile, time, traceback
CAP = 2147483648
MARGIN = 16777216
HERE = pathlib.Path(__file__).resolve().parent
STAGES = {
 'bench_stable': ('/workspace/scratch/work/rx10/target/qualification/bench_stable', '/workspace/scratch/work/rx10'),
 'native185': ('/workspace/scratch/work/rx10/target/qualification/native185', '/workspace/scratch/work/rx10'),
 'native188': ('/workspace/scratch/work/rx10/target/qualification/native188', '/workspace/scratch/work/rx10'),
 'bench_final': ('/workspace/scratch/work/rx10/target/qualification/bench_final', '/workspace/scratch/work/rx10'),
 'bench_final188': ('/workspace/scratch/work/rx10/target/qualification/bench_final188', '/workspace/scratch/work/rx10'),
}
OWNED = [pathlib.Path(p) for p in ['/workspace/pure-protobuf/work/gn13/target','/workspace/scratch/work/cl07/target','/workspace/scratch/work/deadline-admission-native/target','/workspace/scratch/work/rx10/target','/workspace/scratch/work/rx10-baseline/target']]
INPUTS=['Cargo.toml','Cargo.lock','build.rs','clippy.toml','rustfmt.toml','src','tests','pbrs-grpc','protobuf-tonic','proto','vendor/google','rpc-bench','bench/devloop','prost_tat','v4_tat']
ACTIVE_RESOURCES=None

def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def dump(path,obj):
 data=(json.dumps(obj,indent=2)+'\n').encode()
 if ACTIVE_RESOURCES: ACTIVE_RESOURCES.check('evidence_write_before',len(data))
 path.write_bytes(data)
 if ACTIVE_RESOURCES: ACTIVE_RESOURCES.check('evidence_write_after')
def identity(info): return {k:getattr(info,k) for k in ['st_dev','st_ino','st_mode','st_nlink','st_size','st_mtime_ns','st_ctime_ns','st_uid','st_gid']}
def stable(info): return {k:v for k,v in info.items() if k not in ('st_ctime_ns',)}

class Resources:
 def __init__(self,out): self.path=out/'resources.jsonl'; self.last=0; self.samples=[]
 def check(self,phase,write_bytes=0,force=False):
  now=time.monotonic()
  free=os.statvfs('/workspace').f_bavail*os.statvfs('/workspace').f_frsize
  if free < CAP + write_bytes + (MARGIN if write_bytes else 0):
   raise RuntimeError(f'global reserve would be breached: free={free}, pending_write={write_bytes}')
  if force or now-self.last>=2:
   sizes={}
   for root in OWNED:
    total=0
    for parent,_,names in os.walk(root,followlinks=False):
     for name in names:
      try:
       info=(pathlib.Path(parent)/name).stat()
       if stat.S_ISREG(info.st_mode): total+=info.st_size
      except FileNotFoundError: pass
    sizes[str(root)]=total
   sample={'utc':utc(),'phase':phase,'free_bytes':free,'owned_cache_bytes':sizes,'owned_total':sum(sizes.values()),'cap':CAP,'reserve':CAP}
   data=(json.dumps(sample)+'\n').encode()
   if free < CAP+len(data)+MARGIN: raise RuntimeError('resource sample write reserve would be breached')
   with self.path.open('ab') as file: file.write(data)
   self.samples.append(sample); self.last=now
   if sample['owned_total']>CAP: raise RuntimeError('fixed aggregate owned-cache cap breached')
  return free

class Writer:
 def __init__(self,path,resources): self.file=path.open('xb'); self.resources=resources; self.count=0
 def write(self,data):
  self.resources.check('archive_write_before',len(data))
  written=self.file.write(data); self.file.flush(); self.count+=written
  self.resources.check('archive_write_after')
  return written
 def flush(self): self.file.flush()
 def tell(self): return self.file.tell()
 def close(self): self.file.close()


def file_sha(path,resources=None):
 before=identity(path.lstat()); h=hashlib.sha256()
 with path.open('rb') as file:
  if identity(os.fstat(file.fileno()))!=before: raise RuntimeError('file changed before hashing '+str(path))
  for chunk in iter(lambda:file.read(1048576),b''):
   h.update(chunk)
   if resources: resources.check('hashing')
  if identity(os.fstat(file.fileno()))!=before: raise RuntimeError('file changed during hashing '+str(path))
 if identity(path.lstat())!=before: raise RuntimeError('file changed after hashing '+str(path))
 return h.hexdigest()

def tool_identity(path):
 path=pathlib.Path(path)
 entry=identity(path.lstat())
 resolved=path.resolve(strict=True)
 return {'invoked_path':str(path),'invoked_entry_identity':entry,
         'symlink_target':os.readlink(path) if path.is_symlink() else None,
         'resolved_path':str(resolved),'resolved_identity':identity(resolved.lstat()),
         'sha256':file_sha(resolved)}

def source_input(path,resources):
 if not path.is_symlink(): return {'kind':'regular','sha256':file_sha(path,resources)}
 entry=identity(path.lstat()); link=os.readlink(path); resolved=path.resolve(strict=True)
 value={'kind':'symlink','link':link,'entry_identity':entry,'resolved_path':str(resolved),
        'resolved_sha256':file_sha(resolved,resources)}
 if identity(path.lstat())!=entry or os.readlink(path)!=link: raise RuntimeError('source symlink changed '+str(path))
 return value

def scan(root):
 entries={'.': {'kind':'directory','identity':identity(root.lstat())}}
 for parent,dirs,files in os.walk(root,followlinks=False):
  for name in dirs+files:
   path=pathlib.Path(parent)/name; info=path.lstat(); rel=str(path.relative_to(root))
   row={'identity':identity(info)}
   if stat.S_ISREG(info.st_mode): row['kind']='file'
   elif stat.S_ISDIR(info.st_mode): row['kind']='directory'
   elif stat.S_ISLNK(info.st_mode): row.update(kind='symlink',link=os.readlink(path))
   else: raise RuntimeError('unapproved special cache entry '+str(path))
   entries[rel]=row
 return entries


def selected(stage,rel,row):
 if stage!='semver' or row['kind']!='file': return True
 path=pathlib.Path(rel); parts=path.parts
 return row['elf'] or '.fingerprint' in parts or 'build' in parts or 'deps' in parts or path.suffix in ('.d','.json') or path.name in ('Cargo.toml','Cargo.lock','.cargo-lock') or 'debug' not in parts


def hash_manifest(stage,root,entries,resources):
 hashes={}; groups=collections.defaultdict(list)
 for rel,row in sorted(entries.items()):
  if row['kind']!='file': continue
  path=root/rel; key=(row['identity']['st_dev'],row['identity']['st_ino']); groups[key].append(rel)
  if key not in hashes: hashes[key]=file_sha(path,resources)
  if identity(path.lstat())!=row['identity']: raise RuntimeError('file identity drift '+str(path))
  row['sha256']=hashes[key]
  with path.open('rb') as stream: row['elf']=stream.read(4)==b'\x7fELF'
 for rel,row in entries.items():
  row['disposition']='archived' if selected(stage,rel,row) else 'hash_only'
 for paths in groups.values():
  row=entries[paths[0]]
  if row['identity']['st_nlink']!=len(paths): raise RuntimeError('unlocated outside hardlink aliases '+repr(paths))
  for rel in paths: entries[rel]['hardlink_aliases']=paths
 return entries


def ancestors():
 found=set(); pid=os.getpid()
 while pid and pid not in found:
  found.add(pid)
  try: pid=int(pathlib.Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[1])
  except FileNotFoundError: break
 return found


def users(root):
 exclusions=ancestors(); matches=[]; denials=[]; zombies=0
 for proc in pathlib.Path('/proc').iterdir():
  if not proc.name.isdigit() or int(proc.name) in exclusions: continue
  try:
   fields=(proc/'stat').read_text().rsplit(')',1)[1].split()
   if fields[0]=='Z': zombies+=1; continue
   argv=(proc/'cmdline').read_bytes().replace(b'\0',b' ').decode(errors='replace')
   values={'argv':argv}
   for name in ('cwd','exe','maps','fd'):
    try:
     if name in ('cwd','exe'): values[name]=os.readlink(proc/name)
     elif name=='maps': values[name]=(proc/name).read_text()
     else:
      for fd in (proc/name).iterdir():
       try: values['fd:'+fd.name]=os.readlink(fd)
       except FileNotFoundError: pass
    except PermissionError as error: denials.append({'pid':int(proc.name),'field':name,'argv':argv,'error':repr(error)})
    except FileNotFoundError: pass
   chosen=[key for key,value in values.items() if str(root) in value]
   if chosen: matches.append({'pid':int(proc.name),'argv':argv,'fields':chosen})
  except FileNotFoundError: pass
 result={'utc':utc(),'matches':matches,'visibility_denials':denials,'zombies_excluded':zombies,'inspection_ancestors_excluded':sorted(exclusions),'scope':'point-in-time; root daemon inaccessible fd/maps retained; held locks/coordinator ownership supply separate protection'}
 if matches: raise RuntimeError('live cache users '+repr(result))
 return result


def source_context(source,resources):
 head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()
 status=subprocess.check_output(['git','status','--porcelain'],cwd=source,text=True)
 if status: raise RuntimeError('source worktree dirty '+str(source))
 paths=subprocess.check_output(['git','ls-files','-z','--',*INPUTS],cwd=source).split(b'\0')
 return {'worktree':str(source),'head_context_only':head,'status':status,
         'compiler_input_files':{os.fsdecode(p):source_input(source/os.fsdecode(p),resources) for p in paths if p},
         'limit':'current clean source/manifest context; historical compiler identities are original raw capture pins/unknown, not this moving root HEAD'}


def add_archive(root,manifest,path,resources):
 writer=Writer(path,resources)
 try:
  with gzip.GzipFile(fileobj=writer,mode='wb',compresslevel=6,mtime=0,filename='') as compressed:
   with tarfile.open(fileobj=compressed,mode='w|',format=tarfile.PAX_FORMAT) as archive:
    for rel,row in sorted(manifest.items(),key=lambda item:(item[0]!='.',item[0])):
     if row['disposition']!='archived': continue
     actual=root if rel=='.' else root/rel
     if identity(actual.lstat())!=row['identity']: raise RuntimeError('archive source drift '+str(actual))
     archive.add(actual,arcname='cache' if rel=='.' else 'cache/'+rel,recursive=False)
     resources.check('archive_add')
 finally: writer.close()


def verify_archive(path,manifest,resources):
 expected={('cache' if rel=='.' else 'cache/'+rel):row for rel,row in manifest.items() if row['disposition']=='archived'}
 seen={}; canonical={}; hashes={}; logical=0
 with tarfile.open(path,mode='r|gz') as archive:
  for member in archive:
   if member.name not in expected or member.name in seen: raise RuntimeError('archive member mismatch '+member.name)
   row=expected[member.name]; ident=row['identity']
   if member.mode!=stat.S_IMODE(ident['st_mode']) or member.uid!=ident['st_uid'] or member.gid!=ident['st_gid']: raise RuntimeError('archive mode/ownership mismatch '+member.name)
   if row['kind']=='directory':
    if not member.isdir(): raise RuntimeError('archive directory type mismatch')
   elif row['kind']=='symlink':
    if not member.issym() or member.linkname!=row['link']: raise RuntimeError('archive symlink mismatch')
   else:
    key=(ident['st_dev'],ident['st_ino'])
    if key in canonical:
     if not member.islnk() or member.linkname!=canonical[key]: raise RuntimeError('archive hardlink topology mismatch '+member.name)
     h=hashes[member.linkname]
    else:
     if not member.isfile() or member.size!=ident['st_size']: raise RuntimeError('archive regular-file mismatch '+member.name)
     canonical[key]=member.name; digest=hashlib.sha256()
     file=archive.extractfile(member)
     for chunk in iter(lambda:file.read(1048576),b''):
      digest.update(chunk); resources.check('archive_verify_read')
     h=digest.hexdigest()
    if h!=row['sha256']: raise RuntimeError('archive content hash mismatch '+member.name)
    hashes[member.name]=h; logical+=ident['st_size']
   seen[member.name]=True; resources.check('archive_verify_member')
 if set(seen)!=set(expected): raise RuntimeError('archive missing members')
 return {'verified_members':len(seen),'verified_regular_paths':len(hashes),'verified_logical_bytes_with_aliases':logical,
         'mode_ownership_content_link_topology_pass':True,'archive_sha256':file_sha(path,resources),'archive_bytes':path.stat().st_size}


def verify_live(root,manifest,resources):
 fresh=scan(root)
 if set(fresh)!=set(manifest): raise RuntimeError('cache path set changed before retirement')
 checked={}
 for rel,row in manifest.items():
  if fresh[rel]['kind']!=row['kind'] or fresh[rel]['identity']!=row['identity']: raise RuntimeError('cache inode/mode/size/time changed '+rel)
  if row['kind']=='symlink' and fresh[rel]['link']!=row['link']: raise RuntimeError('live symlink changed')
  if row['kind']=='file':
   key=(row['identity']['st_dev'],row['identity']['st_ino'])
   if key not in checked: checked[key]=file_sha(root/rel,resources)
   if checked[key]!=row['sha256']: raise RuntimeError('fresh cache content drift '+rel)
 return {'utc':utc(),'complete_path_inode_mode_hashes_pass':True,'unique_files_hashed':len(checked)}


def remove_qualified(root,manifest):
 parent_fd=os.open(root.parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
 root_fd=None; removed=[]
 try:
  root_fd=os.open(root.name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=parent_fd)
  if identity(os.fstat(root_fd))!=manifest['.']['identity']: raise RuntimeError('root inode changed at retirement')
  def inner(fd,prefix):
   for name in sorted(os.listdir(fd)):
    rel=prefix+name; expected=manifest[rel]; actual=os.stat(name,dir_fd=fd,follow_symlinks=False)
    # Parent/alias link counts and directory sizes change as recorded siblings are removed.
    fields=('st_dev','st_ino','st_mode','st_uid','st_gid') if expected['kind']=='directory' else ('st_dev','st_ino','st_mode','st_size','st_mtime_ns','st_uid','st_gid')
    if any(getattr(actual,key)!=expected['identity'][key] for key in fields): raise RuntimeError('retirement entry changed '+rel)
    if expected['kind']=='directory':
     child_fd=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=fd)
     try: inner(child_fd,rel+'/')
     finally: os.close(child_fd)
     os.rmdir(name,dir_fd=fd)
    else: os.unlink(name,dir_fd=fd)
    removed.append(rel)
  inner(root_fd,'')
  root_info=os.stat(root.name,dir_fd=parent_fd,follow_symlinks=False)
  if (root_info.st_dev,root_info.st_ino)!=(manifest['.']['identity']['st_dev'],manifest['.']['identity']['st_ino']): raise RuntimeError('root replaced before rmdir')
  os.rmdir(root.name,dir_fd=parent_fd)
  removed.append('.')
 finally:
  if root_fd is not None: os.close(root_fd)
  os.close(parent_fd)
 return removed


def preserve_context(stage,out,source,context,resources):
 copied={}
 targets=[]
 targets += [source/name for name in ('Cargo.toml','Cargo.lock','pbrs-grpc/Cargo.toml','protobuf-tonic/Cargo.toml')]
 logs=pathlib.Path('/workspace/pure-protobuf/work/logs')
 if stage=='semver': targets += [p for p in logs.glob('*semver*.log')]
 if stage=='gn13': targets += list((source/'work/logs').glob('*'))
 if stage=='native':
  records=source/'work/native-admission-gates'
  targets += [records/name for name in ('037-final-native-interop-build185.meta.json','037-final-native-interop-build185.stdout.log','037-final-native-interop-build185.stderr.log','package-final-locked-stable99.meta.json','package-final-locked-stable99.stdout.log','package-final-locked-stable99.stderr.log','cache-reclaim-after-final-six-consumers-and-tonic-tests.json','cache-cleanup-before-final.json')]
 targets += [logs/name for name in ('native-passed-executable-prune.json','remaining-passed-executable-prune.json')]
 for index,path in enumerate(targets):
  if not path.is_file(): continue
  dest=out/'context'/f'{index:02d}-{path.name}'; dest.parent.mkdir(exist_ok=True)
  h=file_sha(path,resources); size=path.stat().st_size
  resources.check('context_copy_before',size)
  shutil.copy2(path,dest)
  if file_sha(dest,resources)!=h: raise RuntimeError('context copy hash mismatch')
  copied[str(path)]={'retained':str(dest.relative_to(out)),'sha256':h,'bytes':size}
 dump(out/'context-copies.json',copied)
 return copied


def main():
 global ACTIVE_RESOURCES
 parser=argparse.ArgumentParser(); parser.add_argument('--stage',choices=STAGES,required=True); parser.add_argument('--output-root',type=pathlib.Path,required=True); parser.add_argument('--attempt'); args=parser.parse_args()
 output=args.output_root.resolve()
 if not output.is_relative_to(pathlib.Path('/workspace/scratch/work/rx10/work/qualification/retirements')): raise RuntimeError('output outside approved work directory')
 stage=args.stage
 if args.attempt is not None and not args.attempt.isidentifier(): raise RuntimeError('invalid fresh attempt label')
 out=output/(stage+('_'+args.attempt if args.attempt else '')); out.mkdir(exist_ok=False)
 resources=Resources(out); ACTIVE_RESOURCES=resources; resources.check('initial',force=True)
 root,source=map(pathlib.Path,STAGES[stage]); result={'stage':stage,'attempt':args.attempt,'argv':sys.argv,'cwd':str(pathlib.Path.cwd()),'cache':str(root),'source':str(source),'utc_start':utc(),'operation':'new specifically authorized RX10 qualification namespaces only; all SDK/source/bin/raw proofs excluded','retired':False,'tool':{'python':tool_identity(sys.executable),'version':sys.version,'helper_sha256':file_sha(pathlib.Path(__file__))},'env':{key:os.environ.get(key) for key in ('PATH','PYTHONPATH','PYTHONHOME','PYTHONHASHSEED','RUSTUP_HOME','CARGO_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','CARGO_INCREMENTAL','CARGO_PROFILE_DEV_DEBUG','CARGO_PROFILE_TEST_DEBUG','PROTOC')}}
 lock_fds=[]
 try:
  entries=scan(root); result['root_identity']=entries['.']['identity']
  dump(out/'initial-stat-inventory.json',entries)
  dump(out/'users-before-locks.json',users(root))
  lock_records=[]
  for rel,row in entries.items():
   if pathlib.Path(rel).name!='.cargo-lock': continue
   fd=os.open(root/rel,os.O_RDWR|os.O_NOFOLLOW)
   lock_fds.append(fd)
   if identity(os.fstat(fd))!=row['identity']: raise RuntimeError('Cargo lock inode drift')
   fcntl.flock(fd,fcntl.LOCK_EX|fcntl.LOCK_NB); fcntl.lockf(fd,fcntl.LOCK_EX|fcntl.LOCK_NB)
   lock_records.append({'path':str(root/rel),'identity':row['identity'],'flock':'exclusive nonblocking held','fcntl':'exclusive nonblocking held'})
  dump(out/'held-locks.json',{'locks':lock_records,'proc_locks':pathlib.Path('/proc/locks').read_text()})
  if scan(root)!=entries: raise RuntimeError('cache changed acquiring locks')
  dump(out/'users-after-locks.json',users(root))
  before=source_context(source,resources); dump(out/'source-before.json',before)
  preserve_context(stage,out,source,before,resources)
  manifest=hash_manifest(stage,root,entries,resources)
  hash_only=[rel for rel,row in manifest.items() if row['disposition']=='hash_only']
  if stage=='semver' and len(hash_only)!=46: raise RuntimeError('semver selected/hash-only scope differs from approved46')
  if stage!='semver' and hash_only: raise RuntimeError('full remaining cache must preserve every path')
  dump(out/'manifest.json',manifest); dump(out/'hash-only-paths.json',hash_only)
  archive=out/'payload.tar.gz.partial'; add_archive(root,manifest,archive,resources)
  verification=verify_archive(archive,manifest,resources); dump(out/'archive-verification.json',verification)
  verify_live(root,manifest,resources)
  after=source_context(source,resources); dump(out/'source-after.json',after)
  if before['compiler_input_files']!=after['compiler_input_files'] or after['status']: raise RuntimeError('provider compiler-input change')
  if stage!='semver' and before['head_context_only']!=after['head_context_only']: raise RuntimeError('isolated owned source HEAD changed')
  result['moving_root_context']={'before':before['head_context_only'],'after':after['head_context_only'],'compiler_inputs_unchanged':True,'historical_labels_unchanged':True}
  dump(out/'users-immediately-before-retirement.json',users(root))
  final=verify_live(root,manifest,resources); dump(out/'fresh-before-retirement-verification.json',final)
  result['free_before_retirement']=resources.check('immediately_before_retirement',force=True)
  # All content/link/mode checks passed; only now may the approved root be retired.
  removed=remove_qualified(root,manifest); dump(out/'removed-paths.json',removed)
  result['retired']=True
  archive.rename(out/'payload.tar.gz'); archive=out/'payload.tar.gz'
  result['post_retirement_archive_sha256']=file_sha(archive,resources)
  if result['post_retirement_archive_sha256']!=verification['archive_sha256']: raise RuntimeError('archive changed after retirement')
  if root.exists() or root.is_symlink(): raise RuntimeError('retired root still present')
  unique={}
  for row in manifest.values():
   if row['kind']=='file': unique[(row['identity']['st_dev'],row['identity']['st_ino'])]=row
  result.update(archive=verification,manifest_sha256=file_sha(out/'manifest.json',resources),
     regular_paths=sum(row['kind']=='file' for row in manifest.values()),
     logical_namespace_bytes_with_aliases=sum(row['identity']['st_size'] for row in manifest.values() if row['kind']=='file'),
     unique_regular_bytes=sum(row['identity']['st_size'] for row in unique.values()),
     hash_only_paths=len(hash_only),removed_entries=len(removed),
     free_after_retirement=resources.check('finished',force=True),
     historical_limit='New RX10 ordinary cache only. Source/tool/ELF identities are the separately preserved raw captures. No benchmark/performance/shipping inference. Full remaining payloads retained.',
     status='verified_retired',exit=0)
 except BaseException as error:
  result.update(status='invalid_preservation_attempt_no_further_deletion',exit=1,error=repr(error),traceback=traceback.format_exc())
 finally:
  for fd in reversed(lock_fds): os.close(fd)
  result.update(utc_finish=utc(),observed_min_free=min((s['free_bytes'] for s in resources.samples),default=None),observed_max_owned_cache=max((s['owned_total'] for s in resources.samples),default=None),resource_sampling_limit='periodic observed extrema; each archive write also checks free-space guard',source_worktrees_and_other_namespaces_untouched=True)
  dump(out/'record.json',result)
 print(json.dumps(result,indent=2),flush=True)
 return result['exit']

if __name__=='__main__': raise SystemExit(main())
