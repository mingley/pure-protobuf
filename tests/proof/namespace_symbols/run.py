#!/usr/bin/env python3
"""Compile and run namespace controls in four native storage/default profiles."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def inputs():
    paths = subprocess.check_output(
        ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z', '--',
         'src', 'Cargo.toml', 'Cargo.lock', 'build.rs', 'clippy.toml',
         'rustfmt.toml', 'vendor/google/conformance_fds.bin',
         'tests/proof/namespace_symbols'], cwd=ROOT,
    ).decode().split('\0')
    return {
        name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
        for name in sorted(set(paths)) if name and (ROOT / name).is_file()
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    before = inputs()
    rows = []
    passed = True
    for runtime in ['default', 'bare', 'absolute', 'local']:
        env['PBRS_NS_RUNTIME'] = runtime
        for profile, features in [('lazy', ''), ('owned', 'owned'), ('safe', 'safe'), ('owned-safe', 'owned,safe')]:
            command = ['cargo', 'run', '--locked', '--offline', '--manifest-path', str(HERE / 'Cargo.toml')]
            if features:
                command += ['--features', features]
            result = subprocess.run(command, cwd=ROOT, env=env, text=True, capture_output=True)
            (out / f'{runtime}-{profile}.stdout').write_text(result.stdout)
            (out / f'{runtime}-{profile}.stderr').write_text(result.stderr)
            checks = [line for line in result.stdout.splitlines() if line.startswith('PASS ')]
            observations = [line for line in result.stdout.splitlines() if line.startswith('OBS ')]
            valid = result.returncode == 0 and len(checks) == 4 and len(observations) == 1
            rows.append({'profile': profile, 'runtime': runtime, 'command': command, 'exit_code': result.returncode,
                         'checks': checks, 'observations': observations, 'passed': valid})
            passed &= valid
            if not valid:
                break
        if not passed:
            break
    negative = None
    if passed:
        env['PBRS_NS_RUNTIME'] = 'default'
        command = ['cargo', 'check', '--locked', '--offline', '--manifest-path',
                   str(HERE / 'Cargo.toml'), '--features', 'bare-legacy']
        result = subprocess.run(command, cwd=ROOT, env=env, text=True, capture_output=True)
        (out / 'bare-legacy.stdout').write_text(result.stdout)
        (out / 'bare-legacy.stderr').write_text(result.stderr)
        expected = result.returncode != 0 and 'E0277' in result.stderr and 'Default' in result.stderr
        negative = {'command': command, 'exit_code': result.returncode,
                    'expected_failure': True, 'passed': expected}
        passed &= expected
    after = inputs()
    report = {
        'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'finished_utc': datetime.now(timezone.utc).isoformat(),
        'scope': 'Compiled namespace and functional wire controls; no RPC, performance or universal compiler-parity claim.',
        'profiles': rows, 'bare_legacy_control': negative, 'source_inputs_sha256': before,
        'source_inputs_unchanged': before == after,
        'changed_source_files': sorted(name for name in set(before) | set(after) if before.get(name) != after.get(name)),
        'passed': passed and before == after and len(rows) == 16,
    }
    (out / 'results.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: value for key, value in report.items() if key != 'source_inputs_sha256'}, indent=2))
    if not report['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
