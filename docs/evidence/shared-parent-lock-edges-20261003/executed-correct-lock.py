#!/usr/bin/env python3
"""Isolated seven-edge-only standalone lock correction; no compilation."""
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

ROOT=Path('/workspace/pure-protobuf')
WT=Path('/workspace/scratch/work/shared-parent-lock-edges')
OUT=WT/'docs/evidence/shared-parent-lock-edges-20261003'
BASE='ad79b48ee34d4fd67346560da195f69ee6a846c3'
SUPPORT=('atomic-waker','fnv','futures-sink','indexmap','slab','tokio-util','tracing')
LOCKS=('Cargo.lock','bench/devloop/Cargo.lock','bench/devloop/adoption/Cargo.lock')
COMMANDS=[]
def utc():return datetime.now(timezone.utc).isoformat()
def sha(data):return hashlib.sha256(data).hexdigest()
def dump(name,data):
    p=OUT/name;p.parent.mkdir(parents=True,exist_ok=True)
    p.write_text(json.dumps(data,indent=2,sort_keys=True)+'\n')
def run(argv,env=None):
    start=utc();p=subprocess.run(argv,cwd=WT,env=env,capture_output=True,check=False)
    i=len(COMMANDS);raw=OUT/'raw';raw.mkdir(exist_ok=True)
    (raw/f'command-{i}.stdout').write_bytes(p.stdout);(raw/f'command-{i}.stderr').write_bytes(p.stderr)
    row={'argv':argv,'cwd':str(WT),'started_utc':start,'finished_utc':utc(),'exit_code':p.returncode,
         'stdout':f'raw/command-{i}.stdout','stderr':f'raw/command-{i}.stderr'}
    COMMANDS.append(row);dump('commands.json',COMMANDS)
    return p,row
def git(*args):
    p,row=run(['git','-C',str(WT),*args]);p.check_returncode();return p.stdout
def packages(data):
    return {(p['name'],p['version'],p.get('source')):p for p in tomllib.loads(data.decode())['package']}
def tuples(data):
    return sorted([{'name':p['name'],'version':p['version'],'source':p.get('source'),'checksum':p.get('checksum')}
                   for p in packages(data).values()],key=lambda p:(p['name'],p['version'],p['source'] or ''))
def main():
    OUT.mkdir(parents=True,exist_ok=True)
    assert git('rev-parse','HEAD').decode().strip()==BASE
    tools=json.loads((ROOT/'work/shared-parent-release-plan-20261003/actual-tool-pins-rechecked.json').read_text())
    for row in tools.values():assert sha(Path(row['path']).read_bytes())==row['sha256']
    dump('tool-pins.json',tools)
    environment=dict(os.environ,CARGO_HOME=str(ROOT/'work/toolchain/cargo'),RUSTUP_HOME=str(ROOT/'work/toolchain/rustup'),
                     CARGO_TARGET_DIR=str(ROOT/'work/shared-parent-lock-edges-20261003/metadata-target'),RUSTC=tools['rustc']['path'],
                     CARGO_INCREMENTAL='0',CARGO_BUILD_JOBS='1',LC_ALL='C')
    environment['PATH']=str(ROOT/'target/conformance-build')+':'+str(ROOT/'work/toolchain/cargo/bin')+':'+environment.get('PATH','')
    dump('metadata-environment.json',{k:environment[k] for k in ('CARGO_HOME','RUSTUP_HOME','CARGO_TARGET_DIR','RUSTC','CARGO_INCREMENTAL','CARGO_BUILD_JOBS','LC_ALL','PATH')})
    before={name:(WT/name).read_bytes() for name in LOCKS}
    for name,data in before.items():
        assert git('show',BASE+':'+name)==data
        p=OUT/'before'/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
    assert sha(before['bench/devloop/Cargo.lock'])=='a5fa1e79eb2b3741513e479c393fda804f2f6a19416b9560929f033fb944398c'
    def metadata(manifest,features=None):
        argv=[tools['cargo']['path'],'metadata','--offline','--locked','--format-version','1','--manifest-path',str(WT/manifest)]
        if features:argv+=['--features',features]
        return run(argv,environment)
    first,first_record=metadata('bench/devloop/Cargo.toml','bridge')
    dump('first-locked-metadata.json',{**first_record,'source_commit':BASE,'expected_result':'rejected old lock; no automatic pin update',
         'lock_sha256':{name:sha(data) for name,data in before.items()},'compilation':False})
    assert first.returncode!=0 and b'lock' in first.stderr.lower(),first.stderr.decode()
    assert all((WT/name).read_bytes()==data for name,data in before.items())
    parent='bench/devloop/Cargo.lock';blocks=before[parent].decode().split('[[package]]')
    indices=[i for i,b in enumerate(blocks) if re.search(r'^name = "pbrs-grpc"$',b,re.M)]
    assert len(indices)==1;i=indices[0];block=blocks[i]
    dependencies=tomllib.loads('[[package]]'+block)['package'][0]['dependencies']
    assert not set(SUPPORT)&set(dependencies)
    start=block.index('dependencies = [\n')+len('dependencies = [\n');end=block.index('\n]',start)
    replacement=''.join(' "'+name+'",\n' for name in sorted(dependencies+list(SUPPORT))).rstrip('\n')
    blocks[i]=block[:start]+replacement+block[end:]
    corrected='[[package]]'.join(blocks).encode();assert sha(corrected)=='61399401c8e57ceb6aa697fb404bee9ac22f6ec48441faa8dfae74daabbf83c4'
    (WT/parent).write_bytes(corrected)
    after={name:(WT/name).read_bytes() for name in LOCKS};audit={}
    for name,data in before.items():
        old,new=packages(data),packages(after[name]);assert tuples(data)==tuples(after[name])
        changed=[k for k in old if old[k]!=new[k]]
        if name==parent:
            assert len(changed)==1 and changed[0][0]=='pbrs-grpc'
            a,b=old[changed[0]],new[changed[0]];assert set(a)==set(b)
            assert {k:v for k,v in a.items() if k!='dependencies'}=={k:v for k,v in b.items() if k!='dependencies'}
            assert set(b['dependencies'])-set(a['dependencies'])==set(SUPPORT) and set(a['dependencies'])<=set(b['dependencies'])
        else:assert after[name]==data
        p=OUT/'after'/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(after[name])
        audit[name]={'before_sha256':sha(data),'after_sha256':sha(after[name]),'packages':len(old),
                     'all_name_version_source_checksum_tuples_unchanged':True,'changed_package_nodes':[k[0] for k in changed],
                     'added_edges':list(SUPPORT) if name==parent else [],'indexmap_versions':[p['version'] for p in old.values() if p['name']=='indexmap']}
        dump('tuples/'+name.replace('/','-')+'.json',tuples(data))
    frozen153=git('show',BASE+':docs/evidence/tc32b-collector-20261002/parent-lock-before.toml')
    old153=packages(frozen153);current=packages(corrected);assert len(old153)==153
    assert set(old153)<=set(current) and all(all(old153[k].get(f)==current[k].get(f) for f in ('name','version','source','checksum')) for k in old153)
    results=[]
    for manifest,feature in (('bench/devloop/Cargo.toml','bridge'),('bench/devloop/Cargo.toml',None),
                             ('bench/devloop/adoption/Cargo.toml','bridge'),('bench/devloop/adoption/Cargo.toml',None)):
        process,record=metadata(manifest,feature);process.check_returncode();graph=json.loads(process.stdout)
        lock=after[manifest.removesuffix('Cargo.toml')+'Cargo.lock'];pins=packages(lock)
        for package in graph['packages']:
            key=(package['name'],package['version'],package.get('source'));assert key in pins,'unbound package '+str(key)
        assert all((WT/name).read_bytes()==data for name,data in after.items())
        grpc=[p for p in graph['packages'] if p['name']=='pbrs-grpc']
        if grpc:
            direct={d['name'] for d in grpc[0]['dependencies'] if d['kind']!='dev'};assert set(SUPPORT)<=direct
            indexmap=[d for d in grpc[0]['dependencies'] if d['name']=='indexmap'];assert len(indexmap)==1 and indexmap[0]['req']=='^2.14.0'
        else:assert 'adoption' in manifest
        results.append({**record,'manifest':manifest,'features':feature or 'default','reported_packages':len(graph['packages']),
                        'all_reported_package_tuples_bound_to_unchanged_lock':True,'compilation':False})
    dump('metadata-results.json',results)
    manifest=tomllib.loads((WT/'pbrs-grpc/Cargo.toml').read_text())
    requirement=manifest['dependencies']['indexmap'];assert requirement['version']=='2.14.0'
    summary={'schema':'shared-parent-existing-lock-edge-audit/1','source_commit':BASE,'shipping_release_source_commit':None,
             'lock_audit':audit,'frozen_parent154_tuples_retained':True,'frozen_pre_bridge153_tuples_retained':True,
             'adoption_lock_byte_identical':True,'root_lock_byte_identical':True,'only_shipping_file_changed':parent,
             'pbrs_grpc_indexmap_requirement':requirement,'first_locked_metadata_rejected':True,
             'after_locked_offline_metadata_runs':len(results),'compiler_build_capture_invocations':0,
             'qualification':'Source/graph preparation only; final opt-out integration and root freeze remain pending. No performance qualification.'}
    dump('lock-edge-audit.json',summary)
    print(json.dumps(summary,indent=2))
if __name__=='__main__':main()
