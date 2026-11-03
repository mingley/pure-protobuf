#!/usr/bin/env python3
"""Read the follow-up capsule without extracting or executing its proof files."""
import hashlib
import io
import json
from functools import lru_cache
from pathlib import Path, PurePosixPath
import re
import posixpath
import statistics
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent
ROOT = next(path for path in HERE.parents if (path / '.git').exists())
BASE = 'a35701582fb9e6964c855386b0d9c991c50345ba'
PREFIX = 'work/followup-20261102/'
BENCH_REPORTS = {'devloop-final-build.json', 'devloop-final-tests.json', 'adoption-final-tests.json'}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def safe_name(name):
    path = PurePosixPath(name)
    assert not path.is_absolute() and '..' not in path.parts and str(path) == name, name
    return name


def load_json(files, name):
    return json.loads(files[safe_name(name)])


@lru_cache(maxsize=None)
def git_blob(path):
    safe_name(path)
    proc = subprocess.run(['git', 'show', BASE + ':' + path], cwd=ROOT, capture_output=True)
    return proc.stdout if proc.returncode == 0 else None


def load_capsule():
    manifest = json.loads((HERE / 'manifest.json').read_text())
    assert manifest['schema'] == 'pbrs.readiness-followup-capsule.v1'
    assert manifest['base_commit'] == BASE
    assert manifest['qualified'] is False and manifest['performance_ranking_claim'] is False
    raw = (HERE / 'raw.tar.gz').read_bytes()
    assert sha(raw) == manifest['archive_sha256'], 'archive hash differs'
    files = {}
    total = 0
    with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as archive:
        for member in archive.getmembers():
            name = safe_name(member.name)
            assert member.isfile() and name not in files, ('invalid or duplicate member', name)
            assert 0 <= member.size < 8 * 1024 * 1024, ('oversized member', name)
            total += member.size
            assert total < 64 * 1024 * 1024, 'oversized capsule'
            files[name] = archive.extractfile(member).read()
    assert set(files) == set(manifest['files']), 'member set differs'
    for name, data in files.items():
        record = manifest['files'][name]
        assert record == {'bytes': len(data), 'sha256': sha(data)}, name
    return manifest, files


def source_snapshot(files):
    snapshot = load_json(files, 'source-inputs.json')
    assert snapshot['base_commit'] == BASE
    inputs = snapshot['inputs_sha256']
    assert inputs
    overlays = snapshot['overlays']
    assert overlays.keys() <= inputs.keys()
    for path, expected in inputs.items():
        data = git_blob(path)
        if path in overlays:
            overlay = overlays[path]
            assert overlay['base_sha256'] == (sha(data) if data is not None else None), path
            assert overlay['archive_path'] == 'source-overlay/' + safe_name(path), path
            data = files[overlay['archive_path']]
            assert sha(data) == overlay['sha256'], path
        assert data is not None and sha(data) == expected, ('source input differs', path)
    return inputs


def final_reports(manifest, files, inputs):
    rows = manifest['final_reports']
    assert rows and len({row['report'] for row in rows}) == len(rows)
    coverage = set()
    for row in rows:
        name = safe_name(row['report'])
        assert name.startswith(PREFIX) and name.endswith('.json'), name
        report = load_json(files, name)
        declared_inputs = report.get('inputs_sha256')
        if row['format'] == 'core-miri-summary':
            declared_inputs = report['source_inputs_sha256']
        elif row['format'] == 'target-check':
            declared_inputs = report['source_before']
            assert name.startswith(PREFIX + 'i686/')
            assert report['inputs_unchanged'] is True and report['source_after'] == declared_inputs
        else:
            assert report['source_inputs_unchanged'] is True
        assert declared_inputs, name
        if row['format'] == 'readiness-check':
            assert name.startswith(PREFIX + 'qa/')
            assert report['schema'] == 'pbrs.readiness-check.v1'
            assert report['exit_code'] == report['process_returncode'] == 0, name
            assert report['disk_guard'] is False, name
            assert report['command'] and report['rustc'], name
            log = str(PurePosixPath(name).with_suffix('.log'))
            assert report['log'] == PurePosixPath(log).name and sha(files[log]) == report['log_sha256'], name
        elif row['format'] == 'benchmark-check':
            assert PurePosixPath(name).parent == PurePosixPath(PREFIX)
            assert PurePosixPath(name).name in BENCH_REPORTS
            assert report['exit_code'] == 0 and report['command'] and report['rustc'], name
            log = str(PurePosixPath(name).with_suffix('.log'))
            assert sha(files[log]) == report['log_sha256'], name
        elif row['format'] == 'python-contracts':
            assert name == PREFIX + 'rpc-python-final.json'
            assert report['schema'] == 'pbrs.benchmark-python-contracts.v1' and len(report['results']) == 2
            for result in report['results']:
                assert result['exit_code'] == 0 and result['command']
                log = safe_name(result['log'])
                assert '/' not in log and sha(files[PREFIX + log]) == result['log_sha256']
        elif row['format'] == 'core-miri-summary':
            assert name == PREFIX + 'i686/miri-x86_64-integrated-summary.json'
            assert report['passed'] == 16 and report['failed'] == report['ignored'] == report['measured'] == 0
            assert len(report['reports']) == 2
            total = 0
            for child_row in report['reports']:
                child = PREFIX + 'i686/' + safe_name(child_row['label']) + '.json'
                log = str(PurePosixPath(child).with_suffix('.log'))
                value = load_json(files, child)
                assert sha(files[child]) == child_row['report_sha256']
                assert sha(files[log]) == child_row['log_sha256'] == value['log_sha256']
                assert value['exit_code'] == 0 and value['inputs_unchanged'] is True
                assert value['source_before'] == value['source_after'] == declared_inputs
                assert 'miri' in value['command'] and 'x86_64-unknown-linux-gnu' in value['command']
                assert '-Zmiri-strict-provenance' in value['MIRIFLAGS']
                summaries = [list(map(int, match)) for match in re.findall(
                    rb'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;', files[log])]
                assert summaries == child_row['results']
                assert all(failed == ignored == measured == 0 for _, failed, ignored, measured, _ in summaries)
                total += sum(values[0] for values in summaries)
            assert total == 16
        elif row['format'] == 'target-check':
            assert report['exit_code'] == 0 and report['command'] and report['rustc']
            assert report['execution'] and report['source_commit']
            log = str(PurePosixPath(name).with_suffix('.log'))
            assert sha(files[log]) == report['log_sha256']
        else:
            raise AssertionError('unsupported final report format: ' + str(row['format']))
        for path, expected in declared_inputs.items():
            assert inputs[path] == expected, (name, path)
            coverage.add(path)
    rpc = load_json(files, manifest['functional_rpc_report'])
    for path, expected in rpc['inputs_sha256'].items():
        assert inputs[path] == expected, ('functional RPC source differs', path)
        coverage.add(path)
    assert coverage == set(inputs), 'final source union differs'


def marker(data, prefix):
    rows = [json.loads(line[len(prefix):]) for line in data.decode().splitlines()
            if line.startswith(prefix)]
    assert len(rows) == 1, (prefix, len(rows))
    return rows[0]


def rpc_inputs(files, report_name=PREFIX + 'rpc-input-children/report.json'):
    safe_name(report_name)
    assert report_name.startswith(PREFIX) and PurePosixPath(report_name).name == 'report.json'
    directory = str(PurePosixPath(report_name).parent) + '/'
    report = load_json(files, report_name)
    assert report['schema'] == 'pbrs.rpc-input-functional-proof.v1'
    assert report['status'] == 'passed' and report['source_inputs_unchanged'] is True
    assert report['checked_cells'] == 48 and report['successful_children'] == 192
    assert len(report['commands']) == 192
    profiles = {
        'adoption': ['native_pbrs', 'native_prost', 'tonic_pbrs', 'tonic_prost'],
        'tonic_transport': ['reference_reference', 'native_client_reference_server',
                            'reference_client_native_server', 'native_native'],
    }
    expected = {f'rpc.{stack}.{profile}.maps.n{size}.{shape}'
                for stack, values in profiles.items() for profile in values
                for size in (8, 64, 512) for shape in ('unary', 'server_stream')}
    assert set(report['cells']) == expected and len(report['cells']) == 48
    identities = {}
    seen = set()
    for row in report['commands']:
        assert row['exit_code'] == 0
        command = row['command']
        cell = command[2]
        assert cell in expected and command[1] == 'run-cell'
        iters = int(command[command.index('--iters') + 1])
        assert iters in (1, 2)
        for channel in ('stdout', 'stderr'):
            name = safe_name(row[channel])
            assert '/' not in name and sha(files[directory + name]) == row[channel + '_sha256']
        match = re.fullmatch(re.escape(cell) + r'\.r([01])\.n([12])\.stderr\.log', row['stderr'])
        assert match and int(match[2]) == iters
        key = (cell, int(match[1]), iters)
        assert key not in seen
        seen.add(key)
        child = marker(files[directory + row['stderr']], '__CHILD__ ')
        network = marker(files[directory + row['stderr']], '__QUALIFICATION__ ')
        assert child['cell'] == cell and child['iters'] == iters
        fingerprint = child['input_wire_fingerprint']
        length = child['input_wire_bytes']
        assert re.fullmatch(r'fnv1a64:[0-9a-f]{16}', fingerprint)
        assert type(length) is int and length > 0
        identity = (fingerprint, length)
        assert identity == (row['input_wire_fingerprint'], row['input_wire_bytes'])
        assert identity == (network['input_wire_fingerprint'], network['input_wire_bytes'])
        assert network['input_fingerprint_scope'] == 'common prepared wire; RPC map entry order may vary'
        assert network['request_bytes'] == length
        stack, size = cell.split('.')[1], int(cell.split('.')[4][1:])
        assert identities.setdefault(size, identity) == identity
        if stack == 'tonic_transport':
            assert network['id'] == cell and network['native_request_bytes'] == length
            assert network['response_bytes_per_message'] == length
            assert network['request_read_checksum'] == network['response_read_checksum']
            assert network['responses'] == (4 if cell.endswith('.server_stream') else 1)
            assert all(network[name] is True for name in
                       ('full_decoded_equality', 'equal_wire_work', 'accepted_tcp_nodelay'))
        else:
            assert network['response_bytes'] == length and network['stream_replies'] == 4
    assert seen == {(cell, repeat, iters) for cell in expected for repeat in (0, 1) for iters in (1, 2)}
    assert report['input_identities'] == {
        str(size): {'fingerprint': identity[0], 'bytes': identity[1]}
        for size, identity in identities.items()}
    build_name = safe_name(report['build_record'])
    build = load_json(files, build_name)
    assert sha(files[build_name]) == report['build_record_sha256']
    assert build['exit_code'] == 0 and build['source_inputs_unchanged'] is True
    assert build['inputs_sha256'] == report['inputs_sha256']
    assert sha(files[str(PurePosixPath(build_name).with_suffix('.log'))]) == build['log_sha256']
    artifact = report['cargo_artifact']
    assert artifact['target']['name'] == 'devloop' and artifact['target']['kind'] == ['bin']
    assert artifact['profile']['opt_level'] == '0' and artifact['profile']['test'] is False
    assert artifact['profile']['debug_assertions'] is True
    artifacts = []
    for line in files[str(PurePosixPath(build_name).with_suffix('.log'))].decode().splitlines():
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        if item.get('reason') == 'compiler-artifact' and item.get('executable') == report['binary']:
            artifacts.append(item)
    assert len(artifacts) == 1
    assert {key: artifacts[0][key] for key in artifact} == artifact
    assert re.fullmatch(r'[0-9a-f]{64}', report['binary_sha256'])
    return len(seen)


def detecting_controls(files):
    registry = load_json(files, PREFIX + 'registry-observation.json')
    assert registry['source'] == 'public crates.io API' and registry['observed_utc']
    expected_versions = {'pbrs': ('0.2.1', '0.2.0'),
                         'pbrs-grpc': ('0.1.0-alpha.3', '0.1.0-alpha.2'),
                         'protobuf-tonic': ('0.1.0-alpha.3', '0.1.0-alpha.2')}
    assert len(registry['crates']) == 3
    assert {row['crate'] for row in registry['crates']} == expected_versions.keys()
    for row in registry['crates']:
        assert row['status'] == 'observed' and row['candidate_present'] is False
        assert (row['candidate'], row['max_version']) == expected_versions[row['crate']]
    required = load_json(files, PREFIX + 'required-ci-a3570158.json')
    assert required['source_commit'] == BASE and required['run_id'] == 41115687791
    assert len(required['jobs']) == 11
    assert all(row['status'] == 'completed' and row['conclusion'] == 'success' for row in required['jobs'])
    compatibility = load_json(files, PREFIX + 'scheduled-compatibility-a3570158.json')
    assert compatibility['source_commit'] == BASE and compatibility['run_id'] == 41299721164
    assert {row['name'] for row in compatibility['jobs'] if row['conclusion'] == 'failure'} == {'fuzz-campaign', 'target-matrix'}
    prefix = PREFIX + 'i686/'
    assert b'Undefined Behavior' in files[prefix + 'miri-i686-baseline.log']
    assert b'optional_bytes' in files[prefix + 'miri-i686-baseline.log']
    before = load_json(files, prefix + 'layout-before-complete.json')
    after = load_json(files, prefix + 'layout-after.json')
    old = {row['target']: row['result'] for row in before['rows']}
    new = {row['target']: row['result'] for row in after['rows']}
    assert old['native'] == new['native'] == {'LazyBytes': [40, 8], 'LazyStr': [40, 8], 'Wire': [32, 8]}
    assert old['i686-unknown-linux-gnu'] == {'LazyBytes': [20, 4], 'LazyStr': [28, 4], 'Wire': [20, 4]}
    assert new['i686-unknown-linux-gnu'] == {'LazyBytes': [24, 4], 'LazyStr': [32, 4], 'Wire': [20, 4]}
    seed = files[PREFIX + 'descriptor/crash-52f06f01d32b69d1.bin']
    assert len(seed) == 511 and sha(seed) == '52f06f01d32b69d137bb6333e51508492c0b086c7efa66fef3b406284c2d4b28'
    assert b'1 passed; 3 failed;' in files[PREFIX + 'descriptor/baseline-tests-corrected.log']


def mechanism_screen(files):
    directory = PREFIX + 'chunk-screen/'
    build = load_json(files, directory + 'build.json')
    report = load_json(files, directory + 'results.json')
    assert report['schema'] == 'pbrs.buffer-chunk-mechanism-screen.v1'
    assert report['dedicated_host'] is False and report['executable_unchanged'] is True
    assert report['build'] == build and report['seed'] == 20261102
    assert len(report['cpu_affinity']) == 1 and report['finished_unix_seconds'] > report['started_unix_seconds']
    assert sha(files[directory + 'main.rs']) == build['source_sha256']
    assert sha(files[directory + 'run.py']) == report['driver_sha256']
    assert '-O' in build['command'] and '-Ccodegen-units=1' in build['command']
    assert build['bytes_artifact']['profile']['opt_level'] == '3'
    assert build['bytes_artifact']['profile']['test'] is False
    assert build['bytes_artifact']['profile']['debug_assertions'] is False
    assert re.fullmatch(r'[0-9a-f]{64}', build['bytes_rlib_sha256'])
    assert re.fullmatch(r'[0-9a-f]{64}', build['executable_sha256'])
    expected = {(1048576, 16384, 50001), (8388608, 16384, 8001),
                (8388608, 65536, 32001), (512, 16384, 2000001), (65536, 1024, 50001)}
    assert len(report['rows']) == 5
    seen = set()
    for row in report['rows']:
        case = (row['frame_bytes'], row['chunk_bytes'], row['rounds'])
        assert case in expected and case not in seen
        seen.add(case)
        assert len(row['pairs']) == 5
        ratios = []
        for pair in row['pairs']:
            assert len(pair['order']) == 2 and set(pair['order']) == {'slices', 'split'}
            assert set(pair['samples']) == {'slices', 'split'}
            for mode, sample in pair['samples'].items():
                fields = sample['stdout'].split()
                assert fields[:4] == [mode, *map(str, case)]
                assert len(fields) == 6 and int(fields[4]) == sample['timed_wall_ns']
                assert int(fields[5]) == sample['sink']
                assert sample['timed_wall_ns'] > 0 and sample['whole_process_cpu_ns'] > 0
            ratio = pair['samples']['split']['timed_wall_ns'] / pair['samples']['slices']['timed_wall_ns']
            assert ratio == pair['timed_wall_ratio']
            ratios.append(ratio)
        assert row['median_ratio'] == statistics.median(ratios)
        assert row['ratio_range'] == [min(ratios), max(ratios)]
    assert seen == expected
    return len(report['rows'])


def aggregate_rpc(files, inputs, primary_name):
    proof = load_json(files, PREFIX + 'rpc-final-aggregate-functional-check.json')
    report_name = PREFIX + 'rpc-final-aggregate-report.json'
    report = load_json(files, report_name)
    command_name = PREFIX + 'rpc-final-aggregate-smoke.json'
    command = load_json(files, command_name)
    primary = load_json(files, primary_name)
    assert proof['schema'] == 'pbrs.rpc-aggregate-functional-check.v1'
    assert proof['performance_qualified'] is False and proof['instruction_metrics'] == 'not_run'
    assert proof['source_inputs_unchanged'] is True and proof['checked_aggregate_rows'] == 6
    assert proof['binary_sha256'] == primary['binary_sha256']
    assert proof['build_record_sha256'] == primary['build_record_sha256']
    assert sha(files[report_name]) == proof['report_sha256']
    assert command['exit_code'] == 0 and command['source_inputs_unchanged'] is True
    assert sha(files[PREFIX + 'rpc-final-aggregate-smoke.log']) == command['log_sha256']
    assert all(inputs[path] == digest for path, digest in command['inputs_sha256'].items())
    assert report['schema'] == 'devloop/1' and len(report['cells']) == 6
    expected = {f'rpc.{profile}.maps.n{size}.unary'
                for profile in ('adoption.native_pbrs', 'tonic_transport.reference_reference')
                for size in (8, 64, 512)}
    assert {row['id'] for row in report['cells']} == expected
    rows = []
    for row in report['cells']:
        size = row['id'].split('.')[4][1:]
        identity = primary['input_identities'][size]
        assert row['input_wire_fingerprint'] == identity['fingerprint']
        assert row['input_wire_bytes'] == identity['bytes'] and row['iters'] == 1 and row['repeats'] == 2
        assert row['instructions']['status'] == 'not_run' and row['instruction_method'] == 'not_run'
        rows.append({key: row[key] for key in ('id', 'input_wire_fingerprint', 'input_wire_bytes', 'iters', 'repeats')})
    assert rows == proof['rows']
    return len(rows)


def partitioned_workspace(files, inputs, report_name=PREFIX + 'qa/native-partitions-coverage.json'):
    directory = PREFIX + 'qa/'
    safe_name(report_name)
    assert str(PurePosixPath(report_name).parent) + '/' == directory
    report = load_json(files, report_name)
    metadata = load_json(files, directory + 'native-partitions-metadata.json')
    assert report['schema'] in ('pbrs.workspace-partitions.v2', 'pbrs.workspace-partitions-resume.v1') and report['status'] == 'passed'
    assert all(inputs[path] == digest for path, digest in report['inputs_sha256'].items())
    for name, digest in report['helper_sha256'].items():
        assert sha(files[safe_name(name)]) == digest, ('partition helper changed', name)
    members = set(metadata['workspace_members'])
    registry, docs = {}, {}
    def key(package, target):
        return package + '::' + target['kind'][0] + '::' + target['name']
    for package in metadata['packages']:
        if package['id'] not in members:
            continue
        for target in package['targets']:
            if target['kind'] == ['lib'] and target['doctest']:
                assert target['name'] not in docs
                docs[target['name']] = package['id']
            if target['kind'] not in (['lib'], ['bin'], ['test']) or not target['test']:
                continue
            identity = key(package['id'], target)
            assert identity not in registry
            registry[identity] = {'package_id': package['id'], 'target': target,
                                  'expected_features': sorted(package['features'])}
    assert registry == report['registry'] and docs == report['doctest_libraries']
    assert report['metadata_command'] == ['cargo', 'metadata', '--no-deps', '--all-features', '--locked', '--format-version=1']
    assert report['native_targets'] == len(registry) and report['coverage'].keys() == registry.keys()
    groups = [['--lib'], ['--bins']] + [['--test', name] for name in sorted({
        row['target']['name'] for row in registry.values() if row['target']['kind'] == ['test']})]
    assert report['groups'] == groups
    base = ['cargo', 'test', '--workspace', '--all-features', '--locked']
    if report['schema'] == 'pbrs.workspace-partitions-resume.v1':
        binding = report['resume']
        initial_name = directory + 'native-partitions-coverage.json'
        initial = load_json(files, initial_name)
        assert initial['schema'] == 'pbrs.workspace-partitions.v2' and initial['status'] == 'running'
        assert sha(files[initial_name]) == binding['prior_coverage_sha256']
        assert binding['prior_wrapper'] == 'native-partitions-final.json'
        wrapper_name = directory + binding['prior_wrapper']
        wrapper = load_json(files, wrapper_name)
        assert sha(files[wrapper_name]) == binding['prior_wrapper_sha256']
        assert wrapper['schema'] == 'pbrs.readiness-check.v1'
        assert wrapper['exit_code'] == wrapper['process_returncode'] == 1
        assert wrapper['disk_guard'] is False and wrapper['source_inputs_unchanged'] is True
        assert wrapper['command'] == ['python3', PREFIX + 'docs/workspace-partitions.py', '--run']
        assert wrapper['log'] == 'native-partitions-final.log'
        wrapper_log = files[directory + wrapper['log']]
        assert sha(wrapper_log) == wrapper['log_sha256'] == binding['prior_wrapper_log_sha256']
        assert re.search(rb"AssertionError: \('consumer cache still active', '\d+'\)\s*$", wrapper_log)
        assert wrapper['inputs_sha256'] == initial['inputs_sha256'] == report['inputs_sha256']
        assert wrapper['rustc'] == initial['rustc'] == report['rustc']
        assert all(initial[name] == report[name] for name in
                   ('registry', 'groups', 'doctest_libraries', 'corroboration', 'native_targets'))
        assert all(report['helper_sha256'][name] == digest for name, digest in initial['helper_sha256'].items())
        assert report['commands'][:len(initial['commands'])] == initial['commands']
        assert all(report['coverage'][name] == row for name, row in initial['coverage'].items())
        completed = [row['command'] for row in initial['commands'] if row['command'][:5] == base]
        assert completed == [base + group + ['--message-format=json'] for group in groups[:len(completed)]]
        assert binding['completed_groups'] == list(range(1, len(completed) + 1))
        assert binding['completed_targets'] == len(initial['coverage'])
        assert binding['completed_passes'] == sum(row['result']['passed'] for row in initial['coverage'].values())
        assert binding['source_inputs'] == len(initial['inputs_sha256'])
        assert binding['remaining_groups'] == len(groups) - len(completed)
        assert binding['execution_credit'] == 'only completed commands from this same frozen partition pass; no earlier baseline executions'
        assert load_json(files, directory + 'native-partitions-resume-metadata.json') == metadata
    prior = load_json(files, 'work/readiness-qa-20261030/partitions-coverage.json')
    old = {key(row['package_id'], row['target']) for row in prior['inventory'].values()}
    assert report['corroboration'] == {'prior_targets': len(old), 'removed': sorted(old - registry.keys()),
                                     'added': sorted(registry.keys() - old), 'executions_reused': 0}
    assert not report['corroboration']['removed']
    command_rows = {}
    for row in report['commands']:
        assert row['exit_code'] == 0 and row['source_inputs_unchanged'] is True
        name = safe_name(row['log'])
        assert '/' not in name and name not in command_rows
        assert sha(files[directory + name]) == row['log_sha256']
        assert load_json(files, directory + str(PurePosixPath(name).with_suffix('.json'))) == row
        command_rows[name] = row
        argv = row['command']
        if argv[:3] == ['cargo', 'clean', '--target-dir']:
            assert len(argv) == 4
            allowed = {metadata['workspace_root'] + '/target/' + child
                       for child in ('integration-consumers', 'caller-rustls')}
            assert argv[3] in allowed
        else:
            assert argv in [base + group + ['--message-format=json'] for group in groups] + [base + ['--doc']]
    native_commands = [row['command'] for row in report['commands'] if row['command'][:5] == base
                       and row['command'] != base + ['--doc']]
    assert native_commands == [base + group + ['--message-format=json'] for group in groups]
    doc_rows = [row for row in report['commands'] if row['command'] == base + ['--doc']]
    assert len(doc_rows) == 1
    summary = re.compile(r'^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;')
    running = re.compile(r'^\s+Running .+ \((.+)\)\s*$')
    def suites(data, doctests=False):
        active, completed = None, {}
        for raw in data.decode().splitlines():
            line = re.sub(r'\x1b\[[0-9;]*m', '', raw)
            match = re.match(r'^\s+Doc-tests (\S+)\s*$', line) if doctests else running.match(line)
            if match:
                assert active is None, ('incomplete selected suite', active)
                active = match[1] if doctests else posixpath.normpath(posixpath.join(metadata['workspace_root'], match[1]))
            match = summary.match(line)
            if match and active is not None:
                status, *counts = match.groups()
                passed, failed, ignored, measured, filtered = map(int, counts)
                assert status == 'ok' and failed == measured == filtered == 0
                assert active not in completed
                completed[active] = dict(status=status, passed=passed, failed=failed, ignored=ignored,
                                         measured=measured, filtered=filtered)
                active = None
        assert active is None
        return completed
    covered = set()
    for group in groups:
        argv = base + group + ['--message-format=json']
        rows = [row for row in report['commands'] if row['command'] == argv]
        assert len(rows) == 1
        name = rows[0]['log']
        data = files[directory + name]
        artifacts = {}
        for line in data.decode().splitlines():
            try:
                item = json.loads(line)
            except json.JSONDecodeError:
                continue
            if (item.get('reason') == 'compiler-artifact' and item.get('package_id') in members
                    and item.get('executable') and item.get('profile', {}).get('test')):
                identity = key(item['package_id'], item['target'])
                assert identity not in artifacts
                artifacts[identity] = item
        expected = {identity for identity, row in registry.items()
                    if (group == ['--lib'] and row['target']['kind'] == ['lib'])
                    or (group == ['--bins'] and row['target']['kind'] == ['bin'])
                    or (group[0] == '--test' and row['target']['kind'] == ['test'] and row['target']['name'] == group[1])}
        assert expected and set(artifacts) == expected and not (covered & expected)
        completed = suites(data)
        assert set(completed) == {item['executable'] for item in artifacts.values()}
        for identity, artifact in artifacts.items():
            entry = report['coverage'][identity]
            for field in ('package_id', 'target', 'profile', 'features', 'fresh'):
                assert entry[field] == artifact[field], (identity, field)
            assert entry['target'] == registry[identity]['target']
            assert sorted(entry['features']) == registry[identity]['expected_features']
            assert entry['profile']['opt_level'] == '0' and entry['profile']['test'] is True
            assert entry['executable'] == artifact['executable'] and entry['log'] == name
            assert re.fullmatch(r'[0-9a-f]{64}', entry['binary_sha256'])
            assert entry['result'] == completed[artifact['executable']]
        covered |= expected
    assert covered == set(registry)
    doctests = suites(files[directory + doc_rows[0]['log']], doctests=True)
    assert set(doctests) == set(docs) and report['doctests'] == doctests
    totals = {
        'native_targets': len(covered),
        'native_passed': sum(row['result']['passed'] for row in report['coverage'].values()),
        'native_ignored': sum(row['result']['ignored'] for row in report['coverage'].values()),
        'doctest_passed': sum(row['passed'] for row in doctests.values()),
        'doctest_ignored': sum(row['ignored'] for row in doctests.values()),
    }
    assert all(report[name] == value for name, value in totals.items())
    receipt = load_json(files, PREFIX + 'completed-cold-adapter-cleanup.json')
    assert receipt['exit_code'] == 0 and receipt['command'][:3] == ['cargo', 'clean', '--target-dir']
    assert len(receipt['command']) == 4
    private = receipt['command'][3]
    assert private.startswith(metadata['workspace_root'] + '/target/pbrs-onboarding-adapters-noprotoc-')
    assert private.endswith('/build') and receipt['free_after'] > receipt['free_before']
    assert receipt['completion_marker'] == 'test adapters_build_cold_from_descriptor_sets_without_protoc ... ok'
    assert receipt['completion_marker'].encode() in files[safe_name(receipt['completion_log'])]
    for name in ('retired-worktree.json', 'retired-candidate-worktree.json'):
        record = load_json(files, PREFIX + name)
        assert record['status_before'] == '' and record['exit_code'] == 0
        assert re.fullmatch(r'[0-9a-f]{40}', record['source_commit'])
        assert record['command'] == ['git', 'worktree', 'remove', record['path']]
    return totals


def verify(manifest, files):
    inputs = source_snapshot(files)
    final_reports(manifest, files, inputs)
    children = rpc_inputs(files, manifest['functional_rpc_report'])
    detecting_controls(files)
    mechanism_rows = mechanism_screen(files)
    workspace = partitioned_workspace(files, inputs, manifest['workspace_report'])
    aggregates = aggregate_rpc(files, inputs, manifest['functional_rpc_report'])
    return {'selected_final_inputs': len(inputs), 'accepted_final_reports': len(manifest['final_reports']),
            'functional_rpc_children': children, 'isolated_mechanism_rows': mechanism_rows,
            'functional_aggregate_rows': aggregates,
            'workspace': workspace, 'qualified': False, 'new_cpu_ranking_claim': False}


if __name__ == '__main__':
    manifest, files = load_capsule()
    print(json.dumps(verify(manifest, files)))
