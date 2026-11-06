#!/usr/bin/env python3
"""Compile forbid-unsafe consumers and retain the intentional legacy failure."""
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
         'tests/proof/safe_defaults'], cwd=ROOT,
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
    env.setdefault('CARGO_TARGET_DIR', str(ROOT / 'target'))
    before = inputs()
    rows = []
    for profile, features in [('lazy', ''), ('owned', 'owned'), ('boxed', 'boxed')]:
        command = ['cargo', 'run', '--locked', '--offline', '--manifest-path', str(HERE / 'Cargo.toml')]
        if features:
            command += ['--features', features]
        result = subprocess.run(command, cwd=ROOT, env=env, text=True, capture_output=True)
        (out / f'{profile}.stdout').write_text(result.stdout)
        (out / f'{profile}.stderr').write_text(result.stderr)
        checks = [line for line in result.stdout.splitlines() if line.startswith('PASS ')]
        row = {'profile': profile, 'command': command, 'exit_code': result.returncode, 'checks': checks}
        rows.append(row)
        assert result.returncode == 0 and len(checks) == 4, (row, result.stderr)
    command = ['cargo', 'check', '--locked', '--offline', '--manifest-path', str(HERE / 'Cargo.toml'), '--features', 'forbid-legacy']
    result = subprocess.run(command, cwd=ROOT, env=env, text=True, capture_output=True)
    (out / 'forbid-legacy.stdout').write_text(result.stdout)
    (out / 'forbid-legacy.stderr').write_text(result.stderr)
    assert result.returncode != 0 and ('unsafe_code' in result.stderr or 'unsafe block' in result.stderr), result.stderr
    rows.append({'profile': 'forbid-legacy', 'command': command, 'exit_code': result.returncode, 'expected_failure': True})
    after = inputs()
    report = {
        'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'finished_utc': datetime.now(timezone.utc).isoformat(),
        'scope': 'Generated bindings compile with consumer forbid(unsafe_code); runtime dependencies retain their own lint policy. Selected source inventory is not a filesystem read trace.',
        'profiles': rows, 'source_inputs_sha256': before,
        'source_inputs_unchanged': before == after,
        'changed_source_files': sorted(name for name in set(before) | set(after) if before.get(name) != after.get(name)),
    }
    (out / 'results.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: value for key, value in report.items() if key != 'source_inputs_sha256'}, indent=2))
    if before != after:
        raise SystemExit(2)


if __name__ == '__main__':
    main()
