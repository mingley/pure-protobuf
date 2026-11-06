#!/usr/bin/env python3
"""Check archived inputs and arithmetic without executing archived code."""
from pathlib import Path
import argparse
import copy
import gzip
import hashlib
import json
import statistics
import subprocess

HERE=Path(__file__).resolve().parent
def sha(value): return hashlib.sha256(value).hexdigest()
def load_files(folder):
    manifest=json.loads((folder/'manifest.json').read_text())
    compressed=(folder/'raw.json.gz').read_bytes()
    assert sha(compressed)==manifest['compressed_sha256']
    with gzip.open(folder/'raw.json.gz','rb') as stream: raw=stream.read(8*1024**2+1)
    assert len(raw)<=8*1024**2 and len(raw)==manifest['uncompressed_bytes']
    assert sha(raw)==manifest['uncompressed_sha256']
    document=json.loads(raw)
    assert document['schema']=='pbrs.packed-wire-raw.v1'
    files={name:value.encode() for name,value in document['files'].items()}
    assert files.keys()==manifest['files'].keys()
    for name,item in manifest['files'].items():
        assert len(files[name])==item['bytes'] and sha(files[name])==item['sha256'],name
    return manifest,files

def find_checkout(folder):
    for path in [folder,Path.cwd()]:
        result=subprocess.run(['git','rev-parse','--show-toplevel'],cwd=path,capture_output=True,text=True)
        if result.returncode==0: return Path(result.stdout.strip())
    raise AssertionError('Run in a checkout containing the named Git base')

def validate(files,manifest,checkout=None):
    def data(name): return json.loads(files[name])
    def lines(name): return [json.loads(line) for line in files[name].splitlines()]
    plan=data('frozen-plan.json')
    frozen=data('controller-freeze.json')
    build_set=data('canonical-build-set.json')
    assert plan['source_base']==manifest['source_base']=='e72629e4bf6225b5b18b72e64c758eb3a174518d'
    assert frozen['controller_source_sha256']==sha(files['controller.py'])
    assert frozen['plan_sha256']==build_set['plan_sha256']==sha(files['frozen-plan.json'])
    assert frozen['build_set_sha256']==sha(files['canonical-build-set.json'])
    for name,key in [('harness/src/main.rs','harness_source_sha256'),('harness/src/reference.rs','reference_source_sha256'),
                     ('harness/Cargo.toml','harness_manifest_sha256'),('harness/Cargo.lock','harness_lock_sha256')]:
        assert sha(files[name])==plan[key],name
    assert sha(files['guard-only-cpu-baseline-packed.rs'])==plan['baseline_source_sha256']
    assert sha(files['candidate-packed.rs'])==plan['candidate_source_sha256']
    assert plan['baseline_source_sha256']=='3995c4a2203698df6b909606b0e7f48cdf960c286d38dcbf85fba1812996d308'
    assert plan['candidate_source_sha256']=='a7d0f416bab91a1e73d39ab4f7e8bb7f1e5b6ba092c23ad0cb7a627f8eaf313f'
    expected_cases=[f'tat.{op}' for op in ['fresh','cached','encode_get']]
    expected_cases += [f'i32.canonical.{count}.{op}' for count in [8,4096]
                       for op in ['fresh','cached','encode_get','get_encode','owned']]
    expected_cases += ['i32.early.8.fresh','i32.late.4096.fresh','bool.normalize.8.fresh',
                       'fixed32.canonical.4096.fresh','fixed32.canonical.4096.cached']
    assert [row['case'] for row in plan['rows']]==expected_cases
    assert plan['cpu_pairs']==9 and plan['allocation_iterations']==64
    assert plan['warmup_iterations']==16 and plan['warmup_objects_independent'] and plan['pool_batch']==32
    cells=[(row,peer) for row in plan['rows'] for peer in row['peers']]
    assert len(cells)==24
    for row,peer in cells:
        assert row['peers']==(['pbrs','prost-grow','prost-reserve'] if row['case'].startswith('tat.') else ['pbrs'])
    schedule=[]
    for pair in range(9):
        for number,(row,peer) in enumerate(cells):
            variants=['baseline','candidate'] if (pair+number)%2==0 else ['candidate','baseline']
            schedule.extend(dict(pair=pair,case=row['case'],peer=peer,variant=variant,iterations=row['iterations']) for variant in variants)
    assert schedule==plan['cpu_schedule'] and len(schedule)==432

    builds={}
    for name in build_set['reports']:
        report=data(name);variant,mode=report['variant'],report['mode'];builds[variant,mode]=report
        assert frozen['build_reports'][name]==sha(files[name])
        assert report['exit_code']==0 and report['guard'] is None
        assert report['target_bytes']<250*1024**2 and report['free_bytes']>1024**3
        assert report['source_before']==report['source_after']
        overlay='guard-only-cpu-baseline-packed.rs' if variant=='baseline' else 'candidate-packed.rs'
        assert report['source_before']['src/packed.rs']==sha(files[overlay])
        assert report['source_before']['src/packed/wire_tests.rs']==sha(files['tree/src/packed/wire_tests.rs'])
        assert report['executable_sha256']==report['retained_sha256']
        log=name[:-5]+'.jsonl'
        assert report['build_log_sha256']==sha(files[log])
        messages=[json.loads(line) for line in files[log].splitlines() if line.startswith(b'{')]
        assert messages[-1]==dict(reason='build-finished',success=True)
        for target,field in [('pbrs','library_artifact'),('packed-wire-screen','binary_artifact')]:
            assert [item for item in messages if item.get('reason')=='compiler-artifact' and item['target']['name']==target]==[report[field]]
        inputs=report['constant_inputs']
        for key in ['harness_source_sha256','reference_source_sha256','harness_manifest_sha256','harness_lock_sha256']:
            assert inputs[key]==plan[key]
        for item in ['Cargo.toml','Cargo.lock']:
            assert inputs['runtime_manifest_sha256' if item=='Cargo.toml' else 'runtime_lock_sha256']==sha(files['tree/'+item])
        assert inputs['plan_sha256']==sha(files['frozen-plan.json'])
    assert set(builds)=={(variant,mode) for variant in ['baseline','candidate'] for mode in ['cpu','alloc']}
    for mode in ['cpu','alloc']:
        a,b=builds['baseline',mode],builds['candidate',mode]
        assert a['constant_inputs']==b['constant_inputs']
        assert a['canonical_source_root']==b['canonical_source_root'] and a['canonical_harness_root']==b['canonical_harness_root']
        assert a['rustc_commands']==b['rustc_commands'] and a['crate_metadata']==b['crate_metadata']
        assert {name for name in a['source_before'] if a['source_before'][name]!=b['source_before'][name]}=={'src/packed.rs'}
        for field in ['binary_artifact','library_artifact']:
            for key in ['package_id','manifest_path','profile','features','filenames']:
                assert a[field][key]==b[field][key],(field,key)
    assert builds['baseline','cpu']['crate_metadata']['pbrs']=='faefb173c03080ab'
    if checkout is not None:
        selected=builds['baseline','cpu']['source_before']
        altered={'Cargo.toml','Cargo.lock','src/packed.rs','src/packed/wire_tests.rs'}
        names=sorted(set(selected)-altered)
        request=''.join(f'{plan["source_base"]}:{name}\n' for name in names).encode()
        packed=subprocess.check_output(['git','cat-file','--batch'],input=request,cwd=checkout)
        offset=0
        for name in names:
            end=packed.index(b'\n',offset);header=packed[offset:end].split()
            assert len(header)==3 and header[1]==b'blob',name
            length=int(header[2]);offset=end+1
            assert selected[name]==sha(packed[offset:offset+length]),name
            offset+=length+1
        assert offset==len(packed)
        original=subprocess.check_output(['git','show',plan['source_base']+':src/packed.rs'],cwd=checkout)
        assert original==files['baseline-packed.rs']

    for mode in ['cpu','alloc']:
        expected=schedule if mode=='cpu' else [dict(case=row['case'],peer=peer,variant=variant,iterations=64)
                                              for row,peer in cells for variant in ['baseline','candidate']]
        records=lines(mode+'-processes.jsonl');summary=data(mode+'-summary.json')
        assert len(records)==len(expected)==summary['planned']==summary['observed']==summary['passed']
        assert summary['raw_sha256']==sha(files[mode+'-processes.jsonl'])
        assert summary['plan_sha256']==sha(files['frozen-plan.json'])
        for index,(record,step) in enumerate(zip(records,expected,strict=True)):
            assert all(record[key]==value for key,value in step.items())
            assert record['sequence']==index and record['status']=='pass' and record['exit_code']==0 and not record['stderr']
            assert record['executable_sha256']==builds[record['variant'],mode]['retained_sha256']
            assert record['controller_freeze_sha256']==sha(files['controller-freeze.json'])
            assert record['command'][1:]==[record['case'],record['peer'],str(record['iterations'])]
            value=record['output'];assert json.loads(record['stdout'])==value
            assert value['schema']=='pbrs.packed-wire-screen.v1'
            assert value['semantic_checks'] and value['independent_warmup'] and value['tat_lazy_unknown_checked']
            assert value['allocation_instrumentation']==(mode=='alloc')
            assert value['case']==record['case'] and value['peer']==record['peer'] and value['iterations']==record['iterations']
            if record['case'].startswith('tat.'):
                incoming=87;outgoing=87 if record['peer']=='pbrs' else 83
                accessed=112 if record['case']=='tat.encode_get' else 0
            else:
                codec,pattern,count,operation=record['case'].split('.');count=int(count)
                incoming=count*(4 if codec=='fixed32' else 1)+(pattern in ['early','late'])
                outgoing=count*4+4 if codec=='fixed32' else (10 if count==8 else 4099)
                accessed=(84 if count==8 else 4096) if operation in ['encode_get','get_encode'] else 0
            assert value['input_bytes_per_op']==incoming and value['encoded_bytes_per_op']==outgoing
            assert value['checksum']==record['iterations']*(outgoing+accessed)
            if mode=='cpu':
                assert value['cpu_ns']>0 and value['wall_ns']>0
                assert value['allocation_calls'] is value['requested_bytes'] is None
            else:
                assert value['cpu_ns'] is value['wall_ns'] is None
                assert value['allocation_calls']>0 and value['requested_bytes']>0
        assert [row['case'] for row in summary['rows']]==expected_cases
        for row in summary['rows']:
            for peer,value in row['peers'].items():
                groups={variant:[record for record in records if record['case']==row['case'] and record['peer']==peer and record['variant']==variant]
                        for variant in ['baseline','candidate']}
                assert all(len(group)==(9 if mode=='cpu' else 1) for group in groups.values())
                if mode=='cpu':
                    for metric in ['cpu_ns','wall_ns']:
                        for variant,group in groups.items():
                            samples=[record['output'][metric]/record['iterations'] for record in group]
                            assert value[variant][metric]==dict(median=statistics.median(samples),minimum=min(samples),maximum=max(samples),samples=samples)
                        ratios=[b['output'][metric]/a['output'][metric] for a,b in zip(groups['baseline'],groups['candidate'],strict=True)]
                        key='paired_candidate_over_baseline_'+('cpu' if metric=='cpu_ns' else 'wall')
                        assert value[key]==dict(median=statistics.median(ratios),minimum=min(ratios),maximum=max(ratios),samples=ratios)
                else:
                    for variant,group in groups.items():
                        output=group[0]['output']
                        assert value[variant]=={key:output[key]/64 for key in ['allocation_calls','requested_bytes']}

    for name in ['final-unit','final-clippy']+[f'final-miri-{bits}-{case}' for bits in ['64','32'] for case in ['recovery','races','layout','hook-panic','windows']]:
        receipt=data(name+'.json')
        assert receipt['exit_code']==0 and receipt['guard'] is None
        assert receipt['source']['src/packed.rs']==plan['candidate_source_sha256']
        assert receipt['source']['src/packed/wire_tests.rs']==sha(files['tree/src/packed/wire_tests.rs'])
    assert b'114 passed; 0 failed' in files['final-unit.log']
    for bits in ['64','32']:
        for case in ['recovery','races','layout','hook-panic','windows']:
            assert (b'3 passed; 0 failed' if case=='recovery' else b'1 passed; 0 failed') in files[f'final-miri-{bits}-{case}.log']
        name=f'guard-only-miri-{bits}'
        assert data(name+'.json')['exit_code']==0
        assert data(name+'.json')['source']['src/packed.rs']==plan['baseline_source_sha256']
        assert b'2 passed; 0 failed' in files[name+'.log']
    assert data('baseline-recovery-both-red.json')['exit_code']==101
    assert b'0 passed; 2 failed' in files['baseline-recovery-both-red.log']
    assert data('candidate-recovery-both-red.json')['exit_code']==101
    assert b'0 passed; 2 failed' in files['candidate-recovery-both-red.log']
    assert data('baseline-guard-only-green.json')['exit_code']==0
    assert b'2 passed; 0 failed' in files['baseline-guard-only-green.log']
    old=lines('before-tat-byte-report-fix/alloc-processes.jsonl');new=lines('alloc-processes.jsonl')
    assert len(old)==len(new)==48
    corrected=0
    for a,b in zip(old,new,strict=True):
        assert all(a[key]==b[key] for key in ['case','peer','variant'])
        for key,value in a['output'].items():
            if key=='encoded_bytes_per_op': corrected+=value!=b['output'][key]
            else: assert value==b['output'][key]
    assert corrected==12
    return dict(passed=True,archived_files=len(files),cpu_processes=432,allocation_processes=48,
                preliminary_allocation_processes=48,minimal_runtime_tests=114,
                final_targeted_miri_executions=14,guard_only_miri_executions=4,
                scope='retained source/receipt/accounting check; no executable replay or ranking qualification')

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--packet',type=Path,default=HERE)
    args=parser.parse_args()
    manifest,files=load_files(args.packet)
    checkout=find_checkout(args.packet)
    answer=validate(files,manifest,checkout)
    def altered_json(values,name,change):
        document=json.loads(values[name]);change(document)
        values[name]=(json.dumps(document,indent=2)+'\n').encode()
    negatives=[]
    altered=dict(files)
    altered_json(altered,'cpu-summary.json',lambda value:value['rows'][0]['peers']['pbrs']['paired_candidate_over_baseline_cpu'].__setitem__('median',999.0))
    negatives.append(('changed paired ratio',altered))
    altered=dict(files)
    altered['cpu-processes.jsonl']=b'\n'.join(altered['cpu-processes.jsonl'].splitlines()[:-1])+b'\n'
    altered_json(altered,'cpu-summary.json',lambda value:value.__setitem__('raw_sha256',sha(altered['cpu-processes.jsonl'])))
    negatives.append(('missing final CPU process',altered))
    altered=dict(files)
    altered_json(altered,'cpu-summary.json',lambda value:value['rows'].pop(14))
    negatives.append(('missing late-noncanonical summary row',altered))
    for label,mode,field,bad in [('wrong executable binding','cpu','executable_sha256','0'*64),
                                 ('clock in allocation record','alloc','output',None),
                                 ('wrong Prost output size','cpu','output',87)]:
        altered=dict(files)
        records=[json.loads(line) for line in altered[mode+'-processes.jsonl'].splitlines()]
        record=next(value for value in records if value['peer']=='prost-reserve') if bad==87 else records[0]
        if field=='executable_sha256': record[field]=bad
        elif bad==87:
            record['output']['encoded_bytes_per_op']=87
            record['stdout']=json.dumps(record['output'])+'\n'
        else:
            record['output']['cpu_ns']=1
            record['stdout']=json.dumps(record['output'])+'\n'
        altered[mode+'-processes.jsonl']=b''.join((json.dumps(value)+'\n').encode() for value in records)
        altered_json(altered,mode+'-summary.json',lambda value:value.__setitem__('raw_sha256',sha(altered[mode+'-processes.jsonl'])))
        negatives.append((label,altered))
    for label,altered in negatives:
        try: validate(altered,manifest)
        except (AssertionError,KeyError,StopIteration): pass
        else: raise AssertionError('negative control accepted: '+label)
    answer['negative_controls_rejected']=len(negatives)
    answer['checkout_root']=str(checkout)
    print(json.dumps(answer))

if __name__=='__main__': main()
