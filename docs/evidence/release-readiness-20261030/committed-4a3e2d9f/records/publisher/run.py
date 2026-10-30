#!/usr/bin/env python3
"""Retain a bounded local packaging-only rehearsal at an explicit clean SHA."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time

ROOT = Path('/workspace/pure-protobuf')
OUT = ROOT/'work/release-rehearsal-final'
EXPECTED = '4a3e2d9f82edb8f0a94ae0f9f6a9bc500537607d'
OUT.mkdir(exist_ok=True)
assert not (OUT/'report.json').exists(), 'preserve prior rehearsal'


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT, text=True).strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


env = os.environ.copy()
tokens = ['CARGO_REGISTRY_TOKEN', 'CARGO_REGISTRIES_CRATES_IO_TOKEN', 'CRATES_IO_TOKEN']
present = any(bool(env.get(name)) for name in tokens)
for name in tokens:
    env.pop(name, None)
env.update(CARGO_HOME=str(ROOT/'work/cargo'), RUSTUP_HOME=str(ROOT/'work/rustup'),
           CARGO_TARGET_DIR=str(OUT/'target'), CARGO_BUILD_JOBS='1', CARGO_INCREMENTAL='0',
           CARGO_PROFILE_DEV_DEBUG='0', DRY_RUN='1', RELEASE_TAG='v0.2.1')
env['PATH'] = str(ROOT/'work/cargo/bin')+':'+str(ROOT/'work/toolchain/protoc/bin')+':'+env['PATH']
assert git('rev-parse', 'HEAD') == EXPECTED, 'unexpected HEAD'
assert not git('status', '--porcelain=v1'), 'requires clean tracked and untracked checkout'
paths = subprocess.check_output(
    ['git', 'ls-files', '-z', 'Cargo.toml', 'Cargo.lock', 'build.rs', 'src', 'README.md',
     'LICENSE-APACHE', 'LICENSE-MIT', 'vendor/google', 'protobuf-tonic', 'pbrs-grpc',
     'examples/greeter', 'clippy.toml', 'rustfmt.toml', 'scripts/publish-crates.sh'],
    cwd=ROOT).decode().split('\0')
inputs = {path: digest(ROOT/path) for path in paths if path and (ROOT/path).is_file()}
minimum = 750*1024*1024
command = ['bash', 'scripts/publish-crates.sh']
report = {'schema': 'pbrs.local-release-rehearsal.v1', 'source_commit': EXPECTED,
          'scope': 'offline package-only dry run; no upload, tag, SBOM, hosted attestation or version-status API check',
          'command': command, 'input_sha256': inputs, 'started_utc': now(),
          'disk_before': shutil.disk_usage(ROOT).free, 'disk_minimum_bytes': minimum,
          'disk_guard': False, 'status_before': git('status', '--porcelain=v1'),
          'environment': {name: env[name] for name in ['DRY_RUN', 'RELEASE_TAG', 'CARGO_HOME',
                          'RUSTUP_HOME', 'CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS', 'CARGO_INCREMENTAL',
                          'CARGO_PROFILE_DEV_DEBUG']},
          'registry_token_variables_present_before_removal': present,
          'registry_token_variables_forwarded': False,
          'tools': {name: subprocess.check_output(cmd, cwd=ROOT, env=env, text=True).strip()
                    for name, cmd in [('rustc', ['rustc', '-Vv']), ('cargo', ['cargo', '-V']),
                                      ('git', ['git', '--version']), ('python', [sys.executable, '--version'])]}}
assert report['disk_before'] > minimum
start = time.monotonic()
with (OUT/'publisher.log').open('wb') as log:
    proc = subprocess.Popen(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT,
                            start_new_session=True)
    report['pid'] = proc.pid
    (OUT/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    while True:
        try:
            exit_code = proc.wait(timeout=5)
            break
        except subprocess.TimeoutExpired:
            if shutil.disk_usage(ROOT).free < minimum:
                report['disk_guard'] = True
                os.killpg(proc.pid, signal.SIGTERM)
                try:
                    proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(proc.pid, signal.SIGKILL)
                    proc.wait()
                exit_code = 99
                break
report.update(exit=exit_code, finished_utc=now(), elapsed_seconds=time.monotonic()-start,
              disk_after=shutil.disk_usage(ROOT).free, head_after=git('rev-parse', 'HEAD'),
              status_after=git('status', '--porcelain=v1'),
              inputs_unchanged=all(digest(ROOT/p) == value for p,value in inputs.items()),
              publisher_log_sha256=digest(OUT/'publisher.log'))
(OUT/'report.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps({key:value for key,value in report.items() if key != 'input_sha256'}, indent=2))
assert report['head_after'] == EXPECTED and report['inputs_unchanged'] and not report['status_after']
raise SystemExit(exit_code)
