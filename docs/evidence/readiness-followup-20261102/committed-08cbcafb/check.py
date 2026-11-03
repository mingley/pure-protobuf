#!/usr/bin/env python3
"""Verify committed input binding, retained packages and bounded smoke records."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = next(path for path in HERE.parents if (path / '.git').exists())
COMMIT = '08cbcafb3042d5d465fc764ceb7f2578095f510d'
CONSUMER_COMMIT = 'a35701582fb9e6964c855386b0d9c991c50345ba'
PREVIOUS = 'docs/evidence/readiness-followup-20261102/'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read(path):
    return (HERE / path).read_bytes()


def load(path):
    return json.loads(read(path))


def blob(path):
    return subprocess.check_output(['git', 'show', COMMIT + ':' + path], cwd=ROOT)


def verify_inputs(inputs):
    assert inputs
    for path, digest in inputs.items():
        name = PurePosixPath(path)
        assert not name.is_absolute() and '..' not in name.parts
        assert sha(blob(path)) == digest, ('committed input differs', path)


def normalized_audit(value):
    value = json.loads(json.dumps(value))
    for row in value['archives']:
        row['archive'] = Path(row['archive']).name
        row['comparison']['archive'] = Path(row['comparison']['archive']).name
    return value


def main():
    manifest = load('manifest.json')
    assert manifest['schema'] == 'pbrs.committed-followup-packet.v1'
    assert manifest['source_commit'] == COMMIT and manifest['qualified'] is False
    found = {str(path.relative_to(HERE)) for path in HERE.rglob('*') if path.is_file()
             and path != HERE / 'manifest.json'}
    assert found == set(manifest['files']), 'packet member set differs'
    for path, record in manifest['files'].items():
        name = PurePosixPath(path)
        assert not name.is_absolute() and '..' not in name.parts
        data = read(path)
        assert record == {'bytes': len(data), 'sha256': sha(data)}, path

    tree = subprocess.check_output(['git', 'rev-parse', COMMIT + '^{tree}'], cwd=ROOT, text=True).strip()
    binding = load('records/source-binding.json')
    assert binding['source_commit'] == COMMIT and binding['source_tree'] == tree
    assert binding['previous_capsule_path'] == PREVIOUS and binding['inputs_checked'] == 1307
    assert sha(blob(PREVIOUS + 'manifest.json')) == binding['previous_manifest_sha256']
    previous_raw = blob(PREVIOUS + 'raw.tar.gz')
    assert sha(previous_raw) == binding['previous_archive_sha256']
    with tarfile.open(fileobj=io.BytesIO(previous_raw), mode='r:gz') as archive:
        snapshot = json.load(archive.extractfile('source-inputs.json'))
    inputs = {row['path']: row['sha256'] for row in binding['inputs']}
    assert len(inputs) == len(binding['inputs']) == 1307
    assert inputs == snapshot['inputs_sha256'], 'previous selected input set differs'
    verify_inputs(inputs)

    publisher = load('records/publisher/report.json')
    assert publisher['schema'] == 'pbrs.local-release-rehearsal.v2'
    assert publisher['source_commit'] == publisher['head_after'] == COMMIT
    assert publisher['source_tree'] == tree and publisher['stage'] == 'complete'
    assert publisher['exit_code'] == publisher['publisher_exit_code'] == publisher['auditor_exit_code'] == 0
    assert publisher['status_before'] == publisher['status_after'] == ''
    assert publisher['inputs_unchanged'] is True and publisher['disk_guard'] is False
    assert publisher['registry_token_variables_forwarded'] is False
    assert publisher['environment']['DRY_RUN'] == '1' and publisher['environment']['RELEASE_TAG'] == 'v0.2.1'
    assert publisher['command'] == ['bash', 'scripts/publish-crates.sh']
    assert publisher['wrapper_sha256'] == sha(read('records/publisher/run.py'))
    assert publisher['publisher_sha256'] == sha(blob('scripts/publish-crates.sh'))
    assert publisher['publisher_log_sha256'] == sha(read('records/publisher/publisher.log'))
    assert publisher['auditor_log_sha256'] == sha(read('records/publisher/archive-audit.log'))
    assert publisher['archive_audit_sha256'] == sha(read('records/publisher/archive-audit.json'))
    verify_inputs(publisher['input_sha256'])
    observed = load('records/publisher/consumer-archives-observed.json')
    assert sha(read('records/publisher/consumer-archives-observed.json')) == publisher['consumer_observation_sha256']
    assert observed['source_commit_compared'] == COMMIT and observed['comparison_commit'] == CONSUMER_COMMIT
    assert observed['source_members_checked'] == publisher['shipped_source_members_checked'] == 419
    assert set(observed['dirty_crates']) == {'pbrs', 'pbrs-grpc'}
    audit = load('records/publisher/archive-audit.json')
    auditor = HERE / 'records/publisher/audit-release-archives.py'
    assert sha(auditor.read_bytes()) == audit['auditor_sha256'] == publisher['auditor_sha256']
    with tempfile.TemporaryDirectory(prefix='pbrs-followup-package-audit-') as temp:
        output = Path(temp) / 'audit.json'
        subprocess.run([sys.executable, str(auditor), '--root', str(ROOT), '--commit', COMMIT,
                        '--packages', str(HERE / 'records/packages/final'),
                        '--compare-packages', str(HERE / 'records/packages/consumer'),
                        '--compare-commit', CONSUMER_COMMIT, '--compare-dirty-crates', 'pbrs', 'pbrs-grpc',
                        '--output', str(output)], check=True, stdout=subprocess.PIPE)
        assert normalized_audit(json.loads(output.read_text())) == normalized_audit(audit)
    packaged = {row['git_path']: row['sha256'] for package in audit['archives'] for row in package['source_file_checks']}
    assert len(packaged) == 419
    verify_inputs(packaged)
    assert len(observed['archives']) == len(audit['archives']) == 3
    for package in observed['archives']:
        name = package['name']
        pair = next(row for row in audit['archives'] if row['name'] == name)
        consumer = HERE / 'records/packages/consumer' / Path(package['archive']).name
        assert sha(consumer.read_bytes()) == package['archive_sha256'] == pair['comparison']['sha256']
        assert package['vcs'] == pair['comparison']['comparison_vcs']
        assert package['source_file_checks'] == pair['source_file_checks']
    assert publisher['archives'] == [{key: row[key] for key in ['name', 'version', 'sha256', 'bytes', 'members']}
                                     for row in audit['archives']]

    wrapper = load('records/resource/wrapper.json')
    assert wrapper['schema'] == 'pbrs.readiness-check.v1'
    assert wrapper['base_commit'] == COMMIT and wrapper['exit_code'] == wrapper['process_returncode'] == 0
    assert wrapper['source_inputs_unchanged'] is True and wrapper['disk_guard'] is False
    assert sha(read('records/resource/wrapper.log')) == wrapper['log_sha256']
    verify_inputs(wrapper['inputs_sha256'])
    report = load('records/resource/report.json')
    assert report['source']['commit'] == COMMIT and report['source']['tree'] == tree
    assert report['source']['cargo_lock_sha256'] == inputs['Cargo.lock']
    assert report['duration_requested_seconds'] == 60 and report['build']['profile'] == 'release'
    assert report['smoke'] == {'status': 'passed', 'failures': []}
    assert report['qualification']['qualified'] is False
    assert report['qualification']['soak_24h']['status'] == 'not_run'
    validator = HERE / 'records/resource/grpc-resource-campaign.py'
    assert validator.read_bytes() == blob('scripts/grpc-resource-campaign.py')
    spec = importlib.util.spec_from_file_location('retained_resource_validator', validator)
    module = importlib.util.module_from_spec(spec)
    sys.dont_write_bytecode = True
    spec.loader.exec_module(module)
    assert module.validate_report(report) == []
    assert [json.loads(line) for line in read('records/resource/events.jsonl').splitlines()] == report['events']
    progress = load('records/resource/progress.json')
    assert progress['state'] == 'passed' and progress['qualified'] is False
    messages = [json.loads(line) for line in read('records/resource/build.stdout.jsonl').splitlines()]
    artifacts = [row for row in messages if row.get('reason') == 'compiler-artifact'
                 and row.get('executable') == report['binary']['build_path']]
    assert len(artifacts) == 1 and artifacts[0]['profile'] == report['build']['cargo_profile']
    assert artifacts[0]['target']['name'] == 'resource_qualification'
    assert re.search(rb'test result: ok\. 1 passed; 0 failed;', read('records/resource/test.stdout.log'))
    controls = load('records/checker-controls/report.json')
    assert controls['schema'] == 'pbrs.committed-followup-negative-controls.v1'
    assert controls['detected'] == len(controls['controls']) == 4
    assert {row['name'] for row in controls['controls']} == {
        'source-binding', 'publisher-dirty', 'resource-promotion', 'validator-pin'}
    for control in controls['controls']:
        assert control['exit_code'] != 0 and control['rehashed_manifest'] is True
        text = read('records/checker-controls/' + control['log'])
        assert sha(text) == control['log_sha256']
        assert b'AssertionError' in text and control['expected_failure'].encode() in text
    print(json.dumps({'source_commit': COMMIT, 'packet_files': len(found), 'selected_inputs': len(inputs),
                      'package_source_members': len(packaged), 'resource_seconds': report['duration_actual_seconds'],
                      'resource_cycles': max(event.get('cycle', 0) for event in report['events']), 'qualified': False}))


if __name__ == '__main__':
    main()
