"""Bundled regeneration contracts, without compiling Rust or downloading tools."""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BINDINGS = sorted(path.name for path in (ROOT / "src/generated").glob("*.rs")
                  if path.name != "mod.rs")


class RegenerationTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "repo"
        for relative in ("scripts/regen-generated.sh", "scripts/gen.sh",
                         "src/generated/mod.rs", "vendor/google/PIN",
                         "vendor/google/SHA", "rustfmt.toml"):
            destination = self.repo / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, destination)
        self.generated = self.repo / "src/generated"
        self.registry = (self.generated / "mod.rs").read_bytes()
        for name in BINDINGS:
            (self.generated / name).write_text(f"stale {name}\n")
        (self.generated / "untracked.txt").write_text("keep this scratch file\n")
        (self.generated / "scratch.rs").write_text("keep this unrelated Rust file\n")
        (self.generated / "google/protobuf").mkdir(parents=True)
        (self.generated / "google/protobuf/untracked.rs").write_text("keep hierarchy\n")
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.source = self.repo / "third_party/protobuf"
        self.source.mkdir(parents=True)
        self.target = self.repo / "target"
        self.build = self.target / "conformance-build"
        self.build.mkdir(parents=True)
        pin = (self.repo / "vendor/google/PIN").read_text().strip()
        sha = (self.repo / "vendor/google/SHA").read_text().strip()
        (self.build / ".pbrs-protobuf-pin").write_text(f"{pin} {sha}\n")
        self.write_executable(self.bin / "git", """#!/usr/bin/env bash
if [[ "$*" == *"rev-parse HEAD"* ]]; then
  printf '%s\\n' "${FAKE_SOURCE_SHA:-$FAKE_PIN_SHA}"
elif [[ "$*" == *"diff --quiet"* ]]; then
  exit "${FAKE_SOURCE_DIRTY:-0}"
elif [[ "$*" == *"ls-files"* ]]; then
  printf '%s' "${FAKE_UNTRACKED_PROTO:-}"
fi
""")
        self.write_executable(self.bin / "cargo", """#!/usr/bin/env python3
import json
import os
from pathlib import Path
target = Path(os.environ['CARGO_TARGET_DIR'])
if os.environ.get('CARGO_BUILD_TARGET'):
    target /= os.environ['CARGO_BUILD_TARGET']
plugin = target / 'debug/protoc-gen-pbrs'
plugin.parent.mkdir(parents=True, exist_ok=True)
plugin.write_text('#!/usr/bin/env bash\\nexit 0\\n')
plugin.chmod(0o755)
if not os.environ.get('FAKE_MISSING_PLUGIN_ARTIFACT'):
    print(json.dumps({'reason': 'compiler-artifact',
                      'manifest_path': str(Path.cwd() / 'Cargo.toml'),
                      'target': {'name': 'protoc-gen-pbrs', 'kind': ['bin']},
                      'executable': str(plugin)}))
""")
        self.write_executable(self.bin / "rustfmt", "#!/usr/bin/env bash\nexit 0\n")
        self.write_executable(self.build / "protoc", """#!/usr/bin/env python3
import os
import sys
from pathlib import Path
if sys.argv[1:] == ['--version']:
    print(os.environ.get('FAKE_PROTOC_VERSION', 'libprotoc 35.1'))
    sys.exit(0)
if os.environ.get('FAKE_PLUGIN_SELECTION_LOG'):
    selected = next(arg.split('=', 2)[2] for arg in sys.argv if arg.startswith('--plugin='))
    with Path(os.environ['FAKE_PLUGIN_SELECTION_LOG']).open('a') as log:
        log.write(selected + '\\n')
out_argument = next(arg.split('=', 1)[1] for arg in sys.argv if arg.startswith('--pbrs_out='))
out = Path(out_argument.rsplit(':', 1)[-1])
proto = Path(sys.argv[-1])
name = proto.with_suffix('.rs').name
if os.environ.get('FAKE_MISSING_OUTPUT') == name:
    sys.exit(0)
out.mkdir(parents=True, exist_ok=True)
ambient = os.environ.get('PURE_PROTOBUF_RUNTIME_CRATE', '')
(out / name).write_text(f'fresh {name}{ambient}\\n')
(out / 'mod.rs').write_text('generated registry must not replace handwritten registry\\n')
hierarchy = out / 'google/protobuf'
hierarchy.mkdir(parents=True, exist_ok=True)
(hierarchy / name).write_text('hierarchical output must remain staged\\n')
""")
        # The old script uses PATH; the new one requires the pinned build.
        (self.bin / "protoc").symlink_to(self.build / "protoc")
        self.environment = {
            **os.environ,
            "PATH": f"{self.bin}:{os.environ['PATH']}",
            "CARGO_TARGET_DIR": str(self.target),
            "FAKE_PIN_SHA": sha,
            "PURE_PROTOBUF_RUNTIME_CRATE": "ambient-setting-must-not-leak",
        }

    @staticmethod
    def write_executable(path, content):
        path.write_text(content)
        path.chmod(0o755)

    def run_script(self, *arguments, **environment):
        return subprocess.run(
            ["bash", "scripts/regen-generated.sh", *arguments],
            cwd=self.repo, env={**self.environment, **environment},
            capture_output=True, text=True,
        )

    def snapshot(self):
        return {path.relative_to(self.generated).as_posix():
                (path.read_bytes(), path.stat().st_mtime_ns)
                for path in self.generated.rglob("*") if path.is_file()}

    def install_fresh_bindings(self):
        for name in BINDINGS:
            (self.generated / name).write_text(f"fresh {name}\n")

    def test_generation_preserves_registry_and_untracked_files_and_repeats(self):
        before = self.snapshot()
        first = self.run_script()
        self.assertEqual(first.returncode, 0, first.stderr)
        after = self.snapshot()
        self.assertEqual(set(before), set(after))
        for name in BINDINGS:
            self.assertEqual(after[name][0], f"fresh {name}\n".encode())
        for name in set(before) - set(BINDINGS):
            self.assertEqual(before[name], after[name])
        second = self.run_script()
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(after, self.snapshot(), "unchanged bindings must not be rewritten")

    def test_check_passes_without_changing_any_binding_or_scratch_file(self):
        self.install_fresh_bindings()
        before = self.snapshot()
        result = self.run_script("--check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(before, self.snapshot())

    def test_check_rejects_drift_without_changing_any_file(self):
        self.install_fresh_bindings()
        (self.generated / "field_mask.rs").write_text("intentional drift\n")
        before = self.snapshot()
        result = self.run_script("--check")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("field_mask.rs", result.stderr)
        self.assertEqual(before, self.snapshot())

    def test_provenance_mismatches_fail_before_writing(self):
        for environment in ({"FAKE_SOURCE_SHA": "wrong-sha"},
                            {"FAKE_SOURCE_DIRTY": "1"},
                            {"FAKE_UNTRACKED_PROTO": "src/google/protobuf/untracked.proto"},
                            {"FAKE_PROTOC_VERSION": "libprotoc 34.0"}):
            with self.subTest(environment=environment):
                before = self.snapshot()
                result = self.run_script(**environment)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(before, self.snapshot())
        (self.build / ".pbrs-protobuf-pin").write_text("wrong pin stamp\n")
        before = self.snapshot()
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(before, self.snapshot())

    def test_missing_late_output_never_partially_updates_bindings(self):
        before = self.snapshot()
        result = self.run_script(FAKE_MISSING_OUTPUT="wrappers.rs")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("wrappers.rs", result.stderr)
        self.assertEqual(before, self.snapshot())

    def test_registry_scope_mismatch_is_rejected(self):
        with (self.generated / "mod.rs").open("a") as registry:
            registry.write("pub mod future_binding;\n")
        before = self.snapshot()
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("registry", result.stderr)
        self.assertEqual(before, self.snapshot())

    def test_custom_cargo_target_still_uses_fixed_pinned_build(self):
        self.install_fresh_bindings()
        (self.bin / "protoc").unlink()
        self.write_executable(self.bin / "protoc", "#!/usr/bin/env bash\nexit 9\n")
        before = self.snapshot()
        result = self.run_script("--check", CARGO_TARGET_DIR=str(self.root / "cargo-target"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / "cargo-target/debug/protoc-gen-pbrs").is_file())
        self.assertEqual(before, self.snapshot())

    def test_symlinked_binding_cannot_overwrite_an_unrelated_file(self):
        unrelated = self.root / "unrelated.rs"
        unrelated.write_text("preserve external data\n")
        (self.generated / "field_mask.rs").unlink()
        (self.generated / "field_mask.rs").symlink_to(unrelated)
        before = self.snapshot()
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("symlink", result.stderr)
        self.assertEqual(before, self.snapshot())
        self.assertEqual(unrelated.read_text(), "preserve external data\n")

    def test_invalid_arguments_do_not_trigger_generation(self):
        for arguments in (("--unknown",), ("--check", "extra"), ("",)):
            with self.subTest(arguments=arguments):
                before = self.snapshot()
                result = self.run_script(*arguments)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn("usage", result.stderr)
                self.assertEqual(before, self.snapshot())

    def test_target_specific_current_plugin_cannot_fall_back_to_stale_default(self):
        self.install_fresh_bindings()
        stale = self.target / "debug/protoc-gen-pbrs"
        stale.parent.mkdir(parents=True, exist_ok=True)
        self.write_executable(stale, "#!/usr/bin/env bash\n# stale default plugin\nexit 0\n")
        selections = self.root / "plugin-selections.txt"
        triple = "x86_64-unknown-linux-gnu"
        before = self.snapshot()
        result = self.run_script("--check", CARGO_BUILD_TARGET=triple,
                                 FAKE_PLUGIN_SELECTION_LOG=str(selections))
        self.assertEqual(result.returncode, 0, result.stderr)
        current = self.target / triple / "debug/protoc-gen-pbrs"
        self.assertTrue(current.is_file())
        self.assertEqual(set(selections.read_text().splitlines()), {str(current)})
        self.assertEqual(stale.read_text(), "#!/usr/bin/env bash\n# stale default plugin\nexit 0\n")
        self.assertEqual(before, self.snapshot())

    def test_missing_cargo_artifact_cannot_fall_back_to_stale_default(self):
        stale = self.target / "debug/protoc-gen-pbrs"
        stale.parent.mkdir(parents=True, exist_ok=True)
        self.write_executable(stale, "#!/usr/bin/env bash\nexit 0\n")
        before = self.snapshot()
        result = self.run_script(FAKE_MISSING_PLUGIN_ARTIFACT="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(before, self.snapshot())


if __name__ == "__main__":
    unittest.main()
