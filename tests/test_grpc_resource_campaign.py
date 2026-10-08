"""Reject incomplete or overstated resource-campaign evidence."""
import copy
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("campaign", Path(__file__).resolve().parents[1] / "scripts/grpc-resource-campaign.py")
CAMPAIGN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CAMPAIGN)


def report():
    events = [{**{key: 0 for key in CAMPAIGN.GAUGES}, "phase": "baseline", "cycle": 0,
               "rss_bytes": 1000, "rss_hwm_bytes": 1000, "rss_sample_source": "linux_proc_status_single_read"}]
    for cycle in range(1, 5):
        for phase in CAMPAIGN.PHASES:
            event = {**events[0], "phase": phase, "cycle": cycle}
            if phase == "profile":
                event.update(tls=(cycle - 1) % 4 >= 2, gzip=cycle % 2 == 0,
                             mixed_payload_bytes=[0, 1024, 65536, 1048576], byte_budget=8388608,
                             message_limit=2097152, server_deadline_ms=3000)
            elif phase == "fault":
                event.update(transport="plaintext_tcp", recovery_probe="warmed_independent_connection",
                             recovery_code="OK", probe_timeout_ms=300, fault=["RstStream(Cancel)", "Goaway", "TcpReset"][(cycle - 1) % 3])
            elif phase == "slow_reader":
                event.update(stall_wait_ms=30, producer_progress_before_hold=12, producer_progress_after_hold=12,
                             producer_sent_messages=12, producer_done=False)
            elif phase != "warmup":
                event.update(producer_sent_messages=128, producer_done=True)
            events.append(event)
    limits = {key: {"soft": 1024, "hard": 1024} for key in CAMPAIGN.LIMIT_NAMES}
    return {"schema": "pbrs.resource-campaign.v4", "sampled_rss_peak_bytes": 1000, "source": {"commit": "a" * 40, "tree": "b" * 40,
            "dirty": False, "cargo_lock_sha256": "c" * 64}, "binary": {"sha256": "d" * 64},
            "tools": {"cargo": "cargo", "rustc": "rustc", "python": "python"},
            "commands": {"build": ["build"], "test": ["test"]}, "duration_requested_seconds": 30,
            "duration_actual_seconds": 30.5, "process_limits": copy.deepcopy(limits),
            "process_limits_requested": limits, "settings": CAMPAIGN.SETTINGS.copy(), "events": events,
            "exit_code": 0, "qualification": {"qualified": False, "soak_24h": {"status": "not_run"}},
            "process_samples": [{"elapsed_seconds": t, "memory_bytes": {"VmRSS": 1000, "VmHWM": 1000, "VmSize": 2000},
                                 "os_threads": 3, "file_descriptors": 6} for t in range(31)]}


class CampaignEvidenceTests(unittest.TestCase):
    def test_complete_preview(self):
        self.assertEqual(CAMPAIGN.validate_report(report()), [])

    def test_claiming_24_hours_after_preview_is_rejected(self):
        value = report()
        value["qualification"]["soak_24h"]["status"] = "completed"
        self.assertTrue(CAMPAIGN.validate_report(value))

    def test_failed_24_hour_attempt_cannot_be_marked_completed(self):
        value = report()
        value.update(duration_requested_seconds=86400, duration_actual_seconds=503, exit_code=101)
        value["qualification"]["soak_24h"]["status"] = "completed"
        self.assertIn("24-hour disposition does not match elapsed duration and execution outcome", CAMPAIGN.validate_report(value))
        self.assertEqual(CAMPAIGN.soak_disposition(86400, 503, 101), "failed")

    def test_elapsed_day_with_resource_failure_is_not_completed(self):
        self.assertEqual(CAMPAIGN.soak_disposition(86400, 86401, 0, ["unrecovered permit"]), "failed")
        self.assertEqual(CAMPAIGN.soak_disposition(86400, 86401, 0), "completed")

    def test_short_actual_duration_is_rejected(self):
        value = report()
        value["duration_requested_seconds"] = 86400
        self.assertTrue(CAMPAIGN.validate_report(value))

    def test_missing_tls_gzip_profile_and_wrong_schedule_are_rejected(self):
        for mutate in [lambda e: e.update(tls=False), lambda e: e.update(gzip=False),
                       lambda e: e.update(mixed_payload_bytes=[1024]), lambda e: e.update(byte_budget=2**32)]:
            value = report()
            for event in value["events"]:
                if event["phase"] == "profile":
                    mutate(event)
            self.assertTrue(CAMPAIGN.validate_report(value))

    def test_incomplete_faults_and_wrong_transport_are_rejected(self):
        for field, replacement in [("fault", "Cancel"), ("transport", "tls")]:
            value = report()
            for event in value["events"]:
                if event["phase"] == "fault":
                    event[field] = replacement
            self.assertTrue(CAMPAIGN.validate_report(value))

    def test_fault_recovery_requires_a_successful_independent_rpc(self):
        for field, replacement in [("recovery_probe", "faulted_channel"), ("recovery_code", "UNAVAILABLE"),
                                   ("probe_timeout_ms", 3000)]:
            value = report()
            for event in value["events"]:
                if event["phase"] == "fault":
                    event[field] = replacement
            self.assertIn("missing successful independent fault recovery probe", CAMPAIGN.validate_report(value))

    def test_unrecovered_permit_and_resource_growth_are_rejected(self):
        for field, replacement in [("server_byte_tokens", 1), ("file_descriptors", 1000),
                                   ("rss_bytes", 10**9), ("tokio_alive_tasks", 3)]:
            value = report()
            value["events"][-1][field] = replacement
            self.assertTrue(CAMPAIGN.validate_report(value))

    def test_rss_samples_remain_raw_and_sampled_peak_is_exact(self):
        value = report()
        value["events"][0]["rss_hwm_bytes"] = 900
        self.assertEqual(CAMPAIGN.validate_report(value), [])
        value["process_samples"][0]["memory_bytes"]["VmRSS"] = 1200
        self.assertIn("sampled RSS peak does not match retained observations", CAMPAIGN.validate_report(value))
        value["sampled_rss_peak_bytes"] = 1200
        self.assertEqual(CAMPAIGN.validate_report(value), [])
        for replacement in [None, 0, -1, "1200", 1199, 1201]:
            value["sampled_rss_peak_bytes"] = replacement
            self.assertIn("sampled RSS peak does not match retained observations", CAMPAIGN.validate_report(value))

    def test_memory_source_and_positive_raw_counters_are_required(self):
        for field, replacement in [("rss_sample_source", None), ("rss_sample_source", "separate_reads"),
                                   ("rss_bytes", 0), ("rss_hwm_bytes", 0)]:
            value = report()
            value["events"][0][field] = replacement
            self.assertIn("invalid process RSS accounting", CAMPAIGN.validate_report(value))

    def test_sparse_samples_cannot_certify_duration(self):
        value = report()
        value["process_samples"] = value["process_samples"][:1]
        self.assertTrue(CAMPAIGN.validate_report(value))

    def test_source_drift_failure_cannot_be_revalidated(self):
        value = report()
        value["smoke"] = {"status": "failed", "failures": ["source drift"]}
        self.assertTrue(CAMPAIGN.validate_report(value))

    def test_missing_phase_and_failed_child_are_rejected(self):
        value = report()
        value["events"].pop(3)
        self.assertTrue(CAMPAIGN.validate_report(value))
        value = report()
        value["exit_code"] = 1
        self.assertTrue(CAMPAIGN.validate_report(value))

    def test_resource_run_never_implies_full_production_qualification(self):
        value = report()
        value["qualification"]["qualified"] = True
        self.assertTrue(CAMPAIGN.validate_report(value))


if __name__ == "__main__":
    unittest.main()
