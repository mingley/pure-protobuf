#!/usr/bin/env python3
"""Verify text receipts and accounting. No Git, workspace, binary or replay."""
from pathlib import Path
import copy
import gzip
import hashlib
import io
import json

HERE = Path(__file__).resolve().parent
BASE = '10d94697dc89e3aa89c843720f7150f1186f417a'
UNION = 'followon-union/'
PROFILE = dict(kind='native dev/test', opt_level=0, debug=0, incremental=False, all_features=True, offline=True, jobs=1)
CASES = [m + '.' + s for m in ['direct', 'balanced'] for s in ['unary', 'server_streaming', 'client_streaming', 'bidi']]


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encode(value):
    return (json.dumps(value, indent=2) + '\n').encode()


def unpack(compressed, manifest):
    assert sha(compressed) == manifest['compressed_sha256'] and len(compressed) == manifest['compressed_bytes']
    with gzip.GzipFile(fileobj=io.BytesIO(compressed), mode='rb') as stream:
        raw = stream.read(16 * 1024**2 + 1)
    assert len(raw) <= 16 * 1024**2 and len(raw) == manifest['uncompressed_bytes']
    assert sha(raw) == manifest['uncompressed_sha256']
    document = json.loads(raw)
    assert document['schema'] == 'pbrs.authority-storage-text.v1'
    return {name: value.encode() for name, value in document['files'].items()}


def load():
    manifest = json.loads((HERE / 'manifest.json').read_bytes())
    assert manifest['schema'] == 'pbrs.authority-storage-packet.v1'
    assert sha((HERE / 'README.md').read_bytes()) == manifest['readme_sha256']
    assert sha((HERE / 'check.py').read_bytes()) == manifest['checker_sha256']
    return unpack((HERE / 'raw.json.gz').read_bytes(), manifest), manifest


def verify(files, manifest):
    def data(name):
        return json.loads(files[name])
    def digest(name):
        return sha(files[name])
    def marked(raw, prefix):
        return [json.loads(line.split(prefix, 1)[1]) for line in raw.decode().splitlines() if prefix in line]
    assert manifest['source_base'] == BASE
    assert set(files) == set(manifest['files'])
    for name, raw in files.items():
        assert not Path(name).is_absolute() and '..' not in Path(name).parts
        assert not name.endswith('-bin') and '/target/' not in name
        assert manifest['files'][name] == dict(bytes=len(raw), sha256=sha(raw)), name
    historical = manifest['historical_executables']
    assert len(historical) == 6
    # Historical seals bind omitted executable SHA declarations, not bytes.
    for name, prefix in [('results-binding.json', ''), (UNION + 'results-binding.json', UNION), (UNION + 'controls-binding.json', '')]:
        for member, expected in data(name)['files'].items():
            target = prefix + member
            assert (digest(target) if target in files else historical[target]) == expected, target
    plans = {'': data('execution-plan.json'), UNION: data(UNION + 'plan.json')}
    for prefix, plan in plans.items():
        assert plan['profile'] == PROFILE and plan['source_base'] == BASE
        assert plan['cases'] == CASES
        for name, expected in plan['frozen_files'].items():
            assert digest(prefix + name) == expected, prefix + name
    assert plans['']['global_minimum_free_bytes'] == plans[UNION]['global_minimum_free_bytes'] == 1024**3
    assert plans[UNION]['baseline_or_v1_rerun'] is False and plans[UNION]['no_cpu_measurement']
    assert plans[UNION]['controls_binding_sha256'] == digest(UNION + 'controls-binding.json')
    assert plans[UNION]['union_source_sha256'] == digest(UNION + 'union-source.json')
    assert plans[UNION]['capture_helper_sha256'] == digest('build-and-collect.py')
    assert plans['']['prepared_baseline_inventory_sha256'] == digest('prepared-baseline-inputs.json')
    for name, expected in plans['']['common_manifests_locks'].items():
        assert digest('common/' + name) == expected
    baseline = data('prepared-baseline-inputs.json')
    assert len(baseline) == 499
    captured = data('candidate-source.json')
    union_source = data(UNION + 'union-source.json')
    assert captured['base'] == union_source['source_base'] == BASE
    assert captured['patch_sha256'] == digest('candidate-production.patch')
    assert len(captured['paths']) == 12 and sum(x['role'] == 'production' for x in captured['paths'].values()) == 11
    expected_sources = {'baseline': dict(baseline), 'candidate': dict(baseline)}
    for name, entry in captured['paths'].items():
        assert digest('baseline-production/' + name) == baseline[name] == entry['baseline_sha256']
        assert digest('candidate-production/' + name) == entry['candidate_sha256']
        expected_sources['candidate'][name] = entry['candidate_sha256']
    expected_sources['union'] = dict(expected_sources['candidate'])
    for name, expected in union_source['source_sha256'].items():
        assert digest(UNION + 'candidate-production/' + name) == expected
        expected_sources['union'][name] = expected
    assert expected_sources['union'] == union_source['expected_canonical_inventory']
    assert {n for n in baseline if expected_sources['candidate'][n] != expected_sources['union'][n]} == set(union_source['changes_vs_v1'])
    assert len(union_source['changes_vs_v1']) == 7
    client = 'pbrs-grpc/src/client.rs'
    assert baseline[client] == sha(files['common/' + client] + files['layout-tail.rs'])
    assert baseline['pbrs-grpc/tests/authority_cost_constructor.rs'] == digest('constructor-alloc.rs')
    prior_pins = data(UNION + 'primary-v3-inputs.json')['source_sha256']
    reviewed = data(UNION + 'union-inputs.json')['source_sha256']
    assert len(prior_pins) == 200 and len(reviewed) == 205
    for pins, variant in [(prior_pins, 'candidate'), (reviewed, 'union')]:
        for name, expected in pins.items():
            assert (digest('common/' + client) if name == client else expected_sources[variant][name]) == expected, name
    for name in ['Cargo.toml', 'Cargo.lock', 'build.rs', 'pbrs-grpc/Cargo.toml', 'pbrs-grpc/build.rs']:
        assert digest('common/' + name) == reviewed[name] == baseline[name]
    assert union_source['union_patch_sha256'] == digest(UNION + 'connection-route-union.patch')
    assert data(UNION + 'union-inputs.json')['patch_sha256'] == union_source['union_patch_sha256']
    assert data(UNION + 'primary-v3-inputs.json')['patch_sha256'] == digest('candidate-production.patch')
    summaries = {'': data('summary.json'), UNION: data(UNION + 'summary.json')}
    for summary in summaries.values():
        assert [row['case'] for row in summary['rows']] == CASES, 'summary case grid'
        assert summary['profile'] == PROFILE
    old_context = data('records/execution-context.json')
    new_context = data(UNION + 'records/execution-context.json')
    assert all(old_context[k] == new_context[k] for k in ['profile', 'toolchain', 'shared_target', 'explicit_environment'])
    assert not old_context['cpu_measurement'] and not new_context['cpu_measurement'] and new_context['reused_controls']

    def capture(prefix, stem):
        name = prefix + 'records/' + stem
        receipt = data(name + '-process.json')
        out, err = files[name + '.stdout'], files[name + '.stderr']
        assert receipt['stdout_sha256'] == sha(out) and receipt['stderr_sha256'] == sha(err)
        assert receipt['exit_code'] == 0 and receipt['resource_guard'] is None
        assert not receipt['termination_escalated_to_sigkill'] and receipt['wait_timeout_seconds'] == 15
        assert receipt['minimum_free_bytes'] > 1024**3
        return out, receipt

    all_rows, builds, layouts = {}, {}, {}
    for variant, prefix, plan_name in [('baseline', '', 'execution-plan.json'), ('candidate', '', 'execution-plan.json'), ('union', UNION, UNION + 'plan.json')]:
        report = data(prefix + 'records/' + variant + '-build.json')
        builds[variant] = report
        assert report['source_before'] == report['source_after'] == expected_sources[variant]
        before_name = prefix + 'records/' + (variant + '-' if not prefix else '') + 'source-before.json'
        after_name = before_name.replace('source-before.json', 'source-after.json')
        assert data(before_name) == data(after_name) == expected_sources[variant]
        assert report['plan_sha256'] == digest(plan_name)
        context = old_context if not prefix else new_context
        assert context['plan_sha256'] == digest(plan_name)
        raw, receipt = capture(prefix, variant + '-build')
        assert report['process'] == receipt
        messages = [json.loads(line) for line in raw.splitlines() if line.startswith(b'{')]
        assert messages[-1] == dict(reason='build-finished', success=True)
        for kind, target in [('layout', 'pbrs_grpc'), ('constructor', 'authority_cost_constructor')]:
            entry = report['selected'][kind]
            assert entry['artifact'] in messages and entry['artifact']['target']['name'] == target
            assert entry['executable_sha256'] == entry['retained_sha256'] == historical[prefix + entry['retained_file']]
            assert entry['artifact']['profile']['opt_level'] == '0' and entry['artifact']['profile']['debuginfo'] == 0
        commands_raw = files[prefix + 'records/' + variant + '-rustc.jsonl']
        assert sha(commands_raw) == report['compiler_records_sha256']
        commands = [json.loads(line)['argv'] for line in commands_raw.splitlines()]
        relevant = sorted(x for x in commands if '--crate-name' in x and x[x.index('--crate-name') + 1] in ['pbrs_grpc', 'authority_cost_constructor'])
        assert len(relevant) == 3 and not any(any('incremental=' in a for a in x) for x in relevant)
        assert relevant == data('records/matched-commands.json')['commands']
        if prefix:
            assert relevant == report['matched_commands']
        assert digest(prefix + 'records/' + variant + '-hello.rs') == report['hello_sha256']
        raw, _ = capture(prefix, variant + '-layout')
        layout = data(prefix + 'records/' + variant + '-layout.json')
        assert marked(raw, 'AUTHORITY_COST_LAYOUT ') == [layout]
        assert layout['pointer_bits'] == 64 and not layout['offsets_observed']
        assert [layout[k]['size'] for k in ['live_conn', 'optional_live_conn', 'channel_local', 'channel']] == ([168, 168, 184, 16] if variant == 'candidate' else [160, 160, 184, 16])
        assert all(layout[k]['align'] == 8 for k in ['live_conn', 'optional_live_conn', 'channel_local', 'channel'])
        layouts[variant] = layout
        all_rows[variant] = []
        for case in CASES:
            saved = data(prefix + 'records/' + variant + '-' + case + '.json')
            raw, receipt = capture(prefix, variant + '-' + case)
            assert saved['process'] == receipt
            assert saved['executable_sha256'] == report['selected']['constructor']['retained_sha256'], 'case executable binding'
            assert marked(raw, 'AUTHORITY_COST_ALLOC ') == saved['rows']
            rows = saved['rows']
            assert [(x['phase'], x['sample']) for x in rows] == [('cold', 0)] + [('warm', n) for n in range(8)], 'sample completeness'
            for row in rows:
                assert row['case'] == case and row['schema'] == 'pbrs.authority-constructor-cost.v1'
                assert row['cpu_ns'] is row['wall_ns'] is None, 'counter clock separation'
                assert row['current_thread_only'] and not row['future_polled'] and not row['yield_in_windows']
                for window in ['construct', 'drop']:
                    count = row[window]; events = count['events']
                    assert not count['event_overflow'] and len(events) <= 64
                    assert all(x['kind'] in ['a', 'z', 'r', 'd'] and x['align'] > 0 and x['align'] & (x['align'] - 1) == 0 for x in events)
                    alloc = [x for x in events if x['kind'] in ['a', 'z', 'r']]
                    free = [x for x in events if x['kind'] == 'd']
                    assert count['allocation_calls'] == len(alloc), 'event allocation calls'
                    assert count['requested_bytes'] == sum(x['new_size'] for x in alloc), 'event requested bytes'
                    assert count['reallocation_calls'] == sum(x['kind'] == 'r' for x in events) == 0
                    assert count['deallocation_calls'] == len(free)
                    assert count['deallocated_bytes'] == sum(x['old_size'] for x in free)
                assert row['drop']['allocation_calls'] == row['drop']['requested_bytes'] == 0
                assert row['construct']['deallocation_calls'] == 0
                all_rows[variant].append(dict(variant=variant, **row))
            assert all(x['construct'] == rows[1]['construct'] and x['drop'] == rows[1]['drop'] for x in rows[1:])
            assert rows[0]['construct']['requested_bytes'] - rows[1]['construct']['requested_bytes'] == 88
            assert rows[0]['construct']['allocation_calls'] - rows[1]['construct']['allocation_calls'] == 1
            assert rows[0]['drop'] == rows[1]['drop']
            triple = next(x for x in summaries[UNION]['rows'] if x['case'] == case)
            label = 'v1' if variant == 'candidate' else variant
            assert triple['variants'][label]['cold'] == rows[0] and triple['variants'][label]['warm'] == rows[1]
    assert all_rows['baseline'] + all_rows['candidate'] == data('records/all-allocation-records.json')
    assert all_rows['union'] == data(UNION + 'records/all-allocation-records.json')
    assert all(len(rows) == 72 for rows in all_rows.values())
    for kind in ['layout', 'constructor']:
        a = builds['baseline']['selected'][kind]['artifact']
        for variant in ['candidate', 'union']:
            b = builds[variant]['selected'][kind]['artifact']
            assert all(a[k] == b[k] for k in ['package_id', 'manifest_path', 'profile', 'features', 'filenames'])
    assert builds['union']['hello_sha256'] == builds['candidate']['hello_sha256']
    for row in summaries[UNION]['rows']:
        case, vs = row['case'], row['variants']
        a, b, c = (vs[v]['warm']['construct']['requested_bytes'] for v in ['baseline', 'v1', 'union'])
        assert row['union_minus_baseline'] == c - a and row['union_minus_v1'] == c - b, 'three-way delta recomputation'
        shape = case.split('.')[1]
        assert b - a == dict(unary=24, server_streaming=24, client_streaming=0, bidi=40)[shape]
        assert c - a == dict(unary=16, server_streaming=16, client_streaming=0, bidi=32)[shape]
        old_row = next(x for x in summaries['']['rows'] if x['case'] == case)
        assert old_row['constructor_requested_delta'] == b - a
        assert old_row['variants']['baseline'] == vs['baseline'] and old_row['variants']['candidate'] == vs['v1']
    assert summaries[UNION]['layouts'] == dict(baseline=layouts['baseline'], v1=layouts['candidate'], union=layouts['union'])
    assert data('records/complete.json')['passed'] and data(UNION + 'records/complete.json')['passed']
    assert data(UNION + 'records/complete.json')['baseline_and_v1_rerun'] is False
    return dict(passed=True, text_files=len(files), records=216, windows=432, layouts=3,
                cases=8, measured_samples_per_case_variant=9, reused_records=144, new_union_records=72,
                historical_executable_hashes=6, source_pins=205,
                scope='Retained text/receipt accounting only; no executable rehash/replay, Git/worktree, CPU, release or RPC qualification')


def reseal(files, manifest):
    """Test-only synthetic fixtures: update integrity dependencies, no bypass."""
    files = dict(files)
    def update(name, change):
        old = json.loads(files[name]); new = copy.deepcopy(old); change(new)
        if new != old:
            files[name] = encode(new)
    for _ in range(12):
        previous = dict(files)
        for name in list(files):
            if name.endswith('-process.json'):
                stem = name[:-len('-process.json')]
                if stem + '.stdout' in files:
                    update(name, lambda x, s=stem: x.update(stdout_sha256=sha(files[s + '.stdout']), stderr_sha256=sha(files[s + '.stderr'])))
        for name in list(files):
            if name.endswith('.json') and name[:-5] + '-process.json' in files:
                value = json.loads(files[name])
                if isinstance(value, dict) and 'process' in value:
                    update(name, lambda x, n=name: x.__setitem__('process', json.loads(files[n[:-5] + '-process.json'])))
        for name, prefix in [('results-binding.json', ''), (UNION + 'controls-binding.json', ''), (UNION + 'results-binding.json', UNION)]:
            def maps(x, p=prefix):
                for n in x['files']:
                    if p + n in files:
                        x['files'][n] = sha(files[p + n])
            update(name, maps)
        for prefix, name in [('', 'execution-plan.json'), (UNION, 'plan.json')]:
            def frozen(x, p=prefix):
                for n in x['frozen_files']:
                    x['frozen_files'][n] = sha(files[p + n])
                if p:
                    x['controls_binding_sha256'] = sha(files[UNION + 'controls-binding.json'])
                    x['union_source_sha256'] = sha(files[UNION + 'union-source.json'])
            update(prefix + name, frozen)
        for prefix, plan in [('', 'execution-plan.json'), (UNION, 'plan.json')]:
            for name in [prefix + 'records/execution-context.json'] + ([prefix + 'records/union-build.json'] if prefix else ['records/baseline-build.json', 'records/candidate-build.json']):
                update(name, lambda x, p=prefix, n=plan: x.__setitem__('plan_sha256', sha(files[p + n])))
        update(UNION + 'execution-freeze.json', lambda x: x.update(plan_sha256=sha(files[UNION + 'plan.json']), frozen_files=json.loads(files[UNION + 'plan.json'])['frozen_files'], reused_control_binding_sha256=sha(files[UNION + 'controls-binding.json'])))
        if previous == files:
            break
    else:
        raise AssertionError('synthetic integrity graph did not settle')
    synthetic = copy.deepcopy(manifest)
    synthetic['files'] = {n:dict(bytes=len(v), sha256=sha(v)) for n,v in files.items()}
    raw = (json.dumps(dict(schema='pbrs.authority-storage-text.v1', files={n:v.decode() for n,v in files.items()}), ensure_ascii=False, separators=(',', ':')) + '\n').encode()
    compressed = gzip.compress(raw, compresslevel=9, mtime=0)
    synthetic.update(compressed_bytes=len(compressed), compressed_sha256=sha(compressed), uncompressed_bytes=len(raw), uncompressed_sha256=sha(raw))
    return unpack(compressed, synthetic), synthetic


def main():
    files, manifest = load()
    answer = verify(files, manifest)
    cases = []
    def case_change(label, marker, change):
        altered = dict(files); name = UNION + 'records/union-direct.unary.json'
        value = json.loads(altered[name]); change(value); altered[name] = encode(value)
        prefix = 'AUTHORITY_COST_ALLOC '
        original = altered[name[:-5] + '.stdout'].decode().splitlines(keepends=True)
        rows = iter(value['rows']); output = []
        for line in original:
            if prefix in line:
                row = next(rows, None)
                if row is not None:
                    output.append(line.split(prefix, 1)[0] + prefix + json.dumps(row) + '\n')
            else:
                output.append(line)
        altered[name[:-5] + '.stdout'] = ''.join(output).encode()
        cases.append((label, marker, altered))
    case_change('wrong requested total', 'event requested bytes', lambda x:x['rows'][1]['construct'].__setitem__('requested_bytes', x['rows'][1]['construct']['requested_bytes'] + 1))
    case_change('missing warm sample', 'sample completeness', lambda x:x['rows'].pop())
    case_change('clock in allocation row', 'counter clock separation', lambda x:x['rows'][1].__setitem__('cpu_ns', 1))
    case_change('wrong historical executable', 'case executable binding', lambda x:x.__setitem__('executable_sha256', '0' * 64))
    for label, marker, change in [('hidden remaining cost', 'three-way delta recomputation', lambda x:x['rows'][0].__setitem__('union_minus_baseline', 0)), ('omitted case', 'summary case grid', lambda x:x['rows'].pop())]:
        altered = dict(files); name = UNION + 'summary.json'; value = json.loads(altered[name]); change(value); altered[name] = encode(value)
        cases.append((label, marker, altered))
    for label, marker, altered in cases:
        altered, synthetic = reseal(altered, manifest)
        try:
            verify(altered, synthetic)
        except AssertionError as error:
            assert str(error) == marker, (label, 'wrong failure point', str(error))
        else:
            raise AssertionError('corruption accepted: ' + label)
    answer['semantic_corruptions_rejected'] = len(cases)
    print(json.dumps(answer))


if __name__ == '__main__':
    main()
