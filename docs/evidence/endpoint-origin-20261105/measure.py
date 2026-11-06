#!/usr/bin/env python3
import argparse, datetime, hashlib, json, os, pathlib, random, statistics, subprocess, time

parser=argparse.ArgumentParser(description='Run 9 randomized pairs per mode and separate allocation observations')
parser.add_argument('--out',type=pathlib.Path,required=True)
OUT=parser.parse_args().out.resolve()
CAPTURE=OUT/'measurements'
CAPTURE.mkdir(exist_ok=False)
CPU=min(os.sched_getaffinity(0))
RANDOM=random.Random(0x20261105)
MODES=['stock','deadline','origin-http']
PAIRS=9
ITERATIONS=20_000
WARMUP=1_000
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def inputs():
    return {str(p.relative_to(OUT)):sha(p) for variant in ['baseline','candidate','driver-baseline','driver-candidate'] for p in sorted((OUT/variant).rglob('*')) if p.is_file()}
manifest={'schema':'pbrs.origin-fast-path-capture.v1','started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'scope':'shared-host current-thread in-memory unary client+server process CPU; not network/TLS/RPC ranking',
 'cpu_affinity':[CPU],'iterations':ITERATIONS,'warmup':WARMUP,'pairs':PAIRS,
 'inputs_before':inputs(),'binaries':{},'runs':[],'host_before':{name:(pathlib.Path('/proc')/name).read_text() for name in ['loadavg','cpuinfo']}}
for variant in ['baseline','candidate']:
    for kind in ['cpu','allocations']:
        path=OUT/f'{variant}-{kind}-bin'
        manifest['binaries'][path.name]=sha(path)

def execute(variant,kind,mode,pair,sequence,iterations,warmup):
    binary=OUT/f'{variant}-{kind}-bin'
    command=[str(binary),mode,str(iterations),str(warmup)]
    start=time.monotonic()
    proc=subprocess.run(command,cwd=OUT,capture_output=True,text=True,timeout=60,
        preexec_fn=lambda:os.sched_setaffinity(0,{CPU}))
    stem=f'{kind}-{mode}-{pair}-{sequence}-{variant}'
    (CAPTURE/f'{stem}.stdout').write_text(proc.stdout)
    (CAPTURE/f'{stem}.stderr').write_text(proc.stderr)
    row={'variant':variant,'kind':kind,'mode':mode,'pair':pair,'sequence':sequence,'command':command,
         'exit_code':proc.returncode,'elapsed_seconds':time.monotonic()-start,'stdout':f'{stem}.stdout','stderr':f'{stem}.stderr',
         'stdout_sha256':sha(CAPTURE/f'{stem}.stdout'),'stderr_sha256':sha(CAPTURE/f'{stem}.stderr')}
    manifest['runs'].append(row)
    (CAPTURE/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    assert proc.returncode==0,(stem,proc.stderr)
    data=json.loads(proc.stdout)
    assert data['schema']=='pbrs.origin-default-diagnostic.v1'
    assert data['mode']==mode and data['iterations']==iterations and data['successful']==iterations
    assert data['warmup']==warmup and data['allocation_instrumentation']==(kind=='allocations')
    assert data['request_scheme']=='http' and data['request_path']=='/origin.Bench/Call'
    assert data['request_authority']==('benchmark.local' if mode=='origin-http' else 'benchmark.local:80')
    assert data['wire_payload_bytes']==10 and data['framed_bytes_per_direction']==15
    assert data['process_cpu_ns']>0 and data['wall_ns']>0
    return data

tasks=[(mode,pair) for mode in MODES for pair in range(PAIRS)]
RANDOM.shuffle(tasks)
for mode,pair in tasks:
    order=['baseline','candidate']; RANDOM.shuffle(order)
    for seq,variant in enumerate(order): execute(variant,'cpu',mode,pair,seq,ITERATIONS,WARMUP)
for mode in MODES:
    for pair in range(3):
        order=['baseline','candidate'];RANDOM.shuffle(order)
        for seq,variant in enumerate(order): execute(variant,'allocations',mode,pair,seq,128,128)

summary=[]
for mode in MODES:
    paired=[]
    base_values=[];candidate_values=[]
    allocations={'baseline':[],'candidate':[]}
    for pair in range(PAIRS):
        pair_rows={r['variant']:json.loads((CAPTURE/r['stdout']).read_text()) for r in manifest['runs'] if r['kind']=='cpu' and r['mode']==mode and r['pair']==pair}
        assert set(pair_rows)=={'baseline','candidate'}
        for key in ['iterations','warmup','successful','checksum','wire_payload_bytes','framed_bytes_per_direction','request_scheme','request_authority','request_path','channel_handle_bytes']:
            assert pair_rows['baseline'][key]==pair_rows['candidate'][key],(mode,key,pair_rows)
        old=pair_rows['baseline']['process_cpu_ns']/ITERATIONS
        new=pair_rows['candidate']['process_cpu_ns']/ITERATIONS
        paired.append(new/old);base_values.append(old);candidate_values.append(new)
    for variant in ['baseline','candidate']:
        for r in manifest['runs']:
            if r['kind']=='allocations' and r['mode']==mode and r['variant']==variant:
                data=json.loads((CAPTURE/r['stdout']).read_text())
                allocations[variant].append({'calls_per_rpc':data['allocation_calls']/data['iterations'],
                    'bytes_per_rpc':data['requested_bytes']/data['iterations']})
    summary.append({'mode':mode,'paired_new_old_cpu':paired,'median_ratio':statistics.median(paired),
        'range':[min(paired),max(paired)],'baseline_cpu_ns':statistics.median(base_values),'candidate_cpu_ns':statistics.median(candidate_values),
        'allocations':allocations})
manifest.update(finished_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),inputs_after=inputs(),
                host_after={'loadavg':pathlib.Path('/proc/loadavg').read_text()})
assert manifest['inputs_before']==manifest['inputs_after']
assert len(manifest['runs'])==72
(CAPTURE/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
(CAPTURE/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
