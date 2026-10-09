"""Finish checks, freeze the benchmark, then gate the unchanged resource run."""
import hashlib,json,os,shutil,subprocess,sys,time
from pathlib import Path
ROOT=Path('/workspace/pure-protobuf');PIN=sys.argv[1];OUT=ROOT/'work/resource-009-prerequisites';OUT.mkdir(exist_ok=True)
CONTROLLER=ROOT/'work/campaign-v5-007/controller.py';SHA='f3f110905fb62642cf5948869708aa1bf3cca2bc58d394b34c7b611ea7c52cac'
state=dict(source=PIN,state='waiting_for_native_checks',qualified=False,passed=False,actual_24h_started=False,actual_24h_completed=False,steps=[])
if (OUT/'status.json').exists():
 previous=json.loads((OUT/'status.json').read_text())
 assert previous['source']==PIN and previous['state']=='failed' and not previous.get('actual_24h_started')
 assert previous.get('error',{}).get('type')=='OSError' and 'No space left on device' in previous['error']['message']
 assert not (ROOT/'work/campaign-v5-009').exists()
 shutil.copy2(OUT/'status.json',OUT/'infrastructure-failure.json')
 shutil.copy2(OUT/'controller.py',OUT/'controller-before-disk-recovery.py')
 state['steps']=previous['steps'];state['resumed_after_disk_exhaustion']=True
(OUT/'controller.py').write_bytes(Path(__file__).read_bytes())
def save():
 p=OUT/'status.json.tmp';p.write_text(json.dumps(state,indent=2)+'\n');p.replace(OUT/'status.json')
def clean():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()==PIN
 assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=all'],cwd=ROOT,text=True)
def run(name,args):
 clean();state['state']=name;save();directory=OUT/name;directory.mkdir();start=time.monotonic()
 with (directory/'stdout').open('w') as out,(directory/'stderr').open('w') as err:p=subprocess.run(args,cwd=ROOT,stdout=out,stderr=err)
 clean();row=dict(name=name,command=args,exit_code=p.returncode,elapsed_seconds=time.monotonic()-start);state['steps'].append(row);save();return p.returncode
save()
try:
 while True:
  try:native=json.loads((ROOT/'work/native-abeb-gates/results.json').read_text())
  except (OSError,json.JSONDecodeError):native={}
  if native.get('state') in ['finished','failed']:break
  time.sleep(3)
 state['native_checks_passed']=native.get('passed') is True;save()
 if not state['steps']:run('publish_native',['python3',str(ROOT/'work/publish-native-abeb.py')])
 os.environ['PROTOC']=str(ROOT/'work/toolchain/protoc/bin/protoc');os.environ['PATH']=str(ROOT/'work/toolchain/protoc/bin')+os.pathsep+os.environ['PATH']
 checks=[]
 for name,args in [('format',['cargo','fmt','--all','--','--check']),('clippy',['cargo','clippy','--locked','-p','pbrs','-p','pbrs-grpc','--all-targets','--all-features','--','-D','warnings']),('python_contracts',['python3','-B','-m','unittest','-q','tests.test_grpc_load_smoke','tests.test_dominance_ledger','tests.test_grpc_resource_campaign','tests.test_counter_campaign','tests.test_counter_capsule'])]:
  previous=[row for row in state['steps'] if row['name']==name]
  checks.append(previous[-1]['exit_code'] if previous else run(name,[str(ROOT/'work/run-rust'),*args]))
 # Every compile/test from the native run and adapter correction has ended.
 # Remove only the inactive debug cache before retrying documentation so
 # disk exhaustion cannot block the later cleanup. Keep all raw failures.
 for proc in Path('/proc').iterdir():
  if proc.name.isdigit():
   try:args=(proc/'cmdline').read_bytes().split(b'\0')
   except OSError:continue
   if b'--crate-name' in args:raise RuntimeError('unexpected active compiler before disk recovery')
 cache=ROOT/'work/target/debug'
 if cache.exists():
  state['disk_recovery_removed_inactive_debug_bytes']=sum(p.stat().st_size for p in cache.rglob('*') if p.is_file());shutil.rmtree(cache);save()
 os.environ['RUSTDOCFLAGS']='-D warnings'
 checks.append(run('rustdoc_retry',[str(ROOT/'work/run-rust'),'cargo','doc','--locked','--no-deps','--all-features','-p','pbrs','-p','pbrs-grpc']))
 state['strict_checks_passed']=all(code==0 for code in checks);save()
 # Preserve the isolated consumer's real output, even if another target
 # failed. Its outer test intentionally prints this output only on failure.
 run('caller_rustls_raw',[str(ROOT/'work/run-rust'),'cargo','test','--locked','--manifest-path','pbrs-grpc/tests/caller-rustls/Cargo.toml','--target-dir',str(ROOT/'work/target/caller-rustls'),'--message-format=json'])
 # All compiler children above have exited. Fail rather than delete a cache
 # if an unexpected compiler is active. Raw reports and copied failed ELFs
 # are outside this cache and stay intact.
 for proc in Path('/proc').iterdir():
  if proc.name.isdigit():
   try:args=(proc/'cmdline').read_bytes().split(b'\0')
   except OSError:continue
   if b'--crate-name' in args:raise RuntimeError('unexpected active compiler before inactive debug-cache cleanup')
 cache=ROOT/'work/target/debug';state['inactive_debug_cache_removed_bytes']=sum(p.stat().st_size for p in cache.rglob('*') if p.is_file());shutil.rmtree(cache);save()
 code=run('benchmark_build',[str(ROOT/'work/run-rust'),'python3',str(ROOT/'scripts/build-rpc-bench.py'),'--source',PIN,'--output',str(ROOT/'work/benchmark-abeb')])
 if code:raise RuntimeError('benchmark build failed; logs retained')
 state['benchmark_ready']=True;save()
 state['diagnostic_only'] = not state['native_checks_passed'] or not state['strict_checks_passed'] or any(row['exit_code'] for row in state['steps'] if row['name']=='caller_rustls_raw')
 state['limits']='known regression failures retained; resource execution is diagnostic and never establishes overall qualification'
 assert hashlib.sha256(CONTROLLER.read_bytes()).hexdigest()==SHA
 state.update(actual_24h_started=None,actual_24h_completed=None,live_resource_status='work/campaign-v5-009/controller-status.json');save()
 code=run('preview_and_day',[str(ROOT/'work/run-rust'),'python3',str(CONTROLLER),'--source',PIN,'--source-dir',str(ROOT),'--root',str(ROOT),'--output',str(ROOT/'work/campaign-v5-009'),'--build-profile','release','--publish-main','--evidence-path','docs/evidence/grpc-resource-soak-20261009-attempt-009'])
 outcome=json.loads((ROOT/'work/campaign-v5-009/controller-status.json').read_text());days=[row for row in outcome['outcomes'] if row['duration_requested_seconds']==86400]
 state.update(state='finished',resource_outcome=outcome,actual_24h_started=bool(days),actual_24h_completed=bool(days and (days[0].get('soak_24h') or {}).get('status')=='completed'),passed=code==0 and bool(days) and all(row['runner_exit_code']==0 and row['validator_exit_code']==0 for row in outcome['outcomes']));save()
except BaseException as error:
 state.update(state='failed',passed=False,error=dict(type=type(error).__name__,message=str(error)));save();raise
