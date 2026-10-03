"""Python-only process/resource/preservation and corrupted-evidence fixtures; no Cargo or perf."""
import copy
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest import mock

import factor_common as f
import retirement_adapter as retirement


HERE = Path(__file__).parent


def module(name, file):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); return m


c = module("factor_coordinator_test", "coordinate-gn03-factors.py")
a = module("factor_auditor_test", "audit-gn03-factors.py")
prep = module("factor_prepare_test", "prepare-factor-inputs.py")
lock_fixture = module("factor_lock_fixture", "lock-lifetime-fixture.py")


def wait_for(predicate, timeout=3.0):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if predicate():
            return
        time.sleep(0.01)
    raise AssertionError("fixture deadline exceeded")


class Fixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="gn03-python-fixture-", dir=HERE)
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def observer(self):
        return f.ProcessObserver(self.root / "processes.json")

    def child(self, observer, code="import time;time.sleep(120)"):
        return observer.Popen([sys.executable, "-B", "-c", code], start_new_session=True,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def test_independent_sessions_cleanup_and_no_global_monkeypatch(self):
        original = subprocess.Popen; observer = self.observer()
        p1 = self.child(observer); p2 = self.child(observer)
        self.assertNotEqual(p1.pid, p2.pid)
        self.assertTrue(all(f.live_group(row) for row in observer.owners))
        observer.cleanup(0.3)
        self.assertTrue(all(row["exit_code"] is not None for row in observer.owners))
        self.assertTrue(all(not f.live_group(row) for row in observer.owners))
        self.assertIs(subprocess.Popen, original)

    def test_exited_reaped_leader_lingering_child_is_cleaned(self):
        observer = self.observer(); ready = self.root / "child.pid"
        code = "import subprocess,sys,time;from pathlib import Path;p=subprocess.Popen([sys.executable,'-B','-c','import time;time.sleep(120)']);Path(sys.argv[1]).write_text(str(p.pid));time.sleep(.2)"
        parent = observer.Popen([sys.executable, "-B", "-c", code, str(ready)], start_new_session=True,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_for(ready.exists); self.assertEqual(parent.wait(timeout=3), 0)
        self.assertIsNone(f.read_proc(parent.pid))
        self.assertTrue(f.live_group(observer.owners[0]))
        observer.cleanup(0.3)
        self.assertFalse(f.live_group(observer.owners[0]))
        child = f.read_proc(int(ready.read_text()))
        self.assertTrue(child is None or child["state"] == "Z")

    def test_unanchored_and_reused_group_refuse_before_signal(self):
        observer = self.observer(); p = self.child(observer)
        try:
            bad = copy.deepcopy(observer.owners[0]); del bad["anchor"]
            with self.assertRaisesRegex(ValueError, "unanchored"):
                f.live_group(bad)
            bad = copy.deepcopy(observer.owners[0]); bad["start_ticks"] += 1; bad["anchor_stat"]["start_ticks"] += 1
            with mock.patch.object(os, "killpg") as kill:
                with self.assertRaisesRegex(ValueError, "identity changed"):
                    f.live_group(bad)
                kill.assert_not_called()
        finally:
            observer.cleanup(0.3)

    def test_phase_without_new_session_refuses_no_child(self):
        observer = self.observer()
        with mock.patch.object(subprocess, "Popen") as popen:
            with self.assertRaisesRegex(ValueError, "new-session"):
                observer.Popen([sys.executable, "-B", "-c", "pass"])
            popen.assert_not_called()

    def test_sampled_cache_and_floor_failure_before_launch(self):
        observer = self.observer()
        for cache, free in ((f.CAP + 1, f.FLOOR * 2), (0, f.FLOOR - 1)):
            with self.assertRaises(ValueError):
                with f.ResourceGuard(self.root / "cache", "/workspace", observer, self.root / "guard.json",
                                     sample=lambda: (cache, free)):
                    self.fail("guard admitted bad resource bounds")

    def test_guard_failure_cleans_actual_child_and_blocks_next_phase(self):
        observer = self.observer(); p = self.child(observer)
        calls = [0]
        def sample():
            calls[0] += 1
            return (0 if calls[0] == 1 else f.CAP + 1, f.FLOOR * 2)
        with self.assertRaisesRegex(ValueError, "resource guard failed"):
            with f.ResourceGuard(self.root / "cache", "/workspace", observer, self.root / "guard.json",
                                 interval=.02, sample=sample) as guard:
                wait_for(lambda: bool(guard.failures))
        self.assertIsNotNone(p.poll())
        self.assertTrue(observer.blocked.is_set())
        with self.assertRaisesRegex(ValueError, "refusing new phase"):
            self.child(observer)

    def test_environment_rejects_all_extra_compiler_target_controls(self):
        tools = {"rustc": {"path": "/qualified/rustc"}, "protoc": {"path": "/qualified/protoc"}}
        keys = list(f.UNSUPPORTED_ENV) + ["CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER", "CC_x86_64_unknown_linux_gnu", "CARGO_PROFILE_DEV_DEBUG"]
        for key in keys:
            with self.subTest(key=key), mock.patch.dict(os.environ, {key: "unexpected"}, clear=True):
                with self.assertRaises(ValueError): f.clean_env(tools, "default")
        with mock.patch.dict(os.environ, {"SB09_PBRS_EMIT_JSON": "0", "PURE_PROTOBUF_TEST": "x"}, clear=True):
            env, cleared = f.clean_env(tools, "shared")
            self.assertEqual(env["SB09_PBRS_SHARED_DESCRIPTOR_SET"], "1")
            self.assertNotIn("SB09_PBRS_EMIT_JSON", env)
            self.assertEqual(len(cleared), 2)

    def test_parent_manifest_environment_mask_restores_even_after_failure(self):
        env = {"SB09_PBRS_RUNTIME_PROFILE": "minimal", "PURE_PROTOBUF_TEST": "fixture", "PATH": "/retained/path"}
        with mock.patch.dict(os.environ, env, clear=True):
            with self.assertRaisesRegex(RuntimeError, "synthetic"):
                with f.clean_parent_codegen_env():
                    self.assertEqual(dict(os.environ), {"PATH": "/retained/path"})
                    raise RuntimeError("synthetic")
            self.assertEqual(dict(os.environ), env)

    def test_ofd_and_flock_remain_held_after_another_fd_hash_closes(self):
        record = lock_fixture.run_fixture(self.root / "locks")
        self.assertEqual(record["status"], "passed")
        self.assertEqual(record["observations_count"], 10)

    def cache(self):
        cache = self.root / "cache"; (cache / "debug/deps").mkdir(parents=True)
        (cache / "debug/.cargo-lock").write_bytes(b"")
        (cache / "debug/deps/object.o").write_bytes(b"\x7fELFsynthetic-not-executed")
        os.link(cache / "debug/deps/object.o", cache / "debug/consumer")
        (cache / "debug/consumer").chmod(0o755)
        (cache / "debug/.fingerprint/x").mkdir(parents=True)
        (cache / "debug/.fingerprint/x/output").write_bytes(b"full-fingerprint-content")
        (cache / "debug/build/x/out").mkdir(parents=True)
        (cache / "debug/build/x/out/generated.rs").write_bytes(b"full-generated-build-output")
        (cache / "debug/alias").symlink_to("consumer")
        return cache

    def test_full_cache_preservation_modes_links_payload_and_fd_retirement(self):
        cache = self.cache(); output = self.root / "preservation"
        result = retirement.retire_full_cache(cache, output, lambda: None, lambda resource: {"source": "synthetic-static-fixture"})
        self.assertEqual(result["status"], "verified_retired"); self.assertFalse(cache.exists())
        self.assertEqual(result["hash_only_cache_files"], 0)
        self.assertTrue(result["mode_ownership_content_link_topology_pass"])
        self.assertEqual(result["held_lock_count"], 1)
        manifest = f.load_json(output / "manifest.json")
        self.assertEqual(manifest["debug/consumer"]["hardlink_aliases"], ["debug/consumer", "debug/deps/object.o"])
        self.assertEqual(manifest["debug/alias"]["link"], "consumer")
        r = retirement.primitives()
        verification = r.verify_archive(output / "payloads.tar.gz", manifest, retirement.Resources(cache, output))
        self.assertEqual(verification["verified_members"], len(manifest))

    def test_external_hardlink_refuses_and_retains_original_cache(self):
        cache = self.cache(); os.link(cache / "debug/consumer", self.root / "outside-alias")
        with self.assertRaisesRegex(RuntimeError, "outside hardlink"):
            retirement.retire_full_cache(cache, self.root / "preservation", lambda: None, lambda r: {"synthetic": True})
        self.assertTrue(cache.exists()); self.assertTrue((self.root / "outside-alias").exists())

    def test_source_drift_after_archive_refuses_retirement(self):
        cache = self.cache(); calls = [0]
        def source(resources):
            calls[0] += 1; return {"synthetic_source_version": calls[0]}
        with self.assertRaisesRegex(ValueError, "source/prepared inputs changed"):
            retirement.retire_full_cache(cache, self.root / "preservation", lambda: None, source)
        self.assertTrue(cache.exists()); self.assertTrue((self.root / "preservation/payloads.tar.gz").exists())

    def test_prune_inode_replacement_refuses_unowned_file(self):
        r = retirement.primitives(); cache = self.cache(); entries = r.scan(cache)
        victim = cache / "debug/.cargo-lock"; victim.unlink(); victim.write_bytes(b"replacement")
        with self.assertRaisesRegex(RuntimeError, "entry changed"):
            r.remove_qualified(cache, entries)
        self.assertEqual(victim.read_bytes(), b"replacement")

    def test_preservation_reserve_failure_leaves_cache_without_archive(self):
        cache = self.cache()
        sample = type("VFS", (), {"f_bavail": f.FLOOR - 1, "f_frsize": 1})()
        with mock.patch.object(os, "statvfs", return_value=sample), self.assertRaisesRegex(ValueError, "floor"):
            retirement.retire_full_cache(cache, self.root / "preservation", lambda: None, lambda r: {})
        self.assertTrue(cache.exists()); self.assertFalse((self.root / "preservation/payloads.tar.gz").exists())

    def test_streaming_archive_guard_failure_preserves_cache(self):
        cache = self.cache(); original = retirement.Resources.check
        def guarded(instance, phase, write_bytes=0, force=False):
            if phase == "archive_write_before": raise ValueError("synthetic in-stream reserve failure")
            return original(instance, phase, write_bytes, force)
        with mock.patch.object(retirement.Resources, "check", guarded), self.assertRaisesRegex(ValueError, "in-stream"):
            retirement.retire_full_cache(cache, self.root / "preservation", lambda: None, lambda r: {"synthetic": True})
        self.assertTrue(cache.exists()); self.assertTrue((cache / "debug/build/x/out/generated.rs").exists())

    def test_verified_archive_corruption_is_rejected(self):
        cache = self.cache(); output = self.root / "preservation"
        retirement.retire_full_cache(cache, output, lambda: None, lambda r: {"synthetic": True})
        archive = output / "payloads.tar.gz"; data = bytearray(archive.read_bytes()); data[-8] ^= 1; archive.write_bytes(data)
        with self.assertRaises((OSError, RuntimeError)):
            retirement.primitives().verify_archive(archive, f.load_json(output / "manifest.json"), retirement.Resources(cache, output))

    def test_source_sha_and_path_set_corruption_refuse(self):
        root = self.root / "source"; root.mkdir(); (root / "input.rs").write_bytes(b"source")
        expected = {"input.rs": f.sha(root / "input.rs")}
        f.source_pins(root, expected)
        (root / "input.rs").write_bytes(b"corrupted")
        with self.assertRaisesRegex(ValueError, "pin changed"): f.source_pins(root, expected)
        (root / "extra.rs").write_bytes(b"extra")
        with self.assertRaisesRegex(ValueError, "file set changed"): f.source_pins(root, expected)

    def synthetic_run(self):
        """Build retained bytes only: fake ELF is never executed and every tool path is inert."""
        run = self.root / "run"; consumer = run / "cases/small/consumer"; cache = consumer.parent / "target-r0"
        for path in (consumer / "proto", consumer / "generated", consumer / "src", cache / "release"):
            path.mkdir(parents=True, exist_ok=True)
        runtime = self.root / "fixed-runtime"; runtime.mkdir()
        manifest = '[package]\nname="cg19-consumer-small"\nversion="0.0.0"\nedition="2021"\n[workspace]\n[dependencies]\npbrs={path=' + json.dumps(str(runtime)) + '}\n[profile.release]\nopt-level=3\nlto="thin"\ncodegen-units=1\n'
        (consumer / "Cargo.toml").write_text(manifest)
        (consumer / "src/main.rs").write_bytes(b"synthetic marker1 consumer, never compiled")
        inputs = {}
        for name in ("part_00.proto", "part_01.proto"):
            path = consumer / "proto" / name; path.write_bytes(b"synthetic exact proto " + name.encode())
            inputs["work/source-volume/small/proto/" + name] = {"bytes": path.stat().st_size, "sha256": f.sha(path), "path": str(path)}
        fds = self.root / "fixture.fds"; fds.write_bytes(b"synthetic canonical FDS")
        inputs["work/source-volume/small/fixture.fds"] = {"bytes": fds.stat().st_size, "sha256": f.sha(fds), "path": str(fds)}
        (consumer / "generated/output.rs").write_bytes(b"synthetic exact generated output")
        oracle = {"status": "passed", "files": f.inventory(consumer / "generated"), "canonical_fds_sha256": f.sha(fds)}
        f.write_json(run / "generated-oracle.json", oracle)
        binary = cache / "release/cg19-consumer-small"; binary.write_bytes(b"\x7fELFsynthetic-not-executed")
        driver = self.root / "driver"; driver.write_bytes(b"\x7fELFsynthetic-driver-not-executed")
        artifact = {"path": str(driver), "sha256": f.sha(driver)}
        tools = {k: {"path": "/inert-fixture/" + k} for k in ("cargo", "rustc", "protoc")}
        tool_record = {"status": "qualified", "closure_limits": "SYNTHETIC fixture, never a real BUILD proof", "tools": tools}
        f.write_json(run / "tool-envelope.json", tool_record)
        f.write_json(run / "root-lease.json", {"synthetic_fixture_only": True})
        prepared_sha = "synthetic-prepared-pin"
        build = {"status": "passed", "source": f.SOURCES["base"], "artifact": artifact,
                 "prepared_sha256": prepared_sha, "tools_sha256": f.sha(run / "tool-envelope.json")}
        f.write_json(run / "driver-BUILD.json", build)
        cargo = tools["cargo"]["path"]; protoc = tools["protoc"]["path"]
        args = ["--offline", "--locked", "--manifest-path", str(consumer / "Cargo.toml"), "--target-dir", str(cache), "--bin", "cg19-consumer-small"]
        gen = [str(driver), str(consumer / "proto"), str(consumer / "generated"), protoc, "part_00.proto", "part_01.proto"]
        commands = {"consumer_lock": [cargo, "generate-lockfile", "--offline", "--manifest-path", str(consumer / "Cargo.toml")],
                    "generation": gen, "generation_unchanged": gen, "check_clean": [cargo, "check", *args],
                    "check_incremental": [cargo, "check", *args], "build_release": [cargo, "build", "--release", *args], "release_smoke": [str(binary)]}
        phases, raw, owners = {}, {}, []
        for i, name in enumerate(f.ALL_PHASES):
            logs = {}
            for key, suffix in (("stdout_log", "stdout"), ("stderr_log", "stderr")):
                rel = "raw/" + name + "." + suffix; path = run / rel; path.parent.mkdir(exist_ok=True)
                path.write_bytes(b"1\n" if name == "release_smoke" and suffix == "stdout" else b"")
                raw[rel] = {"bytes": path.stat().st_size, "sha256": f.sha(path)}; logs[key] = rel
            phases[name] = {"command": commands[name], "status": "passed", "exit_code": 0, "elapsed_ns": i + 1,
                            "peak_rss_bytes": i + 10, **logs}
            if name == "release_smoke": phases[name]["output_verified"] = True
            env = {"CARGO_BUILD_JOBS": "1", "CARGO_NET_OFFLINE": "true", "RUSTC": tools["rustc"]["path"], "PROTOC": protoc}
            if name in ("check_clean", "check_incremental", "build_release"): env["CARGO_INCREMENTAL"] = "0" if name == "build_release" else "1"
            owners.append({"phase": name, "argv": commands[name] if name == "release_smoke" else ["/usr/bin/time", "-v", *commands[name]],
                           "exit_code": 0, "pid": 900000 + i, "pgid": 900000 + i, "sid": 900000 + i,
                           "finished_utc": "2026-01-01T00:00:01+00:00", "environment": env,
                           "cwd": str(consumer) if name == "release_smoke" else str(runtime)})
        f.write_json(run / "processes.json", {"owners": owners})
        f.write_json(run / "resource.json", {"failures": [], "cap_bytes": f.CAP, "floor_bytes": f.FLOOR,
                                             "samples": [{"allocated_cache_bytes": 4096, "global_free_bytes": f.FLOOR * 2}]})
        cell = {"case": "small", "repeat": 0, "generator": "pbrs", "profile": {"resolved": {"emit_reflection": True,
                "emit_json": True, "emit_text": True, "shared_descriptor_set": False, "runtime_profile": "default", "stubs": "none"}},
                "phases": phases, "output": {"rust_bytes": sum(v["bytes"] for v in oracle["files"].values()), "unchanged_generation_mtimes_preserved": True},
                "release_binary": {"path": binary.relative_to(run).as_posix(), "size_bytes": binary.stat().st_size, "sha256": f.sha(binary)}}
        f.write_json(run / "summary.json", {"status": "unqualified", "qualification": {"qualified": False}, "cells": [cell]})
        record = {"schema": "gn03-factor-RUN/1", "status": "passed_local_unqualified", "qualified": False, "factor": "bdef",
                  "repeat": 0, "case": "small", "source_pair": f.SOURCES, "generator_source": f.SOURCES["base"], "consumer_runtime_source": f.SOURCES["base"],
                  "prepared_sha256": prepared_sha, "harness_sha256": f.HARNESS_SHA, "profile": "default", "jobs": 1,
                  "seed": 190019, "timeout_seconds": 900, "rss_sample_ms": 100, "cold_target_initially_absent": True,
                  "run_dir": str(run), "target": str(cache), "driver_artifact": artifact, "raw_outputs": raw, "metrics": a.metric_values(cell),
                  "started_utc": "2026-01-01T00:00:00+00:00", "finished_utc": "2026-01-01T00:00:02+00:00"}
        for name, key in (("tool-envelope.json", "tools_sha256"), ("driver-BUILD.json", "driver_BUILD_sha256"), ("root-lease.json", "lease_sha256"),
                          ("summary.json", "summary_sha256"), ("generated-oracle.json", "oracle_sha256"), ("processes.json", "processes_sha256"), ("resource.json", "resource_sha256")):
            record[key] = f.sha(run / name)
        f.write_json(run / "RUN.json", record)
        prepared = {"consumer_ROOT": str(runtime), "inputs": inputs, "consumer_main_sha256": {"small": {"1": f.sha(consumer / "src/main.rs")}},
                    "output_pins": {"small": {"bdef": oracle["files"]}}}
        return run, prepared, {"cell": "bdef", "repeat": 0, "case": "small"}, prepared_sha

    def test_complete_synthetic_retained_run_and_raw_corruption_refusal(self):
        run, prepared, expected, pin = self.synthetic_run()
        self.assertEqual(a.audit_run(run, prepared, expected, pin)["retention"]["status"], "awaiting_verified_retirement")
        path = run / "raw/generation.stderr"; path.write_bytes(b"corrupted raw")
        with self.assertRaisesRegex(ValueError, "raw log bytes changed"): a.audit_run(run, prepared, expected, pin)

    def test_resealed_semantic_phase_argv_metric_profile_and_exit_corruption_refuse(self):
        run, prepared, expected, pin = self.synthetic_run()
        original = f.load_json(run / "summary.json"); record = f.load_json(run / "RUN.json")
        changes = [("argv", lambda cell: cell["phases"]["check_clean"]["command"].append("--jobs=8")),
                   ("metric", lambda cell: cell["phases"]["generation"].update(elapsed_ns=999)),
                   ("profile", lambda cell: cell["profile"]["resolved"].update(shared_descriptor_set=True)),
                   ("coverage", lambda cell: cell["phases"].pop("check_clean")),
                   ("exit", lambda cell: cell["phases"]["build_release"].update(exit_code=1))]
        for label, change in changes:
            with self.subTest(label=label):
                altered = copy.deepcopy(original); change(altered["cells"][0]); f.write_json(run / "summary.json", altered)
                updated = copy.deepcopy(record); updated["summary_sha256"] = f.sha(run / "summary.json"); f.write_json(run / "RUN.json", updated)
                with self.assertRaises(ValueError): a.audit_run(run, prepared, expected, pin)

    def test_resealed_owner_compiler_override_and_group_corruption_refuse(self):
        run, prepared, expected, pin = self.synthetic_run()
        original = f.load_json(run / "processes.json"); record = f.load_json(run / "RUN.json")
        for label in ("argv", "group", "rustdocflags", "targetlinker"):
            with self.subTest(label=label):
                altered = copy.deepcopy(original); owner = altered["owners"][0]
                if label == "argv": owner["argv"].append("extra")
                if label == "group": owner["sid"] += 1
                if label == "rustdocflags": owner["environment"]["RUSTDOCFLAGS"] = "override"
                if label == "targetlinker": owner["environment"]["CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER"] = "/wrong/linker"
                f.write_json(run / "processes.json", altered)
                updated = copy.deepcopy(record); updated["processes_sha256"] = f.sha(run / "processes.json"); f.write_json(run / "RUN.json", updated)
                with self.assertRaises(ValueError): a.audit_run(run, prepared, expected, pin)

    def test_resealed_wrong_smoke_and_resource_bounds_refuse(self):
        run, prepared, expected, pin = self.synthetic_run()
        record = f.load_json(run / "RUN.json"); smoke = run / "raw/release_smoke.stdout"; smoke.write_bytes(b"2\n")
        record["raw_outputs"]["raw/release_smoke.stdout"] = {"bytes": 2, "sha256": f.sha(smoke)}
        f.write_json(run / "RUN.json", record)
        with self.assertRaisesRegex(ValueError, "smoke output changed"): a.audit_run(run, prepared, expected, pin)
        smoke.write_bytes(b"1\n"); record["raw_outputs"]["raw/release_smoke.stdout"]["sha256"] = f.sha(smoke)
        resource = f.load_json(run / "resource.json"); resource["samples"][0]["allocated_cache_bytes"] = f.CAP + 1
        f.write_json(run / "resource.json", resource); record["resource_sha256"] = f.sha(run / "resource.json"); f.write_json(run / "RUN.json", record)
        with self.assertRaisesRegex(ValueError, "resource bounds exceeded"): a.audit_run(run, prepared, expected, pin)

    def test_original_schedule_complete_unique_and_seeded(self):
        schedule = f.load_json(HERE / "schedule-proposal.json")
        for case in ("small", "100", "1000"):
            rows = c.schedule_for("repeated", case)
            self.assertEqual(len(rows), 20)
            self.assertEqual({(x["cell"], x["repeat"]) for x in rows}, {(cell, rep) for cell in f.FACTORS for rep in range(5)})
            self.assertEqual(len(c.schedule_for("screen", case)), 4)
        self.assertEqual(len(schedule["full"]), 60)

    def test_root_lease_requires_exact_helper_source_and_fixed_budget(self):
        prepared = self.root / "prepared.json"; f.write_json(prepared, {"synthetic": True})
        lease_path = self.root / "lease.json"
        args = SimpleNamespace(execute=True, root_lease=str(lease_path), prepared=str(prepared), operation="bootstrap",
                               campaign=str(self.root / "campaign"), version="base")
        lease = {"schema": "gn03-factor-root-lease/1", "authorized_by": "/root", "status": "granted",
                 "operation": "bootstrap", "campaign": args.campaign, "prepared_sha256": f.sha(prepared),
                 "coordinator_sha256": f.sha(HERE / "coordinate-gn03-factors.py"), "common_sha256": f.sha(HERE / "factor_common.py"),
                 "components_sha256": f.component_pins(), "schedule_sha256": f.sha(HERE / "schedule-proposal.json"),
                 "source_pair": f.SOURCES, "consumer_runtime_source": f.SOURCES["base"], "jobs": 1,
                 "cache_cap_bytes": f.CAP, "free_floor_bytes": f.FLOOR, "version": "base", "kind": "ordinary_bootstrap"}
        f.write_json(lease_path, lease); self.assertEqual(c.verify_lease(args, {})["jobs"], 1)
        for key, value in (("status", "pending"), ("components_sha256", {}), ("jobs", 8), ("free_floor_bytes", f.FLOOR - 1)):
            altered = {**lease, key: value}; f.write_json(lease_path, altered)
            with self.assertRaises(ValueError): c.verify_lease(args, {})

    def test_contrasts_preserve_each_factor_and_losses(self):
        rows = [{"factor": factor, "metrics": {"source": value, "check": value}} for factor, value in
                (("bdef", 100), ("cdef", 101), ("bshr", 80), ("cshr", 60))]
        contrasts = a.derive_contrasts(rows)
        defaults = [x for x in contrasts["contrasts"] if x["contrast"] == "candidate_at_default"]
        self.assertTrue(all(x["loses"] for x in defaults))
        self.assertFalse(any(x["contrast"] == "candidate_shared_vs_baseline_default" for x in contrasts["contrasts"]))
        self.assertAlmostEqual(contrasts["interaction_descriptive_only"]["source"], (60 / 80) / (101 / 100) - 1)
        with self.assertRaisesRegex(ValueError, "coverage incomplete"): a.derive_contrasts(rows[:-1])

    def test_original_metrics_reject_bool_missing_and_negative(self):
        cell = {"output": {"rust_bytes": 1}, "release_binary": {"size_bytes": 4},
                "phases": {phase: {"elapsed_ns": 1, "peak_rss_bytes": 1} for phase in f.PHASES}}
        self.assertEqual(len(a.metric_values(cell)), 12)
        for bad in (True, -1, 0, None):
            broken = copy.deepcopy(cell); broken["phases"]["generation"]["elapsed_ns"] = bad
            with self.assertRaises(ValueError): a.metric_values(broken)

    def test_source_only_lock_subset_keeps_exact_tuples_and_driver_graph(self):
        text = 'version = 4\n\n[[package]]\nname="pbrs"\nversion="0.2.0"\ndependencies=["bytes"]\n\n[[package]]\nname="bytes"\nversion="1.0.0"\nsource="registry+fixture"\nchecksum="frozen"\n\n[[package]]\nname="unreachable"\nversion="9.0.0"\n'
        lock, tuples = prep.lock_subset(text, "cg19-generator")
        self.assertEqual(tuples, [("bytes", "1.0.0", "registry+fixture", "frozen")])
        self.assertIn('name = "cg19-generator"', lock); self.assertNotIn('name="unreachable"', lock)


if __name__ == "__main__":
    unittest.main()
