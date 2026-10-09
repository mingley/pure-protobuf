"""Run the complete native inventory on an unchanged source, retaining failures."""
import hashlib,json,os,shutil,subprocess,sys,time
from pathlib import Path
ROOT=Path('/workspace/pure-protobuf');OUT=ROOT/'work/native-abeb-gates';OUT.mkdir(exist_ok=False)
PIN=sys.argv[1]
def clean():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()==PIN
 assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=all'],cwd=ROOT,text=True)
def sha(path):
 h=hashlib.sha256()
 with path.open('rb') as stream:
  for block in iter(lambda:stream.read(1024*1024),b''):h.update(block)
 return h.hexdigest()
os.environ['PROTOC']=str(ROOT/'work/toolchain/protoc/bin/protoc');os.environ['PATH']=str(ROOT/'work/toolchain/protoc/bin')+os.pathsep+os.environ['PATH']
clean();(OUT/'controller.py').write_bytes(Path(__file__).read_bytes())
targets=['lib','resource_bounds','serving','early_upload_response','codegen']+[p.stem for p in sorted((ROOT/'pbrs-grpc/tests').glob('*.rs')) if p.stem not in ['resource_bounds','serving','early_upload_response','codegen','caller_rustls']]+['caller_rustls']
report=dict(source=PIN,state='running',passed=False,qualified=False,targets=targets,msrv=[],results=[],limits=['one independent test case at a time; original workloads, assertions and deadlines','failures retained; no feature, resource-day or performance qualification'])
def save():
 temporary=OUT/'results.json.tmp';temporary.write_text(json.dumps(report,indent=2)+'\n');temporary.replace(OUT/'results.json')
save()
def run(name,args,target,records):
 clean();directory=OUT/name;directory.mkdir();start=time.monotonic()
 with (directory/'stdout').open('w') as out,(directory/'stderr').open('w') as err:p=subprocess.run([str(ROOT/'work/run-rust'),*args],cwd=ROOT,stdout=out,stderr=err)
 row=dict(name=name,command=args,exit_code=p.returncode,elapsed_seconds=time.monotonic()-start)
 matches=[]
 for line in (directory/'stdout').read_text().splitlines():
  try:m=json.loads(line)
  except json.JSONDecodeError:continue
  if m.get('reason')=='compiler-artifact' and m.get('manifest_path')==str(ROOT/'pbrs-grpc/Cargo.toml') and m.get('target',{}).get('name')==target and m.get('profile',{}).get('test') and m.get('executable'):matches.append(m)
 if len(matches)==1:
  binary=Path(matches[0]['executable']);row.update(artifact=matches[0],executable_sha256=sha(binary),executable_bytes=binary.stat().st_size)
  if p.returncode:shutil.copy2(binary,directory/'failed-test')
  binary.unlink();row['inactive_executable_cache_removed']=True
 else:row['artifact_error']='expected one compiler-selected executable'
 clean();row['source_unchanged']=True;records.append(row);save();print(name,p.returncode,flush=True)
 return row
try:
 for name,selector,target in [('msrv_unit',['--lib'],'pbrs_grpc'),('msrv_upload',['--test','early_upload_response'],'early_upload_response')]:
  run(name,['cargo','+1.85.0','test','--locked','-p','pbrs-grpc',*selector,'--target-dir',str(ROOT/'work/target-msrv-abeb'),'--message-format=json','--','--test-threads=1'],target,report['msrv'])
 # MSRV artifacts and commands are recorded above. Keep any failed executables;
 # the inactive compiler cache is reproducible and would crowd out profiling.
 directory=ROOT/'work/target-msrv-abeb'
 report['inactive_msrv_cache_removed_bytes']=sum(p.stat().st_size for p in directory.rglob('*') if p.is_file());shutil.rmtree(directory);save()
 for name in targets:
  run(name,['cargo','test','--locked','-p','pbrs-grpc','--all-features',*(['--lib'] if name=='lib' else ['--test',name]),'--message-format=json','--','--test-threads=1'],'pbrs_grpc' if name=='lib' else name,report['results'])
  # No other compiler job is scheduled during this controller. Each cargo
  # process has exited; retain its outputs and remove only incremental caches.
  cache=ROOT/'work/target/debug/incremental'
  for path in cache.glob(('pbrs_grpc' if name=='lib' else name)+'-*'):
   if path.is_dir():shutil.rmtree(path)
 report.update(state='finished',passed=len(report['results'])==len(targets) and all(row['exit_code']==0 and 'artifact_error' not in row for row in report['results']+report['msrv']));save()
except BaseException as error:
 report.update(state='failed',passed=False,error=dict(type=type(error).__name__,message=str(error)));save();raise
