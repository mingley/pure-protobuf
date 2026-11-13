"""Release-event guards, with no registry access, tags, or workflow dispatch."""

import copy
import json
import os
from pathlib import Path
import re
import runpy
import shutil
import subprocess
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "scripts/release-context.py"
POLICY = runpy.run_path(str(HELPER))
select_context = POLICY["select_context"]
ContextError = POLICY["ContextError"]
SHA = "a" * 40
REPOSITORY = "mingley/pure-protobuf"
REPOSITORY_ID = 1340219952


def automatic_payload():
    repository = {"id": REPOSITORY_ID, "full_name": REPOSITORY}
    return {
        "repository": copy.deepcopy(repository),
        "workflow_run": {
            "id": 123,
            "run_attempt": 1,
            "html_url": f"https://github.com/{REPOSITORY}/actions/runs/123",
            "name": "ci",
            "path": ".github/workflows/ci.yml",
            "event": "push",
            "status": "completed",
            "conclusion": "success",
            "head_branch": "main",
            "head_sha": SHA,
            "head_repository": copy.deepcopy(repository),
        },
    }


def request(**changes):
    arguments = {
        "event_name": "workflow_run", "payload": automatic_payload(),
        "repository": REPOSITORY, "repository_id": str(REPOSITORY_ID),
        "source_sha": SHA, "source_ref": "refs/heads/main",
        "checked_out_sha": SHA, "required_ci_result": "skipped",
    }
    arguments.update(changes)
    return select_context(**arguments)


def workflow_step(name):
    source = (ROOT / ".github/workflows/release.yml").read_text()
    start = source.index(f"      - name: {name}\n")
    end = source.find("\n      - ", start + 1)
    return source[start:end if end >= 0 else None]


class ReleaseContextTest(unittest.TestCase):
    def assert_skipped(self, **changes):
        result = request(**changes)
        self.assertFalse(result["eligible"])
        self.assertEqual(result["mode"], "skip")
        self.assertNotIn("release_tag", result)

    def test_exact_successful_own_main_ci_selects_only_dry_run(self):
        result = request()
        self.assertTrue(result["eligible"])
        self.assertEqual(result["mode"], "dry-run")
        self.assertEqual(result["release_tag"], "")
        self.assertEqual(result["source"]["sha"], SHA)
        self.assertEqual(result["source"]["checked_out_sha"], SHA)
        self.assertEqual(result["required_ci"]["head_sha"], SHA)
        self.assertEqual(result["required_ci"]["authority"], "upstream-ci")

    def test_automatic_inputs_cannot_enable_an_upload(self):
        payload = automatic_payload()
        payload["inputs"] = {"dry_run": "false", "confirm": "publish"}
        payload["mode"] = "publish"
        self.assertEqual(request(payload=payload)["mode"], "dry-run")

    def test_stale_ci_and_wrong_checkout_are_no_ops(self):
        for sha in ("b" * 40, None, "", "not-a-sha"):
            with self.subTest(sha=sha):
                payload = automatic_payload()
                payload["workflow_run"]["head_sha"] = sha
                self.assert_skipped(payload=payload)
        self.assert_skipped(checked_out_sha="b" * 40)
        self.assert_skipped(source_sha="not-a-sha")

    def test_wrong_workflow_name_or_path_is_a_no_op(self):
        for key, value in (("name", "other"), ("path", ".github/workflows/other.yml")):
            with self.subTest(key=key):
                payload = automatic_payload()
                payload["workflow_run"][key] = value
                self.assert_skipped(payload=payload)

    def test_non_push_events_and_non_main_refs_are_no_ops(self):
        for event in ("pull_request", "pull_request_target", "workflow_dispatch", "workflow_call"):
            with self.subTest(event=event):
                payload = automatic_payload()
                payload["workflow_run"]["event"] = event
                self.assert_skipped(payload=payload)
        payload = automatic_payload()
        payload["workflow_run"]["head_branch"] = "feature"
        self.assert_skipped(payload=payload)
        for ref in ("refs/heads/feature", "refs/tags/v0.2.1", "refs/pull/1/merge"):
            with self.subTest(ref=ref):
                self.assert_skipped(source_ref=ref)

    def test_failed_cancelled_skipped_and_unfinished_ci_are_no_ops(self):
        for conclusion in (None, "failure", "cancelled", "skipped", "neutral", "timed_out"):
            with self.subTest(conclusion=conclusion):
                payload = automatic_payload()
                payload["workflow_run"]["conclusion"] = conclusion
                self.assert_skipped(payload=payload)
        payload = automatic_payload()
        payload["workflow_run"]["status"] = "in_progress"
        self.assert_skipped(payload=payload)

    def test_wrong_repository_name_or_id_is_a_no_op(self):
        for location in ("repository", "head_repository"):
            for key, value in (("id", REPOSITORY_ID + 1), ("id", str(REPOSITORY_ID)), ("full_name", "fork/pure-protobuf")):
                with self.subTest(location=location, key=key, value=value):
                    payload = automatic_payload()
                    target = payload if location == "repository" else payload["workflow_run"]
                    target[location][key] = value
                    self.assert_skipped(payload=payload)

    def test_malformed_run_metadata_is_a_no_op(self):
        for key, value in (("id", "123"), ("id", 0), ("id", True), ("run_attempt", None), ("run_attempt", "1"), ("html_url", "https://example.invalid/run/123"), ("head_repository", [])):
            with self.subTest(key=key):
                payload = automatic_payload()
                payload["workflow_run"][key] = value
                self.assert_skipped(payload=payload)
        for payload in ({}, {"workflow_run": "bad"}, []):
            with self.subTest(payload=payload):
                self.assert_skipped(payload=payload)

    def test_automatic_request_cannot_claim_reusable_ci_success(self):
        for result in ("success", "failure", "cancelled", ""):
            with self.subTest(result=result):
                self.assert_skipped(required_ci_result=result)

    def test_tag_and_manual_requests_cannot_skip_reusable_ci(self):
        for event, payload in (("push", {}), ("workflow_dispatch", {"inputs": {"dry_run": "true"}})):
            for result in ("skipped", "failure", "cancelled", "", "neutral"):
                with self.subTest(event=event, result=result):
                    with self.assertRaisesRegex(ContextError, "successful reusable CI"):
                        request(event_name=event, payload=payload, source_ref="refs/tags/v0.2.1", required_ci_result=result)

    def test_tag_push_keeps_publish_mode_and_tag(self):
        result = request(event_name="push", payload={}, source_ref="refs/tags/v0.2.1", required_ci_result="success")
        self.assertEqual(result["mode"], "publish")
        self.assertEqual(result["release_tag"], "v0.2.1")
        self.assertEqual(result["required_ci"]["authority"], "reusable-ci")

    def test_manual_rehearsal_accepts_boolean_or_event_string(self):
        for value in (True, "true"):
            with self.subTest(value=value):
                result = request(event_name="workflow_dispatch", payload={"inputs": {"dry_run": value}}, required_ci_result="success")
                self.assertEqual(result["mode"], "dry-run")

    def test_manual_publish_requires_false_confirmation_and_release_tag(self):
        for value in (False, "false"):
            with self.subTest(value=value):
                result = request(event_name="workflow_dispatch", payload={"inputs": {"dry_run": value, "confirm": "publish"}}, source_ref="refs/tags/v0.2.1", required_ci_result="success")
                self.assertEqual(result["mode"], "publish")
        for inputs, ref in (({"dry_run": "false", "confirm": "publish"}, "refs/heads/main"), ({"dry_run": "false"}, "refs/tags/v0.2.1"), ({"confirm": "publish"}, "refs/tags/v0.2.1"), ({"dry_run": "yes", "confirm": "publish"}, "refs/tags/v0.2.1"), ({"dry_run": 1, "confirm": "publish"}, "refs/tags/v0.2.1")):
            with self.subTest(inputs=inputs, ref=ref):
                with self.assertRaises(ContextError):
                    request(event_name="workflow_dispatch", payload={"inputs": inputs}, source_ref=ref, required_ci_result="success")

    def test_main_push_and_unsupported_events_cannot_publish(self):
        for event in ("push", "pull_request", "pull_request_target", "repository_dispatch"):
            with self.subTest(event=event):
                with self.assertRaises(ContextError):
                    request(event_name=event, payload={}, required_ci_result="success")

    def test_manual_wrong_checkout_is_rejected(self):
        with self.assertRaisesRegex(ContextError, "checked-out SHA"):
            request(event_name="workflow_dispatch", payload={"inputs": {"dry_run": True}}, checked_out_sha="b" * 40, required_ci_result="success")

    def test_cli_checks_real_checkout_and_rejects_changed_context_before_writing(self):
        sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        payload = automatic_payload()
        payload["workflow_run"]["head_sha"] = sha
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            event = root / "event.json"
            event.write_text(json.dumps(payload))
            outputs = root / "outputs"
            metadata = root / "release-context.json"
            env = os.environ.copy()
            env.update({"GITHUB_EVENT_PATH": str(event), "GITHUB_EVENT_NAME": "workflow_run", "GITHUB_REPOSITORY": REPOSITORY, "GITHUB_REPOSITORY_ID": str(REPOSITORY_ID), "GITHUB_SHA": sha, "GITHUB_REF": "refs/heads/main", "REQUIRED_CI_RESULT": "skipped", "GITHUB_OUTPUT": str(outputs), "GH_TOKEN": "", "GITHUB_TOKEN": "", "CARGO_REGISTRY_TOKEN": ""})
            result = subprocess.run(["python3", "-B", str(HELPER), "--metadata-path", str(metadata)], cwd=ROOT, env=env, capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            original = json.loads(metadata.read_text())
            self.assertEqual(original["source"]["checked_out_sha"], sha)
            self.assertIn("mode=dry-run\n", outputs.read_text())
            env["RELEASE_CONTEXT_JSON"] = json.dumps(original)
            metadata.unlink()
            verified = subprocess.run(["python3", "-B", str(HELPER), "--verify-context", "--metadata-path", str(metadata)], cwd=ROOT, env=env, capture_output=True, text=True, timeout=20)
            self.assertEqual(verified.returncode, 0, verified.stderr)
            self.assertEqual(json.loads(metadata.read_text()), original)
            metadata.unlink()
            original["mode"] = "publish"
            env["RELEASE_CONTEXT_JSON"] = json.dumps(original)
            rejected = subprocess.run(["python3", "-B", str(HELPER), "--verify-context", "--metadata-path", str(metadata)], cwd=ROOT, env=env, capture_output=True, text=True, timeout=20)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertFalse(metadata.exists())
            # Changing the runner's expected SHA cannot hide the real checkout.
            payload["workflow_run"]["head_sha"] = "b" * 40
            event.write_text(json.dumps(payload))
            env["GITHUB_SHA"] = "b" * 40
            skipped = subprocess.run(["python3", "-B", str(HELPER), "--metadata-path", str(metadata)], cwd=ROOT, env=env, capture_output=True, text=True, timeout=20)
            self.assertEqual(skipped.returncode, 0, skipped.stderr)
            self.assertFalse(metadata.exists())
            self.assertIn("mode=skip\n", outputs.read_text())


class ReleaseWorkflowWiringTest(unittest.TestCase):
    def test_upstream_ci_policy_has_no_conditional_or_tolerated_required_jobs(self):
        path = ROOT / ".github/workflows/ci.yml"
        source = path.read_text() if path.exists() else subprocess.check_output(["git", "show", "HEAD:.github/workflows/ci.yml"], cwd=ROOT, text=True)
        self.assertIn("name: ci\n", source)
        jobs = source.split("\njobs:\n", 1)[1]
        self.assertTrue(re.search(r"^  [a-z][a-z0-9-]*:", jobs, re.MULTILINE))
        self.assertIsNone(re.search(r"^    (if|continue-on-error):", jobs, re.MULTILINE), "upstream-conclusion reuse requires unconditional required jobs")

    def test_publisher_still_requires_ci_and_automatic_dry_mode(self):
        source = (ROOT / ".github/workflows/release.yml").read_text()
        required = source.split("  required:\n", 1)[1].split("\n  context:\n", 1)[0]
        self.assertIn("if: github.event_name != 'workflow_run'", required)
        self.assertIn("uses: ./.github/workflows/ci.yml", required)
        publish = source.split("  publish:\n", 1)[1]
        condition = publish.split("    runs-on:", 1)[0]
        self.assertIn("needs: [required, context]", condition)
        self.assertIn("needs.context.outputs.eligible == 'true'", condition)
        self.assertIn("github.event_name == 'workflow_run' && needs.required.result == 'skipped' && needs.context.outputs.mode == 'dry-run'", condition)
        self.assertIn("github.event_name != 'workflow_run' && needs.required.result == 'success'", condition)
        self.assertIn("ref: ${{ github.sha }}", source)
        self.assertIn("ref: ${{ needs.context.outputs.source_sha }}", publish)
        self.assertIn("--verify-context --metadata-path dist/release-context.json", publish)

    def test_rehearsal_has_a_separate_concurrency_group(self):
        source = (ROOT / ".github/workflows/release.yml").read_text()
        concurrency = source.split("\nconcurrency:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertIn("github.event_name == 'workflow_run'", concurrency)
        self.assertIn("github.event_name == 'workflow_dispatch' && inputs.dry_run", concurrency)
        self.assertIn("format('release-rehearsal-{0}', github.sha)", concurrency)
        self.assertIn("|| 'crates-io-publish'", concurrency)
        self.assertIn("cancel-in-progress: false", concurrency)

    def test_only_publish_mode_can_authenticate_or_create_a_release(self):
        auth = workflow_step("Authenticate to crates.io with OIDC")
        self.assertIn("if: steps.mode.outputs.mode == 'publish'", auth)
        self.assertIn("continue-on-error: true", auth)
        tag = workflow_step("Require a signed and GitHub-verified release tag")
        self.assertIn("if: steps.mode.outputs.mode == 'publish'", tag)
        release = workflow_step("Create GitHub Release")
        self.assertIn("startsWith(github.ref, 'refs/tags/v') && steps.mode.outputs.mode == 'publish'", release)
        packing = workflow_step("Publish crates")
        self.assertIn(
            "steps.mode.outputs.mode == 'publish' && (steps.crates-io.outputs.token || secrets.CRATES_IO_TOKEN) || ''",
            packing,
        )
        self.assertIn("steps.mode.outputs.mode == 'dry-run' && '1' || '0'", packing)
        self.assertIn("run: ./scripts/publish-crates.sh", packing)
        source = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertEqual(source.count("uses: rust-lang/crates-io-auth-action@"), 1)
        self.assertEqual(source.count("run: ./scripts/publish-crates.sh"), 1)

    def test_context_has_read_permissions_and_attested_metadata_uses_selected_sha(self):
        source = (ROOT / ".github/workflows/release.yml").read_text()
        context = source.split("  context:\n", 1)[1].split("\n  publish:\n", 1)[0]
        self.assertIn("      contents: read\n", context)
        self.assertNotIn("id-token:", context)
        self.assertNotIn("attestations:", context)
        self.assertNotIn("contents: write", context)
        publish = source.split("  publish:\n", 1)[1]
        self.assertLess(publish.index("--verify-context"), publish.index("Install Rust toolchain"))
        self.assertLess(publish.index("--verify-context"), publish.index("Authenticate to crates.io"))
        self.assertIn("output-file: dist/pure-protobuf-${{ needs.context.outputs.source_sha }}.cdx.json", publish)
        self.assertIn("name: release-provenance-${{ needs.context.outputs.source_sha }}", publish)
        self.assertIn("subject-path: dist/*", publish)

    @unittest.skipUnless(shutil.which("jq"), "the existing tag step requires jq")
    def test_actual_tag_step_rejects_unsigned_lightweight_misnamed_or_retargeted_tags(self):
        step = workflow_step("Require a signed and GitHub-verified release tag")
        script = textwrap.dedent(step.split("        run: |\n", 1)[1])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            gh = root / "gh"
            gh.write_text("#!/usr/bin/env python3\nimport json,os,sys\np=sys.argv[2]\nif '/git/ref/tags/' in p: print(json.dumps(json.loads(os.environ['FAKE_REF'])['object']))\nelif '/git/tags/' in p: print(os.environ['FAKE_TAG'])\nelse: raise SystemExit(99)\n")
            gh.chmod(0o755)
            cases = (
                ("tag", True, "v0.2.1", "commit", SHA, 0),
                ("commit", True, "v0.2.1", "commit", SHA, 1),
                ("tag", False, "v0.2.1", "commit", SHA, 1),
                ("tag", True, "v0.2.1", "commit", "b" * 40, 1),
                ("tag", True, "v0.2.1", "tag", SHA, 1),
                # A ref alias must not reuse a valid signature for another name.
                ("tag", True, "review-candidate", "commit", SHA, 1),
                ("tag", True, "v0.2.0", "commit", SHA, 1),
                ("tag", True, None, "commit", SHA, 1),
            )
            for kind, verified, signed_name, target_type, target_sha, expected in cases:
                with self.subTest(kind=kind, verified=verified, signed_name=signed_name, target_type=target_type, target_sha=target_sha):
                    env = os.environ.copy()
                    env.update({"PATH": str(root) + os.pathsep + env["PATH"], "GH_TOKEN": "", "GITHUB_REPOSITORY": REPOSITORY, "GITHUB_REF_NAME": "v0.2.1", "RELEASE_SOURCE_SHA": SHA, "FAKE_REF": json.dumps({"object": {"type": kind, "sha": "c" * 40}}), "FAKE_TAG": json.dumps({"tag": signed_name, "verification": {"verified": verified}, "object": {"type": target_type, "sha": target_sha}})})
                    result = subprocess.run(["bash", "-c", script], env=env, capture_output=True, text=True, timeout=20)
                    self.assertEqual(result.returncode, expected, result.stderr + result.stdout)


if __name__ == "__main__":
    unittest.main()
