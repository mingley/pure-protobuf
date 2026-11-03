"""Four rehashed semantic mutations of disposable copies; no workload execution."""
from pathlib import Path
import hashlib,json,shutil,subprocess,sys,tempfile
ROOT=next(path for path in Path(__file__).resolve().parents if (path/'.git').exists())
SOURCE=ROOT/'docs/evidence/readiness-followup-20261102/committed-08cbcafb'
OUT=ROOT/'work/followup-20261102/postcommit-packet/controls';OUT.mkdir(exist_ok=False)
def write(path,value):path.write_text(json.dumps(value,indent=2)+'\n')
results=[]
for name,expected in [('source-binding','previous selected input set differs'),('publisher-dirty','status_before'),('resource-promotion','qualification'),('validator-pin','validator.read_bytes()')]:
 with tempfile.TemporaryDirectory(prefix='committed-packet-control-',dir=ROOT/'work') as temp:
  packet=Path(temp)/'packet';shutil.copytree(SOURCE,packet)
  if name=='source-binding':
   path=packet/'records/source-binding.json';value=json.loads(path.read_text());value['inputs'][0]['sha256']='0'*64;write(path,value)
  elif name=='publisher-dirty':
   path=packet/'records/publisher/report.json';value=json.loads(path.read_text());value['status_after']=' M src/dynamic.rs';write(path,value)
  elif name=='resource-promotion':
   path=packet/'records/resource/report.json';value=json.loads(path.read_text());value['qualification']['qualified']=True;write(path,value)
  else:
   path=packet/'records/resource/grpc-resource-campaign.py';path.write_bytes(path.read_bytes()+b'\n# changed validator\n')
  manifest=json.loads((packet/'manifest.json').read_text())
  relative=str(path.relative_to(packet));data=path.read_bytes();manifest['files'][relative]={'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()};write(packet/'manifest.json',manifest)
  proc=subprocess.run([sys.executable,str(packet/'check.py')],cwd=ROOT,text=True,capture_output=True)
  text=proc.stdout+proc.stderr;(OUT/(name+'.log')).write_text(text)
  assert proc.returncode!=0 and 'AssertionError' in text and expected in text,(name,proc.returncode,text[-1500:])
  results.append({'name':name,'exit_code':proc.returncode,'expected_failure':expected,'rehashed_manifest':True,'log':name+'.log','log_sha256':hashlib.sha256(text.encode()).hexdigest()})
report={'schema':'pbrs.committed-followup-negative-controls.v1','scope':'disposable rehashed record mutations; no Cargo, publisher or resource campaign execution','detected':len(results),'controls':results}
write(OUT/'report.json',report);print(json.dumps(report,indent=2))
