#!/usr/bin/env python3
import argparse, datetime, hashlib, json, os, pathlib, shutil, signal, subprocess, time, tomllib

CAPSULE=pathlib.Path(__file__).resolve().parent
parser=argparse.ArgumentParser(description='Build the frozen CPU and separate allocator observers')
parser.add_argument('--repo',type=pathlib.Path,default=CAPSULE.parents[2])
parser.add_argument('--out',type=pathlib.Path,required=True)
parser.add_argument('--candidate-only',action='store_true')
args=parser.parse_args()
ROOT=args.repo.resolve()
OUT=args.out.resolve()
TARGET=OUT/'target'
FLOOR=1024**3
CAP=600*1024**2
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def files(root): return {str(p.relative_to(root)):sha(p) for p in sorted(root.rglob('*')) if p.is_file()}
def used(path): return sum(p.stat().st_size for p in path.rglob('*') if p.is_file()) if path.exists() else 0
def run(command,cwd,env,stem):
    with stem.with_suffix('.stdout').open('w') as stdout, stem.with_suffix('.stderr').open('w') as stderr:
        process=subprocess.Popen(command,cwd=cwd,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
        guard=None
        while process.poll() is None:
            if shutil.disk_usage(OUT).free<FLOOR or used(TARGET)>CAP:
                guard='disk_floor' if shutil.disk_usage(OUT).free<FLOOR else 'target_cap'
                os.killpg(process.pid,signal.SIGTERM)
                process.wait(timeout=30)
                break
            time.sleep(0.25)
    return process.returncode,guard
manifest=json.loads((OUT/'source-manifest.json').read_text())
env=os.environ.copy()
for variable,local,fallback in [('CARGO_HOME',ROOT/'work/cargo',pathlib.Path.home()/'.cargo'),('RUSTUP_HOME',ROOT/'work/rustup',pathlib.Path.home()/'.rustup')]:
    env.setdefault(variable,str(local if local.is_dir() else fallback))
env.update(CARGO_TARGET_DIR=str(TARGET),CARGO_BUILD_JOBS='1',CARGO_INCREMENTAL='0',CARGO_PROFILE_RELEASE_DEBUG='0')
env['PATH']=str(pathlib.Path(env['CARGO_HOME'])/'bin')+':'+str(ROOT/'work/toolchain/protoc/bin')+':'+env['PATH']
rustc=subprocess.check_output(['rustc','-Vv'],env=env,text=True)
cargo=subprocess.check_output(['cargo','-V'],env=env,text=True)
root_packages={(x['name'],x['version'],x.get('source'),x.get('checksum')) for x in tomllib.loads((OUT/'baseline/Cargo.lock').read_text())['package']}
builds=OUT/'builds'
builds.mkdir(exist_ok=args.candidate_only)
records=[]
if args.candidate_only:
    for kind in ['cpu','allocations']:
        records.append(json.loads((builds/f'baseline-{kind}.json').read_text()))
for variant in (['candidate'] if args.candidate_only else ['baseline','candidate']):
    # Baseline executables are already frozen outside this target. The original
    # capture hit its resource guard before this supported package cleanup.
    if variant=='candidate' and not args.candidate_only:
        cleanup=['cargo','clean','--release','-p','pbrs','-p','pbrs-grpc']
        result,guard=run(cleanup,OUT/'driver-baseline',env,builds/'inter-variant-clean')
        assert result==0 and guard is None,('inter-variant-clean',result,guard)
    source=OUT/variant
    driver=OUT/f'driver-{variant}'
    # Refresh the copied, identical harness after harmless preparation edits.
    (driver/'src/main.rs').write_bytes((OUT/'main.rs').read_bytes())
    metadata=['cargo','metadata','--offline','--format-version=1','--filter-platform=x86_64-unknown-linux-gnu']
    result,guard=run(metadata,driver,env,builds/f'{variant}-resolve')
    assert result==0 and guard is None, (variant,result,guard)
    locked=tomllib.loads((driver/'Cargo.lock').read_text())['package']
    registry={(x['name'],x['version'],x.get('source'),x.get('checksum')) for x in locked if x.get('source')}
    assert registry<=root_packages, sorted(registry-root_packages)
    for counter in [False,True]:
        kind='allocations' if counter else 'cpu'
        name=f'{variant}-{kind}'
        features=[]
        if variant=='candidate': features.append('candidate')
        if counter: features.append('allocations')
        command=['cargo','build','--release','--locked','--offline','--message-format=json']
        if features: command+=['--features',','.join(features)]
        before=files(source)
        driver_before=files(driver)
        started=datetime.datetime.now(datetime.timezone.utc).isoformat()
        start=time.monotonic()
        result,guard=run(command,driver,env,builds/name)
        record={'variant':variant,'kind':kind,'command':command,'started_utc':started,'elapsed_seconds':time.monotonic()-start,'exit_code':result,'guard':guard,'rustc':rustc,'cargo':cargo,'source_inputs_before':before,'source_inputs_after':files(source),'driver_inputs_before':driver_before,'driver_inputs_after':files(driver),'source_inputs_unchanged':before==files(source),'driver_inputs_unchanged':driver_before==files(driver),'target_bytes':used(TARGET),'free_after':shutil.disk_usage(OUT).free}
        (builds/f'{name}.attempt.json').write_text(json.dumps(record,indent=2)+'\n')
        assert result==0 and guard is None,(name,result,guard,str(builds/name))
        assert record['source_inputs_unchanged'] and record['driver_inputs_unchanged']
        artifacts=[json.loads(line) for line in (builds/name).with_suffix('.stdout').read_text().splitlines()]
        executables={x['executable'] for x in artifacts if x.get('reason')=='compiler-artifact' and x.get('executable') and x.get('target',{}).get('name')=='origin-default-diagnostic'}
        assert len(executables)==1,executables
        executable=pathlib.Path(executables.pop())
        frozen=OUT/f'{name}-bin'
        shutil.copy2(executable,frozen)
        record.update(executable=str(executable),binary_sha256=sha(frozen),binary_bytes=frozen.stat().st_size,stdout_sha256=sha((builds/name).with_suffix('.stdout')),stderr_sha256=sha((builds/name).with_suffix('.stderr')))
        for mode in ['stock','deadline','origin-http']:
            process=subprocess.run([str(frozen),mode,'8','4'],cwd=driver,env=env,text=True,capture_output=True,timeout=20)
            (builds/f'{name}-{mode}-precheck.stdout').write_text(process.stdout)
            (builds/f'{name}-{mode}-precheck.stderr').write_text(process.stderr)
            assert process.returncode==0,(name,mode,process.stderr)
            row=json.loads(process.stdout)
            assert row['successful']==8 and row['iterations']==8
        (builds/f'{name}.json').write_text(json.dumps(record,indent=2)+'\n')
        records.append({k:v for k,v in record.items() if not k.endswith('_before') and not k.endswith('_after')})
        print(json.dumps(records[-1]),flush=True)
assert (OUT/'driver-baseline/Cargo.lock').read_bytes()==(OUT/'driver-candidate/Cargo.lock').read_bytes()
(OUT/'build-summary.json').write_text(json.dumps({'builds':records,'target_bytes':used(TARGET),'lock_sha256':sha(OUT/'driver-baseline/Cargo.lock')},indent=2)+'\n')
