#!/usr/bin/env python3
"""Verify retained text records without executing archived source or binaries."""
from pathlib import Path
import argparse
import difflib
import re
import gzip
import hashlib
import json
import math
import random
import statistics
import subprocess

BASE = '10d94697dc89e3aa89c843720f7150f1186f417a'
BASE_LAZY = 'b41f4e1545279ce1b5515bf2e54197c57f0c724209e4388a91db882e735ff85d'
CANDIDATE_LAZY = '2aa61521bf70e07676dd53848a254b8fc9f80ed6a73457fe517a48ec39aad885'
HERE = Path(__file__).resolve().parent

def sha(value):
    return hashlib.sha256(value).hexdigest()

def load(folder):
    manifest = json.loads((folder/'manifest.json').read_text())
    assert sha((folder/'README.md').read_bytes()) == manifest['readme_sha256']
    assert sha((folder/'check.py').read_bytes()) == manifest['checker_sha256']
    compressed = (folder/'raw.json.gz').read_bytes()
    assert sha(compressed) == manifest['compressed_sha256']
    with gzip.open(folder/'raw.json.gz', 'rb') as stream:
        raw = stream.read(12*1024**2+1)
    assert len(raw) <= 12*1024**2 and len(raw) == manifest['uncompressed_bytes']
    assert sha(raw) == manifest['uncompressed_sha256']
    document = json.loads(raw)
    assert document['schema'] == 'pbrs.name-core-evidence.v1'
    files = {name:value.encode() for name,value in document['files'].items()}
    assert files.keys() == manifest['files'].keys()
    for name,value in files.items():
        assert not Path(name).is_absolute() and '..' not in Path(name).parts
        assert not name.endswith('-bin') and '/target/' not in name
        assert len(value) == manifest['files'][name]['bytes']
        assert sha(value) == manifest['files'][name]['sha256']
    return manifest, files

def checkout_root(folder):
    for cwd in [folder, Path.cwd()]:
        result = subprocess.run(['git','rev-parse','--show-toplevel'], cwd=cwd,
                                capture_output=True, text=True)
        if result.returncode == 0:
            return Path(result.stdout.strip())
    raise AssertionError('Run in a checkout containing the named Git base')

def verify(files, manifest, checkout=None):
    def data(name):
        return json.loads(files[name])
    def lines(name):
        return [json.loads(line) for line in files[name].splitlines()]
    plan = data('frozen-plan.json')
    source = data('source-freeze.json')
    frozen = data('controller-freeze.json')
    build_set = data('canonical-build-set.json')
    assert manifest['source_base'] == plan['source_base'] == BASE
    assert manifest['experiment'] == 'medium-core-utf8'
    assert manifest['decision'] == 'REJECT_DO_NOT_ADVANCE'
    assert plan['baseline_source_sha256'] == BASE_LAZY == sha(files['baseline-lazy.rs'])
    assert plan['candidate_source_sha256'] == CANDIDATE_LAZY == sha(files['candidate-lazy.rs']), 'candidate source integrity'
    before = b'''        if s.len() <= Self::MEDIUM_OWNED_COPY && copy_exact {
            require_utf8(s)?;'''
    after = b'''        if s.len() <= Self::MEDIUM_OWNED_COPY && copy_exact {
            core::str::from_utf8(s).map_err(|_| ParseError::new("invalid utf-8"))?;'''
    assert files['baseline-lazy.rs'].count(before) == 1
    assert files['baseline-lazy.rs'].replace(before, after) == files['candidate-lazy.rs']
    assert ''.join(difflib.unified_diff(files['baseline-lazy.rs'].decode().splitlines(True),files['candidate-lazy.rs'].decode().splitlines(True),fromfile='a/src/lazy.rs',tofile='b/src/lazy.rs')).encode()==files['validator.patch']
    assert len(source['files']) == 24
    for name,digest in source['files'].items():
        assert sha(files[name]) == digest, name
    result_freeze = data('result-freeze.json')
    assert result_freeze['decision'] == 'REJECT_DO_NOT_ADVANCE'
    for name,digest in result_freeze['files'].items():
        assert sha(files[name]) == digest, name
    assert plan['harness_source_sha256'] == sha(files['harness/src/main.rs'])
    assert frozen['controller_source_sha256'] == sha(files['controller.py'])
    assert frozen['plan_sha256'] == build_set['plan_sha256'] == sha(files['frozen-plan.json'])
    assert frozen['build_set_sha256'] == sha(files['canonical-build-set.json'])
    assert frozen['source_freeze_sha256'] == sha(files['source-freeze.json'])
    assert plan['rows'] == data('proposed-plan.json')['rows']
    assert plan['cpu_schedule'] == data('proposed-plan.json')['cpu_schedule']
    assert plan['decision_rule'] == data('proposed-plan.json')['decision_rule']
    lengths = [23,24,63,64,65,80,128,255,256,257,4096]
    expected_cases = [f'name.ascii.{n}.borrowed' for n in lengths]
    expected_cases += [f'name.{p}.80.borrowed' for p in ['utf8_2','utf8_3','utf8_4']]
    expected_cases += [f'name.{p}.80.borrowed' for p in ['invalid_early','invalid_late','invalid_cross64','partial_4']]
    expected_cases += ['name.ascii.80.shared','pad.ascii.80.borrowed','pad.ascii.80.shared']
    assert [row['case'] for row in plan['rows']] == expected_cases
    assert plan['pairs'] == 9 and plan['schedule_seed'] == 20261106
    assert plan['allocation_iterations'] == 128
    assert plan['allocation_processes'] == 84
    cells = []
    for row in plan['rows']:
        assert row['peers'] == ['pbrs','prost']
        assert row['iterations'] == (8192 if row['length']==4096 else 32768 if row['length']>256 else 100000)
        for peer in row['peers']:
            cells.append((row,peer,len(cells)))
    assert len(cells) == plan['peer_cells'] == 42
    rng = random.Random(20261106)
    schedule = []
    for pair in range(9):
        ordered = list(cells)
        rng.shuffle(ordered)
        for row,peer,index in ordered:
            variants = ['baseline','candidate'] if (pair+index)%2==0 else ['candidate','baseline']
            schedule.extend(dict(pair=pair,case=row['case'],peer=peer,variant=variant,
                                 iterations=row['iterations']) for variant in variants)
    assert schedule == plan['cpu_schedule'] and len(schedule) == 756

    builds = {}
    for name in build_set['reports']:
        record = data(name)
        variant,mode = record['variant'],record['mode']
        builds[variant,mode] = record
        assert frozen['build_reports'][name] == sha(files[name])
        assert record['exit_code'] == 0 and record['guard'] is None and record['executed'] is False
        assert record['source_before'] == record['source_after']
        overlay = 'baseline-lazy.rs' if variant=='baseline' else 'candidate-lazy.rs'
        assert record['source_before']['src/lazy.rs'] == sha(files[overlay])
        assert record['source_before']['tests/core_utf8.rs'] == sha(files['tree/tests/core_utf8.rs'])
        assert record['executable_sha256'] == record['retained_sha256']
        assert record['target_bytes'] < 350*1024**2 and record['free_bytes'] > 1024**3
        log = name[:-5]+'.jsonl'
        assert record['build_log_sha256'] == sha(files[log])
        messages = [json.loads(line) for line in files[log].splitlines() if line.startswith(b'{')]
        assert messages[-1] == dict(reason='build-finished',success=True)
        for target,field in [('pbrs','library_artifact'),('name-core-utf8-screen','binary_artifact')]:
            assert [x for x in messages if x.get('reason')=='compiler-artifact' and x['target']['name']==target] == [record[field]]
        assert record['library_artifact']['features'] == ['json','reflect','std','text']
        assert record['binary_artifact']['features'] == (['allocations'] if mode=='alloc' else [])
        inputs = record['constant_inputs']
        for name,key in [('harness/src/main.rs','harness_source_sha256'),('harness/Cargo.toml','harness_manifest_sha256'),('harness/Cargo.lock','harness_lock_sha256'),('tree/Cargo.toml','runtime_manifest_sha256'),('tree/Cargo.lock','runtime_lock_sha256'),('source-freeze.json','source_freeze_sha256')]:
            assert inputs[key] == sha(files[name])
        assert inputs['plan_sha256'] == sha(files['frozen-plan.json'])
    assert set(builds) == {(variant,mode) for variant in ['baseline','candidate'] for mode in ['cpu','alloc']}
    for mode in ['cpu','alloc']:
        a,b = builds['baseline',mode],builds['candidate',mode]
        assert a['constant_inputs'] == b['constant_inputs']
        assert a['crate_metadata'] == b['crate_metadata'] and a['rustc_commands'] == b['rustc_commands']
        assert a['canonical_source_root'] == b['canonical_source_root']
        assert a['canonical_harness_root'] == b['canonical_harness_root']
        assert {name for name in a['source_before'] if a['source_before'][name]!=b['source_before'][name]} == {'src/lazy.rs'}
        for field in ['library_artifact','binary_artifact']:
            for key in ['package_id','manifest_path','profile','features','filenames']:
                assert a[field][key] == b[field][key]
    if checkout is not None:
        tracked = builds['baseline','cpu']['source_before']
        changed = {'Cargo.toml','Cargo.lock','src/lazy.rs','tests/core_utf8.rs'}
        names = sorted(set(tracked)-changed)
        request = ''.join(f'{BASE}:{name}\n' for name in names).encode()
        packed = subprocess.check_output(['git','cat-file','--batch'], cwd=checkout, input=request)
        offset = 0
        for name in names:
            end = packed.index(b'\n',offset)
            header = packed[offset:end].split()
            assert len(header)==3 and header[1]==b'blob',name
            size = int(header[2]);offset=end+1
            assert tracked[name] == sha(packed[offset:offset+size]),name
            offset += size+1
        assert offset == len(packed)
        assert subprocess.check_output(['git','show',BASE+':src/lazy.rs'],cwd=checkout) == files['baseline-lazy.rs']
        assert subprocess.check_output(['git','show',BASE+':proto/codec_cases.proto'],cwd=checkout) == files['tree/proto/codec_cases.proto']

    all_records = {}
    for mode in ['cpu','alloc']:
        expected = schedule if mode=='cpu' else [dict(case=row['case'],peer=peer,variant=variant,iterations=128) for row,peer,_ in cells for variant in ['baseline','candidate']]
        records = lines(mode+'-processes.jsonl')
        summary = data(mode+'-summary.json')
        assert len(records)==len(expected)==summary['planned']==summary['observed']==summary['passed'], 'complete process grid'
        assert summary['raw_sha256']==sha(files[mode+'-processes.jsonl'])
        assert summary['plan_sha256']==sha(files['frozen-plan.json'])
        for i,(record,step) in enumerate(zip(records,expected,strict=True)):
            assert record['iterations'] > 0, 'positive iteration count'
            assert all(record[k]==v for k,v in step.items()), 'fixed schedule entry'
            assert record['sequence']==i and record['mode']==mode
            assert record['status']=='pass' and record['exit_code']==0 and not record['stderr']
            assert record['executable_sha256']==builds[record['variant'],mode]['retained_sha256'], 'process executable binding'
            assert record['controller_freeze_sha256']==sha(files['controller-freeze.json'])
            assert record['plan_sha256']==sha(files['frozen-plan.json'])
            assert record['command'][1:]==[record['case'],record['peer'],str(record['iterations'])]
            output=record['output'];assert json.loads(record['stdout'])==output
            assert output['schema']=='pbrs.name-core-screen.v1'
            assert output['case']==record['case'] and output['peer']==record['peer']
            assert output['iterations']==record['iterations']>0
            assert output['full_contents_checked'] and output['warmup_iterations']==256 and output['dispatch_setup_bytes']==4096
            assert output['allocation_instrumentation']==(mode=='alloc')
            row=next(row for row in plan['rows'] if row['case']==record['case'])
            valid=row['pattern'] in ['ascii','utf8_2','utf8_3','utf8_4']
            assert output['valid']==valid and output['shared']==(row['input']=='shared')
            n=row['length'];length_bytes=1 if n<128 else 2
            assert output['payload_bytes']==n
            assert output['input_bytes']==n+1+length_bytes+(131 if row['shape']=='pad' else 0)
            assert output['checksum']==record['iterations']*(n if valid else 1)
            assert output['layout']==dict(cache_size=8,field_offsets_not_observed=True,lazy_str_size=40,name_align=8,name_size=56,proto_string_size=32,unknown_size=8)
            for key in (['cpu_ns','wall_ns'] if mode=='alloc' else ['allocation_calls','requested_bytes']):
                assert output[key] is None, 'measurement counter separation'
            for key in (['allocation_calls','requested_bytes'] if mode=='alloc' else ['cpu_ns','wall_ns']):
                assert type(output[key]) is int and math.isfinite(output[key])
                assert output[key]>=0 if mode=='alloc' else output[key]>0
        assert [row['case'] for row in summary['rows']]==expected_cases, 'complete row grid'
        for row in summary['rows']:
            assert set(row['peers'])=={'pbrs','prost'}
            for peer,value in row['peers'].items():
                groups={variant:[x for x in records if x['case']==row['case'] and x['peer']==peer and x['variant']==variant] for variant in ['baseline','candidate']}
                count=9 if mode=='cpu' else 1
                assert all(len(group)==count for group in groups.values())
                assert value['expected_per_variant']==count
                assert value['observed_per_variant']=={variant:count for variant in groups}
                assert len({x['output']['checksum'] for group in groups.values() for x in group})==1
                if mode=='cpu':
                    for metric in ['cpu_ns','wall_ns']:
                        for variant,group in groups.items():
                            samples=[x['output'][metric]/x['iterations'] for x in group]
                            assert value[variant][metric]==dict(median=statistics.median(samples),minimum=min(samples),maximum=max(samples),samples=samples)
                        ratios=[]
                        for pair in range(9):
                            a=next(x for x in groups['baseline'] if x['pair']==pair)
                            b=next(x for x in groups['candidate'] if x['pair']==pair)
                            ratios.append(b['output'][metric]/a['output'][metric])
                        key='paired_candidate_over_baseline_'+('cpu' if metric=='cpu_ns' else 'wall')
                        assert value[key]==dict(median=statistics.median(ratios),minimum=min(ratios),maximum=max(ratios),samples=ratios), 'paired ratio recomputation'
                else:
                    assert value['baseline']==value['candidate']
                    for variant,group in groups.items():
                        assert value[variant]=={key:group[0]['output'][key]/128 for key in ['allocation_calls','requested_bytes']}
        all_records[mode]=records

    patterns=['invalid_early','invalid_late','invalid_cross64','partial_2','partial_3','partial_4','overlong_2','surrogate_3','out_of_range_4']
    helpers=[]
    for variant in ['baseline','candidate']:
        value=data('helper-'+variant+'.json');process=data('helper-'+variant+'-process.json')
        assert process['exit_code']==0 and not process['stderr']
        assert process['executable_sha256']==builds[variant,'alloc']['retained_sha256']
        assert json.loads(process['stdout'])==value
        assert value['schema']=='pbrs.name-helper-controls.v1' and value['cpu_ns'] is value['wall_ns'] is None
        assert len(value['rows'])==891 and len(value['outer_errors'])==18
        expected=[(n,p,s,b) for n in lengths for p in patterns for s in [0,128,2048] for b in range(3)]
        for row,(n,p,s,b) in zip(value['rows'],expected,strict=True):
            assert (row['length'],row['pattern'],row['suffix'],row['backing'])==(n,p,s,b)
            frame=n+2+s;exact=n+8>=frame or frame>1024 and n*4<frame
            assert row['copy_exact']==exact and row['slot_preserved'] and row['rejected']
            assert row['allocation_calls']==int(n>256 and exact), 'helper allocation formula'
            assert row['requested_bytes']>0 if row['allocation_calls'] else row['requested_bytes']==0
        expected_outer=[f'name.{p}.80.{b}' for p in patterns for b in ['borrowed','shared']]
        assert [row['case'] for row in value['outer_errors']]==expected_outer
        assert all(row['rejected'] and row['allocation_calls']==row['requested_bytes']==0 for row in value['outer_errors'])
        helpers.append(value)
    assert helpers[0]==helpers[1]
    assert data('helper-summary.json')==dict(passed=True,helper_cases_per_variant=891,outer_errors_per_variant=18,baseline_candidate_exact_counts_and_layout_equal=True,clock_free=True)

    native=[('baseline-unit','baseline','receipt-sources/focused-three.rs',113),('baseline-focused-final-with-miri','baseline','receipt-sources/focused-four-before-format.rs',4),('baseline-harness-native-final','baseline','receipt-sources/focused-three.rs',4),('candidate-unit','candidate','focused-tests.rs',113),('candidate-focused-formatted','candidate','focused-tests.rs',4),('candidate-harness-native','candidate','focused-tests.rs',4),('candidate-clippy','candidate','focused-tests.rs',None),('candidate-harness-clippy','candidate','focused-tests.rs',None),('candidate-miri-64','candidate','focused-tests.rs',1),('candidate-miri-32','candidate','focused-tests.rs',1)]
    for name,variant,test_source,passed in native:
        receipt=data(name+'.json');expected=dict(builds[variant,'cpu']['source_before'])
        expected['tests/core_utf8.rs']=sha(files[test_source])
        assert receipt['source_before']==receipt['source_after']==expected
        assert receipt['exit_code']==0 and receipt['guard'] is None
        assert receipt['log_sha256']==sha(files[name+'.log'])
        assert receipt['minimum_free_bytes']>1024**3 and receipt['maximum_team_artifact_bytes']<350*1024**2
        if passed is not None:
            assert (str(passed)+' passed; 0 failed').encode() in files[name+'.log']
        else:
            assert b'error:' not in files[name+'.log']
        if name.startswith('candidate-miri'):
            assert '-Zmiri-strict-provenance' in receipt['miri_flags']
            target='i686-unknown-linux-gnu' if name.endswith('32') else 'x86_64-unknown-linux-gnu'
            assert receipt['command'][receipt['command'].index('--target')+1]==target
    toolchain=data('toolchain-features.json')
    packages={p['name']:p for p in toolchain['packages']}
    assert packages['prost']['version']==packages['prost-derive']['version']=='0.14.4'
    assert packages['pbrs']['features']==['json','reflect','std','text']
    assert packages['simdutf8']['features']==['std']
    assert toolchain['release']==dict(opt_level=3,lto='thin',codegen_units=1,debug=0,incremental=False)
    assert toolchain['RUSTFLAGS'] is toolchain['CARGO_ENCODED_RUSTFLAGS'] is None
    for report in builds.values():
        assert report['constant_inputs']['rustc']==toolchain['rustc']
        assert report['constant_inputs']['cargo']==toolchain['cargo']
    separation=data('binary-separation.json')
    quiet=data('quiet-go.json')
    assert quiet['main']==BASE and quiet['main_clean']
    for variant in ['baseline','candidate']:
        for mode in ['cpu','alloc']:
            stem='canonical01-'+variant+'-'+mode
            entry=separation[stem]
            assert entry['sha256']==builds[variant,mode]['retained_sha256']
            assert entry['build_report_sha256']==sha(files[stem+'.json'])
            assert quiet['artifacts_verified'][stem+'-bin']==entry['sha256']
            assert entry['undefined_clock_symbols']==(['U clock_gettime'] if mode=='cpu' else [])
    verify_assembly(files,source,builds)
    provenance=data('generated-name-provenance.json')
    assert provenance['generated_source_sha256']==sha(files['generated/codec_cases.rs'])
    assert provenance['golden_regeneration_byte_identical'] and provenance['default_Config']
    assert provenance['owned_message_fields_override'] is False
    assert provenance['actual_cached_size_type']=='::pbrs::rt::CachedSize'
    assert provenance['Name_declaration'].encode() in files['generated/codec_cases.rs']

    rule=plan['decision_rule'];decision=data('decision.json')
    assert rule==dict(primary='name.ascii.80.borrowed/pbrs',primary_minimum_improvement=0.02,block_any_pbrs_median_above=1.02,block_all9_slower_pbrs_median_above=1.01,unchanged_prost_shift_above_0_02='ATTRIBUTION_INCONCLUSIVE',different_work_or_elision='ATTRIBUTION_INCONCLUSIVE',passing_means='ADVANCE_ONLY_TO_MATCHED_FULL_DEVLOOP_CONFIRMATION',all_ranges_and_losing_rows_retained=True,no_reference_adjustment=True)
    items=[]
    for row in data('cpu-summary.json')['rows']:
        for peer,value in row['peers'].items():
            q=value['paired_candidate_over_baseline_cpu']
            items.append(dict(case=row['case'],peer=peer,median=q['median'],minimum=q['minimum'],maximum=q['maximum'],slower_pairs=sum(x>1.0 for x in q['samples']),baseline_ns=value['baseline']['cpu_ns']['median'],candidate_ns=value['candidate']['cpu_ns']['median']))
    primary=next(x for x in items if x['case']=='name.ascii.80.borrowed' and x['peer']=='pbrs')
    regressions=[x for x in items if x['peer']=='pbrs' and x['median']>1.02]
    repeated=[x for x in items if x['peer']=='pbrs' and x['median']>1.01 and x['slower_pairs']==9]
    shifts=[x for x in items if x['peer']=='prost' and abs(x['median']-1)>0.02]
    assert decision['primary']==primary and decision['primary_improvement_requirement_met']==(primary['median']<=.98)
    assert decision['regressions_above_2pct']==regressions
    assert decision['all9_slower_above_1pct']==repeated, 'all-nine loss recomputation'
    assert decision['unchanged_prost_shifts_above_2pct']==shifts, 'reference-shift recomputation'
    assert len(regressions)==9 and len(repeated)==4 and len(shifts)==5
    assert decision['decision']=='REJECT_DO_NOT_ADVANCE' and decision['attribution']=='INCONCLUSIVE_BY_UNCHANGED_REFERENCE_GATE'
    assert decision['raw_CPU_sha256']==sha(files['cpu-processes.jsonl'])
    assert decision['raw_allocation_sha256']==sha(files['alloc-processes.jsonl'])
    assert decision['frozen_plan_sha256']==sha(files['frozen-plan.json'])
    assert decision['source_freeze_sha256']==sha(files['source-freeze.json'])
    return dict(passed=True,archived_files=len(files),cpu_processes=756,allocation_processes=84,helper_cases_per_variant=891,outer_error_cases_per_variant=18,native_minimal_per_variant=113,focused_tests_per_variant=4,generated_harness_tests_per_variant=4,targeted_miri_executions=2,decision='REJECT_DO_NOT_ADVANCE',attribution='INCONCLUSIVE_BY_UNCHANGED_REFERENCE_GATE',scope='Source/receipt/accounting verification; no executable replay, full-devloop win or ranking qualification')

REGIONS = ["pbrs_timed_loop", "prost_timed_loop", "prost_untimed_loop", "name_parse_closure", "name_merge_inner", "lazy_from_parse_span", "name_drop", "prost_name_decode_slice", "prost_string_merge_slice", "prost_string_reserve", "core_utf8_validator", "simd_fallback"]
EQUAL = {"pbrs_timed_loop", "prost_timed_loop", "prost_untimed_loop", "prost_name_decode_slice", "prost_string_merge_slice", "prost_string_reserve", "core_utf8_validator", "name_drop"}

def blocks(text):
    found = {}
    name = None
    body = []
    for line in text.splitlines(keepends=True):
        if line.startswith("# BEGIN "):
            assert name is None, "nested region"
            name = line.removeprefix("# BEGIN ").strip()
            assert name in REGIONS and name not in found, "unknown/duplicate region"
            body = []
        elif line.startswith("# END "):
            assert name is not None and line.removeprefix("# END ").strip() == name, "bad region end"
            found[name] = "".join(body)
            name = None
        else:
            assert name is not None, "text outside a region"
            body.append(line)
    assert name is None and list(found) == REGIONS, "missing/reordered region"
    return found

def normalize(text, relocations):
    result = []
    for line in text.splitlines():
        columns = line.split("\t", 2)
        if len(columns) != 3 or not re.fullmatch(r"\s*[0-9a-f]+:", columns[0]):
            continue
        instruction = columns[2].strip()
        if not instruction:
            continue
        if "# " in instruction:
            code, comment = instruction.split("# ", 1)
            target = re.match(r"([0-9a-f]+)\s+<(.+)>", comment.strip())
            if target:
                address = hex(int(target[1], 16))
                label = relocations.get(address, {}).get("resolved_label", target[2])
                code = re.sub(r"-?0x[0-9a-f]+\(%rip\)", "RIP_TARGET", code)
                instruction = code.strip() + " # " + label
        instruction = re.sub(r"\b[0-9a-f]+ <(.+)>", r"<\1>", instruction)
        result.append(instruction)
    assert result, "empty instruction stream"
    return result

def verify_assembly(files,source,builds):
    review=json.loads(files['assembly-evidence/review.json'])
    assert review['schema']=='pbrs.name-core-screen.assembly-evidence.v1'
    assert review['performance_decision'].startswith('REJECT;')
    assert review['source_binding']['source_freeze_sha256']==sha(files['source-freeze.json'])
    assert review['source_binding']['frozen_files']==source['files']
    assert review['source_binding']['runtime_build_source_inputs']==166
    assert review['source_binding']['runtime_build_source_differences']==['src/lazy.rs']
    assert [r['name'] for r in review['regions']]==REGIONS
    assert [r['region'] for r in review['comparisons']]==REGIONS
    selections={}
    for variant in ['baseline','candidate']:
        entry=review['variants'][variant];report=builds[variant,'cpu']
        assert entry['executable_sha256']==report['retained_sha256']
        assert entry['build_record_sha256']==sha(files[entry['build_record_file']])
        assert entry['crate_metadata']==report['crate_metadata']
        assert entry['recorded_rustc_commands']==report['rustc_commands']
        raw=files['assembly-evidence/'+entry['raw_file']]
        assert sha(raw)==entry['raw_sha256']
        selections[variant]=blocks(raw.decode())
    for expected in review['comparisons']:
        region=expected['region']
        streams={v:normalize(selections[v][region],review['variants'][v]['selected_relocations']) for v in ['baseline','candidate']}
        assert {v:len(x) for v,x in streams.items()}==expected['instruction_counts']
        assert {v:sha('\n'.join(x).encode()) for v,x in streams.items()}==expected['normalized_sha256']
        assert (streams['baseline']==streams['candidate'])==expected['same_normalized_stream']
        if region in EQUAL:
            assert streams['baseline']==streams['candidate']
        assert list(difflib.unified_diff(streams['baseline'],streams['candidate'],fromfile='baseline',tofile='candidate',lineterm=''))==expected['normalized_diff']
    assert 'simdutf8::implementation::x86::validate_utf8_basic::FN' in review['variants']['baseline']['selected_relocations']['0x784e0']['resolved_label']
    assert review['variants']['candidate']['selected_relocations']['0x781c8']['resolved_label']=='core::str::converts::from_utf8'


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--packet',type=Path,default=HERE)
    args=parser.parse_args()
    manifest,files=load(args.packet)
    answer=verify(files,manifest,checkout_root(args.packet))
    negatives=[]
    def encode(value):
        return (json.dumps(value,indent=2)+'\n').encode()
    def mutate(name,change):
        altered=dict(files);value=json.loads(altered[name]);change(value)
        altered[name]=encode(value);return altered
    def reseal(altered):
        # Test fixtures only: avoid stopping at a changed content digest before
        # reaching the named accounting invariant. Production verification above
        # has no bypass; none of these mutated bytes is written to the packet.
        receipt=json.loads(altered['result-freeze.json'])
        for name in receipt['files']:
            receipt['files'][name]=sha(altered[name])
        altered['result-freeze.json']=encode(receipt)
        return altered
    negatives.append(('wrong paired median','paired ratio recomputation',mutate('cpu-summary.json',lambda x:x['rows'][0]['peers']['pbrs']['paired_candidate_over_baseline_cpu'].__setitem__('median',999))))
    negatives.append(('omitted losing row','complete row grid',mutate('cpu-summary.json',lambda x:x['rows'].pop(6))))
    negatives.append(('weakened all-nine result','all-nine loss recomputation',mutate('decision.json',lambda x:x.__setitem__('all9_slower_above_1pct',[]))))
    negatives.append(('hidden reference shift','reference-shift recomputation',mutate('decision.json',lambda x:x.__setitem__('unchanged_prost_shifts_above_2pct',[]))))
    altered=mutate('helper-candidate.json',lambda x:x['rows'][0].__setitem__('allocation_calls',1))
    process=json.loads(altered['helper-candidate-process.json'])
    process['stdout']=altered['helper-candidate.json'].decode()
    altered['helper-candidate-process.json']=encode(process)
    negatives.append(('changed helper allocation','helper allocation formula',altered))
    for label,marker,mode,change in [('missing CPU capture','complete process grid','cpu',lambda x:x.pop()),('wrong binary binding','process executable binding','cpu',lambda x:x[0].__setitem__('executable_sha256','0'*64)),('wrong pair order','fixed schedule entry','cpu',lambda x:x[0].__setitem__('pair',8)),('clock in allocation capture','measurement counter separation','alloc',lambda x:x[0]['output'].__setitem__('cpu_ns',1)),('zero-work capture','positive iteration count','cpu',lambda x:x[0].__setitem__('iterations',0))]:
        altered=dict(files);records=[json.loads(line) for line in altered[mode+'-processes.jsonl'].splitlines()];change(records)
        for record in records:
            record['stdout']=json.dumps(record['output'])+'\n'
        altered[mode+'-processes.jsonl']=b''.join((json.dumps(x)+'\n').encode() for x in records)
        summary=json.loads(altered[mode+'-summary.json']);summary['raw_sha256']=sha(altered[mode+'-processes.jsonl'])
        altered[mode+'-summary.json']=encode(summary)
        decision=json.loads(altered['decision.json'])
        decision['raw_CPU_sha256' if mode=='cpu' else 'raw_allocation_sha256']=summary['raw_sha256']
        altered['decision.json']=encode(decision)
        negatives.append((label,marker,altered))
    for label,marker,altered in negatives:
        reseal(altered)
        try:
            verify(altered,manifest)
        except AssertionError as error:
            assert str(error)==marker, (label,'failed at wrong invariant',str(error))
        else:
            raise AssertionError('corruption accepted: '+label)
    altered=dict(files);altered['candidate-lazy.rs']=altered['baseline-lazy.rs']
    try:
        verify(altered,manifest)
    except AssertionError as error:
        assert str(error)=='candidate source integrity'
    else:
        raise AssertionError('missing source change accepted')
    answer['semantic_corruptions_rejected']=len(negatives)
    answer['integrity_corruptions_rejected']=1
    answer['negative_controls_rejected']=len(negatives)+1
    print(json.dumps(answer))

if __name__=='__main__':
    main()
