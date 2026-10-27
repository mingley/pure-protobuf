"""Fail-closed parsing of original Google shared-consumer Cargo logs."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from scripts.shared_consumer_report import parse_results
from scripts.shared_consumer_inventory import (
    EXPECTED_SUITES,
    EXCLUDED_UPSTREAM_SUITES,
    LOCAL_SUITES,
    UPSTREAM_SUITES,
    upstream_source,
    validate_inventory,
    validate_results,
)


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


class SharedConsumerInventoryTest(unittest.TestCase):
    def make_tree(self, directory):
        root = Path(directory)
        tests = root / "rust_out_shared/tests"
        tests.mkdir(parents=True)
        (tests.parent / "Cargo.toml").write_text('[package]\nname="fixture"\nversion="0.0.0"\nedition="2021"\n')
        upstream = root / "protobuf/rust/test/shared"
        upstream.mkdir(parents=True)
        for name in EXPECTED_SUITES:
            (tests / (name + ".rs")).write_text("// consumer fixture\n")
        for name in UPSTREAM_SUITES + EXCLUDED_UPSTREAM_SUITES:
            source = upstream_source(upstream, name)
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_text("// pinned upstream fixture\n")
        return root, upstream

    def test_local_recursion_suite_needs_no_fake_upstream_source(self):
        self.assertEqual(LOCAL_SUITES, ("arena_recursion",))
        with tempfile.TemporaryDirectory() as directory:
            root, upstream = self.make_tree(directory)
            self.assertFalse((upstream / "arena_recursion.rs").exists())
            validate_inventory(root, upstream)

    def test_missing_and_unexpected_consumers_are_rejected(self):
        for kind in ("missing", "unexpected", "renamed"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                root, upstream = self.make_tree(directory)
                tests = root / "rust_out_shared/tests"
                if kind != "unexpected":
                    (tests / "arena_recursion.rs").unlink()
                if kind != "missing":
                    (tests / "surprise.rs").write_text("// unexpected\n")
                with self.assertRaisesRegex(ValueError, "consumer inventory mismatch"):
                    validate_inventory(root, upstream)

    def test_upstream_deletions_new_suites_and_duplicates_are_rejected(self):
        for kind in ("missing", "unexpected", "duplicate"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                root, upstream = self.make_tree(directory)
                if kind == "missing":
                    upstream_source(upstream, "utf8_test").unlink()
                elif kind == "unexpected":
                    nested = upstream / "new_directory"
                    nested.mkdir()
                    (nested / "new_test.rs").write_text("// new upstream suite\n")
                else:
                    (upstream / "utf8_test.rs").write_text("// ambiguous duplicate\n")
                with self.assertRaisesRegex(ValueError, "upstream"):
                    validate_inventory(root, upstream)

    def test_actual_ci_twenty_suite_log_passes_without_dropping_local_cases(self):
        log = Path(__file__).parent / "fixtures/shared-consumer-20-suites.log"
        crates = parse_results(log.read_text())
        validate_results(crates)
        self.assertEqual(len(crates), 20)
        self.assertEqual(sum(result[1] for result in crates.values()), 248)
        self.assertEqual(crates["arena_recursion"], ("ok", 15, 0))
        run = subprocess.run(
            [sys.executable, str(SCRIPT), str(log), "--require-inventory"],
            capture_output=True, text=True, check=True,
        )
        self.assertIn("__CRATES_COUNT__=20", run.stdout)
        self.assertIn("__TOTAL_PASSED__=248", run.stdout)

    def test_wrong_identity_or_empty_failed_suite_never_qualifies(self):
        valid = {name: ("ok", 1, 0) for name in EXPECTED_SUITES}
        for kind in ("missing", "renamed", "unexpected", "zero", "failed"):
            with self.subTest(kind=kind):
                crates = valid.copy()
                if kind in ("missing", "renamed"):
                    del crates["arena_recursion"]
                if kind in ("renamed", "unexpected"):
                    crates["surprise"] = ("ok", 1, 0)
                if kind == "zero":
                    crates["arena_recursion"] = ("ok", 0, 0)
                if kind == "failed":
                    crates["arena_recursion"] = ("FAILED", 14, 1)
                with self.assertRaises(ValueError):
                    validate_results(crates)

    def test_duplicate_actual_ci_suite_is_rejected_by_parser(self):
        log = (Path(__file__).parent / "fixtures/shared-consumer-20-suites.log").read_text()
        with self.assertRaisesRegex(ValueError, "duplicate test result"):
            parse_results(log + "Running tests/arena_recursion.rs (target/test)\ntest result: ok. 15 passed; 0 failed\n")

    def test_nested_auto_discovered_target_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root, upstream = self.make_tree(directory)
            nested = root / "rust_out_shared/tests/hidden_suite"
            nested.mkdir()
            (nested / "main.rs").write_text("#[test] fn hidden() {}\n")
            with self.assertRaisesRegex(ValueError, "unexpected=.*hidden_suite"):
                validate_inventory(root, upstream)

    def test_explicit_targets_and_autotests_are_accounted_for(self):
        for kind in ("disabled", "unknown", "redirect", "harness", "features", "duplicate"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                root, upstream = self.make_tree(directory)
                manifest = root / "rust_out_shared/Cargo.toml"
                text = manifest.read_text()
                if kind == "disabled":
                    text += 'autotests=false\n'
                else:
                    name = "hidden_suite" if kind == "unknown" else "arena_recursion"
                    text += f'\n[[test]]\nname="{name}"\n'
                    if kind == "redirect":
                        text += 'path="tests/hidden_suite/main.rs"\n'
                    elif kind == "harness":
                        text += 'harness=false\n'
                    elif kind == "features":
                        text += 'required-features=["unselected"]\n'
                    elif kind == "duplicate":
                        text += '\n[[test]]\nname="arena_recursion"\n'
                manifest.write_text(text)
                with self.assertRaises(ValueError):
                    validate_inventory(root, upstream)

    def test_explicit_exact_inventory_can_replace_autodiscovery(self):
        with tempfile.TemporaryDirectory() as directory:
            root, upstream = self.make_tree(directory)
            manifest = root / "rust_out_shared/Cargo.toml"
            text = manifest.read_text() + 'autotests=false\n'
            text += ''.join(f'\n[[test]]\nname="{name}"\npath="tests/{name}.rs"\n' for name in EXPECTED_SUITES)
            manifest.write_text(text)
            validate_inventory(root, upstream)

    def test_duplicate_flat_and_nested_target_identity_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root, upstream = self.make_tree(directory)
            nested = root / "rust_out_shared/tests/arena_recursion"
            nested.mkdir()
            (nested / "main.rs").write_text("#[test] fn hidden() {}\n")
            with self.assertRaisesRegex(ValueError, "duplicate consumer target"):
                validate_inventory(root, upstream)

    def test_unknown_nested_unusual_absolute_and_malformed_events_are_rejected(self):
        log = (Path(__file__).parent / "fixtures/shared-consumer-20-suites.log").read_text()
        for event in (
            "tests/hidden_suite/main.rs (target/hidden)",
            "tests/hidden-suite.rs (target/hidden)",
            "/tmp/consumer/tests/hidden.rs (target/hidden)",
            "tests/arena_recursion.rs",
        ):
            with self.subTest(event=event), self.assertRaises(ValueError):
                parse_results(log + f"Running {event}\ntest result: ok. 1 passed; 0 failed\n")


if __name__ == "__main__":
    unittest.main()
