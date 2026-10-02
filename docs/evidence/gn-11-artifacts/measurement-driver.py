import hashlib
import json
import os
import shutil
import subprocess
import time
from pathlib import Path

root=Path('/workspace/scratch/work/gn11')
measure=root/'target/gn11-measurements-corrected'
measure.mkdir(exist_ok=True)
runtime=root/'target/gn11-runtime-base'
main='''include!("../gen/mod.rs");
fn main() {
    let mut b = wktsharing::OptB::new();
    b.set_id(31);
    let wire = pbrs::Serialize::serialize(&b).unwrap();
    let decoded = <wktsharing::OptB as pbrs::Parse>::parse(&wire).unwrap();
    assert_eq!(decoded.id(), 31);
    let mut c = wktsharing::OptC::new();
    c.set_id(41);
    assert_eq!(c.id(), 41);
}
'''
records=[]
for label,generated in [('before','gn11-baseline'),('after','gn11-after')]:
    consumer=measure/label
    assert not consumer.exists(), f'refusing to reuse timed consumer {consumer}'
    (consumer/'src').mkdir(parents=True)
    shutil.copytree(root/'target'/generated,consumer/'gen')
    (consumer/'src/main.rs').write_text(main)
    (consumer/'Cargo.toml').write_text(f'[package]\nname="gn11-{label}-measure"\nversion="0.0.0"\nedition="2021"\n[workspace]\n[dependencies]\npbrs={{path={json.dumps(str(runtime))}}}\n')
    env={**os.environ,'CARGO_TARGET_DIR':str(consumer/'target'),'CARGO_BUILD_JOBS':'1',
         'CARGO_PROFILE_DEV_DEBUG':'0','CARGO_INCREMENTAL':'0'}
    phases=[]
    for phase,args in [('cold_check',['check']),('warm_check',['check']),('build',['build']),('run',['run','--quiet'])]:
        command=['cargo',*args,'--offline','--manifest-path',str(consumer/'Cargo.toml')]
        start=time.perf_counter()
        with (consumer/f'{phase}.log').open('w') as log:
            result=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
        elapsed=time.perf_counter()-start
        phases.append({'phase':phase,'command':command,'seconds':elapsed,'exit':result.returncode})
        print(label,phase,result.returncode,round(elapsed,4),flush=True)
        if result.returncode:
            print((consumer/f'{phase}.log').read_text()[-4000:],flush=True)
            break
    records.append({'label':label,'phases':phases})
summary={'qualification':'dev-loop, local diagnostic only','runtime_source':'cf3eee22c6b06324e299f81b76915651471d8025',
         'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),
         'profile':{'jobs':1,'debug':0,'incremental':False},
         'main_sha256':hashlib.sha256(main.encode()).hexdigest(),
         'watched_protobuf_directory':'present and empty; same committed vendored FDS fallback',
         'vendored_fds_sha256':hashlib.sha256((runtime/'vendor/google/conformance_fds.bin').read_bytes()).hexdigest(),
         'lock_sha256':{label:hashlib.sha256((measure/label/'Cargo.lock').read_bytes()).hexdigest() for label in ['before','after']},
         'plugin_sha256':hashlib.sha256((root/'target/base/debug/protoc-gen-pbrs').read_bytes()).hexdigest(),
         'records':records}
Path('/workspace/pure-protobuf/work/gn11-downstream-cost-corrected.json').write_text(json.dumps(summary,indent=2)+'\n')
