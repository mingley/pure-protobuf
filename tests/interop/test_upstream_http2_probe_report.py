"""Original Go TestSoon failures must not become success-shaped exit codes."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

from scripts import upstream_http2_probe_report
from scripts.upstream_http2_probe_report import CASES, GRPC_SOURCE_SHA, summarize


def report(mode: str, failed: str | None = None, skipped: str | None = None) -> str:
    rows = []
    for name in CASES["framing"] + CASES["tls"]:
        selected = name in CASES[mode]
        is_skipped = not selected or name == skipped
        rows.append({
            "name": name,
            "passed": selected and name != failed and not is_skipped,
            "skipped": is_skipped,
        })
    return "Go test output\n" + json.dumps({"cases": rows}) + "\n"


class UpstreamHttp2ReportTest(unittest.TestCase):
    def test_both_profiles_require_every_applicable_case(self):
        for mode, expected in (("framing", 6), ("tls", 3)):
            with self.subTest(mode=mode):
                result = summarize(report(mode), mode, 0, GRPC_SOURCE_SHA)
                self.assertTrue(result["qualified"])
                self.assertEqual(
                    sum(row["status"] == "passed" for row in result["cases"]),
                    expected,
                )

    def test_advisory_go_exit_zero_cannot_hide_failed_or_skipped_probes(self):
        for failed in (
            "TestSoonSmallMaxFrameSize",
            "TestSoonTLSApplicationProtocol",
        ):
            mode = "framing" if failed in CASES["framing"] else "tls"
            with self.subTest(failed=failed):
                result = summarize(report(mode, failed=failed), mode, 0, GRPC_SOURCE_SHA)
                self.assertFalse(result["qualified"])
                self.assertIn(f"{failed}: failed", result["failures"])
        result = summarize(
            report("framing", skipped="TestSoonClientShortSettings"),
            "framing",
            0,
            GRPC_SOURCE_SHA,
        )
        self.assertFalse(result["qualified"])
        self.assertIn("TestSoonClientShortSettings: not_run", result["failures"])

    def test_missing_duplicate_extra_or_unpinned_proof_is_rejected(self):
        good = report("framing")
        row = {"name": CASES["framing"][0], "passed": True}
        extra = json.loads(good.splitlines()[-1])
        extra["cases"].append({"name": "TestUnexpected", "passed": True})
        for broken in (
            "no Go result",
            "Go test output\n" + json.dumps({"cases": []}),
            good + json.dumps({"cases": [row]}) + "\n",
            "Go test output\n" + json.dumps({"cases": [row, row]}) + "\n",
            "Go test output\n" + json.dumps(extra) + "\n",
        ):
            with self.subTest(broken=broken[:25]):
                with self.assertRaises(ValueError):
                    summarize(broken, "framing", 0, GRPC_SOURCE_SHA)
        with self.assertRaises(ValueError):
            summarize(good, "framing", 0, "0" * 40)
        self.assertFalse(summarize(good, "framing", 1, GRPC_SOURCE_SHA)["qualified"])

    def test_source_guard_rejects_untracked_and_ignored_go_files(self):
        script = Path(__file__).resolve().parents[2] / (
            "scripts/grpc-http2-upstream-server-interop.py"
        )
        spec = importlib.util.spec_from_file_location("upstream_http2_runner", script)
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        with patch.dict(sys.modules, {"upstream_http2_probe_report": upstream_http2_probe_report}):
            runner = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(runner)

        revision = subprocess.CompletedProcess([], 0, GRPC_SOURCE_SHA + "\n", "")
        for status in (
            "",
            " M tools/http2_interop/http2interop.go\n",
            "?? tools/http2_interop/injected_test.go\n",
            "!! tools/http2_interop/ignored_test.go\n",
        ):
            with self.subTest(status=status), patch.object(
                runner, "command", side_effect=[
                    revision, subprocess.CompletedProcess([], 0, status, ""),
                ],
            ) as command:
                if status:
                    with self.assertRaisesRegex(RuntimeError, "modified, untracked, or ignored"):
                        runner.require_clean_source()
                else:
                    self.assertEqual(runner.require_clean_source(), GRPC_SOURCE_SHA)
                args = command.call_args_list[1].args[0]
                self.assertIn("--untracked-files=all", args)
                self.assertIn("--ignored=matching", args)
        with patch.object(runner, "command", side_effect=[
            revision, subprocess.CompletedProcess([], 128, "", "git status failed"),
        ]):
            with self.assertRaisesRegex(RuntimeError, "could not inspect"):
                runner.require_clean_source()


if __name__ == "__main__":
    unittest.main()
