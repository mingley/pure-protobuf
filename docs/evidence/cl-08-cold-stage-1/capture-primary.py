#!/usr/bin/env python3
"""Execute only under the coordinator's quiet lease; frozen original-N primary."""
import hashlib,json,os,subprocess,time
from pathlib import Path

root=Path('work/profiles/cold-primary')
root.mkdir(exist_ok=True)
assert not (root/'metadata.json').exists(), 'retain each attempt separately'
build=json.loads(Path('work/profiles/cl08-cold-release-build.json').read_text())
rows=['rpc.prost.server_stream']
record={'schema':'cl08-cold-primary-capture/1','scope':rows,'iters':200,'repeats':3,
        'preparation_iters':400,'wall':'shared-host diagnostic only','launches':[]}
def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
for revision in ['before','after']:
    binary=build['baseline_binary'] if revision=='before' else build['binary_after']['path']
    expected=build['baseline_sha256'] if revision=='before' else build['binary_after']['sha256']
    source=build['baseline_source'] if revision=='before' else build['source']
    assert sha(binary)==expected
    env=dict(os.environ)
    env['PATH']=str(Path('work/bin').resolve())+':'+env['PATH']
    env['PBRS_REPLAY_VALGRIND']='/workspace/pure-protobuf/work/toolchain/deb/usr/bin/valgrind'
    env['PBRS_REPLAY_RAW']=str((root/(revision+'.raw.jsonl')).resolve())
    env['PBRS_DEVLOOP_COMMIT']=source
    command=[binary,'run','--cells',','.join(rows),'--iters','200','--repeats','3','--out',str(root/(revision+'.json'))]
    launch={'revision':revision,'source':source,'binary':binary,'launch_sha256':sha(binary),'command':command,'started_monotonic':time.monotonic()}
    record['launches'].append(launch)
    (root/'metadata.json').write_text(json.dumps(record,indent=2)+'\n')
    result=subprocess.run(command,env=env,text=True,capture_output=True)
    launch.update({'finished_monotonic':time.monotonic(),'returncode':result.returncode,'after_sha256':sha(binary),'stdout':result.stdout,'stderr':result.stderr})
    (root/'metadata.json').write_text(json.dumps(record,indent=2)+'\n')
    print(revision,result.returncode,result.stdout,result.stderr,flush=True)
    assert result.returncode==0 and launch['launch_sha256']==launch['after_sha256']==expected
command=[build['binary_after']['path'],'compare','--baseline',str(root/'before.json'),'--current',str(root/'after.json')]
result=subprocess.run(command,text=True,capture_output=True)
record['frozen_compare']={'command':command,'returncode':result.returncode,'stdout':result.stdout,'stderr':result.stderr}
(root/'metadata.json').write_text(json.dumps(record,indent=2)+'\n')
(root/'compare.log').write_text(result.stdout+result.stderr)
print('frozen compare',result.returncode,result.stdout,result.stderr,flush=True)
