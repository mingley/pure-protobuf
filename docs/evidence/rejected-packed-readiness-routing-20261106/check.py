#!/usr/bin/env python3
"""Check archived inputs and arithmetic without executing archived code."""
from pathlib import Path
import argparse
import copy
import gzip
import hashlib
import json
import re
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
    assert plan['candidate_source_sha256']=='0906d8f9564393fc725a2ff7c90e410849bb4ef08c667dbd1c0b1b466f338bac'
    assert manifest['experiment']=='cache-first-readiness-routing' and manifest['decision']=='REJECT_UNSHIPPED'
    assert manifest['candidate_tests']==122 and manifest['targeted_miri_executions']==30
    static_plan=data('assembly-plan.json')
    assert static_plan['execute_authorized'] is False and static_plan['timings_authorized'] is False
    for name,digest in static_plan['source_sha256'].items():
        assert sha(files[name])==digest
    assert data('reference-provenance.json')['sha256']==sha(files['harness/src/reference.rs'])
    expected_cases=[f'tat.{op}' for op in ['fresh','cached','encode_get']]
    expected_cases += [f'i32.canonical.{count}.{op}' for count in [8,4096]
                       for op in ['fresh','cached','encode_get','get_encode','owned']]
    expected_cases += ['i32.early.8.fresh','i32.late.4096.fresh','bool.normalize.8.fresh',
                       'fixed32.canonical.4096.fresh','fixed32.canonical.4096.cached']
    assert [row['case'] for row in plan['rows']]==expected_cases
    assert plan['actual_tat_encoded_bytes']==dict(pbrs=87,prost=83)
    assert plan['pre_benchmark_gate_freeze_sha256']==sha(files['correctness-assembly-freeze.json'])
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
        assert report['source_before']['src/packed/fused_tests.rs']==sha(files['tree/src/packed/fused_tests.rs'])
        assert report['source_before']['src/packed/cache_first_tests.rs']==sha(files['tree/src/packed/cache_first_tests.rs'])
        assert report['executable_sha256']==report['retained_sha256']
        if mode=='cpu':
            original_name=report['original_static_build_report']
            original=data(original_name)
            assert report['original_static_build_report_sha256']==sha(files[original_name])
            assert original['mode']=='cpu-static-only' and original['executed'] is False
            expected=dict(original)
            expected.update(mode='cpu',original_static_build_report=original_name,
                            original_static_build_report_sha256=sha(files[original_name]),
                            original_retained_binary='assembly01-'+variant+'-cpu-bin',
                            reuse_inputs_verified=True,benchmark_plan_sha256=sha(files['frozen-plan.json']))
            assert report==expected
            log=original_name[:-5]+'.jsonl'
        else:
            assert report['executed'] is False
            log=name[:-5]+'.jsonl'
        assert report['build_log_sha256']==sha(files[log])
        messages=[json.loads(line) for line in files[log].splitlines() if line.startswith(b'{')]
        assert messages[-1]==dict(reason='build-finished',success=True)
        for target,field in [('pbrs','library_artifact'),('packed-wire-screen','binary_artifact')]:
            assert [item for item in messages if item.get('reason')=='compiler-artifact' and item['target']['name']==target]==[report[field]]
        inputs=report['constant_inputs']
        for key in ['harness_source_sha256','reference_source_sha256','harness_manifest_sha256','harness_lock_sha256']:
            stored='harness_reference_sha256' if mode=='cpu' and key=='reference_source_sha256' else key
            assert inputs[stored]==plan[key]
        for item in ['Cargo.toml','Cargo.lock']:
            assert inputs['runtime_manifest_sha256' if item=='Cargo.toml' else 'runtime_lock_sha256']==sha(files['tree/'+item])
        assert inputs['plan_sha256']==sha(files['assembly-plan.json' if mode=='cpu' else 'frozen-plan.json'])
    assert frozen['host_sha256']==sha(files['host.json'])
    assert build_set['cpu_reused_from_static_builds']
    assert build_set['actual_build_order']==['assembly01-baseline-cpu','assembly01-candidate-cpu','canonical01-baseline-alloc','canonical01-candidate-alloc']
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
    assert builds['baseline','cpu']['crate_metadata']['pbrs']=='5a2a8256ee43519a'
    separation=data('binary-separation-check.json')
    for variant in ['baseline','candidate']:
        for mode in ['cpu','alloc']:
            stem='canonical01-'+variant+'-'+mode
            proof=separation[stem]
            report=builds[variant,mode]
            assert proof['sha256']==report['retained_sha256']
            assert proof['build_report_sha256']==sha(files[stem+'.json'])
            assert proof['crate_metadata']==report['crate_metadata']
            assert proof['cpu_reused']==(mode=='cpu')
            assert proof['undefined_clock_symbols']==(['U clock_gettime'] if mode=='cpu' else [])
    earlier=data('correctness-assembly-freeze.json')
    assert earlier['source_sha256']==plan['candidate_source_sha256']
    for name,digest in earlier['files'].items():
        if name.endswith('-bin'):
            variant='candidate' if 'candidate' in name else 'baseline'
            assert digest==builds[variant,'cpu']['retained_sha256']
        else:
            assert sha(files[name])==digest,name
    if checkout is not None:
        selected=builds['baseline','cpu']['source_before']
        altered={'Cargo.toml','Cargo.lock','src/packed.rs','src/packed/wire_tests.rs','src/packed/fused_tests.rs','src/packed/cache_first_tests.rs'}
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

    candidate_source=builds['candidate','cpu']['source_before']
    baseline_source=builds['baseline','cpu']['source_before']
    kinds={'fused':8,'cache-first':4,'custom-empty':1,'recovery':2}
    for name in ['candidate-unit','candidate-clippy']+[f'miri-{bits}-{kind}' for bits in ['64','32'] for kind in kinds]:
        receipt=data(name+'.json')
        assert receipt['exit_code']==0 and receipt['guard'] is None
        assert receipt['source_before']==receipt['source_after']==candidate_source
        assert receipt['log_sha256']==sha(files[name+'.log'])
        assert receipt['minimum_free_bytes']>1024**3 and receipt['maximum_target_bytes']<250*1024**2
        if name.startswith('miri-'):
            assert '-Zmiri-strict-provenance' in receipt['miri_flags']
            target='i686-unknown-linux-gnu' if '-32-' in name else 'x86_64-unknown-linux-gnu'
            assert receipt['command'][receipt['command'].index('--target')+1]==target
            kind=name.split('-',2)[2]
            assert (str(kinds[kind])+' passed; 0 failed').encode() in files[name+'.log']
    assert b'122 passed; 0 failed' in files['candidate-unit.log']
    assert b'Finished' in files['candidate-clippy.log'] and b'error:' not in files['candidate-clippy.log']
    shared=data('baseline-shared-goldens.json')
    assert shared['source_before']==shared['source_after']==baseline_source
    assert shared['exit_code']==0 and shared['guard'] is None
    assert b'10 passed; 0 failed' in files['baseline-shared-goldens.log']
    assert plan['candidate_source_sha256'].encode() in files['independent-review.json']
    # Inspect the actual full-harness assembly archive, not a tiny proxy.
    gate=data('full-assembly/symbols.json')
    comparison=data('full-assembly/comparison.json')
    routing=data('full-assembly/routing-findings.json')
    assert routing['source_sha256']==plan['candidate_source_sha256']
    assert routing['baseline_source_sha256']==plan['baseline_source_sha256']
    def normalized(text,start,end):
        rows=[]
        for line in text.decode().splitlines():
            match=re.match(r'\s*([0-9a-f]+):\s+(.*)',line)
            if not match: continue
            address=int(match[1],16);op=match[2].split(' #',1)[0]
            assert start<=address<end
            op=re.sub(r'0x[0-9a-f]+\(%rip\)','RIP_RELOCATION(%rip)',op)
            branch=re.match(r'(j\w*|call)\s+([0-9a-f]+)\s+<(.+)>$',op)
            if branch:
                target=int(branch[2],16)
                label='relative+'+hex(target-start) if start<=target<end else branch[3]
                op=branch[1]+' '+re.sub(r'\.llvm\.[0-9]+','',label)
            rows.append([address-start,op])
        return rows
    def selected(variant,category):
        return [item for item in gate['variants'][variant]['symbols'] if item['category']==category]
    for variant,record in gate['variants'].items():
        assert record['binary_sha256']==builds[variant,'cpu']['retained_sha256']
        assert record['source_sha256']==plan[variant+'_source_sha256']
        for symbol in record['symbols']:
            assembly=files['full-assembly/'+symbol['assembly_file']]
            assert symbol['assembly_sha256']==sha(assembly)
            assert '--start-address='+str(symbol['address']) in symbol['command']
            assert '--stop-address='+str(symbol['address']+symbol['bytes']) in symbol['command']
            assert normalized(assembly,symbol['address'],symbol['address']+symbol['bytes'])==symbol['normalized_instructions']
    for category,length in [('scalar_encode',332),('scalar_decode',297),('bulk_decode',1072),('make_pool',856),('measured_loop',1047)]:
        a,b=selected('baseline',category)[0],selected('candidate',category)[0]
        assert a['bytes']==b['bytes']==length
        assert a['normalized_instructions']==b['normalized_instructions']
    assert [selected(v,'field_i32')[0]['bytes'] for v in ['baseline','candidate']]==routing['field_bytes']==[980,867]
    cold=selected('candidate','cold_wire')
    assert len(cold)==1 and cold[0]['bytes']==routing['cold_wire_helper_bytes']==130
    assert hex(cold[0]['address'])==routing['cold_wire_helper_address']=='0xf5350'
    for category,predicate,length in [('typed_encode',lambda x:x['bytes']==149,149),
                                       ('getter_decode',lambda x:'::as_slice::' in x['name'],211)]:
        a=[x for x in selected('baseline','once_callback') if predicate(x)]
        b=[x for x in selected('candidate','once_callback') if predicate(x)]
        assert a and b and all(x['bytes']==length for x in a+b)
        assert all(x['normalized_instructions']==y['normalized_instructions'] for x in a for y in b)
        assert all(x['normalized_equal'] for x in comparison['callbacks'][category])
    wire_calls=[x for x in comparison['relocations']['candidate'] if x['function_category']=='field_i32'
                and x.get('relocation') and any('::encode_wire_cache' in name for name in x['relocation']['target_symbols'])]
    assert [x['instruction_address'] for x in wire_calls]==['0x5063d','0x5068c']
    assert all(x['slot']=='0x17b780' and x['relocation']['target_address']=='0xf5350' for x in wire_calls)
    field=selected('candidate','field_i32')[0]
    ordinary=[x for x in field['normalized_instructions'] if x[1].startswith('call ') and 'OnceLock<alloc::vec::Vec<u8>>' in x[1] and '::packed_bytes::{closure#0}' in x[1]]
    assert len(ordinary)==2
    assert routing['normal_call_not_tail_jump'] and routing['measured_loop_normalized_instructions_equal']
    assert data('decision.json')['decision']=='REJECT_UNSHIPPED_READINESS_ROUTING_OPTIMIZATION'
    row=next(row for row in data('cpu-summary.json')['rows'] if row['case']=='i32.canonical.4096.encode_get')
    assert all(sample>1 for sample in row['peers']['pbrs']['paired_candidate_over_baseline_cpu']['samples'])
    return dict(passed=True,archived_files=len(files),cpu_processes=432,allocation_processes=48,
                baseline_shared_tests=10,minimal_runtime_tests=122,targeted_miri_executions=30,
                decision='REJECT_UNSHIPPED',
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
