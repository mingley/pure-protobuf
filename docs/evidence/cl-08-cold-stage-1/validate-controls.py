#!/usr/bin/env python3
"""Reconstruct all cold Stage1 counters and unchanged controls; retain failures."""
import hashlib,json,math,re,shutil,statistics
from pathlib import Path
build=json.loads(Path('work/profiles/cl08-cold-release-build.json').read_text())
all_results={}
for folder in ['cold-primary','cold-self-controls']:
 root=Path('work/profiles')/folder
 meta=json.loads((root/'metadata.json').read_text())
 rows={}
 for launch in meta['launches']:
  rev=launch['revision'];label=launch.get('label',rev)
  expected=build['baseline_sha256'] if rev=='before' else build['binary_after']['sha256']
  source=build['baseline_source'] if rev=='before' else build['source']
  assert launch['returncode']==0 and launch['launch_sha256']==launch['after_sha256']==expected
  assert hashlib.sha256(Path(launch['binary']).read_bytes()).hexdigest()==expected
  assert launch['source']==source
  report=json.loads((root/(label+'.json')).read_text());assert report['devloop_commit']==source
  assert len(report['cells'])==1 and report['cells'][0]['id']=='rpc.prost.server_stream'
  result=report['cells'][0];assert result['iters']==200 and result['repeats']==3
  assert result['instruction_method']=='differential_callgrind_2n_minus_n'
  raw=[json.loads(line) for line in (root/(label+'.raw.jsonl')).read_text().splitlines()];assert len(raw)==6
  graphs=root/(label+'-graphs');graphs.mkdir(exist_ok=True)
  children=[]
  for i,record in enumerate(raw):
   cmd=record['command'];assert record['returncode']==0 and cmd[cmd.index('run-cell')+1]=='rpc.prost.server_stream'
   assert Path(cmd[cmd.index('run-cell')-1]).resolve()==Path(launch['binary']).resolve()
   n=int(cmd[cmd.index('--iters')+1]);assert n==[200,400][i%2]
   assert cmd[cmd.index('--prepare-iters')+1]=='400'
   child=[json.loads(line.removeprefix('__CHILD__ ')) for line in record['stderr'].splitlines() if line.startswith('__CHILD__ ')];assert len(child)==1
   child=child[0];assert child['iters']==n and child['cell']=='rpc.prost.server_stream'
   refs=re.findall(r'I\s+refs:\s*([\d,]+)',record['stderr']);assert len(refs)==1
   children.append((int(refs[0].replace(',','')),child))
   pid=re.search(r'==([0-9]+)==',record['stderr']).group(1)
   original=Path('/tmp/devloop-callgrind.'+pid);assert original.is_file()
   shutil.copyfile(original,graphs/f'rpc.prost.server_stream.{n}.repeat{i//2}.callgrind')
  computed={'instructions':statistics.median((children[i+1][0]-children[i][0])/200 for i in [0,2,4]),'allocs':statistics.median(children[i][1]['allocs']/200 for i in [0,2,4]),'alloc_bytes':statistics.median(children[i][1]['alloc_bytes']/200 for i in [0,2,4])}
  for key,value in computed.items(): assert result[key]['status']=='measured' and result[key]['data']['value']==value
  rows[label]=result
 all_results[folder]=rows
comparisons=[]
for scope,labels in [('original',['before','after']),('unchanged_baseline',['before-1','before-2']),('unchanged_candidate',['after-1','after-2'])]:
 rows=all_results['cold-primary' if scope=='original' else 'cold-self-controls']
 comparison={'scope':scope,'metrics':[]}
 for metric in ['instructions','allocs','alloc_bytes','syscalls','locks']:
  before,after=[rows[label][metric]['data']['value'] for label in labels]
  delta=(after-before)/before if before else None
  comparison['metrics'].append({'metric':metric,'before':before,'after':after,'relative_delta':delta,'unchanged_abs1percent_stable':abs(delta)<=0.01 if delta is not None and scope!='original' else None})
 comparisons.append(comparison)
original=json.loads(Path('work/profiles/cold-primary/metadata.json').read_text())
controls=json.loads(Path('work/profiles/cold-self-controls/metadata.json').read_text())
assert original['frozen_compare']['returncode']==1
assert [r['returncode'] for r in controls['comparisons']]==[0,1]
summary={'schema':'cl08-cold-controls-validation/1','raw_children':36,'original_compare_exit':1,'unchanged_selfcompare_exits':[0,1],'comparisons':comparisons,'measurement_state':'unqualified_unchanged_syscall_futex_counters_exceed_existing1percent_stability_rule','integration':'not_qualified','full20':'not_run','ledger_allocation':'not_run_for_cold_stage','cold_tls_performance':'not_run','wall':'shared-host diagnostic only'}
Path('work/profiles/cl08-cold-controls-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
