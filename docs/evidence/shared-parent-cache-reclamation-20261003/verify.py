#!/usr/bin/env python3
"""Independent, read-only checker for the sealed cleanup proof.

Checks complete inventories, preservation coverage, every archived payload and
hardlink, all four process scans, and cleanup/space records. Never deletes,
compiles, invokes benchmark tools, or requires the deleted cache to exist.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import tarfile

def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        while data:=f.read(1024*1024):h.update(data)
    return h.hexdigest()

class Parts(io.RawIOBase):
    def __init__(self, paths):self.paths=iter(paths);self.current=None
    def readable(self):return True
    def readinto(self, buffer):
        while True:
            if self.current is None:
                try:self.current=next(self.paths).open('rb')
                except StopIteration:return 0
            data=self.current.read(len(buffer))
            if data:buffer[:len(data)]=data;return len(data)
            self.current.close();self.current=None
    def close(self):
        if self.current is not None:self.current.close()
        super().close()

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory',nargs='?',type=Path,default=Path(__file__).resolve().parent)
    args=parser.parse_args();root=args.directory.resolve()
    def read(name):return json.loads((root/name).read_bytes())
    manifest=read('artifact-manifest.json')['files']
    actual={str(p.relative_to(root)) for p in root.rglob('*') if p.is_file() and p.name!='artifact-manifest.json'}
    assert actual==manifest.keys(),'missing/unbound proof file'
    for name,row in manifest.items():
        path=root/name;assert path.stat().st_size==row['size'] and digest(path)==row['sha256'],name
    before=read('cache-before.json')['entries'];fresh=read('cache-pre-delete.json')['entries']
    renamed=read('quarantine-pre-delete.json')['entries']
    assert before==fresh and before.keys()==renamed.keys()
    for name,row in before.items():
        current=dict(renamed[name]);expected=dict(row)
        if name=='.':current.pop('ctime_ns');expected.pop('ctime_ns')
        assert current==expected,name
    summary=read('summary.json');assert summary['result']=='completed'
    assert summary['base_commit']=='f37b68ce0f426bccf025858ac3339481eb46376d'
    assert summary['exact_deleted_cache']=='/workspace/scratch/work/sb32/target'
    files={name:row for name,row in before.items() if row['kind']=='file'}
    unique={}
    for row in before.values():unique.setdefault((row['dev'],row['ino']),row)
    assert len(files)==2564
    assert sum(r['blocks']*512 for r in unique.values())==summary['cache_before']['unique_allocated_st_blocks_bytes_including_directories']
    elves=set(summary['retained_elf_paths']);assert len(elves)==6
    assert len({(files[n]['dev'],files[n]['ino']) for n in elves})==5
    required={name for name in files if name in elves or name in ('.rustc_info.json','CACHEDIR.TAG','debug/.cargo-lock')
              or name.startswith('debug/build/') or name.startswith('debug/.fingerprint/') or name.endswith('.d')}
    retained=read('retained-inputs.json')['files'];assert retained.keys()==required
    assert all(retained[name]==files[name] for name in required)
    assert sum(n.startswith('debug/.fingerprint/') for n in required)==1270
    assert sum(n.startswith('debug/build/') for n in required)==633
    assert sum(n.endswith('.d') for n in required)==298
    payloads=read('archive-payloads.json')['files']
    assert {n.removeprefix('cache/') for n in payloads if n.startswith('cache/')}==required
    for name in required:
        assert payloads['cache/'+name]=={'sha256':files[name]['sha256'],'size':files[name]['size']}
    pieces=read('archive-parts.json')['parts'];paths=[root/r['path'] for r in pieces]
    h=hashlib.sha256();total=0
    for path,row in zip(paths,pieces):
        assert path.stat().st_size==row['size'] and digest(path)==row['sha256']
        with path.open('rb') as f:
            while data:=f.read(1024*1024):h.update(data);total+=len(data)
    archive=read('archive.json')
    assert total==archive['size'] and h.hexdigest()==archive['sha256']
    assert archive['verified_against_fresh_predelete_input']
    seen={};hardlinks=0
    with io.BufferedReader(Parts(paths)) as stream:
        with tarfile.open(fileobj=stream,mode='r|gz') as tar:
            for member in tar:
                assert member.name in payloads and member.name not in seen
                assert not Path(member.name).is_absolute() and '..' not in Path(member.name).parts
                row=payloads[member.name]
                if member.islnk():
                    assert member.linkname in seen;value=seen[member.linkname];hardlinks+=1
                else:
                    assert member.isfile();h=hashlib.sha256();size=0
                    with tar.extractfile(member) as f:
                        while data:=f.read(1024*1024):h.update(data);size+=len(data)
                    assert size==row['size'];value=h.hexdigest()
                assert value==row['sha256'],member.name;seen[member.name]=value
    assert seen.keys()==payloads.keys() and len(seen)==archive['members']==2219
    scans={}
    for phase in ('before-preservation','after-preservation','immediately-before-rename','after-rename'):
        scan=read('proc/'+phase+'.json');assert not scan['matches']
        for row in scan['permission_limits']:
            status=row['status']
            assert status.get('State','').startswith('Z') or (status.get('Uid','').split()[:1]==['0'] and status.get('Name') in ('dockerd','containerd','containerd-shim','containerd-shim-runc-v2','runc'))
        scans[phase]={'views':len(scan['views']),'permission_limits':len(scan['permission_limits']),'transient':len(scan['transient_disappearance'])}
    rename=read('rename.json')
    assert (rename['dev'],rename['ino'])==(before['.']['dev'],before['.']['ino'])
    assert rename['from']==summary['exact_deleted_cache']
    before_space=summary['global_space_pre_delete']['observed_global_available'];after_space=summary['global_space_after']['observed_global_available']
    assert after_space-before_space==summary['observed_global_available_increase_since_predelete']
    failed=read('attempt-1-head-advance/failure.json')
    assert failed['cache_exists'] and not failed['quarantine_exists'] and failed['type']=='AssertionError'
    print(json.dumps({'result':'passed','files_inventoried':len(files),'payloads_verified':len(seen),'archive_hardlinks_verified':hardlinks,'elf_paths_verified':len(elves),'proc_scans':scans,'space_scope':'global observations are distinct from unique inode allocated/apparent sums; permissions and races remain bounded'},indent=2))

if __name__=='__main__':main()
