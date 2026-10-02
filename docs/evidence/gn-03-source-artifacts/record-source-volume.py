import hashlib, importlib.util, json, subprocess, sys
from pathlib import Path

root=Path.cwd()
spec=importlib.util.spec_from_file_location('codegen_harness',root/'bench/codegen/run.py')
harness=importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)
label=sys.argv[1]
plugin=Path(sys.argv[2]).resolve()
options=sys.argv[3] if len(sys.argv)>3 else 'stubs=none'

def varint(v):
 b=bytearray()
 while v>=128:
  b.append((v&127)|128);v>>=7
 b.append(v);return b

def field(n,v): return varint((n<<3)|2)+varint(len(v))+v

def fields(blob):
 pos=0
 def integer():
  nonlocal pos
  value=0;shift=0
  while True:
   byte=blob[pos];pos+=1;value|=(byte&127)<<shift
   if byte<128:return value
   shift+=7
 while pos<len(blob):
  key=integer();number,wire=key>>3,key&7
  if wire==2:
   size=integer();value=blob[pos:pos+size];pos+=size
  elif wire==0:value=integer()
  elif wire==1:value=blob[pos:pos+8];pos+=8
  elif wire==5:value=blob[pos:pos+4];pos+=4
  else: raise ValueError(wire)
  yield number,wire,value

records={}
for case,(messages,count) in harness.CORPORA.items():
 path=root/'work/source-volume'/case
 proto=path/'proto';proto.mkdir(parents=True,exist_ok=True)
 names=[]
 for i in range(count):
  name=f'part_{i:02d}.proto';names.append(name)
  (proto/name).write_text(harness.render_proto(harness.DEFAULT_SEED,messages,count,i))
 fds=path/'fixture.fds'
 command=['protoc','--include_imports',f'--descriptor_set_out={fds}',f'-I{proto}',*names]
 result=subprocess.run(command,capture_output=True)
 (path/f'{label}-protoc.log').write_bytes(result.stdout+result.stderr)
 assert result.returncode==0,result.stderr
 request=b''.join(field(1,n.encode()) for n in names)+field(2,options.encode())+b''.join(field(15,v) for n,w,v in fields(fds.read_bytes()) if n==1 and w==2)
 (path/f'{label}-request.bin').write_bytes(request)
 result=subprocess.run([str(plugin)],input=request,capture_output=True)
 assert result.returncode==0,result.stderr
 (path/f'{label}-response.bin').write_bytes(result.stdout)
 (path/f'{label}-plugin.log').write_bytes(result.stderr)
 output=path/label;output.mkdir(exist_ok=True)
 generated={}
 for n,w,v in fields(result.stdout):
  if n==1:raise ValueError(v.decode())
  if n!=15 or w!=2:continue
  parts={nn:vv for nn,ww,vv in fields(v) if ww==2}
  name=parts[1].decode();content=parts[15]
  dest=output/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(content)
  generated[name]={'bytes':len(content),'lines':len(content.splitlines()),'sha256':hashlib.sha256(content).hexdigest(),'raw_descriptor_literals':content.count(b'pub const FILE_DESCRIPTOR_SET: &[u8] = &[')}
 records[case]={'messages':messages,'requested_files':count,'files':generated,'total_bytes':sum(v['bytes'] for v in generated.values()),'total_lines':sum(v['lines'] for v in generated.values()),'fds_sha256':hashlib.sha256(fds.read_bytes()).hexdigest(),'protoc_command':command,'options':options}
 print(case,len(generated),records[case]['total_bytes'],flush=True)
record={'case_source':'bench/codegen/run.py CORPORA/DEFAULT_SEED/render_proto; no source info, as Config default','harness_sha256':hashlib.sha256((root/'bench/codegen/run.py').read_bytes()).hexdigest(),'plugin_sha256':hashlib.sha256(plugin.read_bytes()).hexdigest(),'cases':records}
(root/f'work/source-volume-{label}.json').write_text(json.dumps(record,indent=2)+'\n')
