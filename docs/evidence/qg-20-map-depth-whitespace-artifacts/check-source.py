#!/usr/bin/env python3
"""Verify additive exact blank-line correction; no Git/compiler/generation."""
import hashlib,json,re,runpy,tarfile
from pathlib import Path
here=Path(__file__).resolve().parent
sha=lambda b:hashlib.sha256(b).hexdigest()
old=here.parent/'qg-20-map-depth-source-artifacts'
assert sha((old/'raw-source-proof.tar.gz').read_bytes())=='3ccbd9f0c200b72f956f09d3637a53addd74f0481a570a4e586116f50eb93fb2'
original=runpy.run_path(str(old/'check-source.py'))
for name,record in json.loads((here/'artifact-sha256.json').read_text()).items():
 data=(here/name).read_bytes();assert len(data)==record['bytes'] and sha(data)==record['sha256'],name
members=json.loads((here/'raw-member-sha256.json').read_text());payload={}
with tarfile.open(here/'raw-source-proof.tar.gz','r:gz')as archive:
 for m in archive:
  assert m.isfile()and m.name in members and m.name not in payload
  b=archive.extractfile(m).read();r=members[m.name];assert len(b)==r['bytes']and sha(b)==r['sha256'];payload[m.name]=b
assert set(payload)==set(members)
prefix='work/qg20/whitespace-correction-003/'
record=json.loads(payload[prefix+'correction.json']);assert record['removed_blank_lines']==109 and len(record['rows'])==7
pattern=re.compile(record['pattern']);total=0
for r in record['rows']:
 path=r['path'];before=payload[prefix+'before/'+path];after=payload[prefix+'after/'+path]
 assert before==original['payloads']['source-candidate/'+path]
 assert sha(before)==r['before_sha256']and sha(after)==r['after_sha256']
 assert len(before)-len(after)==r['removed_blank_lines']
 expected,n=pattern.subn(record['replacement'],before.decode());assert expected.encode()==after and n==r['removed_blank_lines']
 assert re.sub(rb'\s+',b'',before)==re.sub(rb'\s+',b'',after)
 total+=n
assert total==109
for name,b in payload.items():
 if name.startswith('unchanged-source/'):
  path=name.removeprefix('unchanged-source/')
  old_name=('matched-fixtures/'+path)if path.startswith('tests/')or path=='Cargo.lock'else('source-candidate/'+path)
  assert b==original['payloads'][old_name],path
assert len(re.findall(rb'#\[test\]\s*fn\s+\w+',payload['unchanged-source/tests/fixtures/generated_map_value_depth_consumer.rs']))==15
reconstruction=json.loads(payload[prefix+'source-reconstruction.after.json']);assert reconstruction['operations']==327 and reconstruction['map_entries']==109
assert b'AssertionError' in payload['work/qg20/whitespace-correction-002/apply.stderr']
assert b'"blank_lines_removed": 109' in payload[prefix+'apply.stdout']
print(json.dumps({'status':'passed','scope':'additive source whitespace proof only; actual generation/compile/runtime NOT_RUN','removed_exact_blank_lines':109,'files':7,'operation_reconstruction':327,'unchanged15_oracles_NOT_RUN':True,'original_archive_immutable':True,'raw_members':len(payload)},sort_keys=True))
