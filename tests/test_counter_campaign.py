"""Keep failed and interrupted counter campaigns visible."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    'counter_campaign', Path(__file__).resolve().parents[1] / 'scripts/grpc-counter-campaign.py')
CAMPAIGN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CAMPAIGN)


class CampaignFailures(unittest.TestCase):
    def run_campaign(self, fail_capture=False, interrupt=False, source_drift=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'scripts').mkdir()
            (root / 'scripts/grpc-load-smoke.py').write_text('fixture')
            binary = root / 'benchmark'
            binary.write_bytes(b'fixture')
            record = root / 'build.json'
            record.write_text('{}')
            captures = []
            checks = []

            def verified_build(*args):
                checks.append(args)
                if source_drift and len(checks) > 1:
                    raise ValueError('source changed')
                return {'source': {'commit': 'a' * 40}}

            def run_cell(settings, cell, directory):
                if interrupt and len(captures) == 1:
                    raise KeyboardInterrupt()
                directory.mkdir()
                result = {'passed': not (fail_capture and not captures)}
                (directory / 'run.json').write_text(json.dumps(result))
                captures.append((settings.rpc_count, cell))
                return result

            smoke = SimpleNamespace(verified_build=verified_build, run_cell=run_cell)
            ledger = SimpleNamespace(compare=lambda *args, **kwargs: {'rows': []})
            argv = ['campaign', '--source-checkout', str(root), '--binary', str(binary),
                    '--build-record', str(record), '--output', str(root / 'output'),
                    '--phase', 'native', '--payloads', '0', '--load-levels', '64:1024',
                    '--count', '1', '--repeats', '1']
            with patch('sys.argv', argv), patch.object(CAMPAIGN, 'load_module',
                    side_effect=[smoke, ledger]), contextlib.redirect_stdout(io.StringIO()):
                if interrupt:
                    with self.assertRaises(KeyboardInterrupt):
                        CAMPAIGN.main()
                elif source_drift:
                    with self.assertRaisesRegex(ValueError, 'source changed'):
                        CAMPAIGN.main()
                else:
                    code = CAMPAIGN.main()
                    self.assertEqual(code, int(fail_capture))
            state = json.loads((root / 'output/campaign.json').read_text())
            partials = [json.loads(path.read_text()) for path in
                        (root / 'output').rglob('partial-report.json')]
            reports = list((root / 'output').rglob('report.json'))
            return state, captures, partials, len(reports)

    def test_v2_campaign_retains_verifier_and_rejects_tampering(self):
        for schema, tamper in (("pbrs.rpc-bench-build.v1", False),
                               ("pbrs.rpc-bench-build.v2", False),
                               ("pbrs.rpc-bench-build.v2", True)):
            with self.subTest(schema=schema, tamper=tamper), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                (root / "scripts").mkdir()
                (root / "scripts/grpc-load-smoke.py").write_text("fixture")
                helper = root / "scripts/build-rpc-bench.py"
                helper_bytes = b"frozen verifier\n"
                helper.write_bytes(helper_bytes)
                expected = hashlib.sha256(helper_bytes).hexdigest()
                binary = root / "benchmark"
                binary.write_bytes(b"fixture")
                build_path = root / "build.json"
                build_path.write_text("{}")
                checks = []

                def verified_build(*args, **kwargs):
                    checks.append(kwargs)
                    if kwargs.get("verifier_sha256") is not None:
                        if hashlib.sha256(helper.read_bytes()).hexdigest() != kwargs["verifier_sha256"]:
                            raise ValueError("verifier changed")
                    return {"schema": schema, "source": {"commit": "a" * 40}}

                def run_cell(settings, cell, directory):
                    directory.mkdir()
                    if tamper:
                        (directory.parent / "build-verifier.py").write_bytes(b"changed\n")
                    result = {"passed": True}
                    (directory / "run.json").write_text(json.dumps(result))
                    return result

                smoke = SimpleNamespace(verified_build=verified_build, run_cell=run_cell)
                ledger = SimpleNamespace(compare=lambda *args, **kwargs: {"rows": []})
                argv = ["campaign", "--source-checkout", str(root), "--binary", str(binary),
                        "--build-record", str(build_path), "--output", str(root / "output"),
                        "--phase", "native", "--payloads", "0", "--load-levels", "1:1",
                        "--count", "1", "--repeats", "1"]
                with patch("sys.argv", argv), patch.object(CAMPAIGN, "load_module", side_effect=[smoke, ledger]), \
                        patch.object(CAMPAIGN, "PAIRS", [("native", "pbrs", "tonic", "prost")]), \
                        patch.object(CAMPAIGN, "SHAPES", ["unary"]), contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(CAMPAIGN.main(), 1 if tamper else 0)
                reports = list((root / "output").rglob("report.json"))
                self.assertEqual(len(reports), 8)
                for report_path in reports:
                    report = json.loads(report_path.read_text())
                    if schema.endswith(".v1"):
                        self.assertNotIn("build_verifier_sha256", report)
                        self.assertTrue(report["passed"])
                    else:
                        self.assertEqual(report["build_verifier_sha256"], expected)
                        self.assertEqual(report["build_verifier_file"], "build-verifier.py")
                        self.assertIs(report["build_verifier_unchanged"], not tamper)
                        self.assertIs(report["passed"], not tamper)
                        retained = (report_path.parent / "build-verifier.py").read_bytes()
                        self.assertEqual(retained == helper_bytes, not tamper)
                if schema.endswith(".v1"):
                    self.assertTrue(all(not kwargs for kwargs in checks))
                else:
                    self.assertTrue(any(kwargs.get("verifier_sha256") == expected for kwargs in checks))

    def test_real_ledger_rejects_tampered_v2_verifier_and_preserves_failed_campaign(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "scripts").mkdir()
            (root / "scripts/grpc-load-smoke.py").write_text("fixture")
            helper = root / "scripts/build-rpc-bench.py"
            helper_bytes = b"frozen verifier\n"
            helper.write_bytes(helper_bytes)
            binary = root / "benchmark"
            binary.write_bytes(b"fixture")
            build_path = root / "build.json"
            build_path.write_text("{}")

            def verified_build(*args, **kwargs):
                expected = kwargs.get("verifier_sha256")
                if expected is not None and hashlib.sha256(helper.read_bytes()).hexdigest() != expected:
                    raise ValueError("verifier changed")
                return {"schema": "pbrs.rpc-bench-build.v2", "source": {"commit": "a" * 40}}

            def run_cell(settings, cell, directory):
                directory.mkdir()
                (directory.parent / "build-verifier.py").write_bytes(b"tampered verifier\n")
                result = {"passed": True}
                (directory / "run.json").write_text(json.dumps(result))
                return result

            ledger_path = Path(__file__).resolve().parents[1] / "scripts/dominance-ledger.py"
            ledger_spec = importlib.util.spec_from_file_location("real_ledger", ledger_path)
            ledger = importlib.util.module_from_spec(ledger_spec)
            ledger_spec.loader.exec_module(ledger)
            smoke = SimpleNamespace(verified_build=verified_build, run_cell=run_cell)
            output = root / "output"
            argv = ["campaign", "--source-checkout", str(root), "--binary", str(binary),
                    "--build-record", str(build_path), "--output", str(output),
                    "--phase", "native", "--payloads", "0", "--load-levels", "1:1",
                    "--count", "1", "--repeats", "1"]
            with patch("sys.argv", argv), patch.object(CAMPAIGN, "load_module", side_effect=[smoke, ledger]), \
                    patch.object(CAMPAIGN, "PAIRS", [("native", "pbrs", "tonic", "prost")]), \
                    patch.object(CAMPAIGN, "SHAPES", ["unary"]), contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaisesRegex(ValueError, "invalid or changed build verifier evidence"):
                    CAMPAIGN.main()

            state = json.loads((output / "campaign.json").read_text())
            self.assertEqual(state["state"], "failed")
            self.assertFalse(state["passed"])
            self.assertEqual(state["error"]["type"], "ValueError")
            self.assertIn("invalid or changed build verifier evidence", state["error"]["message"])
            reports = list(output.rglob("report.json"))
            self.assertEqual(len(reports), 2)
            for report_path in reports:
                report = json.loads(report_path.read_text())
                self.assertFalse(report["passed"])
                self.assertFalse(report["build_verifier_unchanged"])
                self.assertEqual((report_path.parent / "build-verifier.py").read_bytes(), b"tampered verifier\n")
                self.assertTrue((report_path.parent / "partial-report.json").exists())

    def test_failed_rpc_is_retained_and_other_profiles_still_run(self):
        state, captures, _, reports = self.run_campaign(fail_capture=True)
        self.assertEqual(state['state'], 'finished')
        self.assertFalse(state['passed'])
        self.assertFalse(state['qualified'])
        self.assertEqual(len(captures), state['expected_captures'])
        self.assertEqual(reports, 8)
        self.assertEqual({count for count, _ in captures}, {1024, 2048})
        self.assertEqual({(cell['tls'], cell['compression']) for _, cell in captures},
                         {(tls, compression) for tls in [False, True]
                          for compression in ['identity', 'gzip']})
        self.assertEqual({cell['shape'] for _, cell in captures}, set(CAMPAIGN.SHAPES))

    def test_interruption_records_terminal_failure_and_completed_capture(self):
        state, captures, partials, reports = self.run_campaign(interrupt=True)
        self.assertEqual(state['state'], 'failed')
        self.assertFalse(state['passed'])
        self.assertFalse(state['qualified'])
        self.assertEqual(state['error']['type'], 'KeyboardInterrupt')
        self.assertEqual(len(captures), 1)
        self.assertEqual(len(partials[0]['runs']), 1)
        self.assertEqual(reports, 0)

    def test_source_drift_prevents_completed_batch_report(self):
        state, captures, partials, reports = self.run_campaign(source_drift=True)
        self.assertEqual(state['state'], 'failed')
        self.assertFalse(state['passed'])
        self.assertEqual(state['error']['message'], 'source changed')
        self.assertEqual(len(captures), 25)
        self.assertEqual(len(partials[0]['runs']), 25)
        self.assertEqual(reports, 0)


if __name__ == '__main__':
    unittest.main()
