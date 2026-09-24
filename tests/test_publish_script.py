"""Release-script tests that cannot contact a registry or upload crates."""

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class PublishScriptTest(unittest.TestCase):
    def run_script(
        self, *, dry_run: bool | str, curl_code: str,
        publish_exit: int = 3, git_dirty: bool = False,
        mismatched_dependency: tuple[str, str, str] | None = None,
    ):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo = root / "repo"
            for relative in (
                "scripts/publish-crates.sh",
                "Cargo.toml",
                "protobuf-tonic/Cargo.toml",
                "pbrs-grpc/Cargo.toml",
            ):
                destination = repo / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes((ROOT / relative).read_bytes())
            if mismatched_dependency:
                crate, section, kind = mismatched_dependency
                manifest = repo / crate / "Cargo.toml"
                content = manifest.read_text()
                header = f"[{section}]\n"
                start = content.index(header) + len(header)
                end = content.find("\n[", start)
                if end < 0:
                    end = len(content)
                block = content[start:end]

                def mismatch(match):
                    line = match.group()
                    if kind == "version":
                        return re.sub(r'version = "[^"]+"', 'version = "9.9.9"', line)
                    if kind == "path":
                        return line.replace('path = ".."', 'path = "../not-core"')
                    raise AssertionError(f"unknown mismatch kind: {kind}")

                mismatched, count = re.subn(
                    r'(?m)^pbrs = \{ version = "[^"]+", path = "\.\." \}$',
                    mismatch,
                    block,
                    count=1,
                )
                self.assertEqual(count, 1, f"missing pbrs constraint in {crate} {section}")
                manifest.write_text(content[:start] + mismatched + content[end:])
            bin_dir = root / "bin"
            bin_dir.mkdir()
            calls_path = root / "calls.txt"
            curl = bin_dir / "curl"
            curl.write_text(
                "#!/usr/bin/env bash\n"
                "printf 'curl\\n' >> \"$FAKE_CALLS_FILE\"\n"
                "if [[ \"$FAKE_CURL_CODE\" == network_error ]]; then exit 7; fi\n"
                "if [[ \"$FAKE_CURL_CODE\" == 404_then_200 ]]; then\n"
                "    if [[ \"$(grep -c '^curl$' \"$FAKE_CALLS_FILE\")\" == 1 ]]; then\n"
                "        printf '404'\n"
                "    else\n"
                "        printf '200'\n"
                "    fi\n"
                "    exit 0\n"
                "fi\n"
                "printf '%s' \"$FAKE_CURL_CODE\"\n"
            )
            cargo = bin_dir / "cargo"
            cargo.write_text(
                "#!/usr/bin/env bash\n"
                "printf 'cargo %s\\n' \"$*\" >> \"$FAKE_CALLS_FILE\"\n"
                "if [[ \"$1\" == publish ]]; then exit \"${FAKE_PUBLISH_EXIT:-3}\"; fi\n"
            )
            git = bin_dir / "git"
            git.write_text(
                "#!/usr/bin/env bash\n"
                "printf 'git %s\\n' \"$*\" >> \"$FAKE_CALLS_FILE\"\n"
                "if [[ \"$1\" == diff && \"$FAKE_GIT_DIRTY\" == 1 ]]; then exit 1; fi\n"
            )
            curl.chmod(0o755)
            cargo.chmod(0o755)
            git.chmod(0o755)
            env = os.environ.copy()
            env.update(
                {
                    "PATH": f"{bin_dir}{os.pathsep}{env['PATH']}",
                    "DRY_RUN": ("1" if dry_run else "0") if isinstance(dry_run, bool) else dry_run,
                    "FAKE_CALLS_FILE": str(calls_path),
                    "FAKE_CURL_CODE": curl_code,
                    "FAKE_PUBLISH_EXIT": str(publish_exit),
                    "FAKE_GIT_DIRTY": "1" if git_dirty else "0",
                    "CARGO_REGISTRY_TOKEN": "not-a-real-token",
                }
            )
            proc = subprocess.run(
                ["bash", str(repo / "scripts/publish-crates.sh")], cwd=repo, env=env,
                capture_output=True, text=True, timeout=20,
            )
            calls = calls_path.read_text().splitlines() if calls_path.exists() else []
            return proc, calls

    def test_registry_failure_does_not_try_to_publish(self):
        for code in ("503", "network_error"):
            with self.subTest(code=code):
                proc, calls = self.run_script(dry_run=False, curl_code=code)
                self.assertNotEqual(proc.returncode, 0)
                self.assertNotIn("cargo publish", "\n".join(calls))
                self.assertIn("crates.io API", proc.stderr)

    def test_dry_run_packs_without_registry_access(self):
        proc, calls = self.run_script(dry_run=True, curl_code="network_error")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertFalse(any(call == "curl" for call in calls))
        packages = [call for call in calls if call.startswith("cargo package")]
        self.assertEqual(len(packages), 3)
        self.assertNotIn("--config", packages[0])
        for call in packages[1:]:
            self.assertRegex(
                call,
                r'--config patch\.crates-io\.pbrs\.path=".*/pbrs-release-stage\.[^"]+"',
            )
        self.assertTrue(any(call.startswith("git worktree add ") for call in calls))
        self.assertTrue(any(call.startswith("git worktree remove ") for call in calls))

    def test_dry_run_refuses_dirty_shipping_sources(self):
        proc, calls = self.run_script(
            dry_run=True, curl_code="network_error", git_dirty=True,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("requires committed crate sources", proc.stderr)
        self.assertFalse(any(call.startswith("cargo package") for call in calls))

    def test_existing_versions_are_skipped_without_upload(self):
        proc, calls = self.run_script(dry_run=False, curl_code="200")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertFalse(any(call.startswith("cargo publish") for call in calls))
        self.assertEqual(sum(call == "curl" for call in calls), 3)

    def test_new_version_waits_for_index_before_continuing(self):
        proc, calls = self.run_script(dry_run=False, curl_code="404_then_200", publish_exit=0)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(sum(call.startswith("cargo publish") for call in calls), 1)
        self.assertEqual(sum(call == "curl" for call in calls), 4)

    def test_mismatched_adapter_core_dependency_fails_before_registry_or_packaging(self):
        for crate in ("protobuf-tonic", "pbrs-grpc"):
            for section in ("build-dependencies", "dependencies"):
                for kind in ("version", "path"):
                    for dry_run in (False, True):
                        with self.subTest(
                            crate=crate, section=section, kind=kind, dry_run=dry_run,
                        ):
                            proc, calls = self.run_script(
                                dry_run=dry_run,
                                curl_code="404",
                                mismatched_dependency=(crate, section, kind),
                            )
                            self.assertNotEqual(proc.returncode, 0)
                            reason = "must require" if kind == "version" else "must point"
                            self.assertIn(f"{crate} {section}.pbrs {reason}", proc.stderr)
                            self.assertFalse(any(call == "curl" for call in calls))
                            self.assertFalse(any(call.startswith("cargo ") for call in calls))

    def test_invalid_dry_run_mode_does_not_turn_into_upload(self):
        proc, calls = self.run_script(dry_run="yes", curl_code="404")
        self.assertNotEqual(proc.returncode, 0)
        self.assertFalse(any(call.startswith("cargo publish") for call in calls))
        self.assertIn("DRY_RUN", proc.stderr)


if __name__ == "__main__":
    unittest.main()
