#!/usr/bin/env python3
"""Capture actual TC32 API costs; execute only in a coordinator measurement lease."""
import argparse
import gzip
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('tc32_audit', HERE / 'audit-tc32-bridges.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args], text=True).strip()


def frozen(root, source, binary, expected):
    audit.require(git(root, 'rev-parse', 'HEAD') == source, 'checked-out source changed')
    audit.require(not git(root, 'status', '--porcelain', '--untracked-files=normal'), 'source is not clean')
    audit.require(audit.sha(binary) == expected, 'binary changed')


def preflight(binary, out):
    result = subprocess.run([str(binary), 'bridge-inventory'], capture_output=True, text=True, timeout=120, check=False)
    (out / 'inventory.stdout').write_text(result.stdout)
    (out / 'inventory.stderr').write_text(result.stderr)
    audit.require(result.returncode == 0, 'actual API inventory failed')
    rows = audit.inventory_rows(json.loads(result.stdout))
    (out / 'inventory.json').write_text(result.stdout)
    result = subprocess.run([str(binary), 'list'], capture_output=True, text=True, timeout=30, check=False)
    (out / 'registry.stderr').write_text(result.stderr)
    audit.require(result.returncode == 0, 'parent registry failed')
    (out / 'registry.txt').write_text(result.stdout)
    audit.require({line.split()[0] for line in result.stdout.splitlines() if line.startswith(audit.PREFIX)} == audit.expected_ids(), 'parent bridge registry incomplete')
    return rows, result.stdout.splitlines()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--build-pin', required=True, type=Path, help='source_commit, binary_sha256, profile=release, and recorded tool/source provenance')
    parser.add_argument('--original-registry', required=True, type=Path, help='retained feature-off parent list; old IDs/order must be identical')
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--iters', type=int, default=16)
    parser.add_argument('--cells', help='comma-separated eligible calibration cells; never a full-coverage result')
    parser.add_argument('--valgrind', type=Path)
    args = parser.parse_args()
    root = HERE.parent
    binary = args.binary.resolve()
    pin = json.loads(args.build_pin.read_text())
    source, expected = pin['source_commit'], pin['binary_sha256']
    audit.validate_build_pin(pin)
    for path, expected_source in pin['source_sha256'].items():
        audit.require(audit.sha(root / path) == expected_source, 'source pin drift: ' + path)
    for tool in pin['tools'].values():
        audit.require(audit.sha(tool['path']) == tool['sha256'], 'compiler/tool pin drift')
    audit.require(git(root / 'third_party/protobuf', 'rev-parse', 'HEAD') == pin['protobuf_source_commit'], 'upstream source pin drift')
    audit.require(args.iters > 0, 'N must be positive')
    frozen(root, source, binary, expected)
    args.out.mkdir(parents=True, exist_ok=False)
    out = args.out.resolve()
    (out / 'graphs').mkdir()
    shutil.copyfile(args.build_pin, out / 'build-pin.json')
    shutil.copyfile(args.original_registry, out / 'original-registry.txt')
    reference = root / 'docs/evidence/tc32a-bridge-qualification-20261002/bridge-inventory.json'
    audit.require(audit.sha(reference) == audit.REFERENCE_SHA256, 'retained TC32a oracle changed')
    shutil.copyfile(reference, out / 'reference-inventory.json')
    meta = {'schema': 'tc32-bridge-cost/1', 'source_commit': source, 'binary': {'path': str(binary), 'sha256': expected},
            'build_pin': pin, 'iters': args.iters, 'prepare_iters': 2 * args.iters, 'warmup': 100, 'repeats': 3,
            'completed': False, 'tools': {}, 'full_eligible_coverage': False,
            'allocation_lifecycle': 'fully read borrowed source to owned converted target, optional full target walk, owned target drop inside existing AllocGuard window; source setup and oracles outside',
            'instructions': 'absolute actual API N/2N differential; no prost-minus-native arithmetic baseline',
            'existing_full_controls': 'not_run by this driver; historical failed frozen campaigns remain failed/unqualified',
            'performance_qualification': 'not established; source/collector qualification and separate unchanged-binary/full controls required',
            'wall': 'shared-host diagnostic only; no headroom/latency claim',
            'driver_sha256': audit.sha(Path(__file__)), 'auditor_sha256': audit.sha(HERE / 'audit-tc32-bridges.py')}
    raw = out / 'raw.jsonl'
    start = time.monotonic()
    try:
        rows, registry = preflight(binary, out)
        original = args.original_registry.read_text().splitlines()
        audit.require([line for line in registry if not line.startswith(audit.PREFIX)] == original, 'old registry/order changed')
        eligible = sorted(row for row, q in rows.items() if q['timing_qualification'] == 'passed')
        selected = args.cells.split(',') if args.cells else eligible
        audit.require(selected and len(set(selected)) == len(selected) and set(selected) <= set(eligible), 'invalid calibration/blocked selection')
        valgrind = str(args.valgrind.resolve()) if args.valgrind else shutil.which('valgrind')
        if valgrind:
            version = subprocess.run([valgrind, '--version'], capture_output=True, text=True, timeout=30, check=False)
            (out / 'valgrind-version.txt').write_text(version.stdout + version.stderr)
            audit.require(version.returncode == 0, 'configured Valgrind failed version probe')
            meta['tools']['valgrind'] = {'path': valgrind, 'sha256': audit.sha(valgrind), 'version': version.stdout.strip()}
        meta.update(selected_cells=selected, original_registry_rows=original,
                    full_eligible_coverage=set(selected) == set(eligible), callgrind_available=bool(valgrind),
                    blocked_maps=[row for row in rows if rows[row]['timing_qualification'] == 'blocked'])
        (out / 'metadata.json').write_text(json.dumps(meta, indent=2) + '\n')
        with raw.open('w') as ledger:
            for cell in selected:
                for repeat in range(3):
                    for n in (args.iters, 2 * args.iters):
                        for collector in (['allocator', 'callgrind'] if valgrind else ['allocator']):
                            stem = f'{cell}.{n}.repeat{repeat}'
                            graph = out / 'graphs' / (stem + '.callgrind')
                            command = [str(binary), 'run-cell', cell, '--iters', str(n), '--prepare-iters', str(2 * args.iters), '--warmup', '100']
                            if collector == 'callgrind':
                                command = [valgrind, '--tool=callgrind', '--cache-sim=no', '--callgrind-out-file=' + str(graph)] + command
                            record = {'cell': cell, 'iters': n, 'repeat': repeat, 'collector': collector, 'command': command, 'validated': False}
                            try:
                                frozen(root, source, binary, expected)
                                record['launch_sha256'] = audit.sha(binary)
                                if valgrind:
                                    audit.require(audit.sha(valgrind) == meta['tools']['valgrind']['sha256'], 'instruction tool drift')
                                audit.require(shutil.disk_usage(out).free >= 2 * 1024 ** 3, 'disk free reserve below 2 GiB; capture stopped')
                                result = subprocess.run(command, capture_output=True, text=True, timeout=600, check=False, env=dict(os.environ, LC_ALL='C'))
                                record.update(returncode=result.returncode, stdout=result.stdout, stderr=result.stderr)
                                record['after_sha256'] = audit.sha(binary)
                                audit.require(record['after_sha256'] == expected and result.returncode == 0, 'failed/drifting child')
                                child = audit.one_json(result.stderr, '__CHILD__ ')
                                qualification = audit.one_json(result.stderr, '__QUALIFICATION__ ')
                                audit.require(qualification == rows[cell], 'child actual API qualification differs from preflight')
                                audit.require(child['cell'] == cell and child['iters'] == n and child.get('input_wire_fingerprint') == rows[cell]['wire_fingerprint'], 'child ID/N/actual-source drift')
                                audit.require(all(type(child[k]) is int and child[k] >= 0 for k in ('allocs', 'alloc_bytes', 'wall_ns')), 'invalid child counters')
                                record.update(child=child, qualification=qualification)
                                if collector == 'callgrind':
                                    uncompressed = graph.read_bytes()
                                    zipped = graph.with_suffix(graph.suffix + '.gz')
                                    # Deterministic lossless compression preserves every graph and
                                    # records both compressed and original hashes to bound disk usage.
                                    zipped.write_bytes(gzip.compress(uncompressed, mtime=0))
                                    graph.unlink()
                                    total, original_sha = audit.graph_total(zipped)
                                    record.update(graph=str(zipped.relative_to(out)), graph_sha256=audit.sha(zipped),
                                                  uncompressed_graph_sha256=original_sha, instruction_total=total)
                                record['validated'] = True
                            except subprocess.TimeoutExpired as error:
                                record.update(returncode=None, error=str(error), stdout=error.stdout.decode('utf-8', errors='replace') if isinstance(error.stdout, bytes) else (error.stdout or ''), stderr=error.stderr.decode('utf-8', errors='replace') if isinstance(error.stderr, bytes) else (error.stderr or ''))
                            except Exception as error:
                                record['error'] = str(error)
                            ledger.write(json.dumps(record) + '\n')
                            ledger.flush()
                            audit.require(record['validated'], record.get('error', 'failed child'))
                print('completed ' + cell, flush=True)
        frozen(root, source, binary, expected)
        meta.update(completed=True, elapsed_seconds_diagnostic=time.monotonic() - start,
                    artifact_sha256={name: audit.sha(out / name) for name in ('raw.jsonl', 'inventory.json', 'registry.txt', 'build-pin.json', 'original-registry.txt', 'reference-inventory.json')})
        (out / 'metadata.json').write_text(json.dumps(meta, indent=2) + '\n')
        report = audit.rebuild(out)
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(f'validated {report["raw_children"]} raw children; full eligible coverage={report["full_eligible_coverage"]}; numeric results await unchanged-binary/full controls')
    except Exception as error:
        meta.update(completed=False, error=str(error), elapsed_seconds_diagnostic=time.monotonic() - start)
        (out / 'metadata.json').write_text(json.dumps(meta, indent=2) + '\n')
        raise


if __name__ == '__main__':
    main()
