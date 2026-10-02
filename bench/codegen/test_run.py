"""Python-only CG-19 harness checks; no Cargo, rustc, or protoc is invoked."""

import contextlib
import hashlib
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import run as harness


class CorpusTests(unittest.TestCase):
    def test_pbrs_profile_preserves_raw_false_and_effective_defaults(self):
        default = harness.pbrs_profile({})
        self.assertEqual(default["kind"], "ordinary-default")
        self.assertEqual(default["resolved"], {
            "emit_reflection": True, "emit_json": True, "emit_text": True,
            "shared_descriptor_set": False, "runtime_profile": "default", "stubs": "none",
        })
        for flag in ("0", "false", "FALSE"):
            profile = harness.pbrs_profile({"SB09_PBRS_SHARED_DESCRIPTOR_SET": flag})
            self.assertEqual(profile["raw"]["SB09_PBRS_SHARED_DESCRIPTOR_SET"], flag)
            self.assertEqual(profile["resolved"], default["resolved"])
            self.assertEqual(profile["kind"], "ordinary-default")
        for flag in ("1", "true", "TRUE"):
            profile = harness.pbrs_profile({"SB09_PBRS_SHARED_DESCRIPTOR_SET": flag})
            self.assertEqual(profile["kind"], "nondefault-diagnostic")
            self.assertTrue(profile["resolved"]["shared_descriptor_set"])

    def test_pbrs_profile_records_lean_defaults_and_actual_stub_runtime(self):
        env = {
            "SB09_PBRS_EMIT_REFLECTION": "0", "SB09_PBRS_EMIT_TEXT": "1",
            "SB09_PBRS_RUNTIME_PROFILE": "minimal",
            "SB09_PBRS_SHARED_DESCRIPTOR_SET": "true",
        }
        profile = harness.pbrs_profile(env)
        self.assertEqual(profile["kind"], "nondefault-diagnostic")
        self.assertFalse(profile["resolved"]["emit_reflection"])
        self.assertFalse(profile["resolved"]["emit_json"])
        self.assertTrue(profile["resolved"]["emit_text"])
        self.assertEqual(profile["resolved"]["runtime_profile"], "minimal")
        for generator, stubs in harness.PBRS_STUB_ENV.items():
            profile = harness.pbrs_profile(env, generator)
            self.assertEqual(profile["resolved"]["stubs"], stubs)
            self.assertEqual(profile["resolved"]["runtime_profile"], "default")
            self.assertEqual(profile["raw"]["SB09_PBRS_RUNTIME_PROFILE"], "minimal")
        for key in ("SB09_PBRS_SHARED_DESCRIPTOR_SET", "SB09_PBRS_EMIT_REFLECTION",
                    "SB09_PBRS_EMIT_JSON", "SB09_PBRS_EMIT_TEXT"):
            with self.subTest(key=key), self.assertRaisesRegex(harness.BenchmarkError, key):
                harness.pbrs_profile({key: "invalid"})

    def test_shared_helper_provenance_fails_on_missing_or_stale_output(self):
        files = {"mod.rs": (1, "registry", 10), "part_00.rs": (1, "messages", 20)}
        name = "__pbrs_shared_descriptors.rs"
        ordinary = harness.pbrs_profile({})
        shared = harness.pbrs_profile({"SB09_PBRS_SHARED_DESCRIPTOR_SET": "true"})
        self.assertFalse(harness.pbrs_helper_provenance(files, ordinary, 2)["active"])
        self.assertFalse(harness.pbrs_helper_provenance(files, shared, 1)["active"])
        with self.assertRaisesRegex(harness.BenchmarkError, "shared descriptor helper"):
            harness.pbrs_helper_provenance(files, shared, 2)
        files[name] = (2, "helper", 30)
        self.assertEqual(harness.pbrs_helper_provenance(files, shared, 2), {
            "active": True, "path": name, "sha256": "helper", "bytes": 30,
        })
        with self.assertRaisesRegex(harness.BenchmarkError, "shared descriptor helper"):
            harness.pbrs_helper_provenance(files, ordinary, 2)
        lean = harness.pbrs_profile({
            "SB09_PBRS_SHARED_DESCRIPTOR_SET": "true", "SB09_PBRS_EMIT_REFLECTION": "0",
        })
        with self.assertRaisesRegex(harness.BenchmarkError, "shared descriptor helper"):
            harness.pbrs_helper_provenance(files, lean, 2)

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
        for messages in (6, 100, 1000):
            with self.subTest(messages=messages):
                text = harness.render_consumer(messages, 0, reference=True)
                self.assertIn('#[path = "../generated/generated.rs"] mod generated;', text)
                self.assertIn("fn roundtrip<T: protobuf::Parse + protobuf::Serialize>", text)
                self.assertEqual(text.count("roundtrip(generated::Message"), messages)
                self.assertIn(f"generated::Message{messages - 1:04d}::new()", text)
                self.assertEqual(
                    text.split("    let wire = ")[1].split("\n\nfn main()")[0],
                    harness.render_consumer(messages, 0).split("    let wire = ")[1].split("\n\nfn main()")[0],
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
                for options in ([], ["--seed", "1"], ["--jobs", "3"], ["--case", "all"]):
                    with self.subTest(options=options), self.assertRaises(SystemExit) as raised:
                        harness.main(["--case", "small", *options, "--reference-protoc", "missing"]
                                     if options else ["--reference-protoc", "missing"])
                    self.assertEqual(raised.exception.code, 2)
            self.assertFalse((root / "target").exists())

    def test_explicit_larger_reference_cells_reach_pipeline_without_claim(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for case in ("100", "1000"):
                with self.subTest(case=case):
                    out = root / "target" / "codegen-bench" / case

                    def fake_run(report, run_dir, cases, seed, jobs, timeout, sample_ms, reference_protoc,
                                   generators=("pbrs",), repeats=5, stub_generators=("pbrs-native",)):
                        self.assertEqual(run_dir, out.resolve())
                        self.assertEqual(cases, [case])
                        self.assertEqual(seed, harness.DEFAULT_SEED)
                        self.assertEqual(jobs, 2)
                        self.assertEqual(reference_protoc.resolve(), (root / "pinned-protoc").resolve())
                        self.assertEqual(tuple(generators), ("pbrs",))
                        self.assertEqual(repeats, 5)
                        self.assertEqual(tuple(stub_generators), ("pbrs-native",))
                        raise harness.BenchmarkError("stub pipeline reached")

                    with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                        os.environ, {"CARGO_BUILD_JOBS": "2"}
                    ), mock.patch.object(
                        harness, "run_cases", side_effect=fake_run
                    ), contextlib.redirect_stderr(io.StringIO()):
                        result = harness.main([
                            "--case", case, "--reference-protoc", str(root / "pinned-protoc"),
                            "--out", str(out),
                        ])
                    self.assertEqual(result, 1)
                    report = json.loads((out / "summary.json").read_text())
                    self.assertEqual(report["cases_requested"], [case])
                    self.assertEqual(report["reference"]["status"], "incomplete")
                    self.assertEqual(report["comparison"]["status"], "not_run")
                    self.assertIsNone(report["comparison"]["losing_cells"])
                    self.assertIn("stub pipeline reached", report["errors"])
                    self.assertIn("partial_corpus_matrix", report["qualification"]["reasons"])
                    self.assertFalse(report["qualification"]["qualified"])

    def test_larger_reference_qualification_fails_before_compilers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for case in ("100", "1000"):
                with self.subTest(case=case):
                    out = root / "target" / "codegen-bench" / case
                    with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                        os.environ, {"CARGO_BUILD_JOBS": "2"}
                    ), mock.patch.object(
                        harness, "run_cases", side_effect=AssertionError("compiler ran")
                    ), contextlib.redirect_stderr(io.StringIO()):
                        result = harness.main([
                            "--case", case, "--reference-protoc", str(root / "pinned-protoc"),
                            "--require-qualified", "--out", str(out),
                        ])
                    self.assertEqual(result, 2)
                    report = json.loads((out / "summary.json").read_text())
                    self.assertEqual(report["status"], "unqualified")
                    self.assertIsNone(report["comparison"]["losing_cells"])
                    self.assertFalse(report["qualification"]["qualified"])

    @unittest.skipUnless(os.environ.get("CG19_PINNED_PROTOC"), "opt-in pinned protoc only")
    def test_pinned_reference_generates_full_larger_corpora(self):
        protoc = Path(os.environ["CG19_PINNED_PROTOC"]).resolve()
        # The protoc binary hash varies across CMake/linker environments, so
        # genuineness is libprotoc 35.1 + --rust_out + pinned source revision
        # (shared with the matrix v4 flow); the generated-byte digests below
        # are the stable semantic pins.
        self.assertEqual(harness.resolve_pinned_protoc(protoc), protoc)
        expected = {
            "100": (6, "1c91b8a07095e3d1a5972933ec6ffda40be5b9c0a067303196c48cba2be19ee4"),
            "1000": (21, "2727bb5aa3979c1e6e085d480dbfa1b4e0fc2290c46384d2f03daa43604001d4"),
        }
        with tempfile.TemporaryDirectory() as temporary:
            for case, (file_count, digest) in expected.items():
                with self.subTest(case=case):
                    case_dir = Path(temporary) / case
                    names, _ = harness.prepare_corpus(case_dir, case, harness.DEFAULT_SEED)
                    generated = case_dir / "reference" / "generated"
                    generated.mkdir(parents=True)
                    proc = subprocess.run(
                        [
                            str(protoc), f"--proto_path={case_dir / 'consumer' / 'proto'}",
                            f"--rust_out={generated}", f"--rust_opt={harness.REFERENCE_RUST_OPT}",
                            *names,
                        ],
                        capture_output=True, text=True, timeout=120, check=True,
                    )
                    self.assertEqual(proc.stderr, "")
                    snapshot = harness.snapshot_generated(generated, "generated.rs")
                    self.assertEqual(len(snapshot), file_count)
                    self.assertEqual(harness.tree_digest(snapshot), digest)
                    entry = (generated / "generated.rs").read_text()
                    for name in names:
                        self.assertIn(f'#[path="{name[:-6]}.u.pb.rs"]', entry)

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
        self._assert_pbrs_pipeline_profile({})
        self._assert_pbrs_pipeline_profile({"SB09_PBRS_SHARED_DESCRIPTOR_SET": "false"})

    def test_shared_pipeline_records_diagnostic_profile_and_helper(self):
        self._assert_pbrs_pipeline_profile({"SB09_PBRS_SHARED_DESCRIPTOR_SET": "true"}, True)

    def _assert_pbrs_pipeline_profile(self, profile_env, expect_helper=False):
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
                    self.assertEqual(
                        env.get("SB09_PBRS_SHARED_DESCRIPTOR_SET"),
                        profile_env.get("SB09_PBRS_SHARED_DESCRIPTOR_SET"),
                    )
                    if expect_helper:
                        harness.write_text(output / "__pbrs_shared_descriptors.rs", "pub const META: &[u8] = &[1];\n")
                elif name == "check-clean":
                    self.assertEqual(env["CARGO_BUILD_JOBS"], "4")
                    target = run_dir / "cases" / "small" / "target-r0"
                    target.mkdir(parents=True)
                    source = run_dir / "cases" / "small" / "consumer" / "src" / "main.rs"
                    stat = source.stat()
                    os.utime(source, ns=(stat.st_atime_ns, stat.st_mtime_ns - 2_000_000_000))
                elif name == "build-release":
                    binary = run_dir / "cases" / "small" / "target-r0" / "release" / "cg19-consumer-small"
                    harness.write_text(binary, "compiled")
                elif name == "release-smoke":
                    binary = run_dir / "cases" / "small" / "target-r0" / "release" / "cg19-consumer-small"
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
                os.environ, {"CARGO_INCREMENTAL": "1", **profile_env}
            ), mock.patch.object(harness, "provenance", return_value=environment), mock.patch.object(
                harness, "timed_command", side_effect=fake_command
            ), mock.patch.object(
                harness, "plain_command", side_effect=fake_plain
            ), mock.patch.object(harness, "source_hashes", return_value={}), mock.patch.object(
                harness.time, "sleep", return_value=None
            ):
                harness.run_cases(report, run_dir, ["small"], harness.DEFAULT_SEED, 4, 15, 100, repeats=1)
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
            self.assertEqual(cell["output"]["unchanged_generation_verified_files"], 4 if expect_helper else 3)
            self.assertEqual(cell["profile"], saved["pbrs_profiles"]["pbrs"])
            self.assertEqual(cell["shared_descriptor_helper"]["active"], expect_helper)
            if expect_helper:
                self.assertEqual(cell["profile"]["kind"], "nondefault-diagnostic")
                self.assertIn("nondefault_pbrs_profile_diagnostic", saved["qualification"]["reasons"])
                self.assertFalse(saved["qualification"]["qualified"])
                self.assertEqual(cell["shared_descriptor_helper"]["sha256"], harness.sha256(
                    run_dir / "cases" / "small" / "consumer" / "generated" / "__pbrs_shared_descriptors.rs",
                ))
            else:
                self.assertEqual(cell["profile"]["kind"], "ordinary-default")
                self.assertNotIn("nondefault_pbrs_profile_diagnostic", saved.get("qualification", {}).get("reasons", []))
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
                is_reference = stem.parent.name.startswith("reference")
                stdout, stderr, paths = harness.log_paths(stem, destination)
                stdout.parent.mkdir(parents=True, exist_ok=True)
                stdout.write_text("")
                stderr.write_text("")
                commands.append((name, is_reference, command))
                consumer = run_dir / "cases" / "small"
                if is_reference:
                    consumer = consumer / "reference-r0" / "consumer"
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
                    repeats=1,
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
            self.assertIn(
                "cases/<case>/target and cases/<case>/reference/target",
                saved["environment"]["cache"]["paired_cold_targets"],
            )
            self.assertFalse((run_dir / "bootstrap-target").exists())
            reference_manifest = (
                run_dir / "cases" / "small" / "reference-r0" / "consumer" / "Cargo.toml"
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


class PeerGeneratorTests(unittest.TestCase):
    def test_prost_consumer_matches_pbrs_work(self):
        for messages in (6, 100):
            with self.subTest(messages=messages):
                text = harness.render_consumer_prost(messages, 0)
                self.assertIn(
                    '#[path = "../generated/bench.cg19.rs"] mod bench_cg19;', text,
                )
                self.assertIn("fn roundtrip<M: prost::Message + Default>(msg: M) -> usize {", text)
                self.assertEqual(text.count("roundtrip(bench_cg19::Message"), messages)
                self.assertIn(f"bench_cg19::Message{messages - 1:04d}::default()", text)
                # Same work shape as the pbrs consumer: serialize, parse, serialize.
                self.assertIn("msg.encode_to_vec()", text)
                self.assertIn("M::decode(", text)
                self.assertIn("parsed.encode_to_vec()", text)
        self.assertNotEqual(
            harness.render_consumer_prost(6, 0), harness.render_consumer_prost(6, 1),
        )
        self.assertEqual(
            harness.render_consumer_for(6, 0, "prost"), harness.render_consumer_prost(6, 0),
        )
        with self.assertRaisesRegex(harness.BenchmarkError, "no consumer renderer"):
            harness.render_consumer_for(6, 0, "capnp")
        dependency = harness.peer_manifest("sb09-prost-consumer-small", "prost")
        self.assertIn(f'prost = "={harness.PROST_VERSION}"', dependency)
        self.assertNotIn("pbrs", dependency)
        self.assertIn('opt-level = 3\nlto = "thin"\ncodegen-units = 1', dependency)
        with self.assertRaisesRegex(harness.BenchmarkError, "unknown peer generator"):
            harness.peer_manifest("x", "pbrs")

    def test_peer_driver_manifest_pins_build_backend(self):
        manifest = harness.peer_driver_manifest("prost")
        self.assertIn(f'prost-build = "={harness.PROST_VERSION}"', manifest)
        self.assertIn('name = "sb09-prost-driver"', manifest)
        self.assertNotIn("pbrs", manifest)
        with self.assertRaisesRegex(harness.BenchmarkError, "unknown peer generator"):
            harness.peer_driver_manifest("v4")

    def test_v4_reuses_reference_consumer_and_manifest(self):
        self.assertEqual(
            harness.render_consumer_for(6, 0, "v4"), harness.render_consumer(6, 0, reference=True),
        )
        self.assertEqual(
            harness.peer_manifest("sb09-v4-consumer-small", "v4"),
            harness.manifest("sb09-v4-consumer-small", reference=True),
        )
        self.assertEqual(harness.PEER_ENTRYPOINT["v4"], "generated.rs")
        self.assertEqual(
            harness.peer_expected_files("v4", ["part_00.proto", "part_01.proto"]),
            frozenset({"generated.rs", "part_00.u.pb.rs", "part_01.u.pb.rs"}),
        )
        with self.assertRaisesRegex(harness.BenchmarkError, "no snapshot table"):
            harness.peer_expected_files("capnp", ["part_00.proto"])
        command = harness.v4_generation_command(
            "/pinned/protoc", Path("/proto"), Path("/gen"), ["a.proto", "b.proto"],
        )
        self.assertEqual(command[0], "/pinned/protoc")
        self.assertIn("--proto_path=/proto", command)
        self.assertIn("--rust_out=/gen", command)
        self.assertIn(f"--rust_opt={harness.REFERENCE_RUST_OPT}", command)
        self.assertEqual(command[-2:], ["a.proto", "b.proto"])

    def test_pinned_protoc_resolution_fails_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with mock.patch.object(harness, "ROOT", root):
                with self.assertRaisesRegex(harness.BenchmarkError, "build-pinned-protoc"):
                    harness.resolve_pinned_protoc()
            fake = root / "target" / "pinned-protoc-build" / "protoc"
            harness.write_text(fake, "#!/bin/sh\necho libprotoc 9.9\n")
            fake.chmod(0o755)
            with mock.patch.object(harness, "ROOT", root):
                with self.assertRaisesRegex(harness.BenchmarkError, "not v35.1"):
                    harness.resolve_pinned_protoc()

    def test_peer_snapshot_table_covers_matrix_peers(self):
        self.assertEqual(harness.PEER_ENTRYPOINT["prost"], harness.PROST_PACKAGE_FILE)
        self.assertEqual(
            harness.peer_expected_files("prost", ["part_00.proto", "part_01.proto"]),
            frozenset({harness.PROST_PACKAGE_FILE}),
        )
        self.assertEqual(harness.PEER_ENTRYPOINT["buffa"], "mod.rs")
        self.assertEqual(
            harness.peer_expected_files("buffa", ["part_00.proto", "part_01.proto"]),
            frozenset({
                "mod.rs", harness.BUFFA_PACKAGE_FILE,
                "part_00.rs", "part_00.__view.rs", "part_01.rs", "part_01.__view.rs",
            }),
        )

    def test_buffa_consumer_matches_pbrs_work(self):
        text = harness.render_consumer_buffa(6, 0)
        self.assertIn(
            'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));', text,
        )
        self.assertIn("fn roundtrip<M: buffa::Message + Default>(msg: M) -> usize {", text)
        self.assertEqual(text.count("roundtrip(bench::cg19::Message"), 6)
        self.assertIn("bench::cg19::Message0005::default()", text)
        self.assertIn("msg.encode(&mut wire)", text)
        self.assertIn("M::decode(&mut &wire[..])", text)
        self.assertNotEqual(text, harness.render_consumer_buffa(6, 1))
        self.assertEqual(
            harness.render_consumer_for(6, 0, "buffa"), harness.render_consumer_buffa(6, 0),
        )
        dependency = harness.peer_manifest("sb09-buffa-consumer-small", "buffa")
        self.assertIn(f'buffa = "={harness.BUFFA_VERSION}"', dependency)
        self.assertNotIn("pbrs", dependency)
        driver = harness.peer_driver_manifest("buffa")
        self.assertIn(f'buffa-build = "={harness.BUFFA_VERSION}"', driver)

    def test_prepare_corpus_writes_protos_always_but_pbrs_consumer_on_request(self):
        with tempfile.TemporaryDirectory() as temporary:
            case_dir = Path(temporary) / "case"
            names, metadata = harness.prepare_corpus(
                case_dir, "small", harness.DEFAULT_SEED, ("prost",),
            )
            self.assertEqual(len(names), 2)
            self.assertEqual(metadata["messages"], 6)
            for name in names:
                self.assertTrue((case_dir / "consumer" / "proto" / name).is_file())
            self.assertFalse((case_dir / "consumer" / "Cargo.toml").exists())
            self.assertFalse((case_dir / "consumer" / "src" / "main.rs").exists())
            consumer, package = harness.prepare_peer_consumer(case_dir, "small", 6, "prost")
            self.assertEqual(package, "sb09-prost-consumer-small")
            self.assertEqual(consumer, case_dir / "gen" / "prost" / "consumer")
            main = (consumer / "src" / "main.rs").read_text()
            self.assertEqual(main, harness.render_consumer_prost(6, 0))

    def test_peer_generator_copy_is_pinned(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shared = root / "shared" / "sb09-prost-driver"
            harness.write_text(shared, "original")
            shared.chmod(0o755)
            run_dir = root / "run"
            copied, digest = harness.copy_peer_generator(shared, run_dir, "prost")
            self.assertEqual(copied, run_dir / "bin" / "sb09-prost-driver")
            self.assertEqual(digest, harness.sha256(copied))
            harness.write_text(shared, "replaced")
            self.assertEqual(copied.read_text(), "original")

    def test_generators_flag_rejects_unknown_before_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                os.environ, {"CARGO_BUILD_JOBS": "2"}
            ), contextlib.redirect_stderr(io.StringIO()):
                for flag in ("", "pbrs,capnp", "prost,,tonic"):
                    with self.subTest(flag=flag), self.assertRaises(SystemExit) as raised:
                        harness.main(["--case", "small", "--generators", flag])
                    self.assertEqual(raised.exception.code, 2)
            self.assertFalse((root / "target").exists())

    def test_generators_flag_reaches_pipeline(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            out = root / "target" / "codegen-bench" / "matrix"

            def fake_run(report, run_dir, cases, seed, jobs, timeout, sample_ms,
                         reference_protoc, generators=("pbrs",), repeats=5, stub_generators=("pbrs-native",)):
                self.assertIsNone(reference_protoc)
                self.assertEqual(tuple(generators), ("pbrs", "prost"))
                self.assertEqual(repeats, 5)
                self.assertEqual(tuple(stub_generators), ("pbrs-native",))
                raise harness.BenchmarkError("stub pipeline reached")

            with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                os.environ, {"CARGO_BUILD_JOBS": "2"}
            ), mock.patch.object(
                harness, "run_cases", side_effect=fake_run
            ), contextlib.redirect_stderr(io.StringIO()):
                result = harness.main([
                    "--case", "small", "--generators", "pbrs,prost", "--out", str(out),
                ])
            self.assertEqual(result, 1)
            report = json.loads((out / "summary.json").read_text())
            self.assertIn("stub pipeline reached", report["errors"])

    def test_prost_pipeline_uses_peer_paths_without_compilers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            run_dir = Path(temporary) / "run"
            run_dir.mkdir()
            shared_target = root / "target" / "integration-consumers"
            shared_target.mkdir(parents=True)
            shared_driver = shared_target / "debug" / "sb09-prost-driver"
            report = {
                "schema_version": "cg19/1", "status": "pending", "setup": {},
                "cells": [], "generators": ["prost"],
            }
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

            def manifest_arg(command, flag="--manifest-path"):
                return Path(command[command.index(flag) + 1])

            def fake_command(command, cwd, env, stem, root, timeout, sample_ms):
                name = stem.name
                stdout, stderr, paths = harness.log_paths(stem, root)
                stdout.parent.mkdir(parents=True, exist_ok=True)
                stdout.write_text("")
                stderr.write_text("")
                if name == "driver-lock-prost":
                    (run_dir / "driver-prost" / "Cargo.lock").write_text("prost lock")
                elif name == "driver-build-prost":
                    self.assertIn("--bin", command)
                    self.assertEqual(command[command.index("--bin") + 1], "sb09-prost-driver")
                    harness.write_text(shared_driver, "test prost driver")
                    shared_driver.chmod(0o755)
                elif name == "consumer-lock":
                    manifest = manifest_arg(command)
                    self.assertEqual(manifest.parent.name, "consumer")
                    manifest.with_name("Cargo.lock").write_text("peer lock")
                elif name == "generation":
                    self.assertEqual(command[0], str(run_dir / "bin" / "sb09-prost-driver"))
                    output = run_dir / "cases" / "small" / "gen" / "prost" / "consumer" / "generated"
                    harness.write_text(output / harness.PROST_PACKAGE_FILE, "pub struct Message;\n")
                elif name == "check-clean":
                    target = Path(command[command.index("--target-dir") + 1])
                    target.mkdir(parents=True)
                    source = manifest_arg(command).parent / "src" / "main.rs"
                    stat = source.stat()
                    os.utime(source, ns=(stat.st_atime_ns, stat.st_mtime_ns - 2_000_000_000))
                elif name == "build-release":
                    target = Path(command[command.index("--target-dir") + 1])
                    package = command[command.index("--bin") + 1]
                    harness.write_text(target / "release" / package, "compiled")
                elif name == "release-smoke":
                    stdout.write_text("1\n")
                if name in ("check-clean", "check-incremental", "build-release"):
                    package = command[command.index("--bin") + 1]
                    verb = "Compiling" if name == "build-release" else "Checking"
                    stderr.write_text(f"{verb} {package} v0.0.0 (test)\n")
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
                harness.run_cases(
                    report, run_dir, ["small"], harness.DEFAULT_SEED, 4, 15, 100,
                    generators=("prost",), repeats=1,
                )
            saved = json.loads((run_dir / "summary.json").read_text())
            self.assertEqual(saved["generators"], ["prost"])
            self.assertEqual(len(saved["cells"]), 1)
            cell = saved["cells"][0]
            self.assertEqual(cell["generator"], "prost")
            self.assertEqual(cell["repeat"], 0)
            self.assertEqual(cell["corpus"]["messages"], 6)
            self.assertEqual(cell["output"]["rust_file_count"], 1)
            self.assertTrue(cell["output"]["unchanged_generation_bytes_verified"])
            self.assertEqual(
                set(cell["phases"]), {
                    "consumer_lock", "generation", "generation_unchanged", "check_clean",
                    "check_incremental", "build_release", "release_smoke",
                },
            )
            self.assertTrue(cell["phases"]["release_smoke"]["output_verified"])
            self.assertNotIn("cargo", cell["phases"]["generation"]["command"][0])
            self.assertEqual(
                cell["phases"]["generation"]["command"][0],
                str(run_dir / "bin" / "sb09-prost-driver"),
            )
            self.assertEqual(
                cell["phases"]["generation"]["command"][3:],
                ["part_00.proto", "part_01.proto"],
            )
            self.assertFalse((run_dir / "cases" / "small" / "consumer" / "Cargo.toml").exists())

    def test_plan_execution_is_seeded_covering_shuffle(self):
        first = harness.plan_execution(["small", "100"], ("pbrs", "prost"), ("pbrs-native",), 2, 7)
        again = harness.plan_execution(["small", "100"], ("pbrs", "prost"), ("pbrs-native",), 2, 7)
        self.assertEqual(first, again)
        self.assertEqual(len(first), 8)
        self.assertEqual(len(set(first)), 8)
        cases = {case for case, _, _ in first}
        self.assertEqual(cases, {"small", "100"})
        shuffled = any(
            harness.plan_execution(["a", "b"], ("pbrs", "prost"), ("pbrs-native",), 1, seed)
            != [("a", "pbrs", 0), ("a", "prost", 0), ("b", "pbrs", 0), ("b", "prost", 0)]
            for seed in range(100)
        )
        self.assertTrue(shuffled)

    def test_summarize_reports_uncertainty(self):
        summary = harness.summarize([3, 1, 2])
        self.assertEqual(
            summary, {"n": 3, "min": 1, "median": 2, "mean": 2.0, "stdev": 1.0},
        )
        single = harness.summarize([42])
        self.assertEqual(single["n"], 1)
        self.assertIsNone(single["stdev"])
        self.assertEqual(single["median"], 42)

    def _matrix_cell(self, case, generator, rep, rust_bytes, binary_bytes, elapsed):
        phases = {}
        for name in harness.MATRIX_PHASES:
            phases[name] = {"elapsed_ns": elapsed, "peak_rss_bytes": elapsed // 2}
        return {
            "case": case, "generator": generator, "repeat": rep,
            "output": {"rust_bytes": rust_bytes},
            "release_binary": {"size_bytes": binary_bytes},
            "phases": phases,
        }

    def test_compute_matrix_lists_median_losses(self):
        report = {
            "repeats": 2,
            "cells": [
                self._matrix_cell("small", "pbrs", 0, 100, 500, 10),
                self._matrix_cell("small", "pbrs", 1, 100, 500, 30),
                self._matrix_cell("small", "prost", 0, 60, 900, 20),
                self._matrix_cell("small", "prost", 1, 60, 900, 40),
            ],
        }
        harness.compute_matrix(report)
        matrix = report["matrix"]
        self.assertEqual(matrix["repeats"], 2)
        self.assertEqual(
            matrix["cells"]["small/pbrs"]["metrics"]["output.rust_bytes"]["median"], 100,
        )
        self.assertEqual(
            matrix["cells"]["small/prost"]["metrics"]["build_release.elapsed_ns"]["n"], 2,
        )
        losses = {
            (row["generator"], row["metric"]): row["generator_loses"]
            for row in matrix["losses"]
        }
        # prost generates fewer bytes (wins) but a bigger binary (loses).
        self.assertFalse(losses[("prost", "output.rust_bytes")])
        self.assertTrue(losses[("prost", "release_binary.size_bytes")])
        self.assertTrue(losses[("prost", "build_release.elapsed_ns")])

    def test_compute_matrix_without_pbrs_has_no_losses(self):
        report = {
            "repeats": 1,
            "cells": [self._matrix_cell("small", "prost", 0, 60, 900, 20)],
        }
        harness.compute_matrix(report)
        self.assertEqual(report["matrix"]["losses"], [])
        self.assertEqual(
            report["matrix"]["cells"]["small/prost"]["metrics"]["output.rust_bytes"]["n"], 1,
        )

    def test_qualification_reasons_track_repeats_and_peers(self):
        self.assertEqual(
            harness.qualification_reasons(False, False, 1),
            ["no_equivalent_reference_peer", "single_run_diagnostic"],
        )
        self.assertEqual(
            harness.qualification_reasons(False, False, 5),
            ["no_equivalent_reference_peer"],
        )
        self.assertEqual(
            harness.qualification_reasons(False, True, 5), [],
        )
        self.assertEqual(
            harness.qualification_reasons(False, True, 5), [],
        )
        self.assertEqual(
            harness.qualification_reasons(True, False, 1),
            ["no_independent_pinned_host_qualification", "single_run_diagnostic",
             "no_paired_replicates_or_uncertainty"],
        )
        self.assertEqual(
            harness.qualification_reasons(True, False, 5),
            ["no_independent_pinned_host_qualification"],
        )

    def test_repeats_flag_reaches_pipeline(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            out = root / "target" / "codegen-bench" / "repeats"

            def fake_run(report, run_dir, cases, seed, jobs, timeout, sample_ms,
                         reference_protoc, generators=("pbrs",), repeats=5, stub_generators=("pbrs-native",)):
                self.assertEqual(repeats, 2)
                raise harness.BenchmarkError("stub pipeline reached")

            with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                os.environ, {"CARGO_BUILD_JOBS": "2"}
            ), mock.patch.object(
                harness, "run_cases", side_effect=fake_run
            ), contextlib.redirect_stderr(io.StringIO()):
                result = harness.main([
                    "--case", "small", "--repeats", "2", "--out", str(out),
                ])
            self.assertEqual(result, 1)
            with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                os.environ, {"CARGO_BUILD_JOBS": "2"}
            ), contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as raised:
                    harness.main(["--case", "small", "--repeats", "0"])
                self.assertEqual(raised.exception.code, 2)

    def test_repeats_use_isolated_targets_and_record_matrix(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            run_dir = Path(temporary) / "run"
            run_dir.mkdir()
            shared_target = root / "target" / "integration-consumers"
            shared_target.mkdir(parents=True)
            shared_generator = shared_target / "debug" / "cg19-generator"
            report = {
                "schema_version": "cg19/1", "status": "pending", "setup": {},
                "cells": [], "generators": ["pbrs"], "repeats": 2,
            }
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
                    harness.write_text(shared_generator, "test generator")
                    shared_generator.chmod(0o755)
                elif name == "consumer-lock":
                    manifest = Path(command[command.index("--manifest-path") + 1])
                    manifest.with_name("Cargo.lock").write_text("consumer lock")
                elif name == "generation":
                    output = run_dir / "cases" / "small" / "consumer" / "generated"
                    harness.write_text(output / "mod.rs", 'include!("part_00.rs");\n')
                    for proto in command[4:]:
                        harness.write_text(
                            output / proto.replace(".proto", ".rs"), "pub struct M;\n",
                        )
                elif name == "check-clean":
                    target = Path(command[command.index("--target-dir") + 1])
                    target.mkdir(parents=True)
                    manifest = Path(command[command.index("--manifest-path") + 1])
                    source = manifest.parent / "src" / "main.rs"
                    stat = source.stat()
                    os.utime(source, ns=(stat.st_atime_ns, stat.st_mtime_ns - 2_000_000_000))
                elif name == "build-release":
                    target = Path(command[command.index("--target-dir") + 1])
                    package = command[command.index("--bin") + 1]
                    harness.write_text(target / "release" / package, "compiled")
                elif name == "release-smoke":
                    stdout.write_text("1\n")
                if name in ("check-clean", "check-incremental", "build-release"):
                    package = command[command.index("--bin") + 1]
                    verb = "Compiling" if name == "build-release" else "Checking"
                    stderr.write_text(f"{verb} {package} v0.0.0 (test)\n")
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
                harness.run_cases(
                    report, run_dir, ["small"], harness.DEFAULT_SEED, 4, 15, 100, repeats=2,
                )
            saved = json.loads((run_dir / "summary.json").read_text())
            self.assertEqual(len(saved["cells"]), 2)
            self.assertEqual({cell["repeat"] for cell in saved["cells"]}, {0, 1})
            targets = [cell["target_dir"] for cell in saved["cells"]]
            self.assertEqual(len(set(targets)), 2)
            self.assertTrue(all("target-r" in target for target in targets))
            self.assertEqual(len(saved["execution_order"]), 2)
            self.assertEqual(
                saved["matrix"]["cells"]["small/pbrs"]["metrics"]["output.rust_bytes"]["n"], 2,
            )
            self.assertEqual(saved["matrix"]["losses"], [])

    def test_stub_corpus_renders_one_service_per_file(self):
        first = harness.render_proto_with_services(harness.DEFAULT_SEED, 6, 2, 0)
        again = harness.render_proto_with_services(harness.DEFAULT_SEED, 6, 2, 0)
        self.assertEqual(first, again)
        self.assertIn("service Service00 {", first)
        self.assertIn("rpc Get (Message0000) returns (Message0001);", first)
        self.assertIn("rpc Watch (Message0000) returns (stream Message0001);", first)
        second = harness.render_proto_with_services(harness.DEFAULT_SEED, 6, 2, 1)
        self.assertIn("service Service01 {", second)
        self.assertIn("rpc Get (Message0003) returns (Message0004);", second)
        self.assertEqual(
            harness.stub_services("svc-small"),
            [("Service00", 0, 1), ("Service01", 3, 4)],
        )
        with tempfile.TemporaryDirectory() as temporary:
            case_dir = Path(temporary) / "case"
            names, metadata = harness.prepare_corpus(
                case_dir, "svc-small", harness.DEFAULT_SEED, ("pbrs-native",),
            )
            self.assertEqual(names, ["part_00.proto", "part_01.proto"])
            self.assertEqual(metadata["messages"], 6)
            self.assertEqual(metadata["services"], ["Service00", "Service01"])
            self.assertFalse((case_dir / "consumer" / "Cargo.toml").exists())

    def test_stub_consumers_do_message_work_plus_server_client(self):
        services = [("Service00", 0, 1), ("Service01", 3, 4)]
        native = harness.render_consumer_pbrs_native(6, 0, services)
        self.assertEqual(native.count("roundtrip(bench::cg19::Message"), 6)
        self.assertIn("impl bench::cg19::Service00 for Service00Svc {}", native)
        self.assertIn("bench::cg19::Service01Server::new(Service01Svc)", native)
        self.assertIn("bench::cg19::Service01Client::new(channel)", native)
        self.assertIn('connect_lazy("127.0.0.1:1")', native)
        self.assertNotEqual(native, harness.render_consumer_pbrs_native(6, 1, services))
        tonic = harness.render_consumer_pbrs_tonic(6, 0, services)
        self.assertEqual(tonic.count("roundtrip(bench::cg19::Message"), 6)
        self.assertIn("impl bench::cg19::Service00 for Service00Svc {", tonic)
        self.assertIn("async fn get(&self", tonic)
        self.assertIn("async fn watch(&self", tonic)
        self.assertIn("type WatchStream = tokio_stream::wrappers::ReceiverStream<", tonic)
        self.assertIn("Channel::from_static(", tonic)
        build = harness.render_consumer_tonic_build(6, 0, services)
        self.assertEqual(build.count("roundtrip(bench_cg19::Message"), 6)
        self.assertIn("#[tonic::async_trait]", build)
        self.assertIn("impl bench_cg19::service00_server::Service00 for Service00Svc {", build)
        self.assertIn("bench_cg19::service01_client::Service01Client::new(channel)", build)
        self.assertEqual(
            harness.render_consumer_for(6, 0, "pbrs-native", services), native,
        )
        with self.assertRaisesRegex(harness.BenchmarkError, "needs services"):
            harness.render_consumer_for(6, 0, "pbrs-native", None)
        self.assertEqual(harness.tonics_snake("Service00"), "service00")
        self.assertEqual(harness.tonics_snake("Lookup"), "lookup")

    def test_stub_manifests_pin_runtime_stacks(self):
        try:
            import tomllib
        except ModuleNotFoundError:
            self.skipTest("Python 3.11+ tomllib required")
        native = harness.stub_manifest("sb09-pbrs-native-consumer-svc-small", "pbrs-native")
        parsed = tomllib.loads(native)
        self.assertIn("pbrs-grpc", parsed["dependencies"])
        for generator in ("pbrs-native", "pbrs-tonic", "tonic-build"):
            with self.subTest(generator=generator):
                manifest = harness.stub_manifest(f"sb09-{generator}-x", generator)
                parsed = tomllib.loads(manifest)
                self.assertEqual(parsed["package"]["name"], f"sb09-{generator}-x")
        self.assertIn('pbrs-grpc = { path = "', native)
        self.assertNotIn("tonic", native)
        ptonic = harness.stub_manifest("sb09-pbrs-tonic-consumer-svc-small", "pbrs-tonic")
        self.assertIn('protobuf-tonic = { path = "', ptonic)
        self.assertIn(f'tonic = {{ version = "={harness.TONIC014_VERSION}"', ptonic)
        self.assertIn(f'tokio-stream = "={harness.TOKIO_STREAM_VERSION}"', ptonic)
        build = harness.stub_manifest("sb09-tonic-build-consumer-svc-small", "tonic-build")
        self.assertIn(f'prost = "={harness.PROST013_VERSION}"', build)
        self.assertIn(f'tonic = {{ version = "={harness.TONIC013_VERSION}"', build)
        self.assertNotIn("pbrs", build)
        with self.assertRaisesRegex(harness.BenchmarkError, "unknown stub generator"):
            harness.stub_manifest("x", "capnp")
        driver = harness.peer_driver_manifest("tonic-build")
        self.assertIn(f'tonic-build = "={harness.TONIC_BUILD_VERSION}"', driver)

    def test_stub_snapshot_tables(self):
        names = ["part_00.proto", "part_01.proto"]
        self.assertEqual(
            harness.peer_expected_files("pbrs-native", names),
            frozenset({"mod.rs", "part_00.rs", "part_01.rs"}),
        )
        self.assertEqual(
            harness.peer_expected_files("pbrs-tonic", names),
            frozenset({"mod.rs", "part_00.rs", "part_01.rs"}),
        )
        self.assertEqual(
            harness.peer_expected_files("tonic-build", names),
            frozenset({harness.PROST_PACKAGE_FILE}),
        )

    def test_generators_for_case_routes_by_corpus_kind(self):
        message = ("pbrs", "prost")
        stubs = ("pbrs-native", "tonic-build")
        self.assertEqual(harness.generators_for_case("small", message, stubs), message)
        self.assertEqual(harness.generators_for_case("100", message, stubs), message)
        self.assertEqual(harness.generators_for_case("svc-small", message, stubs), stubs)
        plan = harness.plan_execution(["small", "svc-small"], message, stubs, 1, 11)
        self.assertEqual(len(plan), 4)
        kinds = {(case, generator) for case, generator, _ in plan}
        self.assertEqual(
            kinds,
            {("small", "pbrs"), ("small", "prost"),
             ("svc-small", "pbrs-native"), ("svc-small", "tonic-build")},
        )

    def test_stub_generators_flag_rejects_unknown_before_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with mock.patch.object(harness, "ROOT", root), mock.patch.dict(
                os.environ, {"CARGO_BUILD_JOBS": "2"}
            ), contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as raised:
                    harness.main(["--case", "svc-small", "--stub-generators", "prost"])
                self.assertEqual(raised.exception.code, 2)
            self.assertFalse((root / "target").exists())

    def test_pbrs_native_pipeline_uses_strict_stubs_without_compilers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            run_dir = Path(temporary) / "run"
            run_dir.mkdir()
            shared_target = root / "target" / "integration-consumers"
            shared_target.mkdir(parents=True)
            shared_generator = shared_target / "debug" / "cg19-generator"
            report = {
                "schema_version": "cg19/1", "status": "pending", "setup": {},
                "cells": [], "generators": ["pbrs"], "stub_generators": ["pbrs-native"],
                "repeats": 1,
            }
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
            seen_env = {}

            def fake_command(command, cwd, env, stem, root, timeout, sample_ms):
                name = stem.name
                stdout, stderr, paths = harness.log_paths(stem, root)
                stdout.parent.mkdir(parents=True, exist_ok=True)
                stdout.write_text("")
                stderr.write_text("")
                if name == "driver-lock":
                    (run_dir / "driver" / "Cargo.lock").write_text("driver lock")
                elif name == "driver-build":
                    harness.write_text(shared_generator, "test generator")
                    shared_generator.chmod(0o755)
                elif name == "consumer-lock":
                    manifest = Path(command[command.index("--manifest-path") + 1])
                    manifest.with_name("Cargo.lock").write_text("stub lock")
                elif name in ("generation", "generation-unchanged"):
                    self.assertEqual(command[0], str(run_dir / "bin" / "cg19-generator"))
                    seen_env[name] = env.get("SB09_PBRS_STUBS")
                    if name == "generation":
                        output = (
                            run_dir / "cases" / "svc-small" / "gen" / "pbrs-native"
                            / "consumer" / "generated"
                        )
                        harness.write_text(output / "mod.rs", 'include!("part_00.rs");\n')
                        for proto in command[4:]:
                            harness.write_text(
                                output / proto.replace(".proto", ".rs"), "pub struct M;\n",
                            )
                elif name == "check-clean":
                    target = Path(command[command.index("--target-dir") + 1])
                    target.mkdir(parents=True)
                    manifest = Path(command[command.index("--manifest-path") + 1])
                    source = manifest.parent / "src" / "main.rs"
                    stat = source.stat()
                    os.utime(source, ns=(stat.st_atime_ns, stat.st_mtime_ns - 2_000_000_000))
                elif name == "build-release":
                    target = Path(command[command.index("--target-dir") + 1])
                    package = command[command.index("--bin") + 1]
                    harness.write_text(target / "release" / package, "compiled")
                elif name == "release-smoke":
                    stdout.write_text("1\n")
                if name in ("check-clean", "check-incremental", "build-release"):
                    package = command[command.index("--bin") + 1]
                    verb = "Compiling" if name == "build-release" else "Checking"
                    stderr.write_text(f"{verb} {package} v0.0.0 (test)\n")
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
                harness.run_cases(
                    report, run_dir, ["svc-small"], harness.DEFAULT_SEED, 4, 15, 100,
                    repeats=1,
                )
            saved = json.loads((run_dir / "summary.json").read_text())
            self.assertEqual(len(saved["cells"]), 1)
            cell = saved["cells"][0]
            self.assertEqual(cell["generator"], "pbrs-native")
            self.assertEqual(cell["corpus"]["services"], ["Service00", "Service01"])
            self.assertEqual(cell["output"]["rust_file_count"], 3)
            self.assertEqual(cell["output"]["unchanged_generation_verified_files"], 3)
            self.assertEqual(
                seen_env, {"generation": "native", "generation-unchanged": "native"},
            )
            self.assertTrue(cell["phases"]["release_smoke"]["output_verified"])
            self.assertEqual(saved["stub_generators"], ["pbrs-native"])

    def test_v4_pipeline_uses_pinned_protoc_and_validated_lock(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            run_dir = Path(temporary) / "run"
            run_dir.mkdir()
            pinned = run_dir / "pinned-protoc"
            pinned.write_text("test protoc")
            report = {
                "schema_version": "cg19/1", "status": "pending", "setup": {},
                "cells": [], "generators": ["v4"],
            }
            environment = {
                "tools": {
                    "cargo": {"executable": "fake-cargo"},
                    "rustc": {"executable": "fake-rustc"},
                    "protoc": {"executable": str(pinned)},
                },
                "repository": {"source_sha256": {}},
                "cache": {},
            }
            pinned_lock = (
                'version = 3\n[[package]]\nname = "protobuf"\n'
                f'version = "{harness.REFERENCE_RUNTIME_VERSION}"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                f'checksum = "{harness.REFERENCE_RUNTIME_CHECKSUM}"\n'
                '[[package]]\nname = "protobuf-macros"\n'
                f'version = "{harness.REFERENCE_RUNTIME_VERSION}"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                f'checksum = "{harness.REFERENCE_MACROS_CHECKSUM}"\n'
            )

            def fake_command(command, cwd, env, stem, root, timeout, sample_ms):
                name = stem.name
                stdout, stderr, paths = harness.log_paths(stem, root)
                stdout.parent.mkdir(parents=True, exist_ok=True)
                stdout.write_text("")
                stderr.write_text("")
                _case_dir = run_dir / "cases" / "small"
                if name == "consumer-lock":
                    manifest = Path(command[command.index("--manifest-path") + 1])
                    manifest.with_name("Cargo.lock").write_text(pinned_lock)
                elif name == "generation":
                    self.assertEqual(command[0], str(pinned))
                    self.assertTrue(any(part.startswith("--rust_out=") for part in command))
                    self.assertIn(f"--rust_opt={harness.REFERENCE_RUST_OPT}", command)
                    output = _case_dir / "gen" / "v4" / "consumer" / "generated"
                    harness.write_text(output / "generated.rs", "pub mod x {}\n")
                    for proto in command[4:]:
                        harness.write_text(
                            output / proto.replace(".proto", ".u.pb.rs"), "pub struct M;\n",
                        )
                elif name == "check-clean":
                    target = Path(command[command.index("--target-dir") + 1])
                    target.mkdir(parents=True)
                    manifest = Path(command[command.index("--manifest-path") + 1])
                    source = manifest.parent / "src" / "main.rs"
                    stat = source.stat()
                    os.utime(source, ns=(stat.st_atime_ns, stat.st_mtime_ns - 2_000_000_000))
                elif name == "build-release":
                    target = Path(command[command.index("--target-dir") + 1])
                    package = command[command.index("--bin") + 1]
                    harness.write_text(target / "release" / package, "compiled")
                elif name == "release-smoke":
                    stdout.write_text("1\n")
                if name in ("check-clean", "check-incremental", "build-release"):
                    package = command[command.index("--bin") + 1]
                    verb = "Compiling" if name == "build-release" else "Checking"
                    stderr.write_text(f"{verb} {package} v0.0.0 (test)\n")
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
                harness, "resolve_pinned_protoc", return_value=pinned
            ), mock.patch.object(
                harness.time, "sleep", return_value=None
            ):
                harness.run_cases(
                    report, run_dir, ["small"], harness.DEFAULT_SEED, 4, 15, 100,
                    generators=("v4",), repeats=1,
                )
            saved = json.loads((run_dir / "summary.json").read_text())
            self.assertEqual(saved["generators"], ["v4"])
            cell = saved["cells"][0]
            self.assertEqual(cell["generator"], "v4")
            self.assertEqual(cell["output"]["rust_file_count"], 3)
            self.assertEqual(
                cell["runtime_lock"]["packages"]["protobuf"]["checksum"],
                harness.REFERENCE_RUNTIME_CHECKSUM,
            )
            self.assertTrue(cell["phases"]["release_smoke"]["output_verified"])
            self.assertFalse((run_dir / "driver-v4").exists())


class RealisticCorpusTests(unittest.TestCase):
    def test_realistic_specs_are_self_consistent(self):
        self.assertEqual(
            set(harness.REALISTIC_CORPORA),
            {"otlp", "googleapis", "envoy-core", "envoy-discovery"},
        )
        for case, spec in harness.REALISTIC_CORPORA.items():
            with self.subTest(case=case):
                inputs, support = spec["inputs"], spec["support"]
                self.assertEqual(len(set(inputs)), len(inputs))
                self.assertFalse(set(inputs) & set(support))
                for name in inputs + support:
                    self.assertIn(name, harness.VENDOR_FILES, name)
                    self.assertIn(name, spec["packages"], name)
                for name in inputs:
                    self.assertIn(name, spec["messages"], name)
                entries = harness.realistic_entries(case)
                self.assertGreater(len(entries), 0)
                messages = [message for _, message in entries]
                self.assertEqual(len(set(messages)), len(messages))
                for name, package in spec["packages"].items():
                    self.assertRegex(package, r"^[a-z][\w.]*$")
                for package in spec["prost_omitted_packages"]:
                    self.assertIn(package, harness.realistic_closure_packages(case))
                for generator, reason in spec["exclude"].items():
                    self.assertIn(generator, harness.MESSAGE_GENERATORS)
                    self.assertTrue(reason)
                names = harness.realistic_generation_names(case)
                self.assertEqual(names[: len(inputs)], inputs)
                self.assertEqual(len(set(names)), len(names))

    def test_keyword_escaping_matches_generators(self):
        self.assertEqual(harness.rust_mod_ident("type"), "r#type")
        self.assertEqual(harness.rust_mod_ident("match"), "r#match")
        self.assertEqual(harness.rust_mod_ident("v3"), "v3")
        self.assertEqual(harness.rust_mod_ident("self"), "self_")
        paths = harness.realistic_entry_paths("envoy-core")
        self.assertIn("envoy::r#type::v3::Percent", paths)
        self.assertIn("envoy::config::core::v3::SocketAddress", paths)
        self.assertEqual(len(paths), 16)

    def test_entries_consumers_cover_every_message(self):
        for case in harness.REALISTIC_CORPORA:
            entries = harness.realistic_entries(case)
            for generator in harness.MESSAGE_GENERATORS:
                if generator in harness.REALISTIC_CORPORA[case]["exclude"]:
                    continue
                with self.subTest(case=case, generator=generator):
                    rendered = harness.render_consumer_entries(case, 0, generator)
                    self.assertEqual(rendered.count("roundtrip("), len(entries))
                    self.assertNotEqual(
                        rendered, harness.render_consumer_entries(case, 1, generator),
                    )
        prost = harness.render_consumer_entries("envoy-core", 0, "prost")
        self.assertIn("pub mod r#type {", prost)
        self.assertIn("/generated/envoy.r#type.v3.rs", prost)
        self.assertIn("envoy::r#type::v3::Percent::default()", prost)
        native = harness.render_consumer_entries("otlp", 0, "pbrs")
        self.assertIn("opentelemetry::proto::trace::v1::Span::new()", native)
        flat = harness.render_consumer_entries("googleapis", 0, "v4")
        self.assertIn(
            '#[path = "../generated/google/api/generated.rs"] mod generated;', flat,
        )
        self.assertIn("generated::HttpRule::new()", flat)

    def test_prost_package_tree_nests_every_segment(self):
        tree = harness.prost_package_tree(["envoy.type.v3", "validate"])
        self.assertIn("pub mod envoy {", tree)
        self.assertIn("pub mod r#type {", tree)
        self.assertIn("pub mod v3 {", tree)
        self.assertIn("/generated/envoy.r#type.v3.rs", tree)
        self.assertIn("pub mod validate {", tree)
        self.assertIn("/generated/validate.rs", tree)
        # One module level per package segment for prost's super:: chains.
        self.assertEqual(tree.count("pub mod "), 4)

    def test_realistic_expected_files_match_spike_layouts(self):
        self.assertEqual(
            harness.prost_package_files("envoy-core"),
            ["envoy.config.core.v3.rs", "envoy.r#type.v3.rs",
             "udpa.annotations.rs", "validate.rs"],
        )
        self.assertEqual(
            set(harness.prost_package_files("envoy-discovery")),
            {"envoy.config.core.v3.rs", "envoy.r#type.v3.rs",
             "envoy.service.discovery.v3.rs", "google.rpc.rs", "udpa.annotations.rs",
             "validate.rs", "xds.annotations.v3.rs", "xds.core.v3.rs"},
        )
        otlp_names = harness.realistic_generation_names("otlp")
        self.assertEqual(
            harness.realistic_expected_files("v4", otlp_names, "otlp"),
            frozenset({
                "opentelemetry/proto/trace/v1/generated.rs",
                "opentelemetry/proto/trace/v1/trace.u.pb.rs",
                "opentelemetry/proto/metrics/v1/metrics.u.pb.rs",
                "opentelemetry/proto/logs/v1/logs.u.pb.rs",
                "opentelemetry/proto/common/v1/common.u.pb.rs",
                "opentelemetry/proto/resource/v1/resource.u.pb.rs",
            }),
        )
        disc_names = harness.realistic_generation_names("envoy-discovery")
        disc_pbrs = harness.realistic_expected_files("pbrs", disc_names, "envoy-discovery")
        # Three status.proto files share one basename: mirrored only, no flat file.
        self.assertNotIn("status.rs", disc_pbrs)
        self.assertIn("google/rpc/status.rs", disc_pbrs)
        self.assertIn("udpa/annotations/status.rs", disc_pbrs)
        self.assertIn("xds/annotations/v3/status.rs", disc_pbrs)
        self.assertIn("discovery.rs", disc_pbrs)
        gapi_names = harness.realistic_generation_names("googleapis")
        self.assertEqual(
            harness.realistic_expected_files("pbrs", gapi_names, "googleapis"),
            frozenset({
                "mod.rs",
                "annotations.rs", "http.rs", "httpbody.rs", "any.rs", "descriptor.rs",
                "google/api/annotations.rs", "google/api/http.rs",
                "google/api/httpbody.rs", "google/protobuf/any.rs",
                "google/protobuf/descriptor.rs",
            }),
        )
        otlp_content = {
            name: {"messages": True, "enums": False, "extends": False}
            for name in otlp_names
        }
        core = harness.realistic_buffa_core(otlp_names, "otlp", otlp_content)
        self.assertIn("mod.rs", core)
        self.assertIn("opentelemetry.proto.trace.v1.trace.rs", core)
        self.assertIn("opentelemetry.proto.trace.v1.trace.__view.rs", core)
        self.assertIn("opentelemetry.proto.trace.v1.mod.rs", core)
        self.assertEqual(len(core), 1 + 2 * len(otlp_names) + 5)
        gapi_content = {
            name: {
                "messages": name != "google/api/annotations.proto",
                "enums": False,
                "extends": name == "google/api/annotations.proto",
            }
            for name in gapi_names
        }
        gapi_core = harness.realistic_buffa_core(gapi_names, "googleapis", gapi_content)
        self.assertIn("google.api.annotations.__ext.rs", gapi_core)
        self.assertNotIn("google.api.annotations.rs", gapi_core)
        self.assertIn("google.api.http.rs", gapi_core)
        enum_content = dict(gapi_content)
        enum_content["google/api/http.proto"] = {
            "messages": False, "enums": True, "extends": False,
        }
        enum_core = harness.realistic_buffa_core(gapi_names, "googleapis", enum_content)
        self.assertIn("google.api.http.rs", enum_core)
        self.assertNotIn("google.api.http.__view.rs", enum_core)
        with self.assertRaisesRegex(harness.BenchmarkError, "does not run"):
            harness.realistic_expected_files("tonic-build", otlp_names, "otlp")
        with self.assertRaisesRegex(harness.BenchmarkError, "content classes"):
            harness.realistic_expected_files("buffa", otlp_names, "otlp")

    def test_buffa_aux_outputs_are_constrained_to_known_suffixes(self):
        core = frozenset({"mod.rs", "a.b.rs", "a.b.__view.rs", "a.mod.rs"})
        observed = {name: None for name in core | {"a.b.__oneof.rs", "a.b.__ext.rs"}}
        harness.assert_buffa_realistic_outputs("otlp", "buffa", observed, core)
        with self.assertRaisesRegex(harness.BenchmarkError, "missing core"):
            harness.assert_buffa_realistic_outputs(
                "otlp", "buffa", {k: v for k, v in observed.items() if k != "a.b.rs"}, core,
            )
        with self.assertRaisesRegex(harness.BenchmarkError, "unexpected auxiliary"):
            harness.assert_buffa_realistic_outputs(
                "otlp", "buffa", {**observed, "a.b.__mystery.rs": None}, core,
            )
        with self.assertRaisesRegex(harness.BenchmarkError, "unexpected auxiliary"):
            harness.assert_buffa_realistic_outputs(
                "otlp", "buffa", {**observed, "zzz.__oneof.rs": None}, core,
            )

    def test_generators_for_case_applies_realistic_exclusions(self):
        message = ("pbrs", "prost", "buffa", "v4")
        stubs = ("pbrs-native",)
        self.assertEqual(
            harness.generators_for_case("envoy-discovery", message, stubs),
            ("pbrs", "prost", "buffa"),
        )
        self.assertEqual(
            harness.generators_for_case("envoy-core", message, stubs), message,
        )
        self.assertEqual(
            harness.excluded_cells(["envoy-discovery", "otlp"], message),
            [{
                "case": "envoy-discovery",
                "generator": "v4",
                "reason": harness.REALISTIC_CORPORA["envoy-discovery"]["exclude"]["v4"],
            }],
        )
        plan = harness.plan_execution(["envoy-discovery"], message, stubs, 1, 11)
        self.assertEqual(len(plan), 3)
        self.assertNotIn("v4", {generator for _, generator, _ in plan})

    def test_peer_manifest_adds_prost_types_only_with_wkt(self):
        with_types = harness.peer_manifest("x", "prost", "googleapis")
        self.assertIn(f'prost-types = "={harness.PROST_TYPES_VERSION}"', with_types)
        without_types = harness.peer_manifest("x", "prost", "otlp")
        self.assertNotIn("prost-types", without_types)
        seeded = harness.peer_manifest("x", "prost")
        self.assertNotIn("prost-types", seeded)

    def test_v4_entrypoint_prediction_points_at_first_input_dir(self):
        self.assertEqual(
            harness.v4_entrypoint_rel("otlp"),
            "opentelemetry/proto/trace/v1/generated.rs",
        )
        self.assertEqual(
            harness.v4_entrypoint_rel("envoy-discovery"),
            "envoy/service/discovery/v3/generated.rs",
        )
        self.assertEqual(
            harness.v4_entrypoint_rel("googleapis"), "google/api/generated.rs",
        )

    def test_prepare_realistic_corpus_verifies_and_lays_out(self):
        spec = harness.REALISTIC_CORPORA["otlp"]

        def fake_vendor(package_override=None, messages_override=None):
            def supply(wanted):
                base = Path(temporary) / "vendor"
                for name in wanted:
                    package = (package_override or {}).get(name, spec["packages"][name])
                    messages = (messages_override or {}).get(name, spec["messages"][name])
                    content = (
                        'syntax = "proto3";\n'
                        f"package {package};\n"
                        + "".join(f"message {m} {{\n}}\n" for m in messages)
                    )
                    dest = base / name
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    dest.write_text(content)
                return base
            return supply

        with tempfile.TemporaryDirectory() as temporary:
            case_dir = Path(temporary) / "case"
            with mock.patch.object(harness, "ensure_vendor", side_effect=fake_vendor()):
                names, metadata = harness.prepare_corpus(
                    case_dir, "otlp", harness.DEFAULT_SEED, ("pbrs",),
                )
            self.assertEqual(names, spec["inputs"])
            self.assertEqual(metadata["messages"], 30)
            self.assertEqual(metadata["proto_file_count"], 5)
            self.assertEqual(metadata["support_file_count"], 0)
            self.assertEqual(len(metadata["inputs"]), 5)
            self.assertEqual(len(metadata["entries"]), 30)
            first_content = metadata["content"][spec["inputs"][0]]
            self.assertEqual(
                first_content, {"messages": True, "enums": False, "extends": False},
            )
            self.assertTrue(
                (case_dir / "consumer" / "proto"
                 / "opentelemetry" / "proto" / "trace" / "v1" / "trace.proto").is_file()
            )
            main_rs = (case_dir / "consumer" / "src" / "main.rs").read_text()
            self.assertIn("opentelemetry::proto::trace::v1::Span::new()", main_rs)
            first = spec["inputs"][0]
            with mock.patch.object(
                harness, "ensure_vendor",
                side_effect=fake_vendor(messages_override={first: ["Wrong"]}),
            ):
                with self.assertRaisesRegex(harness.BenchmarkError, "messages"):
                    harness.prepare_corpus(
                        Path(temporary) / "bad-msgs", "otlp", harness.DEFAULT_SEED, ("pbrs",),
                    )
            with mock.patch.object(
                harness, "ensure_vendor",
                side_effect=fake_vendor(package_override={first: "wrong.pkg"}),
            ):
                with self.assertRaisesRegex(harness.BenchmarkError, "package"):
                    harness.prepare_corpus(
                        Path(temporary) / "bad-pkg", "otlp", harness.DEFAULT_SEED, ("pbrs",),
                    )

    def test_ensure_vendor_fetches_once_and_verifies_hash(self):
        content = b'syntax = "proto3";\npackage test;\n'
        digest = hashlib.sha256(content).hexdigest()
        fake_files = {"x/y.proto": ("repo", "ref", "x/y.proto", digest)}
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary) / "vendor"
            with mock.patch.object(harness, "VENDOR_FILES", fake_files), mock.patch.object(
                harness, "vendor_dir", return_value=base
            ), mock.patch("urllib.request.urlopen") as http:
                response = mock.MagicMock()
                response.read.return_value = content
                http.return_value.__enter__.return_value = response
                self.assertEqual(harness.ensure_vendor(["x/y.proto"]), base)
                self.assertEqual((base / "x" / "y.proto").read_bytes(), content)
                http.assert_called_once()
                http.reset_mock()
                harness.ensure_vendor(["x/y.proto"])
                http.assert_not_called()
                (base / "x" / "y.proto").unlink()
                response.read.return_value = b"tampered"
                with self.assertRaisesRegex(harness.BenchmarkError, "hash mismatch"):
                    harness.ensure_vendor(["x/y.proto"])
                http.reset_mock()
                http.side_effect = OSError("network down")
                with self.assertRaisesRegex(harness.BenchmarkError, "fetch failed"):
                    harness.ensure_vendor(["x/y.proto"])


if __name__ == "__main__":
    unittest.main()
