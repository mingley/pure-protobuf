#!/usr/bin/env python3
"""Audit committed qualification records; never builds or executes project code."""
import argparse, hashlib, json, re
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument('--artifact-root',type=Path)
parser.add_argument('--rustdoc-archive',type=Path)
args=parser.parse_args()
root=Path(__file__).resolve().parent

def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as stream:
        while data:=stream.read(1024*1024): h.update(data)
    return h.hexdigest()

def read(path):return json.loads(path.read_text())
proof=read(root/'qualification.json');plan=read(root/'plan.json')
assert proof['performance']=='not_run'
assert proof['main_mutated_or_pushed_by_this_agent'] is False
assert proof['shipping_identical_to_frozen'] and proof['public_fixtures_identical_to_baseline']
assert proof['candidate_vs_baseline_tracked_difference']==proof['shipping_files']
assert proof['pins']['candidate']=='87f031d7431355ddcc370a5dba9329e74b838b91'
assert proof['pins']['baseline']=='8f009b9452aeab54d075daacb99371d988f89b98'
manifest=read(root/'MANIFEST.json')
actual={str(p.relative_to(root)) for p in root.rglob('*') if p.is_file() and p.name!='MANIFEST.json'}
assert actual==set(manifest['sha256']), 'manifest omission or unexpected files'
for path,sha in manifest['sha256'].items():assert digest(root/path)==sha,path
for path,sha in proof['source_sha256'].items():assert digest(root/'source'/path)==sha,path
before=read(root/'tools-source-before.json');after=read(root/'tools-source-final.json')
assert before['executables_sha256']==after['executables_sha256']
assert before['toolchain_env_sha256']==after['toolchain_env_sha256']
assert after['source_inputs']['candidate']['source']==proof['pins']['candidate']
assert after['source_inputs']['baseline']['source']==proof['pins']['baseline']
assert after['executables_sha256']['protoc']['sha256']=='ba5165ada96fc34d1295b2056ab8ea99756f7896a0ef0449d07f721595702b28'
for tool,version in [('1.85.0/rustc','rustc 1.85.0'),('1.88.0/rustc','rustc 1.88.0'),('1.88.0/clippy-driver','clippy 0.1.88'),('1.88.0/rustdoc','rustdoc 1.88.0')]:assert version in after['versions'][tool]['stdout']
lookup={};logs={}
for entry in proof['executions']:
    name=entry['name'];path=root/'commands'/(name+'.json');data=read(path);log=(root/'commands'/(name+'.log')).read_text()
    assert digest(path)==entry['record_sha256']
    assert digest(root/'commands'/(name+'.log'))==entry['log_sha256']
    assert int((root/'commands'/(name+'.exit')).read_text())==data['exit']==data['native_exit']==entry['exit']
    assert data['source_before']==data['source_after']==entry['source']
    assert not data['tracked_changes_before'] and not data['tracked_changes_after']
    assert data['disk_guard'] is None and data['minimum_free']>=2**31 and data['maximum_owned_cache']<=2**31
    assert data['environment']['CARGO_BUILD_JOBS']=='1'
    assert data['environment']['PROTOC']==after['executables_sha256']['protoc']['path']
    if data['executable_sha256_before'] is not None:assert data['executable_sha256_before']==data['executable_sha256_after']
    counts=re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed;',log)
    assert entry['passed']==sum(int(a) for a,b in counts)
    assert entry['failed']==sum(int(b) for a,b in counts)
    lookup[name]=data;logs[name]=log
assert len(lookup)==24
for name in proof['checks']:assert lookup[name]['exit']==0,name
for name in proof['semantic_reds']:
    stage=next(s for s in plan['stages'] if s['name']==name)
    assert lookup[name]['exit']==101 and stage['red_evidence'] in logs[name]
    assert 'test result: FAILED. 0 passed; 1 failed;' in logs[name]
    build_stage=stage['argv'][0].split(':')[1]
    assert lookup[build_stage]['exit']==0
    assert lookup[build_stage]['source_before']==lookup[name]['source_before']
    assert read(root/'commands'/(name+'.disposition.json'))['disposition']=='executed_baseline_semantic_red'
assert lookup['candidate-native-affected']['exit']==101
assert 'test result: FAILED. 30 passed; 1 failed;' in logs['candidate-native-affected']
assert 'Running tests/tonic_service_transport.rs' not in logs['candidate-native-affected']
assert lookup['candidate-native-strict']['exit']==101 and logs['candidate-native-strict'].count('error: used `expect()`')==2
passed_events=[]
for stage,count in proof['functional']['stages'].items():
    assert lookup[stage]['exit']==0
    target=None;cases=[]
    for line in logs[stage].splitlines():
        running=re.search(r'Running .*\((?:.*?/)?([\w_]+)-[a-f0-9]+\)',line)
        if running:target=running.group(1)
        case=re.fullmatch(r'test (.+) \.\.\. ok',line)
        if case:
            assert target is not None
            cases.append((target,case.group(1)))
    assert len(cases)==count,(stage,len(cases),count)
    passed_events.extend(cases)
assert len(passed_events)==128==proof['functional']['successful_execution_events']
assert len(set(passed_events))==127==proof['functional']['distinct_selected_tests']
assert lookup['candidate-private-bodies']['source_before']==proof['pins']['initial_candidate']
for stage in ['candidate-native-affected-after-fixture-fix','candidate-native-regressions','candidate-generated-transport','candidate-standalone-strict']:assert lookup[stage]['source_before']==proof['pins']['corrected_candidate']
for stage in ['candidate-native-strict-after-fixture-lint-fix','candidate-rustdoc','candidate-producer-error-after-fixture-lint-fix','candidate-msrv-feature-off','candidate-feature-off-graph','candidate-msrv-tonic','candidate-tonic-graph','candidate-scoped-format','candidate-whitespace']:assert lookup[stage]['source_before']==proof['pins']['candidate']
for stage,present in [('candidate-feature-off-graph',False),('candidate-tonic-graph',True)]:
    for package in ['tonic','h2','tower','hyper']:assert bool(re.search(r'\b'+package+r' v\d',logs[stage]))==present
assert lookup['candidate-msrv-feature-off']['argv'][:5]==['rustup','run','1.85.0','cargo','check']
assert lookup['candidate-msrv-tonic']['argv'][:5]==['rustup','run','1.88.0','cargo','check']
assert lookup['candidate-rustdoc']['environment']['RUSTDOCFLAGS']=='-D warnings'
assert '-D' in lookup['candidate-native-strict-after-fixture-lint-fix']['argv'] and 'warnings' in lookup['candidate-native-strict-after-fixture-lint-fix']['argv']
current=(root/'source/pbrs-grpc/src/tonic_server.rs').read_text();base=(root/'source/base-tonic_server.rs').read_text()
for start,end in [('struct IncomingBody {','async fn drain_response'),('async fn drain_response','fn successful_status')]:assert current[current.index(start):current.index(end)]==base[base.index(start):base.index(end)]
assert 'outbound::requested(rpc.config)' in current
assert proof['default_scope']['compiler_cse_or_predicate_elimination']=='not_established'
assert proof['default_scope']['zero_default_instruction_overhead']=='not_run'
assert proof['default_scope']['entire_default_compiled_path_identity']=='not_claimed'
resources=proof['resources']
assert resources['minimum_observed_global_free_bytes']==min(v['minimum_free'] for v in lookup.values())>=2**31
assert resources['maximum_observed_aggregate_owned_cache_bytes']==max(v['maximum_owned_cache'] for v in lookup.values())<=2**31
assert resources['guard_violations']==0 and resources['both_build_caches_retired']
artifact_hashes=set();verified_inodes={};artifact_count=0
for archive in proof['artifact_manifests']:
    path=root/'artifacts'/archive['stage']/'manifest.json'
    assert digest(path)==archive['manifest_sha256']
    data=read(path)
    assert not data['active_cache_consumers']
    assert archive['files']==len(data['artifacts'])
    for entry in data['artifacts']:
        artifact_count+=1;artifact_hashes.add(entry['sha256'])
        if args.artifact_root:
            path=args.artifact_root/archive['stage']/entry['path'];stat=path.stat()
            assert stat.st_size==entry['bytes'],str(path)
            key=(stat.st_dev,stat.st_ino)
            if key not in verified_inodes:verified_inodes[key]=digest(path)
            assert verified_inodes[key]==entry['sha256'],str(path)
    if archive['retired']:assert read(root/'artifacts'/archive['stage']/'retired.json')['free']>=2**31
for stage in proof['semantic_reds']:assert lookup[stage]['executable_sha256_before'] in artifact_hashes
if args.rustdoc_archive:
    doc=read(root/'rustdoc-generated.json')
    assert args.rustdoc_archive.stat().st_size==doc['bytes']
    assert digest(args.rustdoc_archive)==doc['sha256']
print(json.dumps({'status':'PASS','commands':len(lookup),'successful_functional_executions':len(passed_events),'distinct_selected_tests':len(set(passed_events)),'shipping_source':proof['pins']['candidate'],'baseline':proof['pins']['baseline'],'immutable_artifact_files':artifact_count,'artifacts_verified':bool(args.artifact_root),'rustdoc_verified':bool(args.rustdoc_archive),'minimum_observed_free':resources['minimum_observed_global_free_bytes'],'maximum_observed_owned_cache':resources['maximum_observed_aggregate_owned_cache_bytes'],'performance':'not_run'}))
