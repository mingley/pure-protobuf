"""Python-only CG-19 harness checks; no Cargo, rustc, or protoc is invoked."""

import contextlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import run as harness


class CorpusTests(unittest.TestCase):
    def test_seeded_multifile_corpora_have_exact_message_counts(self):
        default_hashes = {
            "small": "bc8f323ff561e0128d96453c1ed20dea33ee032f88b6decde4e8cd74ba2e18be",
            "100": "9d7d15ec3862ea089aaff2ccb73387bd669bff87339414ba88c7525fb5b5e38b",
            "1000": "2efa91c63654eff5249cd4ef60a0a8d1bfdbff9a26db011650b67e32807907c4",
        }
        with tempfile.TemporaryDirectory() as temporary:
            for case, (messages, files) in harness.CORPORA.items():
                case_dir = Path(temporary) / case
                names, metadata = harness.prepare_corpus(case_dir, case, harness.DEFAULT_SEED)
                self.assertEqual(len(names), files)
                self.assertEqual(metadata["messages"], messages)
                self.assertEqual(metadata["sha256"], default_hashes[case])
                sources = [
                    (case_dir / "consumer" / "proto" / name).read_text()
                    for name in names
                ]
                self.assertEqual(sum(text.count("\nmessage Message") for text in sources), messages)
                for index, text in enumerate(sources):
                    self.assertIn('syntax = "proto3";', text)
                    if index:
                        self.assertIn(f'import "part_{index - 1:02d}.proto";', text)
                        self.assertIn(" previous = 5;", text)
                self.assertEqual(
                    [file["sha256"] for file in metadata["inputs"]],
                    [harness.sha256(case_dir / "consumer" / "proto" / name) for name in names],
                )
                self.assertEqual(
                    harness.render_proto(harness.DEFAULT_SEED, messages, files, 0),
                    sources[0],
                )
                self.assertNotEqual(
                    harness.render_proto(harness.DEFAULT_SEED + 1, messages, files, 0),
                    sources[0],
                )

    def test_consumer_uses_every_generated_type_and_incremental_edit(self):
        text = harness.render_consumer(100, 0)
        self.assertIn('include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));', text)
        self.assertEqual(text.count("roundtrip(bench::cg19::Message"), 100)
        self.assertIn("Message0099::new()", text)
        self.assertNotEqual(text, harness.render_consumer(100, 1))
        self.assertIn('opt-level = 3\nlto = "thin"\ncodegen-units = 1', harness.manifest("test"))

    def test_reference_consumer_uses_same_work_with_independent_runtime(self):
        text = harness.render_consumer(6, 0, reference=True)
        self.assertIn('#[path = "../generated/generated.rs"] mod generated;', text)
        self.assertIn("fn roundtrip<T: protobuf::Parse + protobuf::Serialize>", text)
        self.assertEqual(text.count("roundtrip(generated::Message"), 6)
        self.assertIn("generated::Message0005::new()", text)
        self.assertEqual(
            text.split("    let wire = ")[1].split("\n\nfn main()")[0],
            harness.render_consumer(6, 0).split("    let wire = ")[1].split("\n\nfn main()")[0],
        )
        dependency = harness.manifest("cg19-reference-small", reference=True)
        self.assertIn('protobuf = "=4.35.1-release"', dependency)
        self.assertNotIn("pbrs", dependency)
        self.assertIn('opt-level = 3\nlto = "thin"\ncodegen-units = 1', dependency)

    def test_nochange_verification_checks_bytes_mtimes_and_file_set(self):
        with tempfile.TemporaryDirectory() as temporary:
            generated = Path(temporary)
            (generated / "mod.rs").write_text('include!("part_00.rs");')
            part = generated / "part_00.rs"
            part.write_text("pub struct Message0000;")
            original = harness.snapshot_generated(generated)
            harness.assert_unchanged(original, harness.snapshot_generated(generated))
            stat = part.stat()
            os.utime(part, ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000_000))
            with self.assertRaisesRegex(harness.BenchmarkError, "unchanged generation altered"):
                harness.assert_unchanged(original, harness.snapshot_generated(generated))
            part.write_text("pub struct Message0001;")
            os.utime(part, ns=(stat.st_atime_ns, stat.st_mtime_ns))
            with self.assertRaisesRegex(harness.BenchmarkError, "unchanged generation altered"):
                harness.assert_unchanged(original, harness.snapshot_generated(generated))
            (generated / "added.rs").write_text("pub struct Unexpected;")
            with self.assertRaisesRegex(harness.BenchmarkError, "file set"):
                harness.assert_unchanged(original, harness.snapshot_generated(generated))

    def test_reference_nochange_requires_bytes_but_reports_rewritten_mtimes(self):
        with tempfile.TemporaryDirectory() as temporary:
            generated = Path(temporary)
            (generated / "generated.rs").write_text('pub mod __unstable {}')
            part = generated / "part_00.u.pb.rs"
            part.write_text("pub struct Message0000;")
            original = harness.snapshot_generated(generated, "generated.rs")
            self.assertEqual(harness.assert_same_bytes(original, original), 0)
            stat = part.stat()
            os.utime(part, ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000_000))
            rewritten = harness.snapshot_generated(generated, "generated.rs")
            self.assertEqual(harness.assert_same_bytes(original, rewritten), 1)
            part.write_text("pub struct OtherMessage;")
            with self.assertRaisesRegex(harness.BenchmarkError, "changed bytes"):
                harness.assert_same_bytes(original, harness.snapshot_generated(generated, "generated.rs"))

    @unittest.skipIf(sys.version_info < (3, 11), "reference mode requires tomllib")
    def test_reference_lock_fails_closed_on_runtime_and_pbrs_drift(self):
        with tempfile.TemporaryDirectory() as temporary:
            lock = Path(temporary) / "Cargo.lock"
            pinned = (
                'version = 3\n[[package]]\nname = "protobuf"\n'
                f'version = "{harness.REFERENCE_RUNTIME_VERSION}"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                f'checksum = "{harness.REFERENCE_RUNTIME_CHECKSUM}"\n'
                '[[package]]\nname = "protobuf-macros"\n'
                f'version = "{harness.REFERENCE_RUNTIME_VERSION}"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                f'checksum = "{harness.REFERENCE_MACROS_CHECKSUM}"\n'
            )
            lock.write_text(pinned)
            result = harness.reference_lock(lock)
            self.assertEqual(result["lock_sha256"], harness.sha256(lock))
            self.assertEqual(result["packages"]["protobuf"]["checksum"],
                             harness.REFERENCE_RUNTIME_CHECKSUM)
            lock.write_text(pinned.replace(harness.REFERENCE_RUNTIME_CHECKSUM, "wrong"))
            with self.assertRaisesRegex(harness.BenchmarkError, "protobuf version/source/checksum"):
                harness.reference_lock(lock)
            lock.write_text(pinned + '[[package]]\nname = "pbrs"\nversion = "0.1.0"\n')
            with self.assertRaisesRegex(harness.BenchmarkError, "unexpectedly depends on pbrs"):
                harness.reference_lock(lock)

    def test_reference_pin_checks_binary_checkout_and_checked_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            harness.write_text(root / "vendor" / "google" / "PIN", "v35.1\n")
            harness.write_text(root / "vendor" / "google" / "SHA",
                               harness.REFERENCE_REVISION + "\n")
            checked = root / "tonic-bench" / "checked_v4" / "manifest.json"
            harness.write_text(checked, json.dumps({
                "protobuf_source_revision": harness.REFERENCE_REVISION,
                "protoc_version": "libprotoc 35.1",
                "protoc_sha256": harness.REFERENCE_PROTOC_SHA256,
                "runtime_version": harness.REFERENCE_RUNTIME_VERSION,
            }))
            protoc = {
                "version": "libprotoc 35.1",
                "executable_sha256": harness.REFERENCE_PROTOC_SHA256,
            }
            with mock.patch.object(harness, "ROOT", root), mock.patch.object(
                harness.subprocess, "check_output",
                side_effect=[harness.REFERENCE_REVISION + "\n", ""],
            ):
                self.assertEqual(harness.reference_pin(protoc)["source_revision"],
                                 harness.REFERENCE_REVISION)
                with self.assertRaisesRegex(harness.BenchmarkError, "pinned libprotoc"):
                    harness.reference_pin({**protoc, "executable_sha256": "wrong"})
            harness.write_text(root / "vendor" / "google" / "SHA", "wrong\n")
            with mock.patch.object(harness, "ROOT", root):
                with self.assertRaisesRegex(harness.BenchmarkError, "vendor/google/SHA"):
                    harness.reference_pin(protoc)

    def test_reference_option_is_bounded_before_creating_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                os.environ, {"CARGO_BUILD_JOBS": "2"}
            ), contextlib.redirect_stderr(io.StringIO()):
                for options in ([], ["--seed", "1"], ["--jobs", "3"]):
                    with self.subTest(options=options), self.assertRaises(SystemExit) as raised:
                        harness.main(["--case", "small", *options, "--reference-protoc", "missing"]
                                     if options else ["--reference-protoc", "missing"])
                    self.assertEqual(raised.exception.code, 2)
            self.assertFalse((root / "target").exists())

    def test_failed_reference_request_does_not_report_zero_losses(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            out = root / "target" / "codegen-bench" / "failed"
            with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                os.environ, {"CARGO_BUILD_JOBS": "2"}
            ), mock.patch.object(
                harness, "run_cases", side_effect=harness.BenchmarkError("pinned binary mismatch")
            ), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(harness.main([
                    "--case", "small", "--reference-protoc", "missing", "--out", str(out)
                ]), 1)
            report = json.loads((out / "summary.json").read_text())
            self.assertEqual(report["reference"]["status"], "incomplete")
            self.assertEqual(report["comparison"]["status"], "not_run")
            self.assertIsNone(report["comparison"]["losing_cells"])
            self.assertFalse(report["qualification"]["qualified"])

    def test_comparison_preserves_all_losses_and_raw_values(self):
        phases = ("generation", "generation_unchanged", "check_clean",
                  "check_incremental", "build_release")
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            cell = {
                "case": "small", "corpus": {"sha256": "same"},
                "output": {"rust_bytes": 500}, "release_binary": {"size_bytes": 100},
                "phases": {name: {"elapsed_ns": 200, "peak_rss_bytes": 10}
                           for name in phases},
                "reference": {
                    "corpus_sha256": "same",
                    "output": {"rust_bytes": 400},
                    "release_binary": {"size_bytes": 200},
                    "phases": {name: {"elapsed_ns": 100, "peak_rss_bytes": 20}
                               for name in phases},
                },
            }
            report = {"comparison": {"status": "not_run", "metrics": [], "losing_cells": None}}
            harness.compare_cell(report, cell, directory)
            saved = json.loads((directory / "summary.json").read_text())["comparison"]
            self.assertEqual(len(saved["metrics"]), 12)
            self.assertEqual(len(saved["losing_cells"]), 6)
            self.assertEqual({row["metric"] for row in saved["losing_cells"]},
                             {"output.rust_bytes", *(f"{name}.elapsed_ns" for name in phases)})
            self.assertTrue(all(row["pbrs"] > row["reference"] for row in saved["losing_cells"]))
            self.assertTrue(next(row for row in saved["metrics"]
                                 if row["metric"] == "check_clean.peak_rss_bytes"
                                 )["rss_peak_is_lower_bound"])
            self.assertEqual(saved["status"], "partial")
            cell["reference"]["phases"]["generation"]["elapsed_ns"] = None
            with self.assertRaisesRegex(harness.BenchmarkError, "invalid paired measurement"):
                harness.compare_cell(report, cell, directory)
            self.assertEqual(len(report["comparison"]["metrics"]), 12)

    def test_time_and_tree_rss_units_and_missing_evidence(self):
        self.assertEqual(
            harness.parse_time_rss("    4096  maximum resident set size\n", "Darwin"), 4096
        )
        self.assertEqual(
            harness.parse_time_rss("Maximum resident set size (kbytes): 1024\n", "Linux"),
            1024 * 1024,
        )
        with self.assertRaisesRegex(harness.BenchmarkError, "missing Linux"):
            harness.parse_time_rss("", "Linux")
        self.assertEqual(
            harness.tree_rss("10 1 5\n11 10 7\n12 11 3\n13 1 99\n", 10), 15 * 1024
        )
        self.assertEqual(harness.tree_rss("10 1 5\n", 20), 0)

    def test_qualified_option_fails_closed_without_compilers(self):
        with tempfile.TemporaryDirectory() as temporary:
            out = Path(temporary) / "target" / "codegen-bench" / "report"
            with mock.patch.object(harness, "ROOT", Path(temporary)), mock.patch.object(
                harness, "run_cases", side_effect=AssertionError("ran")
            ):
                self.assertEqual(harness.main(
                    ["--case", "small", "--out", str(out), "--require-qualified"]
                ), 2)
            report = json.loads((out / "summary.json").read_text())
            self.assertFalse(report["qualification"]["qualified"])
            self.assertEqual(report["reference"]["status"], "missing")
            self.assertEqual(report["comparison"]["losing_cells"], None)
            self.assertEqual(report["status"], "unqualified")

    def test_invalid_inherited_jobs_is_a_parser_error(self):
        for invalid in ("not-an-integer", "", "0", "9", "-1"):
            with self.subTest(invalid=invalid), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                errors = io.StringIO()
                with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                    os.environ, {"CARGO_BUILD_JOBS": invalid}
                ), contextlib.redirect_stderr(errors), self.assertRaises(SystemExit) as raised:
                    harness.main(["--case", "small", "--require-qualified"])
                self.assertEqual(raised.exception.code, 2)
                self.assertIn("invalid CARGO_BUILD_JOBS", errors.getvalue())
                self.assertFalse((root / "target").exists())

    def test_wrapper_is_in_source_fingerprints(self):
        self.assertEqual(
            harness.source_hashes()["scripts/codegen-bench.sh"],
            harness.sha256(harness.ROOT / "scripts" / "codegen-bench.sh"),
        )

    def test_tool_version_executes_absolute_multicall_shim_without_resolving_it(self):
        with tempfile.TemporaryDirectory() as temporary:
            run_dir = Path(temporary).resolve()
            bin_dir = run_dir / "bin"
            bin_dir.mkdir()
            target = bin_dir / "multicall"
            target.write_text(
                f"#!{sys.executable}\n"
                "import os, sys\n"
                "if os.path.basename(sys.argv[0]) != 'cargo' or sys.argv[1:] != ['--version', '--verbose']:\n"
                "    sys.exit(42)\n"
                "print('cargo-shim 1.0')\n"
            )
            target.chmod(0o755)
            shim = bin_dir / "cargo"
            shim.symlink_to(target)
            with mock.patch.dict(os.environ, {"PATH": str(bin_dir)}):
                result = harness.tool_version("cargo", ["--version", "--verbose"], "cargo", run_dir)
            self.assertEqual(result["executable"], str(shim))
            self.assertEqual(result["resolved_target"], str(target))
            self.assertEqual(result["executable_sha256"], harness.sha256(target))
            self.assertEqual(result["version"], "cargo-shim 1.0")
            self.assertEqual(
                (run_dir / result["stdout_log"]).read_text().strip(), "cargo-shim 1.0"
            )

    def test_generator_copy_is_pinned_against_shared_target_replacement(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shared = root / "shared" / "cg19-generator"
            harness.write_text(shared, "original")
            shared.chmod(0o755)
            run_dir = root / "run"
            copied, digest = harness.copy_generator(shared, run_dir)
            self.assertEqual(copied, run_dir / "bin" / "cg19-generator")
            self.assertEqual(digest, harness.sha256(copied))
            harness.write_text(shared, "replaced")
            self.assertEqual(copied.read_text(), "original")

            original_copy = harness.shutil.copy2

            def concurrent_replacement(source, destination):
                original_copy(source, destination)
                harness.write_text(shared, "changed during copy")

            with mock.patch.object(harness.shutil, "copy2", side_effect=concurrent_replacement):
                with self.assertRaisesRegex(harness.BenchmarkError, "changed during copy"):
                    harness.copy_generator(shared, root / "second-run")

    @unittest.skipUnless(sys.platform in ("darwin", "linux"), "requires system time and ps")
    def test_python_command_keeps_timing_and_raw_logs_separate(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            result = harness.timed_command(
                [sys.executable, "-c", "print('CG19')"],
                directory,
                dict(harness.os.environ),
                directory / "logs" / "python",
                directory,
                15,
                100,
            )
            self.assertEqual(result["exit_code"], 0)
            self.assertGreater(result["elapsed_ns"], 0)
            self.assertGreater(result["peak_rss_bytes"], 0)
            self.assertIn("CG19", (directory / result["stdout_log"]).read_text())
            self.assertTrue((directory / result["stderr_log"]).is_file())

    @unittest.skipUnless(sys.platform in ("darwin", "linux"), "requires process groups")
    def test_plain_command_keeps_binary_stderr_free_of_time_output_and_times_out(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            result = harness.plain_command(
                [sys.executable, "-c", "print('1')"], directory, dict(os.environ),
                directory / "logs" / "plain", directory, 3,
            )
            self.assertEqual(result["exit_code"], 0)
            self.assertEqual(result["timeout_seconds"], 3)
            self.assertEqual(result["cwd"], str(directory))
            self.assertNotIn("elapsed_ns", result)
            self.assertEqual((directory / result["stdout_log"]).read_bytes(), b"1\n")
            self.assertEqual((directory / result["stderr_log"]).read_bytes(), b"")
            with self.assertRaisesRegex(harness.BenchmarkError, "exceeded 1s"):
                harness.plain_command(
                    [sys.executable, "-c", "import time; time.sleep(10)"],
                    directory, dict(os.environ), directory / "logs" / "timeout",
                    directory, 1,
                )
            self.assertTrue((directory / "logs" / "timeout.stderr.log").is_file())

    @unittest.skipUnless(sys.platform in ("darwin", "linux"), "requires system time and ps")
    def test_failed_phase_retains_failure_and_logs(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            report = {"schema_version": "cg19/1", "status": "pending", "phases": {}}
            phases = report["phases"]
            with self.assertRaisesRegex(harness.BenchmarkError, "exited 7"):
                harness.phase(
                    report, phases, "synthetic",
                    [sys.executable, "-c", "import sys; print('failed'); sys.exit(7)"],
                    directory, dict(os.environ), directory, 15, 100,
                    directory / "logs" / "synthetic",
                )
            self.assertEqual(phases["synthetic"]["status"], "failed")
            self.assertEqual(phases["synthetic"]["exit_code"], 7)
            saved = json.loads((directory / "summary.json").read_text())
            self.assertEqual(saved["phases"]["synthetic"]["exit_code"], 7)
            self.assertIn("failed", (directory / phases["synthetic"]["stdout_log"]).read_text())
            self.assertTrue((directory / phases["synthetic"]["stderr_log"]).is_file())

    def test_release_smoke_fails_closed_and_retains_logs(self):
        with tempfile.TemporaryDirectory() as temporary:
            for name, stdout, stderr, exit_code in (
                ("wrong_stdout", b"2\n", b"", 0),
                ("unexpected_stderr", b"1\n", b"warning\n", 0),
                ("nonzero_exit", b"1\n", b"", 7),
            ):
                with self.subTest(name=name):
                    directory = Path(temporary) / name
                    consumer = directory / "consumer"
                    consumer.mkdir(parents=True)
                    binary = consumer / "release-bin"
                    binary.write_text("fixture")
                    report = {
                        "phases": {},
                        "comparison": {"status": "not_run", "losing_cells": None},
                        "qualification": {"qualified": False},
                    }

                    def fake_command(command, cwd, env, stem, run_dir, timeout):
                        self.assertEqual(command, [str(binary)])
                        self.assertEqual(cwd, consumer)
                        self.assertEqual(timeout, 15)
                        out, err, paths = harness.log_paths(stem, run_dir)
                        out.parent.mkdir(parents=True, exist_ok=True)
                        out.write_bytes(stdout)
                        err.write_bytes(stderr)
                        return {
                            "command": command, "cwd": str(cwd), "exit_code": exit_code,
                            "timeout_seconds": timeout, **paths,
                        }

                    with mock.patch.object(harness, "plain_command", side_effect=fake_command):
                        with self.assertRaises(harness.BenchmarkError):
                            harness.release_smoke(
                                report, report["phases"], binary, consumer, {},
                                directory, 90, 100, directory / "logs",
                            )
                    saved = json.loads((directory / "summary.json").read_text())
                    smoke = saved["phases"]["release_smoke"]
                    self.assertEqual(smoke["status"], "failed")
                    self.assertEqual(smoke["exit_code"], exit_code)
                    self.assertEqual((directory / smoke["stdout_log"]).read_bytes(), stdout)
                    self.assertEqual((directory / smoke["stderr_log"]).read_bytes(), stderr)
                    self.assertIsNone(saved["comparison"]["losing_cells"])
                    self.assertFalse(saved["qualification"]["qualified"])
                    if exit_code == 0:
                        self.assertFalse(smoke["output_verified"])
                        self.assertEqual(smoke["expected_stdout"], "1\n")
                        self.assertEqual(smoke["expected_stderr"], "")
                        self.assertIn("release smoke expected", smoke["error"])

    def test_pipeline_records_distinct_phases_and_binary_without_compilers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            run_dir = Path(temporary) / "run"
            run_dir.mkdir()
            shared_target = root / "target" / "integration-consumers"
            shared_target.mkdir(parents=True)
            sentinel = shared_target / "prior-consumer"
            sentinel.write_text("keep")
            shared_generator = shared_target / "debug" / "cg19-generator"
            report = {"schema_version": "cg19/1", "status": "pending", "setup": {}, "cells": []}
            protoc = run_dir / "fake-protoc"
            protoc.write_text("test compiler")
            environment = {
                "tools": {
                    "cargo": {"executable": "fake-cargo"},
                    "rustc": {"executable": "fake-rustc"},
                    "protoc": {"executable": str(protoc)},
                },
                "repository": {"source_sha256": {}},
                "cache": {},
            }

            def fake_command(command, cwd, env, stem, root, timeout, sample_ms):
                name = stem.name
                stdout, stderr, paths = harness.log_paths(stem, root)
                stdout.parent.mkdir(parents=True, exist_ok=True)
                stdout.write_text("")
                stderr.write_text("")
                if name == "driver-lock":
                    (run_dir / "driver" / "Cargo.lock").write_text("driver lock")
                elif name == "driver-build":
                    self.assertEqual(env["CARGO_BUILD_JOBS"], "2")
                    self.assertEqual(env["CARGO_INCREMENTAL"], "1")
                    self.assertEqual(command[command.index("--target-dir") + 1], str(shared_target))
                    harness.write_text(shared_generator, "test generator")
                    shared_generator.chmod(0o755)
                elif name == "consumer-lock":
                    (run_dir / "cases" / "small" / "consumer" / "Cargo.lock").write_text("consumer lock")
                    harness.write_text(shared_generator, "replaced after copy")
                elif name == "generation":
                    output = run_dir / "cases" / "small" / "consumer" / "generated"
                    self.assertEqual(command[0], str(run_dir / "bin" / "cg19-generator"))
                    self.assertEqual((run_dir / "bin" / "cg19-generator").read_text(), "test generator")
                    harness.write_text(output / "mod.rs", 'include!("part_00.rs");\n')
                    for proto in command[4:]:
                        harness.write_text(output / proto.replace(".proto", ".rs"), "pub struct Message;\n")
                elif name == "check-clean":
                    self.assertEqual(env["CARGO_BUILD_JOBS"], "4")
                    target = run_dir / "cases" / "small" / "target"
                    target.mkdir(parents=True)
                    source = run_dir / "cases" / "small" / "consumer" / "src" / "main.rs"
                    stat = source.stat()
                    os.utime(source, ns=(stat.st_atime_ns, stat.st_mtime_ns - 2_000_000_000))
                elif name == "build-release":
                    binary = run_dir / "cases" / "small" / "target" / "release" / "cg19-consumer-small"
                    harness.write_text(binary, "compiled")
                elif name == "release-smoke":
                    binary = run_dir / "cases" / "small" / "target" / "release" / "cg19-consumer-small"
                    self.assertEqual(command, [str(binary)])
                    self.assertEqual(cwd, run_dir / "cases" / "small" / "consumer")
                    self.assertEqual(timeout, 15)
                    stdout.write_text("1\n")
                if name in ("check-clean", "check-incremental", "build-release"):
                    verb = "Compiling" if name == "build-release" else "Checking"
                    stderr.write_text(f"{verb} cg19-consumer-small v0.0.0 (test)\n")
                return {
                    "command": command, "exit_code": 0, "elapsed_ns": 1234,
                    "peak_rss_bytes": 4096, **paths,
                }

            def fake_plain(command, cwd, env, stem, root, timeout):
                result = fake_command(command, cwd, env, stem, root, timeout, 100)
                result.pop("elapsed_ns")
                result.pop("peak_rss_bytes")
                return {**result, "cwd": str(cwd), "timeout_seconds": timeout}

            with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                os.environ, {"CARGO_INCREMENTAL": "1"}
            ), mock.patch.object(harness, "provenance", return_value=environment), mock.patch.object(
                harness, "timed_command", side_effect=fake_command
            ), mock.patch.object(
                harness, "plain_command", side_effect=fake_plain
            ), mock.patch.object(harness, "source_hashes", return_value={}), mock.patch.object(
                harness.time, "sleep", return_value=None
            ):
                harness.run_cases(report, run_dir, ["small"], harness.DEFAULT_SEED, 4, 15, 100)
            saved = json.loads((run_dir / "summary.json").read_text())
            cell = saved["cells"][0]
            self.assertEqual(saved["environment"]["cache"]["bootstrap_target_dir"], str(shared_target))
            self.assertEqual(saved["environment"]["cache"]["bootstrap_build_jobs"], 2)
            self.assertEqual(saved["environment"]["cache"]["bootstrap_cargo_incremental"], "1")
            self.assertTrue(saved["environment"]["cache"]["bootstrap_shared"])
            self.assertFalse(saved["environment"]["cache"]["bootstrap_included_in_measurements"])
            self.assertEqual(saved["setup"]["generator_binary_path"], "bin/cg19-generator")
            self.assertEqual(
                saved["setup"]["generator_binary_sha256"],
                harness.sha256(run_dir / "bin" / "cg19-generator"),
            )
            self.assertEqual(sentinel.read_text(), "keep")
            self.assertEqual(shared_generator.read_text(), "replaced after copy")
            self.assertFalse((run_dir / "bootstrap-target").exists())
            self.assertEqual(cell["corpus"]["messages"], 6)
            self.assertEqual(cell["output"]["unchanged_generation_verified_files"], 3)
            self.assertEqual(cell["release_binary"]["size_bytes"], len("compiled"))
            self.assertEqual(
                set(cell["phases"]), {
                    "consumer_lock", "generation", "generation_unchanged", "check_clean",
                    "check_incremental", "build_release", "release_smoke",
                },
            )
            smoke = cell["phases"]["release_smoke"]
            self.assertTrue(smoke["output_verified"])
            self.assertEqual(smoke["expected_stdout"], "1\n")
            self.assertEqual(smoke["expected_stderr"], "")
            self.assertEqual((run_dir / smoke["stdout_log"]).read_bytes(), b"1\n")
            self.assertEqual((run_dir / smoke["stderr_log"]).read_bytes(), b"")
            self.assertIn("--locked", cell["phases"]["check_clean"]["command"])
            self.assertIn("--release", cell["phases"]["build_release"]["command"])
            self.assertNotIn("cargo", cell["phases"]["generation"]["command"][0])
            self.assertEqual(
                cell["phases"]["generation"]["command"][0], str(run_dir / "bin" / "cg19-generator")
            )
            self.assertEqual((run_dir / "bin" / "protoc").resolve(), protoc.resolve())

    def test_paired_small_pipeline_uses_independent_runtime_and_cold_targets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            run_dir = Path(temporary) / "run"
            run_dir.mkdir()
            shared = root / "target" / "integration-consumers" / "debug" / "cg19-generator"
            protoc = run_dir / "pinned-protoc"
            protoc.write_text("pinned compiler fixture")
            report = {
                "status": "pending", "setup": {}, "cells": [],
                "reference": {"status": "requested"},
                "comparison": {"status": "not_run", "metrics": [], "losing_cells": None},
            }
            environment = {
                "tools": {
                    "cargo": {"executable": "fake-cargo"},
                    "rustc": {"executable": "fake-rustc"},
                    "protoc": {"executable": str(protoc)},
                    "cc": {"executable": "fake-cc"},
                },
                "repository": {"source_sha256": {}}, "cache": {}, "measurement": {},
                "reference_source": {"source_revision": harness.REFERENCE_REVISION},
            }
            commands = []

            def fake_command(command, cwd, env, stem, destination, timeout, sample_ms):
                name = stem.name
                is_reference = stem.parent.name == "reference"
                stdout, stderr, paths = harness.log_paths(stem, destination)
                stdout.parent.mkdir(parents=True, exist_ok=True)
                stdout.write_text("")
                stderr.write_text("")
                commands.append((name, is_reference, command))
                consumer = run_dir / "cases" / "small"
                if is_reference:
                    consumer = consumer / "reference" / "consumer"
                else:
                    consumer = consumer / "consumer"
                package = "cg19-reference-small" if is_reference else "cg19-consumer-small"
                if name == "driver-lock":
                    (run_dir / "driver" / "Cargo.lock").write_text("driver lock")
                elif name == "driver-build":
                    self.assertEqual(env["CARGO_BUILD_JOBS"], "2")
                    self.assertEqual(
                        command[command.index("--target-dir") + 1],
                        str(root / "target" / "integration-consumers"),
                    )
                    harness.write_text(shared, "copied generator")
                    shared.chmod(0o755)
                elif name == "consumer-lock":
                    harness.write_text(consumer / "Cargo.lock", "consumer lock")
                elif name in ("generation", "generation-unchanged"):
                    if is_reference:
                        self.assertEqual(command[0], str(protoc))
                        self.assertIn("--rust_opt=" + harness.REFERENCE_RUST_OPT, command)
                        self.assertEqual(env["CC"], "fake-cc")
                        generated = consumer / "generated"
                        harness.write_text(generated / "generated.rs",
                                           '#[path="part_00.u.pb.rs"] mod part_00;\n')
                        for source in command[4:]:
                            harness.write_text(
                                generated / source.replace(".proto", ".u.pb.rs"),
                                "pub struct Message;\n",
                            )
                    elif name == "generation":
                        self.assertEqual(command[0], str(run_dir / "bin" / "cg19-generator"))
                        generated = consumer / "generated"
                        harness.write_text(generated / "mod.rs", 'include!("part_00.rs");\n')
                        for source in command[4:]:
                            harness.write_text(
                                generated / source.replace(".proto", ".rs"),
                                "pub struct Message;\n",
                            )
                elif name == "check-clean":
                    self.assertFalse(Path(env["CARGO_TARGET_DIR"]).exists())
                    self.assertEqual(env["CARGO_BUILD_JOBS"], "2")
                    self.assertIn("--locked", command)
                    Path(env["CARGO_TARGET_DIR"]).mkdir(parents=True)
                    source = consumer / "src" / "main.rs"
                    stat = source.stat()
                    os.utime(source, ns=(stat.st_atime_ns, stat.st_mtime_ns - 2_000_000_000))
                elif name == "build-release":
                    self.assertEqual(env["CARGO_INCREMENTAL"], "0")
                    harness.write_text(Path(env["CARGO_TARGET_DIR"]) / "release" / package,
                                       f"compiled {package}")
                elif name == "release-smoke":
                    self.assertEqual(command, [
                        str(Path(env["CARGO_TARGET_DIR"]) / "release" / package),
                    ])
                    self.assertEqual(cwd, consumer)
                    self.assertEqual(timeout, 15)
                    stdout.write_text("1\n")
                if name in ("check-clean", "check-incremental", "build-release"):
                    verb = "Compiling" if name == "build-release" else "Checking"
                    stderr.write_text(f"{verb} {package} v0.0.0 (test)\n")
                return {
                    "command": command, "exit_code": 0,
                    "elapsed_ns": 2000 if not is_reference else 1000,
                    "peak_rss_bytes": 4096 if not is_reference else 2048, **paths,
                }

            def fake_plain(command, cwd, env, stem, root, timeout):
                result = fake_command(command, cwd, env, stem, root, timeout, 100)
                result.pop("elapsed_ns")
                result.pop("peak_rss_bytes")
                return {**result, "cwd": str(cwd), "timeout_seconds": timeout}

            with mock.patch.object(harness, "ROOT", root), mock.patch.object(
                harness, "REFERENCE_PROTOC_SHA256", harness.sha256(protoc)
            ), mock.patch.dict(
                os.environ, {"CARGO_INCREMENTAL": "1"}
            ), mock.patch.object(
                harness, "provenance", return_value=environment
            ), mock.patch.object(
                harness, "reference_lock", return_value={
                    "lock_sha256": "verified", "packages": {"protobuf": {"version": "4.35.1-release"}}
                }
            ), mock.patch.object(
                harness, "timed_command", side_effect=fake_command
            ), mock.patch.object(
                harness, "plain_command", side_effect=fake_plain
            ), mock.patch.object(
                harness, "source_hashes", return_value={}
            ), mock.patch.object(
                harness.time, "sleep", return_value=None
            ):
                harness.run_cases(
                    report, run_dir, ["small"], harness.DEFAULT_SEED, 2, 15, 100, protoc,
                )
            saved = json.loads((run_dir / "summary.json").read_text())
            self.assertEqual(saved["reference"]["status"], "measured")
            self.assertEqual(saved["comparison"]["status"], "diagnostic")
            self.assertEqual(len(saved["comparison"]["metrics"]), 12)
            self.assertFalse(any("release_smoke" in row["metric"]
                                 for row in saved["comparison"]["metrics"]))
            self.assertTrue(any(row["metric"] == "check_clean.elapsed_ns"
                                for row in saved["comparison"]["losing_cells"]))
            self.assertEqual(saved["cells"][0]["corpus"]["sha256"],
                             saved["cells"][0]["reference"]["corpus_sha256"])
            self.assertEqual(saved["cells"][0]["reference"]["output"]["rust_file_count"], 3)
            self.assertEqual(saved["cells"][0]["reference"]["consumer_lock_sha256"], "verified")
            self.assertEqual(saved["environment"]["cache"]["bootstrap_build_jobs"], 2)
            self.assertFalse((run_dir / "bootstrap-target").exists())
            reference_manifest = (
                run_dir / "cases" / "small" / "reference" / "consumer" / "Cargo.toml"
            ).read_text()
            self.assertIn('protobuf = "=4.35.1-release"', reference_manifest)
            self.assertNotIn("pbrs", reference_manifest)
            self.assertEqual(
                [is_reference for name, is_reference, _ in commands if name == "check-clean"],
                [False, True],
            )
            self.assertEqual(
                [is_reference for name, is_reference, _ in commands if name == "release-smoke"],
                [False, True],
            )
            for side in (saved["cells"][0], saved["cells"][0]["reference"]):
                smoke = side["phases"]["release_smoke"]
                self.assertTrue(smoke["output_verified"])
                self.assertEqual((run_dir / smoke["stdout_log"]).read_bytes(), b"1\n")
                self.assertEqual((run_dir / smoke["stderr_log"]).read_bytes(), b"")
            self.assertNotEqual(saved["cells"][0]["target_dir"],
                                saved["cells"][0]["reference"]["target_dir"])


if __name__ == "__main__":
    unittest.main()
