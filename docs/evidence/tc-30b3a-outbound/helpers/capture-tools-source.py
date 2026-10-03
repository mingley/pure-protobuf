"""Capture actual tool/source inputs before and after this correctness lease."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

here=Path(__file__).resolve().parent
assert os.environ.get('TC30B_OUTBOUND_ORDINARY_LEASE') == 'granted'
name=sys.argv[1]
output=here/(name+'.json')
assert not output.exists(), output
plan=json.loads((here/'plan.json').read_text())
def sha(p):
    return hashlib.file_digest(p.open('rb'),'sha256').hexdigest()
versions={}
executables={}
for version in ['1.85.0','1.88.0']:
    tools=['rustc','cargo']+(['clippy-driver','rustdoc'] if version=='1.88.0' else [])
    for tool in tools:
        argv=['rustup','run',version,tool,'--version']+(['--verbose'] if tool=='rustc' else [])
        result=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
        assert result.returncode==0, (argv,result.stdout)
        path=Path(subprocess.check_output(['rustup','which','--toolchain',version,tool],text=True).strip()).resolve()
        versions[version+'/'+tool]={'argv':argv,'exit':result.returncode,'stdout':result.stdout}
        executables[version+'/'+tool]={'path':str(path),'bytes':path.stat().st_size,'sha256':sha(path)}
    for path in sorted((Path(os.environ['RUSTUP_HOME'])/'toolchains'/f'{version}-x86_64-unknown-linux-gnu'/'lib').glob('librustc_driver*.so')):
        executables[version+'/'+path.name]={'path':str(path),'bytes':path.stat().st_size,'sha256':sha(path)}
for tool,argv in [('protoc',[plan['environment']['PROTOC'],'--version']),('rustfmt',['rustfmt','--version']),('python',[sys.executable,'--version'])]:
    result=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
    assert result.returncode==0,(argv,result.stdout)
    path=Path(shutil.which(argv[0]) or argv[0]).resolve()
    versions[tool]={'argv':argv,'exit':result.returncode,'stdout':result.stdout}
    executables[tool]={'path':str(path),'bytes':path.stat().st_size,'sha256':sha(path)}
assert versions['protoc']['stdout'].strip()=='libprotoc 35.1'
assert executables['protoc']['sha256']=='ba5165ada96fc34d1295b2056ab8ea99756f7896a0ef0449d07f721595702b28'
sources={}
for lane in ['candidate','baseline']:
    stage=next(x for x in plan['stages'] if x['lane']==lane)
    root=Path(stage['cwd'])
    def git(*args): return subprocess.check_output(['git',*args],cwd=root,text=True).strip()
    assert git('rev-parse','HEAD')==stage['expected_source']
    assert not git('status','--porcelain')
    files=git('ls-files').splitlines()
    selected=[p for p in files if Path(p).suffix in {'.rs','.toml','.lock','.proto','.fds','.pem','.crt','.key'} and not p.startswith(('docs/evidence/','work/'))]
    sources[lane]={'root':str(root),'source':git('rev-parse','HEAD'),'tree':git('rev-parse','HEAD^{tree}'),'tracked_changes':git('status','--porcelain'),'input_sha256':{p:sha(root/p) for p in selected}}
record={'recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'versions':versions,'executables_sha256':executables,'source_inputs':sources,'environment':{k:os.environ.get(k) for k in ['RUSTUP_HOME','CARGO_HOME','PATH','RUSTFLAGS','RUSTC_WRAPPER']},'toolchain_env_sha256':sha(Path('/workspace/pure-protobuf/work/toolchain/env.sh')),'performance':'not_run','disk_free_bytes':shutil.disk_usage(here).free}
output.write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps({'record':str(output),'sources':{k:v['source'] for k,v in sources.items()},'tools':len(executables),'free':record['disk_free_bytes']}))
