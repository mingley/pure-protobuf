"""Integrity and provenance checks for portable counter captures."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('counter_capsule', Path(__file__).resolve().parents[1] / 'scripts/grpc-counter-capsule.py')
capsule = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capsule)
PIN = 'a' * 40


class CounterCapsuleTests(unittest.TestCase):
    def make_capture(self, directory, **overrides):
        source = directory / 'capture'
        source.mkdir()
        record = dict(source=PIN, qualified=False, state='failed', phase='native', passed=False)
        record.update(overrides)
        (source / 'campaign.json').write_text(json.dumps(record))
        (source / 'failed.stderr').write_text('retained deadline failure\n')
        (source / 'partial-report.json').write_text('{"runs": [{"passed": false}]}')
        return source

    def test_failed_partial_capture_round_trips_and_tampering_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = self.make_capture(root)
            summary = capsule.archive(source, root / 'out', PIN, chunk_bytes=150)
            self.assertFalse(summary['campaign']['passed'])
            self.assertEqual(summary['files'], 3)
            self.assertGreater(len(summary['parts']), 1)
            capsule.check(root / 'out')
            archive = root / 'out' / summary['parts'][0]['path'] / 'raw.tar.gz'
            archive.write_bytes(archive.read_bytes() + b'changed')
            with self.assertRaisesRegex(ValueError, 'archive checksum'):
                capsule.check(root / 'out')

    def test_active_wrong_source_and_qualification_are_rejected(self):
        for overrides, message in [({'state': 'running'}, 'still active'),
                                    ({'source': 'b' * 40}, 'does not match'),
                                    ({'qualified': True}, 'do not establish')]:
            with self.subTest(overrides=overrides), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = self.make_capture(root, **overrides)
                with self.assertRaisesRegex(ValueError, message):
                    capsule.archive(source, root / 'out', PIN)
                self.assertFalse((root / 'out').exists())

    def test_symlinks_oversized_files_and_nested_output_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = self.make_capture(root)
            (source / 'outside').symlink_to(root / 'external')
            with self.assertRaisesRegex(ValueError, 'symlink'):
                capsule.archive(source, root / 'out', PIN)
            (source / 'outside').unlink()
            with self.assertRaisesRegex(ValueError, 'chunk size'):
                capsule.archive(source, root / 'out', PIN, chunk_bytes=1)
            with self.assertRaisesRegex(ValueError, 'outside'):
                capsule.archive(source, source / 'out', PIN)
            self.assertFalse((root / 'out').exists())


if __name__ == '__main__':
    unittest.main()
