"""Portable full selected-byte and ten-command audit; no archive extraction."""
import hashlib
import json
from pathlib import Path
import re
import tarfile

HERE = Path(__file__).resolve().parent
SOURCE = '68e4d57d67c73ff87ea1404bee18c3c70a9dfb93'
ROOT_AUDIT_SHA = '6042feab22a494dbf409b5395c2581d84c7629d22df91cdf5805d4807220703a'
ROOT_AUDITOR_SHA = '848fad6ae5611c1f1a94b81ff38bedd607a9eec8f4508c36e913ce3f799ee47f'
GROUP_AUDIT_SHA = '6bf816c217e153d80fda93a0feab98eed60d4287c35578bbbcb2d1942e5f1595'
LINKED_RECORD_SHA = 'b497a26618ae6f3b563109f822a2aa50e073fdf86bcb347b7bde1aa7050cf0ab'
ORIGINAL_GATE_SHA = '5105b0cd16bc7edc103e69b262f87e665ef3de112c6c46b19067063315791abe'
STRICT_GATE_SHA = '112b8d62517b5b7583170fa41e8105a60f010a640163b208190acd86af9fb260'
EXPECTED = ['feature-off185-001', 'core185-001', 'combined-default188-002', 'strict-root188-002',
            'linked-upstream188-001', 'strict-consumer188-001', 'plugin-setup188-001',
            'genuine-regen-001', 'group-shared-001', 'map-shared-002']


def require(value, message):
    if not value:
        raise RuntimeError(message)


def audit(root, expected_status='PASS_FULL_SELECTED_RAW_COMMAND_CAPSULE'):
    manifest = json.loads((root / 'manifest.json').read_text())
    require(manifest['status'] == expected_status and manifest['source'] == SOURCE
            and manifest['accepted_native_steps'] == EXPECTED and manifest['accepted_coordinator_disposition'] == {'failed001_first_two': 2, 'failed002_combined_green': 1, 'failed003_strict_green': 1, 'failed003_linked_native_root_qualified': 1, 'failed005_four_native_greens': 4, 'passed007_final_map': 1}
            and manifest['hash_only_included_members'] == 0, 'qualification scope differs')
    rows = {row['member']: row for row in manifest['rows']}
    require(len(rows) == len(manifest['rows']) and sum(row['bytes'] for row in rows.values()) <= 128 * 1024 * 1024, 'duplicate/overlimit member inventory')
    archive = root / 'raw-command-proof.tar.gz'
    digest = hashlib.sha256()
    with archive.open('rb') as incoming:
        for data in iter(lambda: incoming.read(1048576), b''):
            digest.update(data)
    require(archive.stat().st_size == manifest['archive']['bytes'] and digest.hexdigest() == manifest['archive']['sha256'], 'archive bytes differ')
    seen = set()
    retained = {}
    wanted = {f'commands/{name}/record.json' for name in EXPECTED}
    wanted |= {'commands/core185-001/stdout', 'commands/combined-default188-001/record.json', 'commands/combined-default188-001/stdout',
               'commands/strict-root188-001/record.json', 'commands/strict-root188-001/stderr',
               'commands/linked-upstream188-001/stdout', 'commands/outer-native-004/record.json', 'commands/outer-native-003/record.json', 'commands/outer-native-002/record.json',
               'commands/outer-native-001/record.json', 'commands/outer-native-005/record.json', 'commands/outer-native-006/record.json', 'commands/outer-native-007/record.json',
               'commands/map-shared-001/record.json', 'commands/map-shared-001/stderr',
               'commands/linked-upstream-selected-preservation-001/record.json',
               'local-payload-references/linked-upstream-preservation-record.json', 'original-source/composition-plan.json'}
    wanted |= {'commands/group-child/stdout', 'commands/map-child/stdout',
               'commands/group-shared-payload-001/cargo/stdout', 'commands/map-shared-payload-002/cargo/stdout',
               'commands/genuine-regen-payload-001/record.json',
               'oracles/tests/fixtures/unknown_group_depth_consumer.rs', 'oracles/tests/fixtures/generated_map_value_depth_consumer.rs'}
    wanted |= {'root-qualification/group9-native-reconciliation.json', 'root-qualification/linked-native-reconciliation.json', 'root-qualification/reconcile-linked-output-delta.py',
               'original-source/gate.py', 'continuation-005/consumer_gate.py',
               'original-source/shared_map_consumer.py', 'original-source/shared_map_consumer-accepted-007.py',
               'continuation-007/derived-map-plan.json'}
    with tarfile.open(archive, 'r|gz') as stream:
        for member in stream:
            require(member.isfile() and member.name in rows and member.name not in seen and member.mode == 0o644,
                    'unexpected/duplicate/nonregular member or capsule mode')
            row = rows[member.name]
            require(member.size == row['bytes'], 'member size differs')
            digest = hashlib.sha256()
            payload = bytearray() if member.name in wanted else None
            source = stream.extractfile(member)
            count = 0
            for data in iter(lambda: source.read(1048576), b''):
                count += len(data); digest.update(data)
                if payload is not None:
                    payload.extend(data)
            require(count == row['bytes'] and digest.hexdigest() == row['sha256'], 'complete member payload differs')
            if payload is not None:
                retained[member.name] = bytes(payload)
            seen.add(member.name)
    require(seen == set(rows) and wanted <= set(retained), 'selected coverage incomplete')
    plan = json.loads(retained['original-source/composition-plan.json'])
    require(plan['source'] == SOURCE and len(plan['steps']) == 10, 'original plan/source differs')
    require(hashlib.sha256(retained['original-source/gate.py']).hexdigest() == ORIGINAL_GATE_SHA
            and hashlib.sha256(retained['continuation-005/consumer_gate.py']).hexdigest() == STRICT_GATE_SHA,
            'actual original/strict corrected runner source differs')
    require(hashlib.sha256(retained['root-qualification/linked-native-reconciliation.json']).hexdigest() == ROOT_AUDIT_SHA
            and hashlib.sha256(retained['root-qualification/reconcile-linked-output-delta.py']).hexdigest() == ROOT_AUDITOR_SHA,
            'literal independent linked audit/script bytes differ')
    qualification = json.loads(retained['root-qualification/linked-native-reconciliation.json'])
    require(qualification['status'] == 'PASS_NATIVE_WITH_EXPECTED_GENERATED_OUTPUT_DELTA' and qualification['source'] == SOURCE
            and qualification['original_gate_record_sha256'] == LINKED_RECORD_SHA and qualification['original_native_exit'] == 0
            and qualification['original_wrapper_exit'] == 126 and qualification['all230_original_files_fullhash_unchanged'] is True
            and qualification['mods_reconstructed_byte_exact'] is True and qualification['original_resources_cleanup_source_tools_helpers_SDK_unchanged'] is True,
            'bounded linked qualification split differs')
    old_helper = retained['original-source/shared_map_consumer.py']
    original_line = b" (stage / 'accepted-graph.json').write_bytes(ordinary.Path(case['accepted_record']).read_bytes())\n"
    corrected_line = b" (stage / 'accepted-graph.json').write_text(json.dumps(accepted, indent=2) + '\\n')\n"
    require(old_helper.count(original_line) == 1
            and retained['original-source/shared_map_consumer-accepted-007.py'] == old_helper.replace(original_line, corrected_line),
            'corrected map helper differs beyond the accepted graph return serialization')
    map_plan = json.loads(retained['continuation-007/derived-map-plan.json'])
    expected_map_plan = json.loads(retained['original-source/composition-plan.json'])
    map_step = expected_map_plan['steps'][9]
    map_step['argv'][2] = str(Path(map_step['argv'][2]).with_name('shared_map_consumer-accepted-007.py'))
    map_step['argv'][map_step['argv'].index('--attempt') + 1] = 'map-shared-payload-002'
    expected_map_plan['steps'] = [map_step]
    require(map_plan == expected_map_plan, 'derived map plan contains undeclared source/workload/argv/env/bound changes')
    for index, name in enumerate(EXPECTED):
        record = json.loads(retained[f'commands/{name}/record.json'])
        step = map_plan['steps'][0] if index == 9 else plan['steps'][index]
        is_linked = step['name'] == 'linked-upstream188'
        require(record['native_exit'] == 0 and record['exit'] == (126 if is_linked else 0) and record['guard'] is None
                and record['argv'] == step['argv'] and record['timeout_seconds'] == 900
                and record['runner_sha256'] == (STRICT_GATE_SHA if step['name'] == 'strict-consumer188' else ORIGINAL_GATE_SHA)
                and record['plan_sha256'] == hashlib.sha256(retained['continuation-007/derived-map-plan.json'] if index == 9 else retained['original-source/composition-plan.json']).hexdigest()
                and record['source_before']['commit'] == SOURCE and record['source_before']['status'] == ''
                and all(record[key + '_before'] == record[key + '_after'] for key in ('source', 'tools', 'work_helpers', 'helper_closure'))
                and not any(record.get(key) for key in ('exception', 'cleanup_exception', 'post_cleanup_observation_exception'))
                and record.get('drift') == ('standalone source/manifest/lock changed during direct consumer command' if is_linked else None)
                and not record['processes_after_cleanup']['observed'] and not record['processes_after_cleanup']['inaccessible_pids']
                and record['minimum_free'] >= 2147483648 and record['maximum_owned_apparent_bytes'] <= 2147483648, 'native source/argv/guard/cleanup differs')
        if is_linked:
            require(hashlib.sha256(retained[f'commands/{name}/record.json']).hexdigest() == LINKED_RECORD_SHA, 'original wrapper126 raw changed')
            before = record['consumer_inputs_before']['files']; after = record['consumer_inputs_after']['files']
            require(len(before) == 230 and len(after) == 347 and set(before) <= set(after) and all(after[key] == value for key, value in before.items())
                    and {key: after[key] for key in set(after) - set(before)} == qualification['exact117_additions'], 'literal expected generated delta differs')
            for pin in qualification['protoc27']:
                matched = [row for row in rows.values() if row['original_path'] == pin['path']]
                require(len(matched) == 1 and matched[0]['sha256'] == pin['sha256'], 'root27 protoc record scope differs')
            require(len(qualification['protoc27']) == 27, 'root27 protoc scope incomplete')
    failed = json.loads(retained['commands/combined-default188-001/record.json'])
    require(failed['exit'] == failed['native_exit'] == 101 and failed['source_before']['commit'] == SOURCE
            and b'hello fds: Os { code: 2, kind: NotFound' in retained['commands/combined-default188-001/stdout'], 'historical fixture failure missing/relabelled')
    strict_failed = json.loads(retained['commands/strict-root188-001/record.json'])
    require(strict_failed['exit'] == strict_failed['native_exit'] == 101 and strict_failed['source_before']['commit'] == SOURCE
            and b"couldn't read `tests/../protobuf-tonic/README.md`" in retained['commands/strict-root188-001/stderr'], 'historical literal-include failure missing/relabelled')
    core = retained['commands/core185-001/stdout'].decode()
    require(re.findall(r'test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;', core) == ['77', '12'], 'historical core89 coverage differs')
    upstream = retained['commands/linked-upstream188-001/stdout'].decode()
    counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;', upstream)
    require(len(counts) == 21 and sum(map(int, counts)) == 254, 'upstream21 summaries/254 executions differ')
    for label, count, fixture in [('group', 9, 'unknown_group_depth_consumer.rs'), ('map', 15, 'generated_map_value_depth_consumer.rs')]:
        text = retained['oracles/tests/fixtures/' + fixture].decode()
        expected_names = set(re.findall(r'#\[test\]\s*fn (\w+)\(', text))
        require(len(expected_names) == count, 'frozen generated-consumer oracle mask differs')
        for raw in ['commands/' + label + '-child/stdout', 'commands/' + label + '-shared-payload-' + ('002' if label == 'map' else '001') + '/cargo/stdout']:
            output = retained[raw].decode()
            require(re.findall(r'test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;', output) == [str(count)],
                    'actual complete generated-consumer test count differs')
            statuses = re.findall(r'^test (\S+) \.\.\. (ok|FAILED)$', output, re.MULTILINE)
            if statuses:
                require(len(statuses) == count and {name.rsplit('::', 1)[-1] for name, _ in statuses} == expected_names
                        and all(status == 'ok' for _, status in statuses), 'actual named generated-consumer mask differs')
            else:
                require(not re.findall(r'^(\S+) --- FAILED$', output, re.MULTILINE), 'quiet generated consumer has named failure')
                # Quiet passing names are inferred by exclusion against the frozen fixture.
    regeneration = json.loads(retained['commands/genuine-regen-payload-001/record.json'])
    require(regeneration['status'] == 'PASS' and regeneration['source_unchanged'] and len(regeneration['comparisons']) == 13
            and all(row['byte_equal'] and row['candidate_sha256'] == row['staged_sha256'] for row in regeneration['comparisons'])
            and len(regeneration['commands']) == 20 and all(row['exit'] == 0 for row in regeneration['commands']),
            'genuine thirteen-module regeneration byte comparison differs')
    final = json.loads(retained['commands/outer-native-007/record.json'])
    require(final['status'] == 'passed' and [row['name'] for row in final['rows']] == ['map-shared']
            and final['rows'][0]['wrapper_exit'] == final['rows'][0]['native_exit'] == 0
            and final['rows'][0]['gate_record_sha256'] == rows['commands/map-shared-002/record.json']['sha256'], 'final map incomplete')
    prefix = json.loads(retained['commands/outer-native-005/record.json'])
    require(prefix['status'] == 'failed_or_incomplete' and [row['name'] for row in prefix['rows']] == [step['name'] for step in plan['steps'][5:9]]
            and prefix['error']['message'] == 'qualified original/output payload changed: work/p0-depth-composition-preparation-001/consumers/group-shared/src/lib.rs'
            and all(row['native_exit'] == row['wrapper_exit'] == 0 for row in prefix['rows'][:3])
            and prefix['rows'][3]['wrapper_exit'] == 0, 'fifth failed parent/actual four native greens differ')
    for row in prefix['rows'][:3]:
        require(row['gate_record_sha256'] == rows['commands/' + row['name'] + '-001/record.json']['sha256'], 'fifth-parent completed raw gate mapping differs')
    group = json.loads(retained['root-qualification/group9-native-reconciliation.json'])
    require(hashlib.sha256(retained['root-qualification/group9-native-reconciliation.json']).hexdigest() == GROUP_AUDIT_SHA
            and group['status'] == 'PASS_NATIVE_GROUP9_WITH_EXPECTED_HELPER_FIXTURE_TRANSITION'
            and group['source'] == SOURCE and group['native_exit'] == group['wrapper_exit'] == 0
            and group['original_gate_record_sha256'] == rows['commands/group-shared-001/record.json']['sha256']
            == '2c10de5d794b5b291efcfe931a738594374993673141dc61b1a273c46ca2c96b'
            and group['failed_coordinator_record_sha256'] == rows['commands/outer-native-005/record.json']['sha256']
            and group['failed_coordinator_status_retained'] == prefix['status']
            and group['actual_parent_launch_row'] == prefix['rows'][3]
            and group['other229_original_files_fullhash_unchanged'] is True
            and group['original_finished_native_read_only'] is True and group['proof']['group-shared']['passed'] == 9,
            'literal independent group9/raw failed-parent qualification differs')
    fourth = json.loads(retained['commands/outer-native-004/record.json'])
    failed_preserve = json.loads(retained['commands/linked-upstream-selected-preservation-001/record.json'])
    require(fourth['status'] == 'failed_or_incomplete' and fourth['rows'] == []
            and fourth['first_selected_preservation']['wrapper_exit'] == 1
            and failed_preserve['exit'] == failed_preserve['native_exit'] == 1,
            'fourth failed parent/artifact-capture failure was relabelled')
    selected = json.loads(retained['local-payload-references/linked-upstream-preservation-record.json'])
    require(selected['status'] == 'complete; all copied member hashes rechecked'
            and selected['root_reconciliation']['sha256'] == ROOT_AUDIT_SHA
            and rows['local-payload-references/linked-upstream-preservation-record.json']['sha256']
            == prefix['first_selected_preservation']['payload_record_sha256']
            == '158ff1f30cae65bfb54b41050411ad1670a0f1f75fadb5dadbc08e416dcd6579',
            'first complete excluded selected-asset reference differs')
    previous_map_parent = json.loads(retained['commands/outer-native-006/record.json'])
    previous_map = json.loads(retained['commands/map-shared-001/record.json'])
    require(previous_map_parent['status'] == 'failed_or_incomplete'
            and [row['name'] for row in previous_map_parent['rows']] == ['map-shared']
            and previous_map_parent['rows'][0]['wrapper_exit'] == 1
            and previous_map['native_exit'] == previous_map['exit'] == 1
            and previous_map['argv'] == plan['steps'][9]['argv']
            and previous_map['source_before']['commit'] == SOURCE
            and b"KeyError: 'accepted_record'" in retained['commands/map-shared-001/stderr'],
            'original failed map helper/raw parent was relabelled')
    first = json.loads(retained['commands/outer-native-001/record.json'])
    require(first['status'] == 'failed_or_incomplete' and [row['name'] for row in first['rows'][:2]] == ['feature-off185', 'core185']
            and all(row['native_exit'] == row['wrapper_exit'] == 0 for row in first['rows'][:2]), 'first parent failure or accepted first two rows were relabelled')
    prefix = json.loads(retained['commands/outer-native-002/record.json'])
    require(prefix['status'] == 'failed_or_incomplete' and prefix['rows'][0]['name'] == 'combined-default188'
            and prefix['rows'][0]['native_exit'] == prefix['rows'][0]['wrapper_exit'] == 0, 'earlier parent failure or accepted combined row was relabelled')
    third = json.loads(retained['commands/outer-native-003/record.json'])
    require(third['status'] == 'failed_or_incomplete' and [row['name'] for row in third['rows']] == ['strict-root188', 'linked-upstream188']
            and third['rows'][0]['native_exit'] == third['rows'][0]['wrapper_exit'] == 0
            and third['rows'][1]['wrapper_exit'] == 126 and third['rows'][1]['gate_record_sha256'] == LINKED_RECORD_SHA,
            'third parent failure or accepted strict/qualified linked rows were relabelled')
    print(f'PASS: {len(rows)} full selected members; exact ten native records with original linked wrapper126 retained; historical fixture101 retained')


if __name__ == '__main__':
    audit(HERE)
