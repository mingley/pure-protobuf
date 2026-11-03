#!/usr/bin/env python3
"""Pack and audit local release archives at an explicitly supplied clean SHA.

Always uses DRY_RUN=1. No consumer execution, upload, tag, registry-status
request, SBOM or hosted attestation is performed by this wrapper.
"""
import argparse
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time
import tomllib

ROOT = next(path for path in Path(__file__).resolve().parents if (path / '.git').exists())
HERE = Path(__file__).resolve().parent
PACKAGES = [('pbrs', '', '0.2.1'), ('protobuf-tonic', 'protobuf-tonic', '0.1.0-alpha.3'),
            ('pbrs-grpc', 'pbrs-grpc', '0.1.0-alpha.3')]
MINIMUM_FREE = 750 * 1024 * 1024
PUBLISHER_SHA256 = 'be145322440f0bf942a51d699c2bd55238a6eb33d8ddf8c07d081879d5316943'
AUDITOR_SHA256 = '5e15129e3531f0ed9fff831d5125137fa6136ea23f15efbb7c5ddf30fdc7d73e'


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT, text=True).strip()


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def load_auditor():
    path = HERE / 'audit-release-archives.py'
    spec = importlib.util.spec_from_file_location('local_release_auditor', path)
    module = importlib.util.module_from_spec(spec)
    sys.dont_write_bytecode = True
    spec.loader.exec_module(module)
    return module


def freeze_consumers(source, destination, commit, audit):
    """Observe actual archive identities and compare all source to the new SHA."""
    destination.mkdir()
    rows, identities = [], set()
    dirty = []
    for name, base, version in PACKAGES:
        archive = source / f'{name}-{version}.crate'
        before = sha(archive)
        frozen = destination / archive.name
        shutil.copyfile(archive, frozen)
        assert sha(frozen) == before == sha(archive), 'consumer archive changed while copied'
        members = audit.load_archive(frozen, f'{name}-{version}')
        vcs = json.loads(members['.cargo_vcs_info.json'])
        assert set(vcs) == {'git', 'path_in_vcs'} and vcs['path_in_vcs'] == base
        info = vcs['git']
        assert set(info) in ({'sha1'}, {'sha1', 'dirty'})
        old_sha = info['sha1']
        assert re.fullmatch(r'[0-9a-f]{40}', old_sha)
        assert git('rev-parse', '--verify', old_sha + '^{commit}') == old_sha
        if 'dirty' in info:
            assert info['dirty'] is True, 'unexpected explicit consumer dirty field'
            dirty.append(name)
        identities.add(old_sha)
        normalized = tomllib.loads(members['Cargo.toml'].decode())
        assert normalized['package']['name'] == name and normalized['package']['version'] == version
        checks = []
        for path, data in sorted(members.items()):
            if path in {'Cargo.toml', 'Cargo.lock', '.cargo_vcs_info.json'}:
                continue
            git_path = str(Path(base) / ('Cargo.toml' if path == 'Cargo.toml.orig' else path))
            assert data == audit.git_blob(ROOT, commit, git_path), ('consumer source differs', name, path)
            checks.append({'archive_path': path, 'git_path': git_path,
                           'sha256': hashlib.sha256(data).hexdigest()})
        rows.append({'name': name, 'version': version, 'archive': str(frozen),
                     'archive_sha256': before, 'vcs': vcs, 'source_file_checks': checks})
    # Existing auditor takes one explicit comparison revision and per-crate
    # dirtiness. Mixed-revision consumers need separate reviewed handling.
    assert len(identities) == 1, 'consumer archives name mixed VCS revisions'
    return {'schema': 'pbrs.observed-consumer-packages.v1', 'observed_utc': now(),
            'comparison_commit': identities.pop(), 'dirty_crates': dirty,
            'source_members_checked': sum(len(row['source_file_checks']) for row in rows),
            'source_commit_compared': commit, 'archives': rows,
            'scope': 'archive observation and source equivalence only; no program execution or executable identity'}


def stop_group(proc):
    if proc is None or proc.poll() is not None:
        return
    os.killpg(proc.pid, signal.SIGTERM)
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--commit', required=True, help='full new clean commit SHA; root GO required')
    parser.add_argument('--out', type=Path, required=True, help='fresh ignored output directory')
    parser.add_argument('--consumer-packages', type=Path,
                        default=ROOT / 'target/package-consumer-pack/package')
    args = parser.parse_args()
    assert re.fullmatch(r'[0-9a-f]{40}', args.commit), 'full clean commit SHA required'
    assert git('rev-parse', 'HEAD') == args.commit
    assert not git('status', '--porcelain=v1', '--untracked-files=all'), 'clean checkout required'
    assert sha(ROOT / 'scripts/publish-crates.sh') == PUBLISHER_SHA256, 'publisher changed; review required'
    assert sha(HERE / 'audit-release-archives.py') == AUDITOR_SHA256, 'auditor changed; review required'
    out = args.out.resolve()
    assert out.is_relative_to(ROOT / 'work'), 'output must stay under ignored work/'
    assert not out.exists(), 'preserve earlier attempts'
    assert shutil.disk_usage(ROOT).free > MINIMUM_FREE
    for name, base, version in PACKAGES:
        manifest = tomllib.loads((ROOT / base / 'Cargo.toml').read_text())['package']
        assert manifest['name'] == name and manifest['version'] == version
    out.mkdir(parents=True)
    (out / 'tmp').mkdir()
    env = os.environ.copy()
    token_names = [key for key in env if key in {'CARGO_REGISTRY_TOKEN', 'CRATES_IO_TOKEN'}
                   or (key.startswith('CARGO_REGISTRIES_') and key.endswith('_TOKEN'))]
    token_present = any(bool(env.get(key)) for key in token_names)
    for key in token_names:
        env.pop(key, None)
    env.update(CARGO_HOME=str(ROOT / 'work/cargo'), RUSTUP_HOME=str(ROOT / 'work/rustup'),
               CARGO_TARGET_DIR=str(out / 'target'), CARGO_BUILD_JOBS='1', CARGO_INCREMENTAL='0',
               CARGO_PROFILE_DEV_DEBUG='0', DRY_RUN='1', RELEASE_TAG='v0.2.1', TMPDIR=str(out / 'tmp'))
    env['PATH'] = str(ROOT / 'work/cargo/bin') + ':' + str(ROOT / 'work/toolchain/protoc/bin') + ':' + env['PATH']
    paths = subprocess.check_output(
        ['git', 'ls-files', '-z', 'Cargo.toml', 'Cargo.lock', 'build.rs', 'src', 'README.md',
         'LICENSE-APACHE', 'LICENSE-MIT', 'vendor/google', 'protobuf-tonic', 'pbrs-grpc',
         'examples/greeter', 'clippy.toml', 'rustfmt.toml', 'scripts/publish-crates.sh'], cwd=ROOT
    ).decode().split('\0')
    inputs = {path: sha(ROOT / path) for path in paths if path and (ROOT / path).is_file()}
    report = {'schema': 'pbrs.local-release-rehearsal.v2', 'source_commit': args.commit,
              'source_tree': git('rev-parse', 'HEAD^{tree}'), 'stage': 'preflight',
              'scope': 'offline package-only rehearsal and source audit; no upload, tag, registry check, SBOM or hosted attestation',
              'started_utc': now(), 'input_sha256': inputs,
              'status_before': git('status', '--porcelain=v1', '--untracked-files=all'),
              'disk_before': shutil.disk_usage(ROOT).free, 'disk_minimum_bytes': MINIMUM_FREE,
              'disk_guard': False, 'wrapper_sha256': sha(Path(__file__)),
              'auditor_sha256': sha(HERE / 'audit-release-archives.py'),
              'publisher_sha256': PUBLISHER_SHA256,
              'environment': {key: env[key] for key in ['DRY_RUN', 'RELEASE_TAG', 'CARGO_TARGET_DIR',
                              'CARGO_HOME', 'RUSTUP_HOME', 'CARGO_BUILD_JOBS', 'CARGO_INCREMENTAL',
                              'CARGO_PROFILE_DEV_DEBUG', 'TMPDIR']},
              'registry_token_variables_present_before_removal': token_present,
              'registry_token_variables_forwarded': False,
              'tools': {name: subprocess.check_output(command, cwd=ROOT, env=env, text=True).strip()
                        for name, command in [('rustc', ['rustc', '-Vv']), ('cargo', ['cargo', '-V']),
                                              ('git', ['git', '--version']), ('python', [sys.executable, '--version'])]}}
    write(out / 'report.json', report)
    started = time.monotonic()
    proc = None
    try:
        observed = freeze_consumers(args.consumer_packages.resolve(), out / 'consumer-packages',
                                    args.commit, load_auditor())
        write(out / 'consumer-archives-observed.json', observed)
        report['consumer_observation_sha256'] = sha(out / 'consumer-archives-observed.json')
        command = ['bash', 'scripts/publish-crates.sh']
        report.update(stage='packing', command=command)
        write(out / 'report.json', report)
        with (out / 'publisher.log').open('wb') as log:
            proc = subprocess.Popen(command, cwd=ROOT, env=env, stdout=log,
                                    stderr=subprocess.STDOUT, start_new_session=True)
            report['publisher_pid'] = proc.pid
            write(out / 'report.json', report)
            while True:
                try:
                    code = proc.wait(timeout=5)
                    break
                except subprocess.TimeoutExpired:
                    if shutil.disk_usage(ROOT).free < MINIMUM_FREE:
                        report['disk_guard'] = True
                        stop_group(proc)
                        code = 99
                        break
        report['publisher_exit_code'] = code
        report['publisher_log_sha256'] = sha(out / 'publisher.log')
        assert code == 0, 'publisher packing failed'
        assert git('rev-parse', 'HEAD') == args.commit
        assert not git('status', '--porcelain=v1', '--untracked-files=all')
        audit_command = [sys.executable, str(HERE / 'audit-release-archives.py'),
                         '--root', str(ROOT), '--commit', args.commit,
                         '--packages', str(out / 'target/package'),
                         '--compare-packages', str(out / 'consumer-packages'),
                         '--compare-commit', observed['comparison_commit']]
        if observed['dirty_crates']:
            audit_command += ['--compare-dirty-crates', *observed['dirty_crates']]
        audit_command += ['--output', str(out / 'archive-audit.json')]
        report.update(stage='auditing', audit_command=audit_command)
        write(out / 'report.json', report)
        with (out / 'archive-audit.log').open('wb') as log:
            audited = subprocess.run(audit_command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        report['auditor_exit_code'] = audited.returncode
        report['auditor_log_sha256'] = sha(out / 'archive-audit.log')
        assert audited.returncode == 0, 'archive audit failed'
        audit = json.loads((out / 'archive-audit.json').read_text())
        assert audit['source_commit'] == args.commit and len(audit['archives']) == 3
        report['archive_audit_sha256'] = sha(out / 'archive-audit.json')
        report['archives'] = [{key: row[key] for key in ['name', 'version', 'sha256', 'bytes', 'members']}
                              for row in audit['archives']]
        report['shipped_source_members_checked'] = sum(len(row['source_file_checks']) for row in audit['archives'])
        report['stage'] = 'complete'
        report['exit_code'] = 0
    except BaseException as error:
        stop_group(proc)
        report.update(stage='failed', error_type=type(error).__name__, error=str(error), exit_code=1)
        raise
    finally:
        report.update(finished_utc=now(), elapsed_seconds=time.monotonic() - started,
                      disk_after=shutil.disk_usage(ROOT).free, head_after=git('rev-parse', 'HEAD'),
                      status_after=git('status', '--porcelain=v1', '--untracked-files=all'),
                      inputs_unchanged=all(sha(ROOT / path) == value for path, value in inputs.items()))
        if report['head_after'] != args.commit or report['status_after'] or not report['inputs_unchanged']:
            report['stage'] = 'failed'
            report['exit_code'] = 2
        write(out / 'report.json', report)
    print(json.dumps({key: value for key, value in report.items() if key != 'input_sha256'}, indent=2))
    return report['exit_code']


if __name__ == '__main__':
    raise SystemExit(main())
