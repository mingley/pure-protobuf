import hashlib,json,pathlib,subprocess,tomllib
root=pathlib.Path(__file__).resolve().parents[2];work=pathlib.Path(__file__).resolve().parent
lock=root/'tests/interop/tonic/Cargo.lock';old=lock.read_bytes();before=tomllib.loads(old.decode())
add=['atomic-waker','fnv','futures-sink','indexmap','slab','tokio-util','tracing']
node=next(p for p in before['package'] if p['name']=='pbrs-grpc')
assert not any(x in node['dependencies'] for x in add)
assert all(sum(p['name']==name for p in before['package'])==1 for name in add)
new=old.decode();start=new.index('name = "pbrs-grpc"\n');deps=new.index('dependencies = [\n',start);end=new.index('\n]',deps)
merged=sorted(node['dependencies']+add)
new=new[:deps]+ 'dependencies = [\n'+''.join(' "'+name+'",\n' for name in merged)+new[end+1:]
after=tomllib.loads(new)
identity=lambda p:(p['name'],p['version'],p.get('source'),p.get('checksum'))
bmap={identity(p):p for p in before['package']};amap={identity(p):p for p in after['package']};assert bmap.keys()==amap.keys()
changed=[]
for key,b in bmap.items():
    a=amap[key]
    if a!=b:changed.append({'identity':key,'before':b.get('dependencies',[]),'after':a.get('dependencies',[])})
assert len(changed)==1 and changed[0]['identity'][0]=='pbrs-grpc'
assert set(changed[0]['after'])-set(changed[0]['before'])==set(add)
assert set(changed[0]['before'])<=set(changed[0]['after'])
assert sum(bool(p.get('source')) for p in before['package'])==146
assert next(p['version'] for p in after['package'] if p['name']=='indexmap')=='2.14.2'
lock.write_text(new)
sha=lambda raw:hashlib.sha256(raw).hexdigest();out=root/'docs/evidence/deadline-admission/native-caller-lock'
out.mkdir(parents=True,exist_ok=True);(out/'tonic-Cargo.lock.before').write_bytes(old);(out/'tonic-Cargo.lock.after').write_bytes(lock.read_bytes());(out/'normalize-tonic-lock.py').write_bytes(pathlib.Path(__file__).read_bytes())
proof={'base_source':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'old_lock_sha256':sha(old),'new_lock_sha256':sha(lock.read_bytes()),'all_package_identity_count':len(bmap),'registry_tuple_count':146,'all_package_identities_unchanged':True,'all_other_node_edges_unchanged':True,'changed_nodes':changed,'added_edges':add,'all_existing_pin_identities':{name:identity(next(p for p in before['package'] if p['name']==name)) for name in add},'indexmap_version':'2.14.2','root_lock_sha256':sha((root/'Cargo.lock').read_bytes()),'manifest_sha256':sha((root/'tests/interop/tonic/Cargo.toml').read_bytes()),'first_locked_red':'044-final-generated-tonic-locked-build','no_cargo_or_compiler_invoked':True}
(out/'audit.json').write_text(json.dumps(proof,indent=2)+'\n');(out/'lock.patch').write_bytes(subprocess.check_output(['git','diff','--','tests/interop/tonic/Cargo.lock'],cwd=root));print(json.dumps({'registry_tuple_count':146,'added_edges':add,'old':proof['old_lock_sha256'],'new':proof['new_lock_sha256']},indent=2))
