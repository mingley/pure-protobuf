"""Fail-closed parsing of original Google shared-consumer Cargo logs."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from scripts.shared_consumer_report import parse_results


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/shared_consumer_report.py"
COLORED = (
    "\x1b[1m\x1b[92m     Running\x1b[0m tests/accessors_map_test.rs "
    "(target/debug/deps/accessors_map_test-123)\n"
    "test result: \x1b[32mok\x1b[0m. 35 passed; 0 failed; 0 ignored\n"
    "\x1b[1m\x1b[92m     Running\x1b[0m tests/utf8_test.rs "
    "(target/debug/deps/utf8_test-456)\n"
    "test result: ok. 3 passed; 0 failed; 0 ignored\n"
    "   Doc-tests rust_out_shared\n"
    "test result: ok. 0 passed; 0 failed; 0 ignored\n"
)


class SharedConsumerReportTest(unittest.TestCase):
    def test_colored_cargo_log_counts_every_original_test_crate(self):
        self.assertEqual(
            parse_results(COLORED),
            {
                "accessors_map_test": ("ok", 35, 0),
                "utf8_test": ("ok", 3, 0),
            },
        )
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "cargo.log"
            log.write_text(COLORED, encoding="utf-8")
            run = subprocess.run(
                [sys.executable, str(SCRIPT), str(log)],
                capture_output=True,
                text=True,
                check=True,
            )
            self.assertIn("__CRATES_COUNT__=2", run.stdout)
            self.assertIn("__TOTAL_PASSED__=38", run.stdout)
            self.assertIn("__TOTAL_FAILED__=0", run.stdout)

    def test_missing_or_duplicate_crate_results_fail_explicitly(self):
        unfinished = "Running tests/utf8_test.rs (target/debug/deps/utf8_test-456)\n"
        with self.assertRaisesRegex(ValueError, "missing test result for utf8_test"):
            parse_results(unfinished)
        with self.assertRaisesRegex(ValueError, "duplicate test result for utf8_test"):
            parse_results(COLORED + unfinished + "test result: ok. 1 passed; 0 failed\n")

    def test_empty_log_never_creates_successful_crates(self):
        self.assertEqual(parse_results("test result: ok. 0 passed; 0 failed\n"), {})


if __name__ == "__main__":
    unittest.main()
