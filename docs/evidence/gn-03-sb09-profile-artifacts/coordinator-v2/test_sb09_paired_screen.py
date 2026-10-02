#!/usr/bin/env python3
"""No-build coordinator test using a stub compiler for both cold profiles."""

import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("sb09-paired-screen.py")
spec = importlib.util.spec_from_file_location("paired_screen", SCRIPT)
coordinator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coordinator)


class PairedCoordinatorTest(unittest.TestCase):
    def test_both_profiles_use_common_generator_and_preserve_linked_binaries(self):
        with tempfile.TemporaryDirectory(prefix="gn03-coordinator-test-") as directory:
            root = Path(directory)
            (root / "target/base").mkdir(parents=True)
            generator = root / "frozen-generator"
            generator.write_bytes(b"this is the common generator, never the linked consumer")
            generator_hash = hashlib.sha256(generator.read_bytes()).hexdigest()
            tool = root / "stub-tool"
            tool.write_bytes(b"compiler stub; no live compilation or measurement")
            source = {"test-only-source": "fixed"}
            tools = {name: {"executable": str(tool)} for name in ("cargo", "rustc", "protoc")}
            observed = []
            reclaimed = []
            real_rmtree = shutil.rmtree

            def prepare(case_dir, case, seed, generators):
                consumer = case_dir / "consumer"
                consumer.mkdir(parents=True)
                (consumer / "Cargo.toml").write_text("same consumer manifest for both profiles\n")
                return ["one.proto", "two.proto"], {"sha256": "same-input-corpus"}

            def compiler(report, case, names, corpus, repeat, case_dir, binary,
                         cargo, protoc, env, out, timeout, sample_ms, reference, generation_only):
                self.assertEqual(binary, generator)
                self.assertEqual(hashlib.sha256(binary.read_bytes()).hexdigest(), generator_hash)
                profile = "shared" if env.get("SB09_PBRS_SHARED_DESCRIPTOR_SET") == "true" else "default"
                observed.append((profile, binary))
                target = case_dir / "target-r0"
                self.assertFalse(target.exists(), "each profile must begin with a nonexistent cold target")
                (target / "release/.fingerprint/stub").mkdir(parents=True)
                (target / "release/.fingerprint/stub/input").write_text("stub compiler fingerprint")
                linked = target / "release/consumer"
                linked.write_bytes(("linked " + profile).encode())
                report["cells"].append({
                    "corpus": corpus,
                    "consumer_lock_sha256": "same-locked-consumer",
                    "target_dir": coordinator.h.relative(target, out),
                    "release_binary": {
                        "path": coordinator.h.relative(linked, out),
                        "sha256": coordinator.h.sha256(linked),
                    },
                })

            def reclaim(target, *args, **kwargs):
                out = target.parents[2]
                preserved = out / "linked/consumer"
                self.assertTrue(preserved.is_file(), "retain linked ELF before reclaiming its target")
                self.assertEqual(preserved.read_bytes(), (target / "release/consumer").read_bytes())
                self.assertTrue((out / "fingerprint-sha256.json").is_file())
                reclaimed.append(target)
                real_rmtree(target, *args, **kwargs)

            def check_output(command, **kwargs):
                if command[0] == "rustup":
                    return str(tool) + "\n"
                if "--porcelain" in command:
                    return ""
                return "test-source-commit\n"

            with contextlib.ExitStack() as stack:
                stack.enter_context(patch.object(coordinator, "ROOT", root))
                stack.enter_context(patch.object(coordinator, "bootstrap_once", return_value=(generator, {
                    "binary_sha256": generator_hash,
                })))
                stack.enter_context(patch.object(coordinator.h, "source_hashes", return_value=source))
                stack.enter_context(patch.object(coordinator.h, "provenance", return_value={
                    "repository": {"source_sha256": source}, "tools": tools,
                }))
                stack.enter_context(patch.object(coordinator.h, "prepare_corpus", side_effect=prepare))
                stack.enter_context(patch.object(coordinator.h, "measure_pbrs_cell", side_effect=compiler))
                stack.enter_context(patch.object(coordinator.h, "compute_matrix"))
                stack.enter_context(patch.object(coordinator.h, "matrix_metrics", return_value={"stub": 1}))
                stack.enter_context(patch.object(coordinator.subprocess, "check_output", side_effect=check_output))
                stack.enter_context(patch.object(coordinator.shutil, "which", return_value=str(tool)))
                stack.enter_context(patch.object(coordinator.shutil, "rmtree", side_effect=reclaim))
                stack.enter_context(patch.object(sys, "argv", [str(SCRIPT), "--case", "small",
                    "--campaign", "test-pair", "--execute", "--lease", "test-only-no-measurement"]))
                stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
                try:
                    coordinator.main()
                except RuntimeError:
                    for summary in root.glob("target/codegen-bench/test-pair/*/summary.json"):
                        print(summary.read_text(), file=sys.stderr)
                    raise

            self.assertEqual(observed, [("default", generator), ("shared", generator)])
            self.assertEqual(len(reclaimed), 2)
            self.assertTrue(all(not target.exists() for target in reclaimed))
            result = json.loads((root / "target/codegen-bench/test-pair/pair-small.json").read_text())
            self.assertEqual(result["status"], "complete-local-diagnostic")
            self.assertEqual(len(result["runs"]), 2)
            self.assertTrue(all("preserved_binary" in run for run in result["runs"]))
            self.assertEqual(generator_hash, coordinator.h.sha256(generator))


if __name__ == "__main__":
    unittest.main()
