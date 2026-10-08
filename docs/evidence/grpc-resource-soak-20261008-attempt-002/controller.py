"""Run the pinned preview/day and publish their retained outcomes to main."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import time


def command(argv, cwd, **kwargs):
    return subprocess.run(argv, cwd=cwd, check=True, **kwargs)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capsule(output, destination):
    destination.mkdir(parents=True, exist_ok=False)
    files = [p for p in sorted(output.iterdir()) if p.is_file() and p.name != 'resource-test']
    entries = [{'path':p.name,'bytes':p.stat().st_size,'sha256':sha(p)} for p in files]
    archive = destination/'raw.tar.gz'
    with archive.open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', mtime=0) as zipped:
        with tarfile.open(fileobj=zipped, mode='w') as tar:
            for p in files:
                info = tar.gettarinfo(str(p), arcname=p.name)
                info.uid = info.gid = 0
                info.uname = info.gname = ''
                info.mtime = 0
                with p.open('rb') as stream: tar.addfile(info, stream)
    manifest = {'schema':'pbrs.resource-soak-capsule.v1','archive':{'name':archive.name,'sha256':sha(archive)},'files':entries,'logical_bytes':sum(e['bytes'] for e in entries)}
    (destination/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source',required=True)
    parser.add_argument('--source-dir',type=Path,required=True)
    parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--publish-main',action='store_true')
    parser.add_argument('--evidence-path',type=Path)
    args=parser.parse_args()
    source=args.source_dir.resolve(); root=args.root.resolve(); output=args.output.resolve()
    output.mkdir(parents=True,exist_ok=False)
    (output/'controller.py').write_bytes(Path(__file__).read_bytes())
    preview=output/'preview'; day=output/'24h'
    runner=source/'scripts/grpc-resource-campaign.py'
    commands=[]; outcomes=[]
    for duration,directory in [(30,preview),(86400,day)]:
        argv=['python3',str(runner),'--source',args.source,'--duration',str(duration),'--output',str(directory),'--seed','20261008']
        commands.append(argv)
        status={'state':'running_preview' if duration==30 else 'running_24h','source':args.source,'duration_seconds':duration,'started_unix_seconds':time.time(),'qualified':False,'commands':commands}
        (output/'controller-status.json').write_text(json.dumps(status,indent=2)+'\n')
        with (output/f'runner-{duration}.stdout').open('w') as out, (output/f'runner-{duration}.stderr').open('w') as err:
            result=subprocess.run(argv,cwd=source,stdout=out,stderr=err,check=False)
        report_path=directory/'report.json'
        report=json.loads(report_path.read_text()) if report_path.exists() else {}
        validation=['python3',str(runner),'--validate',str(report_path)]
        with (output/f'validation-{duration}.stdout').open('w') as out, (output/f'validation-{duration}.stderr').open('w') as err:
            checked=subprocess.run(validation,cwd=source,stdout=out,stderr=err,check=False)
        outcomes.append({'duration_requested_seconds':duration,'runner_exit_code':result.returncode,'validator_exit_code':checked.returncode,'source':report.get('source'), 'duration_actual_seconds':report.get('duration_actual_seconds'),'smoke':report.get('smoke'),'soak_24h':report.get('qualification',{}).get('soak_24h'),'validation_command':validation})
        if result.returncode != 0 or checked.returncode != 0:
            break
    status={'state':'finished','source':args.source,'finished_unix_seconds':time.time(),'qualified':False,'commands':commands,'outcomes':outcomes}
    (output/'controller-status.json').write_text(json.dumps(status,indent=2)+'\n')
    if not args.publish_main:
        return
    # Use a fresh detached checkout. Do not modify the active source pin.
    command(['git','fetch','origin','main'],root)
    checkout=output/'publication-checkout'
    command(['git','worktree','add','--detach',str(checkout),'origin/main'],root)
    relative=args.evidence_path or Path('docs/evidence')/f'grpc-resource-soak-{args.source[:8]}-{int(status["finished_unix_seconds"])}'
    if len(relative.parts)!=3 or relative.parts[:2]!=('docs','evidence') or '..' in relative.parts:
        raise ValueError('publication must have its own directory under docs/evidence')
    destination=checkout/relative
    if destination.exists():
        started=json.loads((destination/'started.json').read_text())
        if started.get('source_commit')!=args.source or started.get('controller_sha256')!=sha(output/'controller.py'):
            raise ValueError('existing evidence directory does not belong to this campaign')
    else:
        destination.mkdir(parents=True,exist_ok=False)
    shutil.copy2(output/'controller-status.json',destination/'outcome.json')
    shutil.copy2(output/'controller.py',destination/'controller.py')
    summary=[]
    for directory in [preview,day]:
        if not directory.exists(): continue
        capsule(directory,destination/directory.name)
        report_path=directory/'report.json'
        if report_path.exists():
            report=json.loads(report_path.read_text())
            summary.append(f'- {directory.name}: actual {report.get("duration_actual_seconds")} seconds; resource checks {report.get("smoke",{}).get("status")}; 24-hour disposition {report.get("qualification",{}).get("soak_24h",{}).get("status")}.')
        else:
            summary.append(f'- {directory.name}: no completed report; build or runner failure.')
        shutil.copy2(source/'docs/evidence/grpc-readiness-20261008/check.py',destination/directory.name/'check.py')
        command(['python3',str(destination/directory.name/'check.py')],checkout)
    for p in output.iterdir():
        if p.is_file() and p.suffix in ['.stdout','.stderr']:
            shutil.copy2(p,destination/p.name)
    (destination/'README.md').write_text('# Resource campaign outcome\n\nSource `'+args.source+'`. The controller ran a 30-second preview before requesting an actual 86,400-second campaign. Original reports and failures are retained.\n\n'+'\n'.join(summary)+'\n\nOverall production qualification remains false. This resource fixture does not close performance, feature, allocator high-water, kernel-memory, or dedicated-host release gates. See outcome.json for runner and validator exit codes, and each capsule manifest for raw hashes. Run the capsule check.py to verify its exact inventory.\n')
    command(['git','add',str(relative)],checkout)
    command(['git','diff','--cached','--check'],checkout)
    command(['git','commit','-m','docs(evidence): retain pinned resource campaign outcome'],checkout)
    for attempt in range(3):
        result=subprocess.run(['git','push','origin','HEAD:main'],cwd=checkout,check=False)
        if result.returncode==0:
            status['publication_commit']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=checkout,text=True).strip()
            status['publication_path']=str(relative)
            (output/'controller-status.json').write_text(json.dumps(status,indent=2)+'\n')
            return
        command(['git','fetch','origin','main'],checkout)
        command(['git','rebase','origin/main'],checkout)
    raise RuntimeError('main rejected publication; retained the local commit and all captures')


if __name__=='__main__':
    main()
