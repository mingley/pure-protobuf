"""Publish terminal native checks and their failed cases from a separate checkout."""
import gzip,hashlib,io,json,shutil,subprocess,tarfile
from pathlib import Path
ROOT=Path('/workspace/pure-protobuf');report=json.loads((ROOT/'work/native-abeb-gates/results.json').read_text());assert report['state'] in ['finished','failed']
def command(args,cwd=ROOT):return subprocess.run(args,cwd=cwd,check=True)
command(['git','fetch','origin','main']);CHECKOUT=ROOT/'work/native-abeb-publication';relative=Path('docs/evidence/grpc-native-abeb-20261009')
command(['git','worktree','add','--detach','--no-checkout',str(CHECKOUT),'origin/main']);command(['git','sparse-checkout','init','--no-cone'],CHECKOUT);command(['git','sparse-checkout','set','/'+str(relative)+'/','/docs/evidence/README.md'],CHECKOUT);command(['git','checkout','HEAD'],CHECKOUT)
OUT=CHECKOUT/relative;OUT.mkdir(parents=True,exist_ok=False);inputs={}
for name in ['native-abeb-gates']:
 for p in (ROOT/'work'/name).rglob('*'):
  if p.is_file():
   with p.open('rb') as stream:
    if stream.read(4)==b'\x7fELF':continue
   inputs[name+'/'+p.relative_to(ROOT/'work'/name).as_posix()]=p
entries=[]
with (OUT/'raw.tar.gz').open('wb') as raw,gzip.GzipFile(fileobj=raw,mode='wb',mtime=0) as zipped:
 with tarfile.open(fileobj=zipped,mode='w') as tar:
  for name,p in sorted(inputs.items()):
   data=p.read_bytes();member=tarfile.TarInfo(name);member.size=len(data);tar.addfile(member,io.BytesIO(data));entries.append(dict(path=name,bytes=len(data),sha256=hashlib.sha256(data).hexdigest()))
(OUT/'manifest.json').write_text(json.dumps(dict(schema='pbrs.compatibility-capsule.v1',archive=dict(name='raw.tar.gz',sha256=hashlib.sha256((OUT/'raw.tar.gz').read_bytes()).hexdigest()),files=entries,logical_bytes=sum(row['bytes'] for row in entries)),indent=2)+'\n');shutil.copy2(ROOT/'docs/evidence/grpc-readiness-20261008/check.py',OUT/'check.py')
failed=[dict(name=row['name'],exit_code=row['exit_code'],artifact_error=row.get('artifact_error')) for row in report['results']+report['msrv'] if row['exit_code']!=0 or 'artifact_error' in row]
(OUT/'summary.json').write_text(json.dumps(dict(source=report['source'],qualified=False,passed=report['passed'],state=report['state'],target_count=len(report['targets']),completed_targets=len(report['results']),failures=failed,msrv=report['msrv']),indent=2)+'\n')
(OUT/'README.md').write_text('# Complete native test inventory\n\nSource `'+report['source']+'`. The controller attempts all '+str(len(report['targets']))+' native library/integration targets with all features and one independent test case at a time. '+str(len(report['results']))+' targets completed; '+str(len(failed))+' native/MSRV steps failed. See summary.json and the original output for each failure. No case or deadline was removed or relaxed.\n\nThe actual Rust 1.85 default-feature unit and stalled-upload checks run first. Source cleanliness and Cargo-selected executable hashes are recorded. Passed executable caches are removed; failed executables remain local. The portable archive retains commands, source pins, artifacts, stdout, stderr and every completed result. Run `python3 check.py` to verify its inventory.\n\nA test pass does not close remaining features, resource qualification or performance comparisons. The subsequent preview/day and endpoint counter captures are separate diagnostics; overall production qualification remains false.\n')
command(['python3',str(OUT/'check.py')],CHECKOUT)
index=CHECKOUT/'docs/evidence/README.md';text=index.read_text();index.write_text(text.replace('|---|---|---|\n','|---|---|---|\n| [Complete current native inventory](grpc-native-abeb-20261009/README.md) | All requested native targets attempted on the reset-fix source, plus actual Rust 1.85 checks | Failures remain in the capsule; feature, resource and performance qualification stay separate |\n',1))
command(['git','add','docs/evidence'],CHECKOUT);command(['git','diff','--cached','--check'],CHECKOUT);command(['git','commit','-m','docs(evidence): retain complete current native test outcomes'],CHECKOUT)
for attempt in range(3):
 result=subprocess.run(['git','push','origin','HEAD:main'],cwd=CHECKOUT)
 if result.returncode==0:
  revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=CHECKOUT,text=True).strip();(ROOT/'work/readiness/native-abeb-publication.json').write_text(json.dumps(dict(commit=revision,source=report['source'],qualified=False),indent=2)+'\n');command(['git','worktree','remove',str(CHECKOUT)]);break
 command(['git','fetch','origin','main'],CHECKOUT);command(['git','rebase','origin/main'],CHECKOUT)
else:raise RuntimeError('publication rejected; raw outputs and local commit retained')
