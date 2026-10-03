#!/usr/bin/env python3
from pathlib import Path
import subprocess,json,hashlib,shutil,sys,os,datetime
R=Path('/workspace/pure-protobuf');W=R/'work/final-go-cache-reclamation-20261003';P=W/'proof'
BASE='a4f012ab7aae9ba579dac3b0133e3dc18a1251ff';PREFIX='docs/evidence/final-go-cache-reclamation-20261003/'
sha=lambda raw:hashlib.sha256(raw).hexdigest()
old=(P/'artifact-manifest.json').read_bytes();(P/'artifact-manifest-before-independent-check.json').write_bytes(old)
for ext in ['stdout','stderr']:shutil.copy2(W/('verify.'+ext+'.log'),P/('independent-check.'+ext))
(P/'independent-check.json').write_text(json.dumps({'argv':[sys.executable,str(P/'verify.py'),str(P)],'exit':0,'stdout':'independent-check.stdout','stderr':'independent-check.stderr','checker_sha256':sha((P/'verify.py').read_bytes()),'checked_manifest_sha256_before_adding_capture':sha(old),'scope':'Read-only full inventories/selected payloads/module-h1/process/permission/deletion proof replay; no caches or compiler needed'},indent=2)+'\n')
# Include this sealing script as provenance; it writes only a private Git index,
# new evidence objects and a dedicated leaf branch, never the active checkout.
shutil.copy2(Path(__file__),P/'seal_commit.py')
files={str(p.relative_to(P)):{'bytes':p.stat().st_size,'sha256':sha(p.read_bytes())} for p in sorted(P.rglob('*')) if p.is_file() and p.name!='artifact-manifest.json'}
(P/'artifact-manifest.json').write_text(json.dumps({'schema':'sealed-Go-cache-reclamation/1','files':files},indent=2)+'\n')
checked=subprocess.run([sys.executable,str(P/'verify.py'),str(P)],capture_output=True)
(W/'verify-final.stdout.log').write_bytes(checked.stdout);(W/'verify-final.stderr.log').write_bytes(checked.stderr);assert checked.returncode==0,checked.stderr
index=W/'cleanup-private.index';assert not index.exists()
env={**os.environ,'GIT_INDEX_FILE':str(index)}
def git(*args,input=None):return subprocess.check_output(['git','-C',str(R),*args],env=env,input=input)
current=git('rev-parse','HEAD').decode().strip()
assert subprocess.run(['git','-C',str(R),'merge-base','--is-ancestor',BASE,current]).returncode==0
assert not git('ls-tree','-r','--name-only',BASE,'--',PREFIX)
git('read-tree',BASE)
for p in sorted(P.rglob('*')):
 if p.is_file():
  obj=git('hash-object','-w','--stdin',input=p.read_bytes()).decode().strip()
  git('update-index','--add','--cacheinfo','100644',obj,PREFIX+str(p.relative_to(P)))
tree=git('write-tree').decode().strip()
commit=git('commit-tree',tree,'-p',BASE,input=b'docs(evidence): preserve completed Go peer cache reclamation\n').decode().strip()
git('update-ref','refs/heads/mingley/final-go-cache-reclamation-20261003',commit)
rows=git('diff-tree','--no-commit-id','--name-status','-r',commit).decode().splitlines()
assert len(rows)==len([p for p in P.rglob('*') if p.is_file()])
assert all(row.startswith('A\t'+PREFIX) for row in rows)
assert git('rev-parse','HEAD').decode().strip()==current
j={'commit':commit,'parent':BASE,'coordinator_head_unchanged':current,'added_docs_paths':len(rows),'prefix':PREFIX,'archive':json.loads((P/'archive.json').read_text()),'portable_checker_exit':checked.returncode,'work_only_proof':str(P),'checkout_source_or_index_changed':False,'leaf_branch':'mingley/final-go-cache-reclamation-20261003'}
(W/'cleanup-commit.json').write_text(json.dumps(j,indent=2)+'\n');print(json.dumps(j,indent=2))
