#!/usr/bin/env python3
import argparse, hashlib, json, pathlib, re, subprocess, tarfile

CAPSULE=pathlib.Path(__file__).resolve().parent
parser=argparse.ArgumentParser(description='Export the frozen baseline and ten native overlays')
parser.add_argument('--repo',type=pathlib.Path,default=CAPSULE.parents[2])
parser.add_argument('--out',type=pathlib.Path,required=True)
args=parser.parse_args()
ROOT=args.repo.resolve()
OUT=args.out.resolve()
OUT.mkdir(parents=True,exist_ok=False)
(OUT/'main.rs').write_bytes((CAPSULE/'main.rs').read_bytes())
expected=json.loads((CAPSULE/'source-manifest.json').read_text())
portable=json.loads((CAPSULE/'portable-baseline.json').read_text())
assert portable['recorded_baseline_commit']==expected['baseline_commit']
assert portable['recorded_source_manifest_sha256']==hashlib.sha256((CAPSULE/'source-manifest.json').read_bytes()).hexdigest()
assert set(portable['per_input'])==set(expected['baseline_git_inputs'])
for name,digest in expected['baseline_git_inputs'].items():
    assert portable['per_input'][name]=={'recorded_sha256':digest,'portable_sha256':digest}
with tarfile.open(CAPSULE/'source-overlays.tar.gz','r:gz') as packed:
    overlays={name:packed.extractfile(name).read() for name in expected['candidate_overlays']}
for name,data in overlays.items():
    assert hashlib.sha256(data).hexdigest()==expected['candidate_overlays'][name]
BASE=portable['portable_baseline_commit']
OVERLAYS=[
 'pbrs-grpc/src/client/channel.rs','pbrs-grpc/src/client/pool.rs',
 'pbrs-grpc/src/client/call.rs','pbrs-grpc/src/client/unary.rs',
 'pbrs-grpc/src/client/streaming.rs','pbrs-grpc/src/client/retry.rs',
 'pbrs-grpc/src/wire.rs','pbrs-grpc/src/wire/headers.rs',
 'pbrs-grpc/src/tower_client.rs','pbrs-grpc/src/request.rs',
]
def sha(data): return hashlib.sha256(data).hexdigest()
paths=subprocess.check_output(['git','ls-tree','-r','--name-only','-z',BASE,'--',
 'Cargo.toml','Cargo.lock','build.rs','clippy.toml','rustfmt.toml','src',
 'vendor/google/conformance_fds.bin','pbrs-grpc'],cwd=ROOT).decode().split('\0')
paths=sorted(x for x in paths if x)
assert paths==sorted(expected['baseline_git_inputs']), 'portable Git input set differs'
manifest={'baseline_commit':expected['baseline_commit'],'input_scope':'selected Git source trees, not a filesystem-read trace',
          'baseline_git_inputs':{},'candidate_overlays':{},'variants':{}}
original={}
for path in paths:
    data=subprocess.check_output(['git','show',f'{BASE}:{path}'],cwd=ROOT)
    assert sha(data)==expected['baseline_git_inputs'][path], ('portable baseline input differs',path)
    original[path]=data
    manifest['baseline_git_inputs'][path]=sha(data)
for path in OVERLAYS: manifest['candidate_overlays'][path]=sha(overlays[path])
for variant in ['baseline','candidate']:
    target=OUT/variant
    assert not target.exists(), target
    for path,data in original.items():
        if variant=='candidate' and path in OVERLAYS: data=overlays[path]
        dest=target/path
        dest.parent.mkdir(parents=True,exist_ok=True)
        dest.write_bytes(data)
    cargo=target/'Cargo.toml'
    text=cargo.read_text()
    text,count=re.subn(r'(\[workspace\]\s*members\s*=\s*)\[[\s\S]*?\]',r'\1[".", "pbrs-grpc"]',text,count=1)
    assert count==1
    cargo.write_text(text)
    # Identical observer functions in both isolated exports; no shipping API.
    local=target/'pbrs-grpc/src/client.rs'
    local.write_text(local.read_text()+'''\n// Measurement-only layout observer, absent from production source.\npub(crate) fn __origin_layout_probe() -> (usize, usize) {\n    (std::mem::size_of::<channel::ChannelLocal>(), std::mem::size_of::<channel::Channel>())\n}\n''')
    lib=target/'pbrs-grpc/src/lib.rs'
    lib.write_text(lib.read_text()+'''\n/// Measurement-only layout observer, absent from production source.\n#[doc(hidden)]\npub fn __origin_layout_probe() -> (usize, usize) { client::__origin_layout_probe() }\n''')
    driver=OUT/f'driver-{variant}'
    driver.mkdir()
    (driver/'src').mkdir()
    (driver/'src/main.rs').write_bytes((OUT/'main.rs').read_bytes())
    (driver/'Cargo.lock').write_bytes(original['Cargo.lock'])
    (driver/'Cargo.toml').write_text(f'''[package]\nname="origin-default-diagnostic"\nversion="0.0.0"\nedition="2024"\npublish=false\n[workspace]\n[features]\ndefault=[]\ncandidate=[]\nallocations=[]\n[dependencies]\npbrs-grpc={{path="../{variant}/pbrs-grpc"}}\ntokio={{version="1",features=["rt","macros","io-util","sync","time"]}}\nlibc="0.2"\nserde_json="1"\n[profile.release]\nlto="thin"\ncodegen-units=1\ndebug=0\n''')
    manifest['variants'][variant]={
      'source_files':{str(p.relative_to(target)):sha(p.read_bytes()) for p in sorted(target.rglob('*')) if p.is_file()},
      'driver_files':{str(p.relative_to(driver)):sha(p.read_bytes()) for p in sorted(driver.rglob('*')) if p.is_file()},
      'instrumentation':'common measurement-only layout exports; counting allocator only separate allocations feature',
    }
expected_replay=json.loads(json.dumps(expected))
# The original builder refreshed both identical drivers after preparation.
# Reproduce the actually compiled harness rather than the older initial copy.
for variant in ['baseline','candidate']:
    expected_replay['variants'][variant]['driver_files']['src/main.rs']=sha((CAPSULE/'main.rs').read_bytes())
assert manifest==expected_replay, 'exported inputs differ from the captured inputs'
manifest['reconstruction']={
    'recorded_baseline_commit':expected['baseline_commit'],
    'git_baseline_commit':BASE,
    'portable_alias_manifest_sha256':sha((CAPSULE/'portable-baseline.json').read_bytes()),
}
(OUT/'source-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'git_inputs':len(paths),'overlays':len(OVERLAYS),
                  'recorded_baseline':expected['baseline_commit'],'reconstruction_baseline':BASE},indent=2))
