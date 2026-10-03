#!/usr/bin/env python3
"""Check helper-only overlay and replay explicit snapshots without Cargo."""
from pathlib import Path
import hashlib,json,shutil,subprocess,sys,tempfile
HERE=Path(__file__).resolve().parent
sha=lambda b:hashlib.sha256(b).hexdigest()
r=json.loads((HERE/'overlay.json').read_text())
a=(HERE/'replay-helper.before.py').read_bytes()
b=(HERE/'replay-helper.after.py').read_bytes()
assert sha(a)==r['old_sha256'] and sha(b)==r['new_sha256']
op=r['operation'];assert a.count(op['old'].encode())==op['occurrences']==1
assert a.replace(op['old'].encode(),op['new'].encode())==b
assert sha((HERE/'overlay.patch').read_bytes())==r['exact_patch_sha256']
assert sha((HERE/'inputs/backend/wake-guard-1.88-overlay.json').read_bytes())==r['original_wake_guard_overlay_json_sha256']
assert sha((HERE/'inputs/backend/provenance.json').read_bytes())==r['original_topology_mapping_sha256']
assert (HERE/'inputs/backend/replay-wake-guard-1.88-overlay.py').read_bytes()==b
for label,wanted in [('root-before',0),('normalized-layout-before',1),('root-after',0),('root-explicit-after',0),('normalized-layout-after',0)]:
 c=json.loads((HERE/(label+'.json')).read_text())
 assert c['exit']==wanted and c['no_cargo_or_compiler_invoked']
 assert c['input_sha256_before']==c['input_sha256_after']
 for stream in ['stdout','stderr']:
  raw=(HERE/(label+'.'+stream)).read_bytes();assert sha(raw)==c[stream+'_sha256']
 if wanted==0:assert json.loads((HERE/(label+'.stdout')).read_text())['status']=='pass'
 else:assert b'FileNotFoundError' in (HERE/(label+'.stderr')).read_bytes()
with tempfile.TemporaryDirectory(prefix='h2-wake-package-replay-') as directory:
 d=Path(directory)
 h=d/'pbrs-grpc-0.1.0-alpha.2/src/h2_backend'
 shutil.copytree(HERE/'inputs/backend',h)
 old=d/'before';shutil.copytree(HERE/'inputs/before',old)
 script=h/'replay-wake-guard-1.88-overlay.py'
 script.write_bytes(a)
 red=subprocess.run([sys.executable,str(script),str(old)],cwd=d,capture_output=True)
 assert red.returncode==1 and b'FileNotFoundError' in red.stderr
 assert b'pbrs-grpc/src/h2_backend/admission_tests.rs' in red.stderr
 script.write_bytes(b)
 green=subprocess.run([sys.executable,str(script),str(old)],cwd=d,capture_output=True)
 assert green.returncode==0,green.stderr
 assert json.loads(green.stdout)['status']=='pass'
if (HERE/'artifact-sha256.json').exists():
 for name,pin in json.loads((HERE/'artifact-sha256.json').read_text()).items():
  raw=(HERE/name).read_bytes();assert len(raw)==pin['bytes'] and sha(raw)==pin['sha256']
print(json.dumps({'status':'pass','recorded_gates':5,'original_normalized_fixture_exit':red.returncode,'corrected_normalized_fixture_exit':green.returncode,'cargo_compiler_run':False,'runtime_changed':False},indent=2))
