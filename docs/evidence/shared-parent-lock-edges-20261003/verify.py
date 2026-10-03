#!/usr/bin/env python3
"""Read-only independent lock/evidence check; no Cargo or compilation."""
import hashlib
import json
from pathlib import Path
import tomllib

ROOT=Path(__file__).resolve().parent
EDGES={'atomic-waker','fnv','futures-sink','indexmap','slab','tokio-util','tracing'}
def parse(data):
    return {(p['name'],p['version'],p.get('source')):p for p in tomllib.loads(data.decode())['package']}
def sha(data):return hashlib.sha256(data).hexdigest()
def main():
    manifest=json.loads((ROOT/'artifact-sha256.json').read_bytes())
    actual={str(p.relative_to(ROOT)) for p in ROOT.rglob('*') if p.is_file() and p.name!='artifact-sha256.json'}
    assert actual==manifest.keys()
    for name,row in manifest.items():
        data=(ROOT/name).read_bytes();assert len(data)==row['size'] and sha(data)==row['sha256'],name
    audit=json.loads((ROOT/'lock-edge-audit.json').read_bytes())
    assert audit['source_commit']=='ad79b48ee34d4fd67346560da195f69ee6a846c3'
    assert audit['shipping_release_source_commit'] is None
    counts={}
    for name,count in (('Cargo.lock',172),('bench/devloop/Cargo.lock',154),('bench/devloop/adoption/Cargo.lock',111)):
        old=(ROOT/'before'/name).read_bytes();new=(ROOT/'after'/name).read_bytes()
        a,b=parse(old),parse(new);assert a.keys()==b.keys() and len(a)==count
        for key in a:
            assert {k:v for k,v in a[key].items() if k!='dependencies'}=={k:v for k,v in b[key].items() if k!='dependencies'}
            if name=='bench/devloop/Cargo.lock' and key[0]=='pbrs-grpc':
                assert set(b[key]['dependencies'])-set(a[key]['dependencies'])==EDGES
                assert set(a[key]['dependencies'])<=set(b[key]['dependencies'])
            else:assert a[key]==b[key]
        if name!='bench/devloop/Cargo.lock':assert old==new
        else:assert sha(old)=='a5fa1e79eb2b3741513e479c393fda804f2f6a19416b9560929f033fb944398c' and sha(new)=='61399401c8e57ceb6aa697fb404bee9ac22f6ec48441faa8dfae74daabbf83c4'
        assert [k[1] for k in a if k[0]=='indexmap']==(['2.14.0'] if name=='Cargo.lock' else ['2.14.2'])
        counts[name]=count
    first=json.loads((ROOT/'first-locked-metadata.json').read_bytes())
    assert first['exit_code']==101 and {'--offline','--locked'}<=set(first['argv'])
    assert b'lock' in (ROOT/first['stderr']).read_bytes().lower()
    metadata=json.loads((ROOT/'metadata-results.json').read_bytes());assert len(metadata)==4
    for record in metadata:
        assert record['exit_code']==0 and {'metadata','--offline','--locked'}<=set(record['argv'])
        assert not any(x in record['argv'] for x in ('build','test','clippy','run'))
        graph=json.loads((ROOT/record['stdout']).read_bytes())
        lock='bench/devloop/adoption/Cargo.lock' if 'adoption' in record['manifest'] else 'bench/devloop/Cargo.lock'
        pins=parse((ROOT/'after'/lock).read_bytes())
        for package in graph['packages']:assert (package['name'],package['version'],package.get('source')) in pins
        if lock=='bench/devloop/Cargo.lock':
            grpc=next(p for p in graph['packages'] if p['name']=='pbrs-grpc')
            assert EDGES<={d['name'] for d in grpc['dependencies'] if d['kind']!='dev'}
            assert next(d for d in grpc['dependencies'] if d['name']=='indexmap')['req']=='^2.14.0'
        else:assert not any(p['name']=='pbrs-grpc' for p in graph['packages'])
    old153=parse((ROOT/'frozen-pre-bridge-parent.lock').read_bytes());assert len(old153)==153
    new154=parse((ROOT/'after/bench/devloop/Cargo.lock').read_bytes())
    assert old153.keys()<=new154.keys()
    for key,p in old153.items():
        assert all(p.get(f)==new154[key].get(f) for f in ('name','version','source','checksum'))
    plan=json.loads((ROOT/'refreshed-launch-plan.json').read_bytes())
    historical=json.loads((ROOT/'historical-launch-plan.json').read_bytes())
    assert plan['source_commit'] is None and plan['provisional_inventory_source_commit']==audit['source_commit']
    for key in ('profile','tools','tc32','sb32','full20_controls'):assert plan[key]==historical[key],key
    assert len(plan['full20_controls']['exact_cells'])==20 and plan['full20_controls']['exact_cells']==json.loads((ROOT/'frozen20-cells.json').read_bytes())
    assert plan['tc32']['registered']==336 and plan['sb32']['registered_new_tonic_codegen_rows']==512
    current=json.loads((ROOT/'current-main-schema-inventory.json').read_bytes())
    assert len(current['schema_sha256'])==88 and len(current['required_third_party_schemas'])==14
    print(json.dumps({'result':'passed','package_tuples_retained':counts,'only_new_parent_edges':sorted(EDGES),
                      'frozen153_parent_pins_retained':True,'initial_locked_rejection_preserved':True,
                      'four_metadata_runs_verified':True,'workload_profile_tool_threshold_contract_unchanged':True,
                      'final_release_source_commit':None,'compiler_build_capture_invocations':0},indent=2))
if __name__=='__main__':main()
