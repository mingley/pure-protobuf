#!/usr/bin/env python3
"""Check retained local package and bounded resource evidence against Git."""
import datetime
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = next(path for path in HERE.parents if (path/'.git').exists())
COMMIT = '4a3e2d9f82edb8f0a94ae0f9f6a9bc500537607d'
EARLIER = '26389ccf77bf9342fe6e605f1d797853acd6c39a'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read(path):
    return (HERE/path).read_bytes()


def load(path):
    return json.loads(read(path))


def blob(path):
    return subprocess.check_output(['git', 'show', f'{COMMIT}:{path}'], cwd=ROOT)


def verify_inputs(inputs):
    for path, digest in inputs.items():
        assert sha(blob(path)) == digest, ('committed input differs', path)


def normalized_audit(audit):
    # Files moved into this packet; comparison semantics and bytes must match.
    audit = json.loads(json.dumps(audit))
    for row in audit['archives']:
        row['archive'] = Path(row['archive']).name
        row['comparison']['archive'] = Path(row['comparison']['archive']).name
    return audit


def main():
    manifest = load('manifest.json')
    assert manifest['schema'] == 'pbrs.committed-readiness-packet.v1'
    assert manifest['source_commit'] == COMMIT and manifest['qualified'] is False
    found = {str(path.relative_to(HERE)) for path in HERE.rglob('*') if path.is_file()
             and path != HERE/'manifest.json'}
    assert found == set(manifest['files']), 'packet member set differs'
    for path, record in manifest['files'].items():
        data = read(path)
        assert len(data) == record['bytes'] and sha(data) == record['sha256'], path

    binding = load('records/committed-source-binding.json')
    assert binding['source_commit'] == COMMIT and binding['inputs_checked'] == 464
    assert binding['mismatches'] == []
    tree = subprocess.check_output(['git', 'rev-parse', COMMIT+'^{tree}'], cwd=ROOT, text=True).strip()
    assert tree == binding['source_tree']
    inputs = {row['path']: row['sha256'] for row in binding['inputs']}
    assert len(inputs) == 464
    verify_inputs(inputs)
    previous = 'docs/evidence/release-readiness-20261030/validation/'
    assert sha(blob(previous+'manifest.json')) == binding['validation_manifest_sha256']
    archive = blob(previous+'raw.tar.gz')
    assert sha(archive) == binding['validation_archive_sha256']
    with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as tar:
        snapshot = json.load(tar.extractfile('source-inputs.json'))
    assert snapshot['inputs_sha256'] == inputs

    publisher = load('records/publisher/report.json')
    assert publisher['source_commit'] == COMMIT == publisher['head_after']
    assert publisher['exit'] == 0 and not publisher['disk_guard']
    assert publisher['status_before'] == publisher['status_after'] == ''
    assert publisher['inputs_unchanged'] and not publisher['registry_token_variables_forwarded']
    assert publisher['environment']['DRY_RUN'] == '1'
    assert publisher['environment']['RELEASE_TAG'] == 'v0.2.1'
    assert sha(read('records/publisher/publisher.log')) == publisher['publisher_log_sha256']
    verify_inputs(publisher['input_sha256'])
    registry = load('records/registry-observation.json')
    assert registry['source'] == 'public crates.io API'
    assert datetime.datetime.fromisoformat(registry['observed_utc']) == datetime.datetime.fromisoformat(
        '2026-10-30T19:24:15.447173+00:00')
    expected_versions = {'pbrs': ('0.2.1', '0.2.0'),
                         'pbrs-grpc': ('0.1.0-alpha.3', '0.1.0-alpha.2'),
                         'protobuf-tonic': ('0.1.0-alpha.3', '0.1.0-alpha.2')}
    assert {row['crate'] for row in registry['crates']} == set(expected_versions)
    for row in registry['crates']:
        assert row['status'] == 'observed' and row['candidate_present'] is False
        assert (row['candidate'], row['max_version']) == expected_versions[row['crate']]
    audit = load('records/publisher/archive-audit.json')
    auditor = HERE/'records/publisher/audit-release-archives.py'
    assert sha(auditor.read_bytes()) == audit['auditor_sha256']
    with tempfile.TemporaryDirectory(prefix='pbrs-package-audit-') as temp:
        output = Path(temp)/'audit.json'
        subprocess.run([sys.executable, str(auditor), '--root', str(ROOT), '--commit', COMMIT,
                        '--packages', str(HERE/'records/packages/final'),
                        '--compare-packages', str(HERE/'records/packages/consumer'),
                        '--compare-commit', EARLIER, '--compare-dirty-crates', 'pbrs-grpc',
                        '--output', str(output)], check=True, stdout=subprocess.PIPE)
        assert normalized_audit(json.loads(output.read_text())) == normalized_audit(audit)
    packaged = {row['git_path']: row['sha256'] for package in audit['archives']
                for row in package['source_file_checks']}
    assert len(packaged) == 419
    consumer = load('records/consumer-all-current-source-check.json')
    assert consumer['all_source_members_equal']
    selected = {row['path']: row['sha256'] for package in consumer['archives'] for row in package['files']}
    assert selected == packaged and all(not p['mismatches'] for p in consumer['archives'])
    for package in consumer['archives']:
        path = HERE/'records/packages/consumer'/Path(package['archive']).name
        assert sha(path.read_bytes()) == package['archive_sha256']
        assert all(row['equal'] for row in package['files'])
    narrow = load('records/consumer-selected-current-source-check.json')
    assert narrow['all_equal'] and all(row['equal'] for row in narrow['files'])
    for row in narrow['files']:
        assert row['archive_sha256'] == row['current_sha256'] == packaged['pbrs-grpc/'+row['path']]

    docs = load('records/docs-contract/report.json')
    assert docs['exit_code'] == 0 and docs['source_inputs_unchanged']
    assert docs['inputs_sha256'] == inputs
    assert sha(read('records/docs-contract/test.log')) == docs['log_sha256']
    assert re.search(rb'test result: ok\. 27 passed; 0 failed;', read('records/docs-contract/test.log'))

    wrapper = load('records/resource/wrapper.json')
    assert wrapper['base_commit'] == COMMIT and wrapper['exit_code'] == 0
    assert wrapper['source_inputs_unchanged'] and wrapper['inputs_sha256'] == inputs
    assert sha(read('records/resource/wrapper.log')) == wrapper['log_sha256']
    resource = load('records/resource/report.json')
    assert resource['source']['commit'] == COMMIT and resource['source']['tree'] == tree
    assert resource['source']['cargo_lock_sha256'] == inputs['Cargo.lock']
    assert resource['duration_requested_seconds'] == 60
    assert resource['build']['profile'] == 'release'
    assert resource['smoke'] == {'status': 'passed', 'failures': []}
    assert resource['qualification']['qualified'] is False
    assert resource['qualification']['soak_24h']['status'] == 'not_run'
    validator_path = HERE/'records/resource/grpc-resource-campaign.py'
    assert validator_path.read_bytes() == blob('scripts/grpc-resource-campaign.py')
    spec = importlib.util.spec_from_file_location('pbrs_retained_resource_validator', validator_path)
    module = importlib.util.module_from_spec(spec)
    sys.dont_write_bytecode = True
    spec.loader.exec_module(module)
    assert module.validate_report(resource) == []
    events = [json.loads(line) for line in read('records/resource/events.jsonl').splitlines()]
    assert events == resource['events']
    progress = load('records/resource/progress.json')
    assert progress['state'] == 'passed' and progress['qualified'] is False
    messages = [json.loads(line) for line in read('records/resource/build.stdout.jsonl').splitlines()]
    artifact = [row for row in messages if row.get('reason') == 'compiler-artifact'
                and row.get('executable') == resource['binary']['build_path']]
    assert len(artifact) == 1 and artifact[0]['profile'] == resource['build']['cargo_profile']
    assert artifact[0]['target']['name'] == 'resource_qualification'
    assert re.search(rb'test result: ok\. 1 passed; 0 failed;', read('records/resource/test.stdout.log'))
    print(json.dumps({'source_commit': COMMIT, 'packet_files': len(found),
                      'validation_inputs': len(inputs), 'package_source_members': len(packaged),
                      'resource_smoke': 'passed', 'duration_seconds': resource['duration_actual_seconds'],
                      'qualified': False}))


if __name__ == '__main__':
    main()
