#!/usr/bin/env python3
from pathlib import Path
import subprocess,json,hashlib,sys,shutil,datetime
R=Path('/workspace/scratch/work/h2-embedded')
H=R/'pbrs-grpc/src/h2_backend'
D=R/'docs/evidence/deadline-admission/embedded-topology/wake-replay-package-path'
W=R/'work/h2-embedding/wake-replay-package-path'
N=W/'pbrs-grpc-0.1.0-alpha.2/src/h2_backend'
OLD=R/'docs/evidence/deadline-admission/embedded-topology/wake-guard-1.88'
sha=lambda raw:hashlib.sha256(raw).hexdigest()
files=['provenance.json','wake-guard-1.88-overlay.json','wake-guard-1.88-overlay.patch','admission_tests.rs','admission_additional_tests.rs','replay-wake-guard-1.88-overlay.py']
if sys.argv[1]=='before':
 D.mkdir();N.mkdir(parents=True)
 for name in files: shutil.copy2(H/name,N/name)
 (D/'replay-helper.before.py').write_bytes((H/files[-1]).read_bytes())
else:
 shutil.copy2(H/files[-1],N/files[-1])
 (D/'replay-helper.after.py').write_bytes((H/files[-1]).read_bytes())

def invoke(label,script,old=None):
 argv=[sys.executable,str(script)]
 if old:argv.append(str(old))
 before={str(p):sha(p.read_bytes()) for p in [script,script.parent/'provenance.json',script.parent/'wake-guard-1.88-overlay.json',script.parent/'wake-guard-1.88-overlay.patch',script.parent/'admission_tests.rs',script.parent/'admission_additional_tests.rs',OLD/'admission_tests.rs.before',OLD/'admission_additional_tests.rs.before']}
 utc=datetime.datetime.now(datetime.timezone.utc).isoformat()
 x=subprocess.run(argv,cwd=W,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 (D/(label+'.stdout')).write_bytes(x.stdout);(D/(label+'.stderr')).write_bytes(x.stderr)
 record={'argv':argv,'cwd':str(W),'started_utc':utc,'exit':x.returncode,'stdout_sha256':sha(x.stdout),'stderr_sha256':sha(x.stderr),'input_sha256_before':before,'input_sha256_after':{p:sha(Path(p).read_bytes()) for p in before},'python_path':sys.executable,'python_sha256':sha(Path(sys.executable).read_bytes()),'python_version':subprocess.check_output([sys.executable,'--version'],text=True).strip(),'no_cargo_or_compiler_invoked':True}
 assert record['input_sha256_before']==record['input_sha256_after']
 (D/(label+'.json')).write_text(json.dumps(record,indent=2)+'\n')
 print(label,x.returncode,x.stderr.decode()[-300:])
 return x
if sys.argv[1]=='before':
 assert invoke('root-before',H/files[-1]).returncode==0
 red=invoke('normalized-layout-before',N/files[-1],OLD)
 assert red.returncode!=0 and b'FileNotFoundError' in red.stderr
 assert b'pbrs-grpc/src/h2_backend/admission_tests.rs' in red.stderr
else:
 assert invoke('root-after',H/files[-1]).returncode==0
 assert invoke('root-explicit-after',H/files[-1],OLD).returncode==0
 assert invoke('normalized-layout-after',N/files[-1],OLD).returncode==0
