#!/usr/bin/env python3
"""Portable SHA/content/source/gate/consumer auditor; no compilers or extraction."""
import hashlib,json,pathlib,re,tarfile,tomllib
HERE=pathlib.Path(__file__).resolve().parent
sha=lambda raw:hashlib.sha256(raw).hexdigest()
outer=json.loads((HERE/'artifact-sha256.json').read_text())
assert set(outer)=={p.name for p in HERE.iterdir() if p.is_file() and p.name!='artifact-sha256.json'}
for name,digest in outer.items():assert sha((HERE/name).read_bytes())==digest,name
proof=json.loads((HERE/'proof.json').read_text())
members=json.loads((HERE/'members-sha256.json').read_text())
assert members=={'source_sets':proof['source_sets'],'raw':proof['raw']}

def archive_bytes(record):
    path=HERE/record['archive'];assert sha(path.read_bytes())==record['sha256'],path
    with tarfile.open(path,'r:gz') as archive:
        data={m.name:archive.extractfile(m).read() for m in archive if m.isfile()}
    assert set(data)==set(record['members_sha256']),path
    assert {p:sha(b) for p,b in data.items()}==record['members_sha256'],path
    return data

raw=archive_bytes(proof['raw']);sources={head:archive_bytes(row) for head,row in proof['source_sets'].items()}
for row in proof['records']:
    prefix='raw/'+row['name'];meta=json.loads(raw[prefix+'.meta.json'])
    assert sha(raw[prefix+'.meta.json'])==row['metadata_sha256']
    assert meta['exit']==row['exit'] and meta['pins_unchanged']==row['pins_unchanged']
    for phase in ('before','after'):
        state=meta[phase];source=sources[state['head']]
        for path,digest in state['files'].items():assert sha(source[path])==digest,(row['name'],phase,path)
        for path,item in state.get('direct_inputs',{}).items():
            assert sha(raw[prefix+'.'+phase+'.inputs/'+path])==item['sha256'],(row['name'],phase,path)
    for stream in ('stdout','stderr'):
        assert sha(raw[prefix+'.'+stream+'.log'])==meta[stream+'_sha256'],row['name']
    if row['pins_unchanged']:assert meta['before']==meta['after'],row['name']
    if row['exit']==0:assert not row['stop_reason'],row['name']

counts={}
for name,expected in [('033-final-msrv185-lib',(526,0,1)),('034-final-msrv185-deadline',(10,0,0)),('035-final-msrv185-retry',(18,0,0)),('036-final-msrv185-preface',(10,0,0)),('040-final-tonic188-lib',(544,0,1)),('041-final-tonic188-deadline',(17,0,0)),('042-final-tonic188-retry',(18,0,0)),('043-final-tonic188-preface',(10,0,0)),('046-final-generated-tonic-clients',(8,0,0)),('047-final-generated-tonic-services',(19,0,0))]:
    output=raw['raw/'+name+'.stdout.log'].decode();found=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',output);assert len(found)==1,name
    actual=tuple(map(int,found[0]));assert actual==expected,(name,actual);counts[name]=actual

identity=lambda p:(p['name'],p['version'],p.get('source'),p.get('checksum'))
provider=tomllib.loads(sources[proof['consumers']['source']]['Cargo.lock'].decode());registry={identity(p) for p in provider['package'] if p.get('source')};assert len(registry)==166
for kind in ('path','package'):
    for profile,item in proof['consumers'][kind].items():
        label=f'consumer-final-{kind}-{profile}-run';meta=json.loads(raw['raw/'+label+'.meta.json']);assert meta['exit']==0 and meta['pins_unchanged']
        lock=tomllib.loads(raw['raw/'+label+'.before.inputs/0/Cargo.lock'].decode());pins={identity(p) for p in lock['package'] if p.get('source')}
        assert pins<=registry and len(pins)==105
        assert next(p['version'] for p in lock['package'] if p['name']=='indexmap')=='2.14.0'
        assert item['direct_inputs_before_after_unchanged']

for name,item in proof['packages'].items():
    if name=='embedded_all_members':continue
    path=next(p for p in raw if p.startswith('packages/'+name+'-') and p.endswith('.crate'))
    assert sha(raw[path])==item['crate_sha256'],name
    import io
    with tarfile.open(fileobj=io.BytesIO(raw[path]),mode='r:gz') as archive:
        members={m.name:sha(archive.extractfile(m).read()) for m in archive if m.isfile()}
    assert members==item['members_sha256'],name
embedded=proof['packages']['embedded_all_members'];assert sum(p.endswith('.rs') for p in embedded)==54
provider=sources[proof['consumers']['source']]
for path,digest in embedded.items():assert sha(provider['pbrs-grpc/'+path])==digest,path
# The helper-only package merge leaves all original compilation source bytes intact.
identity_proof=proof['compiler_input_identity'];a=sources[identity_proof['compiler_qualified_source']];b=sources[identity_proof['final_helper_package_source']]
for path,blob in a.items():
    if path.endswith('.rs') or path.endswith('Cargo.toml') or path.endswith('Cargo.lock'):
        assert b[path]==blob,path
# Final test lock changes only one local dependency-node edge set.
final=sources[proof['final_provider_source']];base=sources[proof['consumers']['source']]
a=tomllib.loads(base['tests/interop/tonic/Cargo.lock'].decode());b=tomllib.loads(final['tests/interop/tonic/Cargo.lock'].decode());before={identity(p):p for p in a['package']};after={identity(p):p for p in b['package']};assert before.keys()==after.keys()
changed=[key for key in before if before[key]!=after[key]];assert len(changed)==1 and changed[0][0]=='pbrs-grpc'
node=changed[0];old=set(before[node]['dependencies']);new=set(after[node]['dependencies']);assert not(old-new) and new-old=={'atomic-waker','fnv','futures-sink','indexmap','slab','tokio-util','tracing'}
assert sum(bool(p.get('source')) for p in b['package'])==146
assert next(p['version'] for p in b['package'] if p['name']=='indexmap')=='2.14.2'
assert final['Cargo.lock']==base['Cargo.lock']
print(json.dumps({'status':'pass','source_sets':len(sources),'raw_members':len(raw),'gate_records':len(proof['records']),'checked_final_test_counts':counts,'consumers':6,'provider_registry_tuples':166,'consumer_registry_tuples':105,'test_registry_tuples':146,'backend_rust_members':54,'compiler_or_runtime_launched':False},indent=2))
