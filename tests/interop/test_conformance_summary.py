"""Fail-closed protobuf binary/JSON and text conformance report contracts."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "conformance-report.py"
spec = importlib.util.spec_from_file_location("conformance_report", SCRIPT)
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)


def suite(status: str, success: int, unexpected: int = 0) -> str:
    return (
        f"CONFORMANCE SUITE {status}: {success} successes, 0 skipped, "
        f"0 expected failures, {unexpected} unexpected failures.\n"
    )


def both() -> str:
    return suite("PASSED", 5631) + suite("PASSED", 909)


class ConformanceReportTest(unittest.TestCase):
    def test_both_pinned_suites_are_required(self):
        parsed = report.parse_log(both())
        self.assertEqual(parsed["status"], "passed")
        self.assertEqual(parsed["successes"], 5631)
        self.assertEqual(parsed["text_successes"], 909)
        self.assertEqual(parsed["text_unexpected_failures"], 0)

        for incomplete in (
            suite("PASSED", 5631),
            both() + suite("PASSED", 909),
            suite("PASSED", 5630) + suite("PASSED", 909),
            suite("PASSED", 5631) + suite("FAILED", 908, unexpected=1),
        ):
            with self.subTest(incomplete=incomplete):
                self.assertEqual(report.parse_log(incomplete)["status"], "failed")

    def test_three_passes_require_text_and_exact_counts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, path in report.RUNS:
                log = root / path
                log.parent.mkdir(parents=True, exist_ok=True)
                log.write_text(both(), encoding="utf-8")
            summary = report.build_summary(
                root, "v35.1", "pinned-sha", "dirty-commit", "2026-09-23T00:00:00Z", "2023", True
            )
            self.assertTrue(summary["overall_passed"])
            self.assertEqual(summary["test_counts"]["required_text_run_1"], 909)
            self.assertEqual(summary["test_counts"]["required_text_run_2"], 909)
            self.assertEqual(summary["test_counts"]["recommended_text"], 909)
            self.assertTrue(summary["dirty_source"])

            (root / report.RUNS[2][1]).unlink()
            missing = report.build_summary(
                root, "v35.1", "pinned-sha", "head", "now", "2023", False
            )
            self.assertFalse(missing["overall_passed"])
            self.assertEqual(missing["runs"]["recommended"]["status"], "not_run")

    def test_unreviewed_pin_cannot_reuse_old_counts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for _, path in report.RUNS:
                log = root / path
                log.parent.mkdir(parents=True, exist_ok=True)
                log.write_text(both(), encoding="utf-8")
            result = report.build_summary(root, "v36.0", "new-sha", "head", "now", "2024", False)
            self.assertFalse(result["overall_passed"])
            self.assertIn("unreviewed conformance contract", result["validation_errors"][0])

    def test_cli_rejects_binary_only_success_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for _, path in report.RUNS:
                log = root / path
                log.parent.mkdir(parents=True, exist_ok=True)
                log.write_text(suite("PASSED", 5631), encoding="utf-8")
            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(root), "v35.1", "sha", "head", "now", "2023", "0"],
                capture_output=True, text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("expected 2 suite summaries", result.stderr)
            saved = json.loads((root / "summary.json").read_text())
            self.assertFalse(saved["overall_passed"])
            self.assertEqual(saved["test_counts"]["required_text_run_1"], 0)


if __name__ == "__main__":
    unittest.main()
