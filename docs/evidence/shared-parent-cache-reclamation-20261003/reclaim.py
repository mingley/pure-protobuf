#!/usr/bin/env python3
"""One explicitly authorized historical-cache reclamation; no build/capture.

Hardcoded exact target, fresh inode/content checks, advisory Cargo lock, full
visible process/thread scans and lossless member verification precede removal.
Permission failures and races are recorded, never described as atomic absence.
"""
import collections
from datetime import datetime, timezone
import errno
import fcntl
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tarfile

ROOT = Path('/workspace/pure-protobuf')
BASE = 'f37b68ce0f426bccf025858ac3339481eb46376d'
CACHE = Path('/workspace/scratch/work/sb32/target')
OUT = ROOT / 'work/sb32-cache-reclamation-20261003'
QUARANTINE = OUT / 'authorized-target-pending-deletion'
FLOOR = 2 * 1024**3
COMMANDS = []
FIELDS = ('dev', 'ino', 'mode', 'uid', 'gid', 'nlink', 'size', 'blocks', 'mtime_ns', 'ctime_ns')

def utc(): return datetime.now(timezone.utc).isoformat()
def sha(data): return hashlib.sha256(data).hexdigest()
def dump(name, value):
    path = OUT / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')
def command(argv):
    started = utc()
    result = subprocess.run(argv, capture_output=True, check=False)
    index = len(COMMANDS)
    raw = OUT / 'raw'; raw.mkdir(exist_ok=True)
    (raw / f'command-{index}.stdout').write_bytes(result.stdout)
    (raw / f'command-{index}.stderr').write_bytes(result.stderr)
    COMMANDS.append({'argv': argv, 'started_utc': started, 'finished_utc': utc(),
                     'exit_code': result.returncode, 'stdout': f'raw/command-{index}.stdout',
                     'stderr': f'raw/command-{index}.stderr'})
    dump('commands.json', COMMANDS)
    result.check_returncode()
    return result.stdout
def git(*args): return command(['git', '-C', str(ROOT), *args])
def metadata(info):
    return {name: getattr(info, 'st_' + name) for name in FIELDS}
def file_hash(path):
    before = path.lstat()
    assert stat.S_ISREG(before.st_mode), str(path)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        assert metadata(os.fstat(fd)) == metadata(before), 'opened inode changed: ' + str(path)
        digest = hashlib.sha256()
        while data := os.read(fd, 1024 * 1024): digest.update(data)
        assert metadata(os.fstat(fd)) == metadata(before), 'read mutation: ' + str(path)
    finally: os.close(fd)
    assert metadata(path.lstat()) == metadata(before), 'path mutation: ' + str(path)
    return digest.hexdigest()
def inventory(root):
    rows = {}
    def walk(directory):
        info = directory.lstat()
        assert stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
        relative = str(directory.relative_to(root))
        rows[relative] = {'kind': 'directory', **metadata(info)}
        for path in sorted(directory.iterdir()):
            info = path.lstat()
            if stat.S_ISDIR(info.st_mode): walk(path)
            else:
                assert stat.S_ISREG(info.st_mode), 'special file or symlink: ' + str(path)
                rows[str(path.relative_to(root))] = {'kind': 'file', **metadata(info), 'sha256': file_hash(path)}
    walk(root)
    return rows
def space():
    usage = shutil.disk_usage(ROOT)
    return {'observed_global_total': usage.total, 'observed_global_used': usage.used,
            'observed_global_available': usage.free, 'utc': utc()}
def totals(rows):
    unique = {}
    for row in rows.values(): unique.setdefault((row['dev'], row['ino']), row)
    files = [row for row in rows.values() if row['kind'] == 'file']
    return {'paths': len(rows), 'file_paths': len(files), 'directory_paths': len(rows)-len(files),
            'unique_file_inodes': sum(r['kind']=='file' for r in unique.values()),
            'file_path_apparent_bytes_including_hardlink_aliases': sum(r['size'] for r in files),
            'unique_apparent_bytes_including_directories': sum(r['size'] for r in unique.values()),
            'unique_allocated_st_blocks_bytes_including_directories': sum(r['blocks']*512 for r in unique.values()),
            'allocation_scope': 'unique device/inode st_blocks; overlay/CoW may share extents; not exclusively attributable physical storage'}
def proc_status(path):
    fields = {}
    try:
        for line in (path/'status').read_text().splitlines():
            key, _, value = line.partition(':')
            if key in ('Name','State','Uid','Gid','NSpid','Threads'): fields[key] = value.strip()
    except OSError as error: fields['status_error'] = {'errno':error.errno,'message':str(error)}
    return fields
def process_scan(phase, roots, lock_fd):
    started = utc(); rows = []; matches = []; permission = []; transient = []
    patterns = [re.compile(re.escape(str(root))+r'(?=/|\x00|\s|$|[;"\'])') for root in roots]
    self_pid = os.getpid()
    for leader in sorted(Path('/proc').iterdir(), key=lambda p:p.name):
        if not leader.name.isdigit(): continue
        status = proc_status(leader)
        try: tasks = list((leader/'task').iterdir())
        except OSError as error:
            tasks = []
            transient.append({'pid':leader.name,'surface':'task enumeration','errno':error.errno})
        views = [leader, *[p for p in tasks if p.name != leader.name]]
        for view in views:
            result = {'pid':int(leader.name),'view':str(view),'status':status,'surfaces':{}}
            for surface in ('exe','cwd','cmdline','environ','fd','maps'):
                path = view/surface
                try:
                    if surface in ('exe','cwd'):
                        data = os.readlink(path).encode()
                    elif surface == 'fd':
                        values = []
                        for fd_path in path.iterdir():
                            try: target = os.readlink(fd_path)
                            except FileNotFoundError: continue
                            if int(leader.name)==self_pid and fd_path.name==str(lock_fd):
                                assert target in (str(CACHE/'debug/.cargo-lock'), str(QUARANTINE/'debug/.cargo-lock'))
                                result['excluded_own_guard_fd'] = {'fd':int(fd_path.name),'target':target}
                                continue
                            values.append(target)
                        data = '\n'.join(values).encode()
                    else: data = path.read_bytes()
                    text = data.decode(errors='replace')
                    references = [str(root) for root,pattern in zip(roots,patterns) if pattern.search(text)]
                    result['surfaces'][surface] = {'status':'read','size':len(data),'sha256':sha(data),'cache_references':references}
                    if references: matches.append({'pid':int(leader.name),'view':str(view),'surface':surface,'cache_references':references,'status':status})
                except OSError as error:
                    detail = {'pid':int(leader.name),'view':str(view),'surface':surface,'errno':error.errno,'message':str(error),'status':status}
                    result['surfaces'][surface] = {'status':'unreadable',**detail}
                    (transient if error.errno in (errno.ENOENT,errno.ESRCH) else permission).append(detail)
            rows.append(result)
    record = {'phase':phase,'started_utc':started,'finished_utc':utc(),'views':rows,'matches':matches,
              'permission_limits':permission,'transient_disappearance':transient,
              'qualification':'Only readable surfaces in this container namespace were observed. Known Docker root-process permission failures and zombies are recorded. Enumeration and locking are non-atomic; an unseen/new process could race. No credentials/raw environment or command arguments are retained.'}
    dump(f'proc/{phase}.json', record)
    assert not matches, 'visible active cache reference: '+json.dumps(matches)
    unexpected = [r for r in permission if not r['status'].get('State','').startswith('Z')
                  and not (r['status'].get('Uid','').split()[:1]==['0'] and r['status'].get('Name') in ('dockerd','containerd','containerd-shim','containerd-shim-runc-v2','runc'))]
    assert not unexpected, 'unexpected permission-limited live process: '+json.dumps(unexpected)
    print(json.dumps({'phase':phase,'views':len(rows),'matches':len(matches),'permission_failures':len(permission),'transient':len(transient)}),flush=True)
    return record
class GuardedWriter:
    def __init__(self, stream): self.stream=stream;self.next_check=0
    def write(self, data):
        if self.stream.tell() >= self.next_check:
            current=space();assert current['observed_global_available'] >= FLOOR+len(data),'archive free reserve reached'
            self.next_check=self.stream.tell()+1024*1024
        return self.stream.write(data)
    def flush(self): return self.stream.flush()
def selected(name, row):
    return row['kind']=='file' and (name in ('.rustc_info.json','CACHEDIR.TAG','debug/.cargo-lock','debug/devloop')
          or name.startswith('debug/.fingerprint/') or name.startswith('debug/build/') or name.endswith('.d')
          or (name.startswith('debug/deps/devloop-') and name in ELF_PATHS))
def verify_archive(path, expected):
    seen = {}; members = []
    with tarfile.open(path, 'r|gz') as archive:
        for member in archive:
            assert member.name in expected and member.name not in seen, 'unexpected/duplicate archive member'
            row = expected[member.name]
            if member.islnk():
                assert member.linkname in seen, 'forward/missing hardlink target'
                digest = seen[member.linkname]
                assert digest == row['sha256'], 'hardlink payload mismatch'
            else:
                assert member.isfile()
                digest = hashlib.sha256(); size=0
                with archive.extractfile(member) as stream:
                    while data:=stream.read(1024*1024): digest.update(data);size+=len(data)
                digest=digest.hexdigest();assert size==row['size'] and digest==row['sha256'], member.name
            seen[member.name]=digest
            members.append({'name':member.name,'type':'hardlink' if member.islnk() else 'file','linkname':member.linkname,
                            'payload_sha256':digest,'payload_size':row['size']})
    assert seen.keys()==expected.keys()
    return members

ELF_PATHS = set()
def main():
    assert sys.argv[1:] == ['--execute-authorized-reclamation']
    assert CACHE.resolve()==CACHE and CACHE.is_dir() and CACHE.parent.resolve()==Path('/workspace/scratch/work/sb32')
    assert not QUARANTINE.exists() and not (OUT/'summary.json').exists()
    observed_head=git('rev-parse','HEAD').decode().strip()
    git('merge-base','--is-ancestor',BASE,observed_head)
    dump('observed-main.json',{'requested_cleanup_commit_parent':BASE,'observed_clean_descendant':observed_head,
                              'qualification':'coordinator main may advance; cleanup is additive on fixed requested parent and historical blobs are checked against that parent'})
    assert not git('status','--porcelain','--untracked-files=normal')
    before_space=space();dump('global-space-before.json',before_space)
    for name in ('status','cgroup','mountinfo'):
        (OUT/('self-'+name+'.txt')).write_bytes((Path('/proc/self')/name).read_bytes())
    lock_fd=os.open(CACHE/'debug/.cargo-lock',os.O_RDONLY|os.O_NOFOLLOW)
    fcntl.flock(lock_fd,fcntl.LOCK_EX|fcntl.LOCK_NB)
    dump('guard.json',{'authorization':'root explicitly authorized this exact completed historical target; no builds/captures authorized',
        'base_commit':BASE,'cache_path':str(CACHE),'lock_path':str(CACHE/'debug/.cargo-lock'),'lock_fd':lock_fd,'locking':'Linux advisory flock LOCK_EX|LOCK_NB on existing Cargo cache lock; cooperative only',
        'script_sha256':file_hash(Path(__file__)),'argv':[sys.executable,*sys.argv], 'started_utc':utc()})
    try:
        process_scan('before-preservation',[CACHE],lock_fd)
        before=inventory(CACHE);dump('cache-before.json',{'root':str(CACHE),'entries':before,'totals':totals(before)})
        for name,row in before.items():
            if row['kind']=='file' and (name=='debug/devloop' or name.startswith('debug/deps/devloop-')):
                with (CACHE/name).open('rb') as stream:
                    if stream.read(4)==b'\x7fELF': ELF_PATHS.add(name)
        assert len(ELF_PATHS)==6 and len({(before[n]['dev'],before[n]['ino']) for n in ELF_PATHS})==5
        previous=json.loads((ROOT/'work/shared-parent-release-plan-20261003/cache-review.json').read_text())['debug_elfs']
        assert all(before[name]['sha256']==previous[str(CACHE/name)]['sha256'] for name in ELF_PATHS)
        retained={name:row for name,row in before.items() if selected(name,row)}
        dump('retained-inputs.json',{'files':retained,'elf_paths':sorted(ELF_PATHS),'selection':'all six devloop ELF aliases/five unique inodes; all debug/build inputs/outputs/scripts; every full fingerprint; all .d dep-info; Cargo lock and cache/root provenance files'})
        historical={}
        for prefix in ('docs/evidence/tc32b-collector-20261002','docs/evidence/sb-32-driver','docs/evidence/sb-32','docs/evidence/sb-32-driver.md','docs/evidence/tc32b-collector.md'):
            names=git('ls-tree','-r','--name-only',BASE,'--',prefix).decode().splitlines()
            for name in names:
                data=git('show',BASE+':'+name);assert (ROOT/name).read_bytes()==data
                historical['historical/'+name]={'sha256':sha(data),'size':len(data),'data':data}
        compiled_roots=('src','proto','pbrs-grpc','protobuf-tonic','prost_tat','v4_tat','Cargo.toml','Cargo.lock','build.rs','vendor/google/conformance_fds.bin','bench/corpora/otlp/protos','bench/devloop/src','bench/devloop/proto','bench/devloop/build.rs','bench/devloop/Cargo.toml','bench/devloop/Cargo.lock','bench/devloop/adoption/src','bench/devloop/adoption/proto','bench/devloop/adoption/build.rs','bench/devloop/adoption/Cargo.toml','bench/devloop/adoption/Cargo.lock')
        for pin in ('8ca03da51ab3723f53858800ca5b8e2a33d0f448','5a263139f0963488bab9e34b5282803abc3d9138'):
            names=git('ls-tree','-r','--name-only',pin,'--',*compiled_roots).decode().splitlines()
            data=git('archive','--format=tar',pin,*names)
            historical['historical/source-'+pin+'.tar']={'sha256':sha(data),'size':len(data),'data':data}
        external={}
        original=json.loads(git('show',BASE+':docs/evidence/tc32b-collector-20261002/preserved-sb32-debug-provenance.json'))
        for row in original['files']:
            path=Path('/workspace/scratch/work/tc32b')/row['preserved_path'];digest=file_hash(path)
            assert digest==row['sha256'];external[str(path)]={'sha256':digest,'size':path.stat().st_size}
        tools=json.loads(git('show',BASE+':docs/evidence/tc32b-collector-20261002/tool-pins.json'))
        for name,row in tools.items():
            path=Path(row['path']).resolve();digest=file_hash(path);assert digest==row['sha256']
            external[str(path)]={'sha256':digest,'size':path.stat().st_size,'historical_version':row['version'],'tool':name}
        dump('preserved-external-inputs.json',{'files':external,'scope':'existing immutable external peers, tools and proofs are verified and remain outside deleted target; no version process or build was run'})
        expected={'cache/'+name:{'sha256':row['sha256'],'size':row['size']} for name,row in retained.items()}
        expected.update({name:{k:v for k,v in row.items() if k!='data'} for name,row in historical.items()})
        dump('archive-payloads.json',{'files':expected})
        archive_path=OUT/'retained-cache.tar.gz'; hardlinks={}
        with archive_path.open('xb') as raw:
            writer=GuardedWriter(raw)
            with gzip.GzipFile(fileobj=writer,mode='wb',compresslevel=6,mtime=0) as zipped:
                with tarfile.open(fileobj=zipped,mode='w|',format=tarfile.PAX_FORMAT) as archive:
                    for name,row in sorted(retained.items()):
                        assert metadata((CACHE/name).lstat())=={k:row[k] for k in FIELDS}
                        info=tarfile.TarInfo('cache/'+name);info.mode=stat.S_IMODE(row['mode']);info.uid=row['uid'];info.gid=row['gid'];info.mtime=row['mtime_ns']/1e9
                        key=(row['dev'],row['ino'])
                        if key in hardlinks:
                            info.type=tarfile.LNKTYPE;info.linkname=hardlinks[key];archive.addfile(info)
                        else:
                            info.size=row['size'];hardlinks[key]=info.name
                            with (CACHE/name).open('rb') as stream:archive.addfile(info,stream)
                        assert metadata((CACHE/name).lstat())=={k:row[k] for k in FIELDS}
                    for name,row in sorted(historical.items()):
                        info=tarfile.TarInfo(name);info.size=row['size'];info.mode=0o644;archive.addfile(info,io.BytesIO(row['data']))
            raw.flush();os.fsync(raw.fileno())
        members=verify_archive(archive_path,expected);dump('archive-members-verified.json',members)
        archive_pin={'path':archive_path.name,'sha256':file_hash(archive_path),'size':archive_path.stat().st_size,'members':len(members),'verified_against_fresh_predelete_input':False}
        dump('archive.json',archive_pin)
        print(json.dumps({'phase':'archive-verified','compressed_bytes':archive_pin['size'],'retained_members':len(members),'space':space()}),flush=True)
        process_scan('after-preservation',[CACHE],lock_fd)
        fresh=inventory(CACHE);dump('cache-pre-delete.json',{'root':str(CACHE),'entries':fresh,'totals':totals(fresh)})
        assert fresh==before,'full fresh inventory drift; deletion stopped'
        assert verify_archive(archive_path,expected)==members,'archive re-verification changed'
        for path,row in external.items():assert file_hash(Path(path))==row['sha256'],'external input drift'
        archive_pin['verified_against_fresh_predelete_input']=True;dump('archive.json',archive_pin)
        process_scan('immediately-before-rename',[CACHE],lock_fd)
        assert metadata(CACHE.lstat())=={k:before['.'][k] for k in FIELDS}
        assert not QUARANTINE.exists()
        pre_rename_space=space();dump('global-space-pre-delete.json',pre_rename_space)
        os.rename(CACHE,QUARANTINE)
        assert (QUARANTINE.stat().st_dev,QUARANTINE.stat().st_ino)==(before['.']['dev'],before['.']['ino'])
        assert not CACHE.exists(),'new target appeared; leave quarantine untouched'
        dump('rename.json',{'from':str(CACHE),'to':str(QUARANTINE),'dev':before['.']['dev'],'ino':before['.']['ino'],'utc':utc(),'scope':'same exact authorized directory inode; newly created target at original pathname will never be deleted'})
        process_scan('after-rename',[CACHE,QUARANTINE],lock_fd)
        after_rename=inventory(QUARANTINE)
        for name in before:
            expected_row=dict(before[name]);actual_row=dict(after_rename[name])
            if name=='.':expected_row.pop('ctime_ns');actual_row.pop('ctime_ns')
            assert expected_row==actual_row,'post-rename mutation: '+name
        assert after_rename.keys()==before.keys()
        dump('quarantine-pre-delete.json',{'root':str(QUARANTINE),'entries':after_rename,'totals':totals(after_rename)})
        assert getattr(shutil.rmtree,'avoids_symlink_attacks',False)
        shutil.rmtree(QUARANTINE)
        assert not QUARANTINE.exists() and not CACHE.exists()
        after_space=space();dump('global-space-after.json',after_space)
        dump('summary.json',{'schema':'sb32-historical-cache-reclamation/1','result':'completed','base_commit':BASE,'exact_deleted_cache':str(CACHE),'authorized_inode':{'dev':before['.']['dev'],'ino':before['.']['ino']},'archive':archive_pin,'cache_before':totals(before),'global_space_before':before_space,'global_space_pre_delete':pre_rename_space,'global_space_after':after_space,'observed_global_available_increase_since_predelete':after_space['observed_global_available']-pre_rename_space['observed_global_available'],'retained_elf_paths':sorted(ELF_PATHS),'retained_unique_elf_inodes':5,'retained_full_fingerprint_files':sum(n.startswith('debug/.fingerprint/') for n in retained),'retained_dep_info_files':sum(n.endswith('.d') for n in retained),'retained_build_files':sum(n.startswith('debug/build/') for n in retained),'no_compilers_builds_benchmark_tools_or_other_cache_mutations':True,'qualification':'All archived payloads match fresh full inventories. Visible process/thread surfaces contained no references except the own explicitly excluded guard fd. Known Docker permission restrictions and zombies prevent universal observation; scans, advisory locks, rename and deletion are not a global atomic guarantee. Global free is observed under concurrent external writes, distinct from inode/allocated/apparent totals. No performance claim.', 'finished_utc':utc()})
        print(json.dumps(json.loads((OUT/'summary.json').read_text()),indent=2),flush=True)
    finally:os.close(lock_fd)

if __name__=='__main__':
    OUT.mkdir(parents=True,exist_ok=True)
    try:main()
    except Exception as error:
        dump('failure.json',{'utc':utc(),'type':type(error).__name__,'message':str(error),'cache_exists':CACHE.exists(),'quarantine_exists':QUARANTINE.exists(),'commands':COMMANDS})
        raise
