"""Release-script tests that cannot contact a registry or upload crates."""

import json
import os
import re
import subprocess
import tempfile
import textwrap
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class PublishScriptTest(unittest.TestCase):
    def run_script(
        self, *, dry_run: bool | str, curl_code: str,
        publish_exit: int = 3, git_dirty: bool = False,
        mismatched_dependency: tuple[str, str, str] | None = None,
        curl_codes: tuple[str, ...] = (), registry_token: str = "not-a-real-token",
        collect_artifacts: bool = False, stale_archives: bool = False,
        missing_archive: str | None = None,
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
            curl.write_text(textwrap.dedent(r"""
                #!/usr/bin/env python3
                import json
                import os
                from pathlib import Path
                import sys

                calls = Path(os.environ["FAKE_CALLS_FILE"])
                previous = calls.read_text().splitlines() if calls.exists() else []
                index = previous.count("curl")
                with calls.open("a") as output:
                    output.write("curl\n")
                codes = json.loads(os.environ["FAKE_CURL_CODES"])
                code = codes[index] if index < len(codes) else os.environ["FAKE_CURL_CODE"]
                if code == "network_error":
                    raise SystemExit(7)
                if code == "404_then_200":
                    code = "404" if index == 0 else "200"
                print(code, end="")
            """).lstrip())
            cargo = bin_dir / "cargo"
            cargo.write_text(textwrap.dedent(r"""
                #!/usr/bin/env python3
                import os
                from pathlib import Path
                import sys
                import tomllib

                args = sys.argv[1:]
                with Path(os.environ["FAKE_CALLS_FILE"]).open("a") as output:
                    output.write("cargo " + " ".join(args) + "\n")
                if args[0] == "publish" and int(os.environ["FAKE_PUBLISH_EXIT"]):
                    raise SystemExit(int(os.environ["FAKE_PUBLISH_EXIT"]))
                if args[0] in ("package", "publish"):
                    name = args[args.index("-p") + 1]
                    root = Path(os.environ["FAKE_REPO_ROOT"])
                    manifests = {
                        "pbrs": root / "Cargo.toml",
                        "protobuf-tonic": root / "protobuf-tonic/Cargo.toml",
                        "pbrs-grpc": root / "pbrs-grpc/Cargo.toml",
                    }
                    package = tomllib.loads(manifests[name].read_text())["package"]
                    target = Path(os.environ.get("CARGO_TARGET_DIR", str(root / "target")))
                    if not target.is_absolute():
                        target = root / target
                    package_dir = target / "package"
                    package_dir.mkdir(parents=True, exist_ok=True)
                    archive = package_dir / f"{name}-{package['version']}.crate"
                    archive.write_text(f"mock archive from {args[0]}\n")
            """).lstrip())
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
                    "FAKE_CURL_CODES": json.dumps(curl_codes),
                    "FAKE_REPO_ROOT": str(repo),
                    "FAKE_PUBLISH_EXIT": str(publish_exit),
                    "FAKE_GIT_DIRTY": "1" if git_dirty else "0",
                    "CARGO_REGISTRY_TOKEN": registry_token,
                }
            )
            # Keep the fixture independent of the caller's build target.
            env.pop("CARGO_TARGET_DIR", None)
            package_dir = repo / "target/package"
            if stale_archives:
                package_dir.mkdir(parents=True)
                (package_dir / "unrelated-9.9.9.crate").write_text("stale unrelated archive\n")
                (package_dir / "pbrs-0.0.0.crate").write_text("stale old version\n")
            proc = subprocess.run(
                ["bash", str(repo / "scripts/publish-crates.sh")], cwd=repo, env=env,
                capture_output=True, text=True, timeout=20,
            )
            collection = None
            if proc.returncode == 0 and collect_artifacts:
                if missing_archive:
                    for relative in ("Cargo.toml", "protobuf-tonic/Cargo.toml", "pbrs-grpc/Cargo.toml"):
                        package = tomllib.loads((repo / relative).read_text())["package"]
                        if package["name"] == missing_archive:
                            (package_dir / f"{package['name']}-{package['version']}.crate").unlink()
                workflow = (ROOT / ".github/workflows/release.yml").read_text()
                step = workflow.split("      - name: Collect packaged crates\n", 1)[1]
                step = step.split("\n      - name:", 1)[0]
                script = textwrap.dedent(step.split("        run: |\n", 1)[1])
                collection = subprocess.run(
                    ["bash", "-c", script], cwd=repo, env=env,
                    capture_output=True, text=True, timeout=20,
                )
            calls = calls_path.read_text().splitlines() if calls_path.exists() else []
            artifacts = {
                "collection": collection,
                "packaged": sorted(archive.name for archive in package_dir.glob("*.crate")),
                "collected": {
                    archive.name: archive.read_text()
                    for archive in (repo / "dist").glob("*.crate")
                },
            }
            return proc, calls, artifacts

    def test_registry_failure_does_not_try_to_publish(self):
        for code in ("401", "429", "503", "network_error"):
            with self.subTest(code=code):
                proc, calls, _ = self.run_script(dry_run=False, curl_code=code)
                self.assertNotEqual(proc.returncode, 0)
                self.assertNotIn("cargo publish", "\n".join(calls))
                self.assertIn("crates.io API", proc.stderr)

    def test_dry_run_packs_without_registry_access(self):
        proc, calls, _ = self.run_script(dry_run=True, curl_code="network_error")
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
        proc, calls, _ = self.run_script(
            dry_run=True, curl_code="network_error", git_dirty=True,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("requires committed crate sources", proc.stderr)
        self.assertFalse(any(call.startswith("cargo package") for call in calls))

    def expected_archives(self):
        archives = set()
        for relative in ("Cargo.toml", "protobuf-tonic/Cargo.toml", "pbrs-grpc/Cargo.toml"):
            package = tomllib.loads((ROOT / relative).read_text())["package"]
            archives.add(f"{package['name']}-{package['version']}.crate")
        return archives

    def assert_complete_collection(self, artifacts):
        collection = artifacts["collection"]
        self.assertIsNotNone(collection)
        self.assertEqual(collection.returncode, 0, collection.stderr + collection.stdout)
        self.assertEqual(set(artifacts["collected"]), self.expected_archives())

    def test_all_existing_versions_repack_and_collect_without_upload_or_token(self):
        proc, calls, artifacts = self.run_script(
            dry_run=False, curl_code="200", registry_token="", collect_artifacts=True,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertFalse(any(call.startswith("cargo publish") for call in calls))
        self.assertEqual(sum(call.startswith("cargo package") for call in calls), 3)
        self.assertEqual(sum(call == "curl" for call in calls), 3)
        self.assert_complete_collection(artifacts)
        self.assertTrue(all(value == "mock archive from package\n" for value in artifacts["collected"].values()))

    def test_partial_retry_collects_existing_and_newly_published_versions(self):
        proc, calls, artifacts = self.run_script(
            dry_run=False, curl_code="network_error", publish_exit=0,
            curl_codes=("200", "404", "200", "200"), collect_artifacts=True,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        operations = [call for call in calls if call.startswith("cargo ")]
        self.assertEqual(operations, [
            "cargo package -p pbrs --no-verify",
            "cargo publish -p protobuf-tonic",
            "cargo package -p pbrs-grpc --no-verify",
        ])
        self.assertEqual(sum(call == "curl" for call in calls), 4)
        self.assert_complete_collection(artifacts)
        self.assertEqual(sum(value == "mock archive from publish\n" for value in artifacts["collected"].values()), 1)

    def test_collection_ignores_unrelated_and_old_version_archives(self):
        proc, _, artifacts = self.run_script(
            dry_run=False, curl_code="200", registry_token="",
            collect_artifacts=True, stale_archives=True,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("unrelated-9.9.9.crate", artifacts["packaged"])
        self.assertIn("pbrs-0.0.0.crate", artifacts["packaged"])
        self.assert_complete_collection(artifacts)

    def test_collection_rejects_a_missing_expected_archive(self):
        proc, _, artifacts = self.run_script(
            dry_run=False, curl_code="200", registry_token="",
            collect_artifacts=True, missing_archive="protobuf-tonic",
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertNotEqual(artifacts["collection"].returncode, 0)
        self.assertIn("missing release archive:", artifacts["collection"].stderr)
        self.assertEqual(artifacts["collected"], {})

    def test_new_version_waits_for_index_before_continuing(self):
        proc, calls, _ = self.run_script(dry_run=False, curl_code="404_then_200", publish_exit=0)
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
                            proc, calls, _ = self.run_script(
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
        proc, calls, _ = self.run_script(dry_run="yes", curl_code="404")
        self.assertNotEqual(proc.returncode, 0)
        self.assertFalse(any(call.startswith("cargo publish") for call in calls))
        self.assertIn("DRY_RUN", proc.stderr)


if __name__ == "__main__":
    unittest.main()
