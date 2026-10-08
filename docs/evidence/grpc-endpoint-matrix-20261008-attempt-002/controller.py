"""Build a frozen benchmark, retain endpoint captures, and publish to main."""
import argparse
import gzip
import hashlib
import importlib.util
import io
import itertools
import json
import os
from pathlib import Path
import platform
import random
import shutil
import subprocess
import tarfile
import time
from types import SimpleNamespace


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(argv, cwd, output, name):
    start = time.time()
    with (output / (name + '.stdout')).open('w') as stdout, (output / (name + '.stderr')).open('w') as stderr:
        result = subprocess.run(argv, cwd=cwd, stdout=stdout, stderr=stderr, check=False)
    return {'command': list(map(str, argv)), 'exit_code': result.returncode,
            'elapsed_seconds': time.time() - start}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source', required=True)
    p.add_argument('--source-dir', type=Path, required=True)
    p.add_argument('--root', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--evidence-path', type=Path, required=True)
    p.add_argument('--valgrind', type=Path, required=True)
    args = p.parse_args()
    source, root, output = args.source_dir.resolve(), args.root.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    shutil.copy2(__file__, output / 'controller.py')
    state = {'source': args.source, 'state': 'building', 'qualified': False, 'commands': []}
    concurrent = root / 'work/campaign-v4-004/controller-status.json'
    if concurrent.is_file():
        state['concurrent_resource_campaign_at_start'] = json.loads(concurrent.read_text())

    def save():
        (output / 'controller-status.json').write_text(json.dumps(state, indent=2) + '\n')

    def execute(argv, name):
        state['state'] = name
        save()
        result = run(argv, source, output, name)
        state['commands'].append(result)
        save()
        return result['exit_code']

    build = output / 'build'
    ok = execute(['python3', str(source / 'scripts/build-rpc-bench.py'), '--source', args.source,
                  '--output', str(build)], 'build') == 0
    if ok:
        binary = build / 'rpc-bench'
        functional = output / 'functional'
        execute(['python3', str(source / 'scripts/grpc-load-smoke.py'), '--binary', str(binary),
                 '--build-record', str(build / 'build.json'), '--output', str(functional),
                 '--rpc-count', '20', '--duration', '60', '--allocation-counts', '--context-switches',
                 '--load-levels', '1:1,1:16', '--seed', '20261008'], 'functional')
        # N/2N OS counters run without Callgrind. Instrumented process context
        # switches describe Valgrind's scheduling and cannot replace these rows.
        spec = importlib.util.spec_from_file_location('smoke', source / 'scripts/grpc-load-smoke.py')
        smoke = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(smoke)
        pairs = [['native', 'pbrs', 'tonic', 'prost'], ['native', 'prost', 'tonic', 'prost'],
                 ['tonic', 'prost', 'native', 'pbrs'], ['tonic', 'prost', 'native', 'prost'],
                 ['tonic', 'prost', 'tonic', 'prost']]
        cells = [dict(pair=pair, shape=shape, payload_bytes=1024, tls=False, compression='identity',
                      connections=1, in_flight=1, repeat=repeat)
                 for pair, shape, repeat in itertools.product(pairs, smoke.SHAPES, range(3))]
        random.Random(20261008).shuffle(cells)
        for instrumented in (False, True):
            captures = []
            for n in (200, 400):
                label = f'{"callgrind" if instrumented else "native-counters"}-{n}'
                state['state'] = label
                save()
                directory = output / label
                directory.mkdir()
                captures.append(directory)
                build_record = smoke.verified_build(build / 'build.json', binary)
                settings = SimpleNamespace(binary=binary, duration=60, rpc_count=n, allocation_counts=True,
                                           context_switches=not instrumented,
                                           callgrind=args.valgrind if instrumented else None)
                report = dict(schema='pbrs.load-smoke.v2', binary_sha256=sha(binary),
                              head=args.source, dirty=False, source_verified=True, build_record=build_record,
                              host=platform.uname()._asdict(), cpu_affinity=sorted(os.sched_getaffinity(0)),
                              seed=20261008, cells=cells, rpc_count=n, duration_per_cell_seconds=60,
                              allocation_counts=True, context_switches=not instrumented,
                              callgrind=str(args.valgrind) if instrumented else None,
                              qualification={'qualified': False, 'limits': [
                                  'shared host; no dedicated timing, headroom or noise qualification',
                                  'resource campaign may run concurrently; no isolated-host attribution',
                                  'one KiB h2c identity at concurrency one; no read-all corpus or saturation',
                                  'process totals include setup; OS switches do not measure task wakes',
                                  'Callgrind captures are separate from native OS scheduling counters']}, runs=[])
                (directory / 'manifest.json').write_text(json.dumps(report, indent=2) + '\n')
                for i, cell in enumerate(cells):
                    cell_dir = directory / f'cell-{i:03d}'
                    result = smoke.run_cell(settings, cell, cell_dir)
                    report['runs'].append({'path': f'cell-{i:03d}/run.json', 'passed': result['passed']})
                    for profile in cell_dir.glob('*.callgrind'):
                        with profile.open('rb') as original, gzip.open(str(profile) + '.gz', 'wb') as compressed:
                            shutil.copyfileobj(original, compressed)
                        profile.unlink()
                    print(f'{label}: {i + 1}/{len(cells)} {result["passed"]}', flush=True)
                report['binary_unchanged'] = report['binary_sha256'] == sha(binary)
                try:
                    smoke.verified_build(build / 'build.json', binary)
                    report['source_unchanged'] = True
                except (OSError, ValueError, subprocess.CalledProcessError) as error:
                    report['source_unchanged'] = False
                    report['source_error'] = str(error)
                report['passed'] = report['binary_unchanged'] and report['source_unchanged'] and all(r['passed'] for r in report['runs'])
                (directory / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
            execute(['python3', str(source / 'scripts/dominance-ledger.py'), '--small', str(captures[0]),
                     '--large', str(captures[1]), '--output', str(output / ('callgrind-ledger' if instrumented else 'native-ledger')),
                     '--allow-partial', '--allow-failed-captures'], 'ledger-callgrind' if instrumented else 'ledger-native')
    state['state'] = 'publishing'
    save()
    subprocess.run(['git', 'fetch', 'origin', 'main'], cwd=root, check=True)
    checkout = output / 'publication-checkout'
    subprocess.run(['git', 'worktree', 'add', '--detach', str(checkout), 'origin/main'], cwd=root, check=True)
    relative = args.evidence_path
    if len(relative.parts) != 3 or relative.parts[:2] != ('docs', 'evidence') or '..' in relative.parts:
        raise ValueError('publication must have its own directory under docs/evidence')
    destination = checkout / relative
    started = json.loads((destination / 'started.json').read_text())
    if started['source_commit'] != args.source or started['controller_sha256'] != sha(output / 'controller.py'):
        raise ValueError('publication source/controller mismatch')
    files = [f for f in sorted(output.rglob('*')) if f.is_file() and checkout not in f.parents
             and f != build / 'rpc-bench']
    entries = [{'path': str(f.relative_to(output)), 'bytes': f.stat().st_size, 'sha256': sha(f)} for f in files]
    with (destination / 'raw.tar.gz').open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', filename='', mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode='w') as archive:
            for f, entry in zip(files, entries):
                info = tarfile.TarInfo(entry['path'])
                info.size, info.mode = entry['bytes'], 0o644
                with f.open('rb') as stream:
                    archive.addfile(info, stream)
    manifest = {'archive': {'name': 'raw.tar.gz', 'sha256': sha(destination / 'raw.tar.gz')},
                'files': entries, 'logical_bytes': sum(e['bytes'] for e in entries), 'qualified': False}
    (destination / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    shutil.copy2(source / 'docs/evidence/grpc-readiness-20261008/check.py', destination / 'check.py')
    shutil.copy2(output / 'controller-status.json', destination / 'outcome.json')
    (destination / 'README.md').write_text('# Endpoint matrix outcome\n\nSource `' + args.source + '`. The raw capsule retains the source-pinned release build, functional cells, native N/2N endpoint counters, separate Callgrind captures, and all failed cells. Overall qualification remains false.\n\nThe requested functional matrix covers 2,560 cells at 20 RPCs per cell, with a 60-second cutoff. This checks wiring and completion, not saturation. The N/2N captures cover five endpoint pairings, five shapes, one KiB h2c identity, concurrency one, and three repeats. Read-all corpora, cold/idle, higher loads, task wakeups, syscalls, dedicated-host statistics, and measured noise bounds remain open. See outcome.json and archived report/ledger files for actual results; a requested command is not a successful measurement.\n\nRun `python3 check.py` to verify every retained file.\n')
    subprocess.run(['python3', str(destination / 'check.py')], cwd=checkout, check=True)
    subprocess.run(['git', 'add', str(relative)], cwd=checkout, check=True)
    subprocess.run(['git', 'diff', '--cached', '--check'], cwd=checkout, check=True)
    subprocess.run(['git', 'commit', '-m', 'docs(evidence): publish pinned endpoint matrix outcomes'], cwd=checkout, check=True)
    for _ in range(3):
        result = subprocess.run(['git', 'push', 'origin', 'HEAD:main'], cwd=checkout, check=False)
        if result.returncode == 0:
            state['state'] = 'finished'
            state['publication_commit'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=checkout, text=True).strip()
            save()
            return
        subprocess.run(['git', 'fetch', 'origin', 'main'], cwd=checkout, check=True)
        subprocess.run(['git', 'rebase', 'origin/main'], cwd=checkout, check=True)
    raise RuntimeError('main rejected publication; local commit and captures retained')


if __name__ == '__main__':
    main()
