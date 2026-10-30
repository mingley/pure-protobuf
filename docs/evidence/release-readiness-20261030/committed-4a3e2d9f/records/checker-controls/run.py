#!/usr/bin/env python3
"""Exercise corruption and semantic rejection using disposable packet copies."""
import datetime
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT=Path('/workspace/pure-protobuf')
PACKET=ROOT/'work/post-commit-readiness-capsule'
OUT=ROOT/'work/post-commit-readiness-controls'
OUT.mkdir(exist_ok=False)


def change_json(path, mutate):
    value=json.loads(path.read_text())
    mutate(value)
    path.write_text(json.dumps(value,indent=2)+'\n')


def source_tamper(path):
    with tarfile.open(path,'r:gz') as archive:
        members=[(member,archive.extractfile(member).read()) for member in archive.getmembers() if member.isfile()]
    with tarfile.open(path,'w:gz') as archive:
        for member,data in members:
            if member.name.endswith('/src/h2_backend/proto/streams/store.rs'):
                data+=b'\n// deliberate control mutation\n'
            member.size=len(data)
            archive.addfile(member,io.BytesIO(data))


cases=[
    ('member-digest','records/resource/progress.json',lambda p:p.write_bytes(p.read_bytes()+b' '),False,
     'records/resource/progress.json'),
    ('committed-source','records/committed-source-binding.json',
     lambda p:change_json(p,lambda x:x['inputs'][0].update(sha256='0'*64)),True,'committed input differs'),
    ('dirty-publisher','records/publisher/report.json',
     lambda p:change_json(p,lambda x:x.update(status_after=' M pbrs-grpc/src/lib.rs')),True,
     "publisher['status_before'] == publisher['status_after']"),
    ('consumer-source','records/packages/consumer/pbrs-grpc-0.1.0-alpha.3.crate',source_tamper,True,
     'adapter source difference'),
    ('resource-promotion','records/resource/report.json',
     lambda p:change_json(p,lambda x:x['qualification'].update(qualified=True)),True,
     "resource['qualification']['qualified'] is False"),
    ('validator-pin','records/resource/grpc-resource-campaign.py',lambda p:p.write_bytes(p.read_bytes()+b'\n# control\n'),True,
     "validator_path.read_bytes() == blob('scripts/grpc-resource-campaign.py')"),
]
results=[]
for name,relative,mutate,rehash,expected in cases:
    with tempfile.TemporaryDirectory(prefix='pbrs-packet-control-',dir=ROOT/'work') as temp:
        copy=Path(temp)/'packet'
        shutil.copytree(PACKET,copy)
        path=copy/relative
        mutate(path)
        if rehash:
            def update(value):
                data=path.read_bytes()
                value['files'][relative]={'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
            change_json(copy/'manifest.json',update)
        proc=subprocess.run([sys.executable,str(copy/'check.py')],cwd=ROOT,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
        (OUT/(name+'.log')).write_text(proc.stdout)
        detected=proc.returncode != 0 and expected in proc.stdout
        results.append({'case':name,'mutated_member':relative,'manifest_rehashed':rehash,
                        'exit_code':proc.returncode,'expected_diagnostic':expected,'detected':detected})
        assert detected,(name,proc.returncode,proc.stdout)
report={'schema':'pbrs.packet-negative-controls.v1','observed_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'checker_sha256':hashlib.sha256((PACKET/'check.py').read_bytes()).hexdigest(),
        'scope':'disposable packet mutations; shipping source and original packet untouched',
        'all_detected':all(row['detected'] for row in results),'cases':results}
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'controls':len(results),'all_detected':report['all_detected']}))
