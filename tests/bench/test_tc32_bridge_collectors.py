"""Corrupt retained synthetic collector records; never produce benchmark evidence."""
import copy
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('audit_tc32', ROOT / 'scripts/audit-tc32-bridges.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class BridgeAuditTests(unittest.TestCase):
    def setUp(self):
        scratch = ROOT / 'work/tc32b/test-fixtures'
        scratch.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=scratch)
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.inventory = json.loads((ROOT / 'docs/evidence/tc32a-bridge-qualification-20261002/bridge-inventory.json').read_text())
        self.row = 'codec.adoption.bridge.prost_to_pbrs.entities.n1000.read_all'
        qualified = next(r for r in self.inventory['cells'] if r['id'] == self.row)
        self.pin = {'source_commit': 'a' * 40, 'binary_sha256': 'b' * 64, 'profile': 'release', 'fixture': True}
        self.meta = {'schema': 'tc32-bridge-cost/1', 'completed': True, 'source_commit': 'a' * 40,
                     'binary': {'path': '/fixture/devloop', 'sha256': 'b' * 64}, 'build_pin': self.pin,
                     'iters': 16, 'prepare_iters': 32, 'warmup': 100, 'repeats': 3,
                     'selected_cells': [self.row], 'full_eligible_coverage': False,
                     'original_registry_rows': ['old.codec codec pbrs'], 'callgrind_available': True,
                     'tools': {'valgrind': {'path': '/fixture/valgrind'}}}
        registry = '\n'.join(self.meta['original_registry_rows'] + [f'{row} codec bridge' for row in sorted(audit.expected_ids())]) + '\n'
        (self.directory / 'registry.txt').write_text(registry)
        (self.directory / 'original-registry.txt').write_text('old.codec codec pbrs\n')
        (self.directory / 'build-pin.json').write_text(json.dumps(self.pin))
        self.records = []
        for repeat in range(3):
            for n in (16, 32):
                for collector in ('allocator', 'callgrind'):
                    command = ['/fixture/devloop', 'run-cell', self.row, '--iters', str(n), '--prepare-iters', '32', '--warmup', '100']
                    child = {'cell': self.row, 'iters': n, 'allocs': 7 * n, 'alloc_bytes': 400 * n, 'wall_ns': 900 * n, 'input_wire_fingerprint': qualified['wire_fingerprint']}
                    stderr = '__CHILD__ ' + json.dumps(child) + '\n__QUALIFICATION__ ' + json.dumps(qualified) + '\n'
                    record = {'cell': self.row, 'iters': n, 'repeat': repeat, 'collector': collector, 'validated': True,
                              'returncode': 0, 'launch_sha256': 'b' * 64, 'after_sha256': 'b' * 64,
                              'child': child, 'qualification': copy.deepcopy(qualified)}
                    if collector == 'callgrind':
                        command = ['/fixture/valgrind', '--tool=callgrind', '--cache-sim=no', '--callgrind-out-file=/fixture/graph'] + command
                        total = 12345 + 1000 * n
                        raw = f'events: Ir\nsummary: {total}\n'.encode()
                        filename = f'{n}.{repeat}.callgrind.gz'
                        (self.directory / filename).write_bytes(gzip.compress(raw, mtime=0))
                        record.update(graph=filename, graph_sha256=audit.sha(self.directory / filename),
                                      uncompressed_graph_sha256=hashlib.sha256(raw).hexdigest(), instruction_total=total)
                        stderr += f'==123== I   refs: {total:,}\n'
                    record.update(command=command, stderr=stderr)
                    self.records.append(record)
        self.save()

    def save(self):
        (self.directory / 'inventory.json').write_text(json.dumps(self.inventory))
        (self.directory / 'raw.jsonl').write_text(''.join(json.dumps(r) + '\n' for r in self.records))
        self.meta['artifact_sha256'] = {name: audit.sha(self.directory / name) for name in ('inventory.json', 'raw.jsonl', 'registry.txt', 'original-registry.txt', 'build-pin.json')}
        (self.directory / 'metadata.json').write_text(json.dumps(self.meta))

    def reject(self):
        self.save()
        with self.assertRaises(ValueError):
            audit.rebuild(self.directory)

    def test_reconstructs_absolute_cost_from_both_Ns_and_every_repeat(self):
        report = audit.rebuild(self.directory)
        self.assertEqual(report['raw_children'], 12)
        self.assertFalse(report['full_eligible_coverage'])
        self.assertEqual(report['cells'][0]['metrics']['allocs']['value'], 7)
        self.assertEqual(report['cells'][0]['metrics']['alloc_bytes']['value'], 400)
        self.assertEqual(report['cells'][0]['metrics']['instructions']['value'], 1000)

    def test_wrong_complete_work_oracle_rejected_even_with_same_fingerprint(self):
        record = self.records[0]
        record['qualification']['read_checksum'] ^= 1
        record['stderr'] = record['stderr'].replace('__QUALIFICATION__ ' + json.dumps(next(r for r in self.inventory['cells'] if r['id'] == self.row)), '__QUALIFICATION__ ' + json.dumps(record['qualification']))
        self.reject()

    def test_missing_child_or_duplicate_child_rejected(self):
        self.records.pop()
        self.reject()
        self.records.append(copy.deepcopy(self.records[0]))
        self.reject()

    def test_missing_and_unequal_actual_source_identity_rejected(self):
        for value in (None, '', 'different'):
            child = self.records[0]['child']
            child['input_wire_fingerprint'] = value
            q = self.records[0]['qualification']
            self.records[0]['stderr'] = '__CHILD__ ' + json.dumps(child) + '\n__QUALIFICATION__ ' + json.dumps(q) + '\n'
            self.reject()

    def test_map_selection_and_selective_full_coverage_claim_rejected(self):
        self.meta['selected_cells'] = ['codec.adoption.bridge.prost_to_pbrs.maps.n8.read_all']
        self.reject()
        self.meta['selected_cells'] = [self.row]
        self.meta['full_eligible_coverage'] = True
        self.reject()

    def test_N_dependent_allocation_and_changed_prepare_rejected(self):
        self.records[0]['child']['allocs'] += 1
        q = self.records[0]['qualification']
        self.records[0]['stderr'] = '__CHILD__ ' + json.dumps(self.records[0]['child']) + '\n__QUALIFICATION__ ' + json.dumps(q) + '\n'
        self.reject()
        self.meta['prepare_iters'] = 16
        self.reject()

    def test_graph_disagreement_or_missing_graph_rejected(self):
        self.records[1]['instruction_total'] += 1
        self.reject()
        (self.directory / self.records[1]['graph']).write_bytes(gzip.compress(b'events: Ir\nsummary: 1\n', mtime=0))
        self.reject()

    def test_unchanged_binary_replay_retains_every_failure_and_negative_instability(self):
        first = audit.rebuild(self.directory)
        second = copy.deepcopy(first)
        second['cells'][0]['metrics']['instructions']['value'] = 1011
        result = audit.compare_replays(first, second)
        self.assertEqual(len(result['failed_rows']), 1)
        self.assertFalse(result['comparison_passed'])
        second['cells'][0]['metrics']['instructions']['value'] = 989
        result = audit.compare_replays(first, second)
        self.assertTrue(result['comparison_passed'])
        self.assertFalse(result['replay_stability_passed'])
        self.assertEqual(len(result['unstable_instruction_rows']), 1)

    def test_missing_instruction_result_is_not_a_replay_pass(self):
        first = audit.rebuild(self.directory)
        for row in first['cells']:
            row['metrics']['instructions'] = {'status': 'not_run'}
        result = audit.compare_replays(first, copy.deepcopy(first))
        self.assertFalse(result['comparison_passed'])
        self.assertEqual(len(result['unavailable_rows']), 1)

    def test_binary_drift_and_old_registry_order_rejected(self):
        self.records[0]['after_sha256'] = 'c' * 64
        self.reject()
        self.records[0]['after_sha256'] = 'b' * 64
        self.meta['original_registry_rows'] = ['different codec pbrs']
        self.reject()


if __name__ == '__main__':
    unittest.main()
