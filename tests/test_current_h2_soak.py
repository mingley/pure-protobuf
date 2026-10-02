"""Negative acceptance cases for retained current-h2 smoke evidence."""

import copy
import importlib.util
from pathlib import Path
import resource
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("current_h2_soak", ROOT / "scripts/current-h2-soak.py")
SOAK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SOAK)


def complete_report():
    events = []
    for cycle, phase in [(0, "baseline")] + [(1, phase) for phase in SOAK.PHASES]:
        event = {key: 0 for key in SOAK.GAUGES}
        event.update(cycle=cycle, phase=phase, rss_bytes=1000000, rss_hwm_bytes=1100000,
                     file_descriptors=6, os_threads=3, tokio_alive_tasks=0)
        events.append(event)
    return {"source": {"commit": "a" * 40, "tree": "b" * 40, "dirty": False},
            "settings": SOAK.SETTINGS.copy(), "process_limits": {
                key: {"soft": 1024, "hard": 1024} for key in SOAK.LIMIT_NAMES},
            "exit_code": 0, "events": events,
            "process_samples": [{"elapsed_seconds": 0.01, "pid": 123,
                                 "memory_bytes": {"VmRSS": 1000000, "VmHWM": 1100000,
                                                  "VmSize": 2000000},
                                 "os_threads": 3, "file_descriptors": 6}],
            "qualification": {"qualified": False, "soak_24h": {"status": "not_run"}}}


class CurrentH2EvidenceTest(unittest.TestCase):
    def test_complete_diagnostic_is_valid_and_unqualified(self):
        self.assertEqual(SOAK.validate_report(complete_report()), [])

    def test_missing_wrong_and_dirty_source_rejected_before_build(self):
        for pin in (None, "", "a" * 8, "z" * 40):
            with self.subTest(pin=pin), self.assertRaises(ValueError), mock.patch.object(SOAK, "command") as command:
                SOAK.freeze_source(pin)
                command.assert_not_called()
        with mock.patch.object(SOAK, "command", return_value="a" * 40), self.assertRaisesRegex(ValueError, "differs"):
            SOAK.freeze_source("b" * 40)
        with mock.patch.object(SOAK, "command", side_effect=["a" * 40, " M dirty.rs"]), self.assertRaisesRegex(ValueError, "clean"):
            SOAK.freeze_source("a" * 40)

    def test_retained_dirty_missing_or_short_pin_rejected(self):
        for source in ({}, {"commit": "a" * 8, "dirty": False},
                       {"commit": "a" * 40, "dirty": True}):
            report = complete_report()
            report["source"] = source
            self.assertIn("missing or dirty source pin", SOAK.validate_report(report))

    def test_infinite_zero_negative_and_string_limits_rejected(self):
        for key in SOAK.LIMIT_NAMES:
            for invalid in (-1, 0, float("inf"), None, "unlimited", True):
                with self.subTest(key=key, value=invalid):
                    report = complete_report()
                    report["process_limits"][key]["hard"] = invalid
                    self.assertIn(f"nonfinite or invalid {key} process limit", SOAK.validate_report(report))
        with self.assertRaises(ValueError):
            SOAK.finite_limits(1, resource.RLIM_INFINITY, 128, 4096)

    def test_incomplete_duplicate_reordered_and_missing_baseline_rejected(self):
        for change in (lambda events: events.pop(3), lambda events: events.append(events[-1]),
                       lambda events: events.reverse()):
            report = complete_report()
            change(report["events"])
            self.assertTrue(SOAK.validate_report(report))
        report = complete_report()
        report["events"] = []
        self.assertIn("missing baseline phase accounting", SOAK.validate_report(report))

    def test_missing_gauges_never_coerced_to_zero(self):
        for gauge in SOAK.GAUGES:
            report = complete_report()
            del report["events"][-1][gauge]
            self.assertIn("missing or invalid resource gauge", SOAK.validate_report(report))

    def test_post_fault_leaks_and_incomplete_calls_rejected(self):
        for gauge in ("admitted_calls_active", "server_allocated_bytes", "client_allocated_bytes",
                      "server_byte_tokens", "client_byte_tokens"):
            report = complete_report()
            report["events"][-1][gauge] = 1
            self.assertIn("post-fault permits or admitted calls failed to recover", SOAK.validate_report(report))
        report = complete_report()
        report["events"][-1]["admitted_calls_started"] = 1
        self.assertIn("admitted-call start/end accounting incomplete", SOAK.validate_report(report))

    def test_predeclared_recovery_tolerances_and_budget_are_enforced(self):
        cases = [("rss_bytes", SOAK.SETTINGS["recovery_rss_tolerance_bytes"], "RSS"),
                 ("file_descriptors", SOAK.SETTINGS["recovery_fd_tolerance"], "descriptors"),
                 ("tokio_alive_tasks", SOAK.SETTINGS["recovery_tokio_task_tolerance"], "Tokio tasks")]
        for gauge, tolerance, message in cases:
            report = complete_report()
            report["events"][-1][gauge] += tolerance + 1
            report["events"][-1]["rss_hwm_bytes"] = max(report["events"][-1]["rss_hwm_bytes"], report["events"][-1]["rss_bytes"])
            self.assertTrue(any(message in error for error in SOAK.validate_report(report)))
        report = complete_report()
        report["events"][2]["server_byte_peak"] = SOAK.SETTINGS["server_byte_budget_bytes"] + 1
        self.assertIn("accounted byte peak exceeds tracker budget", SOAK.validate_report(report))

    def test_process_samples_and_failed_child_are_required(self):
        report = complete_report()
        report["process_samples"] = []
        self.assertIn("missing independent process sampling", SOAK.validate_report(report))
        report = complete_report()
        report["exit_code"] = 1
        self.assertIn("resource test child failed", SOAK.validate_report(report))
        report = complete_report()
        report["process_samples"] = [{"memory_bytes": {"VmRSS": 1}}]
        self.assertIn("incomplete independent process sample", SOAK.validate_report(report))

    def test_frozen_settings_and_qualification_claim_rejected(self):
        report = complete_report()
        report["settings"]["max_active_rpcs"] = 0
        self.assertIn("settings differ from frozen scenario", SOAK.validate_report(report))
        report = complete_report()
        report["qualification"]["qualified"] = True
        self.assertTrue(any("production qualification" in error for error in SOAK.validate_report(report)))
        report["qualification"]["soak_24h"]["status"] = "passed"
        self.assertTrue(any("24-hour" in error for error in SOAK.validate_report(report)))

    def test_exact_cargo_artifact_selected_without_stale_path_fallback(self):
        current = {"reason": "compiler-artifact", "manifest_path": str(ROOT / "pbrs-grpc/Cargo.toml"),
                   "target": {"name": "resource_qualification", "kind": ["test"]},
                   "executable": "/tmp/target/debug/deps/resource_qualification-current"}
        old = copy.deepcopy(current)
        old["target"]["name"] = "old"
        self.assertEqual(SOAK.test_executable([old, current]), Path(current["executable"]))
        for entries in ([], [old], [current, current]):
            with self.assertRaises(ValueError):
                SOAK.test_executable(entries)


if __name__ == "__main__":
    unittest.main()
