#!/usr/bin/env python3
"""Build one runtime-only rlib and record allocation-only conversion controls."""
import argparse
import csv
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
INPUT_ROOTS = [
    'Cargo.toml', 'Cargo.lock', 'build.rs', 'clippy.toml', 'rustfmt.toml', '.cargo',
    'src', 'pbrs-grpc', 'protobuf-tonic', 'examples', 'tests', 'bench/devloop',
    'scripts', 'fuzz', 'vendor/google/conformance_fds.bin',
]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def snapshot():
    names = subprocess.check_output(
        ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z', '--', *INPUT_ROOTS],
        cwd=ROOT,
    ).decode().split('\0')
    return {name: sha(ROOT / name) for name in sorted(set(names))
            if name and (ROOT / name).is_file() and not (ROOT / name).is_symlink()}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    env.update(
        CARGO_HOME=str(ROOT / 'work/cargo'), RUSTUP_HOME=str(ROOT / 'work/rustup'),
        CARGO_TARGET_DIR=str(ROOT / 'target'), CARGO_BUILD_JOBS='1',
        CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0',
    )
    env['PATH'] = str(ROOT / 'work/cargo/bin') + ':' + env['PATH']
    report = {
        'schema': 'pbrs.byte-conversion-allocation-diagnostic.v1',
        'scope': 'same-process historical conversion expressions versus current runtime-only pbrs; requested Rust allocation counts/bytes, no CPU/RSS/ranking',
        'profile': 'dev, no default features, debug info disabled; no new dependencies',
        'historical_source_commit': '1c6ec8cacd547cce8c3a8e335320b49d34de40b4',
        'base_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'started_utc': datetime.now(timezone.utc).isoformat(),
        'host_machine': platform.machine(), 'host_system': platform.system(),
        'rustc': subprocess.check_output(['rustc', '-Vv'], env=env, cwd=ROOT, text=True),
        'cargo': subprocess.check_output(['cargo', '-V'], env=env, cwd=ROOT, text=True),
        'compiler_flags': {key: env[key] for key in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS',
                                                   'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
                                                   'CARGO_PROFILE_DEV_OPT_LEVEL') if key in env},
        'input_roots': INPUT_ROOTS,
        'input_scope': 'selected tracked and untracked compiler/test sources, not a filesystem read trace; runtime-only build skips conformance/protoc',
        'source_before': snapshot(),
        'harness_sha256': sha(Path(__file__).with_name('conversions.rs')),
        'runner_sha256': sha(Path(__file__)),
        'free_before': shutil.disk_usage(ROOT).free,
        'stages': [],
    }
    report_path = out / 'report.json'
    rustc_path = Path(subprocess.check_output(['rustup', 'which', 'rustc'], env=env, cwd=ROOT, text=True).strip())
    report['compiler_binary'] = {'path': str(rustc_path), 'sha256': sha(rustc_path)}

    def save():
        report_path.write_text(json.dumps(report, indent=2) + '\n')

    def run(command, stem):
        start = time.monotonic()
        with (out / (stem + '.stdout')).open('w') as stdout, (out / (stem + '.stderr')).open('w') as stderr:
            p = subprocess.Popen(command, env=env, cwd=ROOT, stdout=stdout, stderr=stderr)
            guard = False
            while p.poll() is None:
                if shutil.disk_usage(ROOT).free < 750 * 1024 * 1024:
                    guard = True
                    p.terminate()
                    p.wait(timeout=15)
                    break
                time.sleep(0.1)
        stage = {'command': command, 'exit_code': p.returncode,
                 'elapsed_seconds': time.monotonic() - start, 'disk_guard': guard,
                 'stdout': stem + '.stdout', 'stdout_sha256': sha(out / (stem + '.stdout')),
                 'stderr': stem + '.stderr', 'stderr_sha256': sha(out / (stem + '.stderr'))}
        report['stages'].append(stage)
        save()
        if p.returncode or guard:
            raise RuntimeError(f'{stem} failed: {p.returncode}, disk guard {guard}')

    save()
    source = Path(__file__).with_name('conversions.rs')
    shutil.copyfile(source, out / 'conversions.rs')
    for relative in ['src/lazy.rs', 'src/string.rs', 'tests/runtime.rs']:
        target = out / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / relative, target)
    (out / 'selected-source.patch').write_bytes(subprocess.check_output(
        ['git', 'diff', '--binary', 'HEAD', '--', *INPUT_ROOTS], cwd=ROOT))
    run(['cargo', 'build', '--locked', '--offline', '-p', 'pbrs', '--lib',
         '--no-default-features', '--message-format=json'], 'build')
    artifacts = []
    for line in (out / 'build.stdout').read_text().splitlines():
        event = json.loads(line)
        if event.get('reason') == 'compiler-artifact' and 'lib' in event['target']['kind']:
            for name in event['filenames']:
                path = Path(name)
                if path.suffix == '.rlib':
                    metadata = next(Path(filename) for filename in event['filenames'] if filename.endswith('.rmeta'))
                    artifacts.append({'package_id': event['package_id'], 'target': event['target']['name'],
                                      'path': str(path), 'sha256': sha(path), 'fresh': event['fresh'],
                                      'depfile': str(metadata.with_name(metadata.stem.removeprefix('lib') + '.d'))})
    report['rlibs'] = artifacts
    candidates = [x for x in artifacts if x['target'] == 'pbrs' and str(ROOT) in x['package_id']]
    assert len(candidates) == 1, candidates
    rlib = candidates[0]
    depfile = Path(rlib['depfile'])
    shutil.copyfile(depfile, out / 'compiler-inputs.d')
    deptext = depfile.read_text().replace('\\\n', '')
    compiled_names = set()
    for line in deptext.splitlines():
        if ': ' not in line or line.startswith('#'):
            continue
        _, deps = line.split(': ', 1)
        for name in shlex.split(deps):
            path = Path(name)
            if path.is_absolute():
                name = str(path.relative_to(ROOT))
            compiled_names.add(name)
    compiled_names.update(['Cargo.toml', 'Cargo.lock', 'build.rs', 'clippy.toml'])
    assert compiled_names and all(name in report['source_before'] for name in compiled_names), compiled_names
    report['compiled_input_scope'] = 'rustc depfile compiler-managed source dependencies plus package manifests/lock/build.rs/lints; not a buildscript or toolchain filesystem read trace'
    report['compiled_inputs_before'] = {name: report['source_before'][name] for name in sorted(compiled_names)}
    binary = out / 'conversions'
    run(['rustc', '--edition=2024', '--crate-name', 'byte_conversion_allocations',
         str(out / 'conversions.rs'), '--extern', 'pbrs=' + rlib['path'],
         '-L', 'dependency=' + str(ROOT / 'target/debug/deps'), '-o', str(binary)], 'harness-build')
    report['binary_sha256'] = sha(binary)
    captures = []
    for repeat in range(3):
        stem = 'repeat-' + str(repeat)
        run([str(binary)], stem)
        rows = list(csv.DictReader(io.StringIO((out / (stem + '.stdout')).read_text())))
        assert len(rows) == 60, len(rows)
        captures.append(rows)
    assert all(rows == captures[0] for rows in captures)
    report['rows_per_process'] = 60
    report['processes'] = 3
    report['all_repeats_identical'] = True
    report['source_after'] = snapshot()
    report['source_inputs_unchanged'] = report['source_before'] == report['source_after']
    report['compiled_inputs_after'] = {name: report['source_after'].get(name) for name in sorted(compiled_names)}
    report['compiled_inputs_unchanged'] = report['compiled_inputs_before'] == report['compiled_inputs_after']
    report['free_after'] = shutil.disk_usage(ROOT).free
    report['finished_utc'] = datetime.now(timezone.utc).isoformat()
    report['exit_code'] = 0 if report['compiled_inputs_unchanged'] else 2
    save()
    print(json.dumps({key: value for key, value in report.items()
                      if key not in ('source_before', 'source_after', 'rlibs', 'compiled_inputs_before', 'compiled_inputs_after')}, indent=2))
    return report['exit_code']


if __name__ == '__main__':
    raise SystemExit(main())
