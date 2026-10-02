#!/usr/bin/env python3
"""Additional unchanged controls; never supplant the original failed pair."""
import datetime,hashlib,json,os,subprocess,time
from pathlib import Path
root=Path('work/profiles/cold-self-controls');root.mkdir(exist_ok=True)
assert not (root/'metadata.json').exists()
build=json.loads(Path('work/profiles/cl08-cold-release-build.json').read_text())
record={'schema':'cl08-cold-self-controls/1','scope':['rpc.prost.server_stream'],'iters':200,'repeats':3,'preparation_iters':400,'purpose':'quantify unchanged lock stability against original1percentcontrolrule; originalfailedpairretained','wall':'shared-host diagnostic only','launches':[],'comparisons':[]}
def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
for revision in ['before','after']:
 binary=build['baseline_binary'] if revision=='before' else build['binary_after']['path']
 expected=build['baseline_sha256'] if revision=='before' else build['binary_after']['sha256']
 source=build['baseline_source'] if revision=='before' else build['source']
 for repeat in [1,2]:
  label=f'{revision}-{repeat}';assert sha(binary)==expected
  env=dict(os.environ);env['PATH']=str(Path('work/bin').resolve())+':'+env['PATH']
  env['PBRS_REPLAY_VALGRIND']='/workspace/pure-protobuf/work/toolchain/deb/usr/bin/valgrind'
  env['PBRS_REPLAY_RAW']=str((root/(label+'.raw.jsonl')).resolve());env['PBRS_DEVLOOP_COMMIT']=source
  command=[binary,'run','--cells','rpc.prost.server_stream','--iters','200','--repeats','3','--out',str(root/(label+'.json'))]
  launch={'label':label,'revision':revision,'source':source,'binary':binary,'launch_sha256':sha(binary),'command':command,'started_monotonic':time.monotonic(),'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat()}
  record['launches'].append(launch);(root/'metadata.json').write_text(json.dumps(record,indent=2)+'\n')
  result=subprocess.run(command,env=env,text=True,capture_output=True)
  launch.update({'finished_monotonic':time.monotonic(),'finished_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'returncode':result.returncode,'after_sha256':sha(binary),'stdout':result.stdout,'stderr':result.stderr})
  (root/'metadata.json').write_text(json.dumps(record,indent=2)+'\n');print(label,result.returncode,result.stdout,result.stderr,flush=True)
  assert result.returncode==0 and launch['launch_sha256']==launch['after_sha256']==expected
 command=[binary,'compare','--baseline',str(root/(revision+'-1.json')),'--current',str(root/(revision+'-2.json'))]
 result=subprocess.run(command,text=True,capture_output=True)
 record['comparisons'].append({'revision':revision,'command':command,'returncode':result.returncode,'stdout':result.stdout,'stderr':result.stderr})
 (root/'metadata.json').write_text(json.dumps(record,indent=2)+'\n');(root/(revision+'-compare.log')).write_text(result.stdout+result.stderr)
 print(revision,'frozen selfcompare',result.returncode,result.stdout,result.stderr,flush=True)
