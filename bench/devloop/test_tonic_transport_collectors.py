#!/usr/bin/env python3
"""Synthetic collector regressions; these execute no RPC/performance workload."""

import gzip
import copy
import functools
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, HERE / filename)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


DRIVER = module("sb32_driver", "measure-tonic-transport.py")
AUDIT = module("sb32_audit", "check-tonic-transport-evidence.py")


class CollectorTests(unittest.TestCase):
    def inventory_fixture(self, current):
        root = HERE.parents[1]
        path = DRIVER.QUALIFIED_SPECIMEN_INVENTORY if current else DRIVER.LEGACY_SPECIMEN_INVENTORY
        frozen = json.loads((root / path).read_bytes())
        template = json.loads((root / "docs/evidence/sb-32/preflight-inventory.json").read_bytes())["preflight_rows"][0]
        ids, blocked, rows = [], [], []
        for profile in DRIVER.PROFILES:
            for specimen in frozen:
                for shape in DRIVER.SHAPES:
                    cell = f"{DRIVER.PREFIX}{profile}.{specimen['specimen']}.{shape}"
                    ids.append(cell)
                    q = specimen["qualification"]
                    if not q["equal_wire_work"]:
                        blocked.append(cell)
                        continue
                    row = {**copy.deepcopy(template), **{
                        key: q[key] for key in ("request_bytes", "native_request_bytes", "response_bytes_per_message",
                                               "request_read_checksum", "response_read_checksum")}}
                    row.update(id=cell, responses=4 if shape == "server_stream" else 1)
                    if current:
                        row.update(input_wire_bytes=q["request_bytes"], input_fingerprint_scope=DRIVER.INPUT_SCOPE)
                    rows.append(row)
        inventory = {"registered_ids": ids, "blocked_ids": blocked, "preflight_rows": rows,
                     "registered_count": len(ids), "actual_network_passed": len(rows), "blocked_count": len(blocked)}
        if current:
            inventory["specimen_inventory_path"] = path
        return inventory, frozen

    def test_inventory_derives_legacy_and_current_coverage_and_rejects_missing_work(self):
        for current, counts in ((False, (488, 24)), (True, (512, 0))):
            inventory, frozen = self.inventory_fixture(current)
            for check in (DRIVER.validate_inventory, AUDIT.inventory_cells):
                ids, oracle, blocked = check(inventory, frozen)
                self.assertEqual((len(ids), len(oracle), len(blocked)), (512, *counts))
                wrong_type = copy.deepcopy(frozen)
                wrong_type[0]["qualification"]["equal_wire_work"] = "false"
                with self.assertRaises(AssertionError): check(inventory, wrong_type)
                wrong_count = copy.deepcopy(inventory)
                wrong_count["blocked_count"] = False
                with self.assertRaises(AssertionError): check(wrong_count, frozen)
                for mutation in ("registered", "duplicate", "qualified", "blocked", "bytes"):
                    changed = copy.deepcopy(inventory)
                    if mutation == "registered": changed["registered_ids"].pop()
                    elif mutation == "duplicate": changed["preflight_rows"].append(changed["preflight_rows"][0])
                    elif mutation == "qualified": changed["preflight_rows"].pop()
                    elif mutation == "blocked": changed["blocked_ids"].append(changed["registered_ids"][0])
                    else: changed["preflight_rows"][0]["request_bytes"] += 1
                    with self.assertRaises(AssertionError, msg=f"{current}/{mutation}"):
                        check(changed, frozen)

    def test_new_capture_requires_length_while_historical_network_rows_remain_readable(self):
        old = json.loads((HERE.parents[1] / "docs/evidence/sb-32/preflight-inventory.json").read_bytes())["preflight_rows"][0]
        AUDIT.network(old, old)
        with self.assertRaises(AssertionError): DRIVER.validate_network(old, old)
        row = {**old, "input_wire_bytes": old["request_bytes"], "input_fingerprint_scope": DRIVER.INPUT_SCOPE}
        DRIVER.validate_network(row, old)
        AUDIT.network(row, old, require_length=True)
        for length in (None, -1, True, row["request_bytes"] + 1):
            changed = dict(row)
            if length is None: changed.pop("input_wire_bytes")
            else: changed["input_wire_bytes"] = length
            with self.assertRaises(AssertionError): DRIVER.validate_network(changed, old)
            with self.assertRaises(AssertionError): AUDIT.network(changed, old, require_length=True)

    def provenance_fixture(self, directory):
        root, archive = directory / "checkout", directory / "archive"
        root.mkdir()
        archive.mkdir()
        required = {"Cargo.toml", "vendor/google/conformance_fds.bin",
                    "bench/corpora/otlp/protos/trace.proto", "support.py"}
        contents = {path: ("source:" + path).encode() for path in required | {"outside/extra.rs"}}
        contents.update({path: ("schema:" + path).encode() for path in DRIVER.SCHEMAS})
        contents.update({"outside/a/extra.proto": b"first schema", "outside/b/extra.proto": b"second schema"})
        for path, data in contents.items():
            destination = root / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        binary = root / "devloop"
        binary.write_bytes(b"synthetic pinned release binary")
        sources = {path: DRIVER.digest(contents[path]) for path in required}
        build_sources = {path: checksum for path, checksum in sources.items() if not path.endswith(".py")}
        build_sources["outside/extra.rs"] = DRIVER.digest(contents["outside/extra.rs"])
        schemas = {path: DRIVER.digest(data) for path, data in contents.items() if path.endswith(".proto") and path not in required}
        build = {"binary_sha256": DRIVER.digest(binary.read_bytes()), "profile": "release",
                 "build_argv": ["synthetic-build"], "features": [], "tools": {"synthetic": True},
                 "source_sha256": build_sources, "schema_sha256": schemas}
        record = {"source_commit": "synthetic-commit", "source_sha256": {**sources, **build_sources},
                  "schema_sha256": dict(schemas)}
        return root, archive, binary, contents, required, sources, build, record

    def test_build_sidecar_requires_otlp_fds_and_all_fourteen_schemas(self):
        self.assertEqual(DRIVER.SCHEMAS, AUDIT.SCHEMAS)
        self.assertEqual(len(DRIVER.SCHEMAS), 14)
        self.assertEqual(DRIVER.SOURCE_ROOTS, AUDIT.SOURCE_ROOTS)
        self.assertIn("bench/corpora/otlp/protos", DRIVER.SOURCE_ROOTS)
        self.assertIn("vendor/google/conformance_fds.bin", DRIVER.SOURCE_ROOTS)
        with tempfile.TemporaryDirectory() as temporary:
            root, _, binary, contents, _, sources, build, _ = self.provenance_fixture(Path(temporary))
            with patch.object(DRIVER, "ROOT", root), patch.object(DRIVER, "source_hashes", return_value=sources), \
                    patch.object(DRIVER, "git", side_effect=lambda *argv: contents[argv[1].split(":", 1)[1]]):
                for path in ("vendor/google/conformance_fds.bin", "bench/corpora/otlp/protos/trace.proto"):
                    checksum = build["source_sha256"].pop(path)
                    with self.assertRaises((AssertionError, KeyError)):
                        DRIVER.compiled_inputs(build, "synthetic-commit", binary)
                    build["source_sha256"][path] = checksum
                for path in DRIVER.SCHEMAS:
                    checksum = build["schema_sha256"].pop(path)
                    with self.assertRaises(AssertionError):
                        DRIVER.compiled_inputs(build, "synthetic-commit", binary)
                    build["schema_sha256"][path] = checksum

    def test_build_sidecar_checks_extra_inputs_and_rechecks_source_drift(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, _, binary, contents, _, sources, build, record = self.provenance_fixture(Path(temporary))
            with patch.object(DRIVER, "ROOT", root), patch.object(DRIVER, "source_hashes", return_value=sources), \
                    patch.object(DRIVER, "git", side_effect=lambda *argv: contents[argv[1].split(":", 1)[1]]):
                checked, schemas = DRIVER.compiled_inputs(build, "synthetic-commit", binary)
                self.assertEqual(checked, record["source_sha256"])
                self.assertEqual(set(schemas), set(build["schema_sha256"]))
                self.assertEqual(DRIVER.live_source_hashes(build), checked)
                extra = root / "outside/extra.rs"
                extra.write_bytes(b"changed compiled input")
                self.assertNotEqual(DRIVER.live_source_hashes(build), checked)
                with self.assertRaises(AssertionError):
                    DRIVER.compiled_inputs(build, "synthetic-commit", binary)
                build["source_sha256"]["outside/extra.rs"] = DRIVER.digest(extra.read_bytes())
                with self.assertRaises(AssertionError):
                    DRIVER.compiled_inputs(build, "synthetic-commit", binary)
                extra.write_bytes(contents["outside/extra.rs"])
                build["source_sha256"]["outside/extra.rs"] = DRIVER.digest(extra.read_bytes())
                (root / "outside/a/extra.proto").write_bytes(b"changed extra schema")
                with self.assertRaises(AssertionError):
                    DRIVER.compiled_inputs(build, "synthetic-commit", binary)

    def test_independent_provenance_rejects_omitted_extra_pins_and_schema_corruption(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, archive, _, contents, required, _, build, record = self.provenance_fixture(Path(temporary))
            with patch.object(DRIVER, "ROOT", root):
                DRIVER.archive_schemas(archive, {path: contents[path] for path in build["schema_sha256"]})
            with patch.object(AUDIT, "source_paths", return_value=required):
                AUDIT.compiled_provenance(archive, record, build)
                for collection, path in (("source_sha256", "outside/extra.rs"), ("schema_sha256", "outside/a/extra.proto")):
                    checksum = record[collection].pop(path)
                    with self.assertRaises(AssertionError):
                        AUDIT.compiled_provenance(archive, record, build)
                    record[collection][path] = checksum
                for collection, path in (("source_sha256", "bench/corpora/otlp/protos/trace.proto"),
                                         ("schema_sha256", DRIVER.SCHEMAS[-1])):
                    checksum = build[collection].pop(path)
                    with self.assertRaises(AssertionError):
                        AUDIT.compiled_provenance(archive, record, build)
                    build[collection][path] = checksum
                (archive / "schemas/outside/a/extra.proto").write_bytes(b"tampered archived input")
                with self.assertRaises(AssertionError):
                    AUDIT.compiled_provenance(archive, record, build)

    def test_schema_archive_preserves_same_basename_inputs_and_rejects_escaping_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, archive, _, contents, _, _, build, _ = self.provenance_fixture(Path(temporary))
            with patch.object(DRIVER, "ROOT", root):
                DRIVER.archive_schemas(archive, {path: contents[path] for path in build["schema_sha256"]})
            self.assertEqual((archive / "schemas/outside/a/extra.proto").read_bytes(), b"first schema")
            self.assertEqual((archive / "schemas/outside/b/extra.proto").read_bytes(), b"second schema")
            for path in ("../escape.proto", "/absolute.proto", "a/../escape.proto", "a//b.proto"):
                for checked in (DRIVER.relative_input, AUDIT.relative_input):
                    with self.assertRaises(AssertionError):
                        checked(path)

    def test_capture_retains_nonzero_exit_and_byte_exact_streams_before_parse(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "failure"
            record, stdout, stderr = DRIVER.capture(
                [sys.executable, "-c", "import sys;sys.stdout.buffer.write(b'raw\\x00\\xff');sys.stderr.buffer.write(b'summary\\xff');sys.exit(7)"],
                destination,
            )
            self.assertEqual(record["exit_code"], 7)
            self.assertEqual(stdout, b"raw\x00\xff")
            self.assertEqual(stderr, b"summary\xff")
            restored = json.loads((destination / "process.json").read_bytes())
            self.assertEqual(restored, record)
            self.assertEqual((destination / "stdout").read_bytes(), stdout)
            self.assertEqual((destination / "stderr").read_bytes(), stderr)

    def test_unspawnable_tool_has_an_explicit_disposition_without_an_invented_process_exit(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "not-spawned"
            record, stdout, stderr = DRIVER.capture(["/nonexistent/sb32-tool"], destination)
            self.assertIsNone(record["exit_code"])
            self.assertEqual(record["spawn_error"]["type"], "FileNotFoundError")
            self.assertEqual(stdout, b"")
            self.assertEqual(stderr, b"")
            self.assertEqual(json.loads((destination / "process.json").read_bytes()), record)

    def test_tool_wrapper_keeps_strace_failures_and_callgrind_summary_files(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            real = base / "real-tool.py"
            real.write_text(
                "#!/usr/bin/env python3\nimport pathlib,sys\n"
                "for arg in sys.argv[1:]:\n"
                " if arg.startswith('--callgrind-out-file='):\n"
                "  pathlib.Path(arg.split('=',1)[1].replace('%p','123')).write_text('events: Ir\\nsummary: 123\\ntotals: 123\\n')\n"
                "sys.stdout.buffer.write(b'raw-stdout\\xff')\n"
                "sys.stderr.buffer.write(b'==123== I   refs: 123\\n100.00 0.001 1 42 2 total\\n')\n"
                "sys.exit(5 if '-c' in sys.argv else 0)\n"
            )
            real.chmod(0o755)
            for tool, arguments, expected in (
                ("strace", ["-c", "-f", "fake-child"], 5),
                ("valgrind", ["--tool=callgrind", "--cache-sim=no", "--callgrind-out-file=/tmp/devloop-callgrind.%p", "fake-child"], 0),
            ):
                wrapper = base / tool
                wrapper.symlink_to(HERE / "measure-tonic-transport.py")
                environment = dict(DRIVER.os.environ,
                                   PBRS_SB32_REAL_TOOLS=json.dumps({tool: str(real)}),
                                   PBRS_SB32_TOOL_RAW=str(base / "raw"))
                result = subprocess.run([str(wrapper), *arguments], env=environment, capture_output=True)
                self.assertEqual(result.returncode, expected)
                paths = list((base / "raw").glob(f"*-{tool}/process.json"))
                self.assertEqual(len(paths), 1)
                record = json.loads(paths[0].read_bytes())
                self.assertEqual(record["collector_argv"], [tool, *arguments])
                self.assertEqual((paths[0].parent / "stdout").read_bytes(), result.stdout)
                self.assertEqual((paths[0].parent / "stderr").read_bytes(), result.stderr)
                if tool == "valgrind":
                    self.assertEqual(len(record["callgrind_files"]), 1)
                    archive = paths[0].parent / record["callgrind_files"][0]["path"]
                    self.assertEqual(gzip.decompress(archive.read_bytes()), b"events: Ir\nsummary: 123\ntotals: 123\n")

    def fixture(self, directory, missing_counter=False, failed_trace=False):
        cell = "rpc.tonic_transport.reference_reference.query.d3.v0.unary"
        row = json.loads((HERE.parents[1] / "docs/evidence/sb-32/preflight-inventory.json").read_bytes())["preflight_rows"][0]
        self.assertEqual(row["id"], cell)
        binary, tools = "/fake/pinned-devloop", {"valgrind": "/fake/valgrind", "strace": "/fake/strace"}
        hashes = {}
        ordinal = 0
        for repeat in range(3):
            for tool, count in (("valgrind", 16), ("valgrind", 32), ("strace", 16)):
                if missing_counter and count == 32:
                    continue
                path = directory / f"raw/{ordinal:02}-{tool}"
                path.mkdir(parents=True)
                ordinal += 1
                child = {"cell": cell, "iters": count, "allocs": count * 2,
                         "alloc_bytes": count * 20, "wall_ns": count * 200,
                         "input_wire_fingerprint": row["input_wire_fingerprint"], "copy_counts": None}
                stderr = "__QUALIFICATION__ " + json.dumps(row) + "\n__CHILD__ " + json.dumps(child) + "\n"
                if tool == "valgrind":
                    instructions = 1000 + count * 10
                    if not missing_counter:
                        stderr += f"==1== I   refs: {instructions}\n"
                    argv = [tool, "--tool=callgrind", "--cache-sim=no", "--callgrind-out-file=/tmp/devloop-callgrind.%p", binary, "run-cell", cell, "--iters", str(count), "--prepare-iters", "32"]
                    content = f"events: Ir\nsummary: {instructions}\ntotals: {instructions}\n".encode()
                    compressed = gzip.compress(content, mtime=0)
                    (path / "callgrind.1.gz").write_bytes(compressed)
                    artifacts = [{"path": "callgrind.1.gz", "sha256": AUDIT.sha(compressed), "uncompressed_sha256": AUDIT.sha(content)}]
                else:
                    stderr += "1.00 0.001 1 32 futex\n100.00 0.001 1 320 2 total\n"
                    argv = [tool, "-c", "-f", binary, "run-cell", cell, "--iters", "16"]
                    artifacts = []
                stdout, stderr = b"", stderr.encode()
                (path / "stdout").write_bytes(stdout)
                (path / "stderr").write_bytes(stderr)
                record = {"argv": [tools[tool], *argv[1:]], "collector_argv": argv,
                          "tool": tool, "exit_code": 4 if failed_trace and tool == "strace" else 0,
                          "interrupted": False, "stdout_sha256": AUDIT.sha(stdout),
                          "stderr_sha256": AUDIT.sha(stderr), "callgrind_files": artifacts}
                (path / "process.json").write_text(json.dumps(record))
                hashes[str((path / "process.json").relative_to(directory))] = AUDIT.sha((path / "process.json").read_bytes())
        def measured(value):
            return {"status": "measured", "data": {"value": value, "unit": "synthetic"}}
        def absent():
            return {"status": "not_run", "data": {"reason": "synthetic unavailable tool output"}}
        result = {"id": cell, "kind": "rpc", "codec": "reference_reference", "iters": 16, "repeats": 3,
                  "input_wire_fingerprint": row["input_wire_fingerprint"],
                  "instructions": absent() if missing_counter else measured(10),
                  "instruction_method": "not_run" if missing_counter else "differential_callgrind_2n_minus_n",
                  "allocs": measured(2), "alloc_bytes": measured(20), "wall_ns": measured(200), "copy_counts": None,
                  "syscalls": absent() if failed_trace else measured(20), "locks": absent() if failed_trace else measured(2)}
        return {"raw_tool_records": hashes}, {"cells": [result]}, {cell: row}, tools, binary

    def reconstruct(self, directory, fixture):
        info, report, oracle, tools, binary = fixture
        AUDIT.reconstruct(directory, info, report, oracle, "valgrind", tools, binary)

    def test_independent_reconstruction_checks_allocations_deltas_and_whole_process_syscalls(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            fixture = self.fixture(directory)
            self.reconstruct(directory, fixture)
            fixture[1]["cells"][0]["alloc_bytes"]["data"]["value"] += 1
            with self.assertRaises(AssertionError):
                self.reconstruct(directory, fixture)

    def test_missing_instruction_counter_and_failed_strace_remain_not_run(self):
        for missing_counter, failed_trace in ((True, False), (False, True)):
            with tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                fixture = self.fixture(directory, missing_counter, failed_trace)
                self.reconstruct(directory, fixture)

    def test_missing_strace_raw_cannot_hide_behind_a_reported_median(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            fixture = self.fixture(directory)
            traces = [p for p in fixture[0]["raw_tool_records"] if "strace" in p]
            del fixture[0]["raw_tool_records"][traces[0]]
            with self.assertRaises(AssertionError):
                self.reconstruct(directory, fixture)

    def test_rehashed_prepared_lengths_are_checked_in_raw_children_and_aggregates(self):
        for changed in ("child", "network", "aggregate"):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                fixture = self.fixture(directory)
                cell, oracle = next(iter(fixture[2].items()))
                length = oracle["request_bytes"]
                oracle.update(input_wire_bytes=length, input_fingerprint_scope=AUDIT.INPUT_SCOPE)
                fixture[1]["cells"][0]["input_wire_bytes"] = length
                for key in fixture[0]["raw_tool_records"]:
                    path = directory / key
                    process = json.loads(path.read_bytes())
                    lines = []
                    for line in (path.parent / "stderr").read_text().splitlines():
                        prefix = next((p for p in ("__CHILD__ ", "__QUALIFICATION__ ") if line.startswith(p)), None)
                        if prefix:
                            row = json.loads(line[len(prefix):])
                            row["input_wire_bytes"] = length
                            if prefix == "__QUALIFICATION__ ": row["input_fingerprint_scope"] = AUDIT.INPUT_SCOPE
                            line = prefix + json.dumps(row)
                        lines.append(line)
                    stderr = ("\n".join(lines) + "\n").encode()
                    (path.parent / "stderr").write_bytes(stderr)
                    process["stderr_sha256"] = AUDIT.sha(stderr)
                    path.write_text(json.dumps(process))
                    fixture[0]["raw_tool_records"][key] = AUDIT.sha(path.read_bytes())
                self.reconstruct(directory, fixture)
                if changed == "aggregate":
                    fixture[1]["cells"][0]["input_wire_bytes"] += 1
                else:
                    key = next(iter(fixture[0]["raw_tool_records"]))
                    path = directory / key
                    process = json.loads(path.read_bytes())
                    marker = "__CHILD__ " if changed == "child" else "__QUALIFICATION__ "
                    lines = (path.parent / "stderr").read_text().splitlines()
                    for index, line in enumerate(lines):
                        if line.startswith(marker):
                            row = json.loads(line[len(marker):])
                            row["input_wire_bytes"] += 1
                            lines[index] = marker + json.dumps(row)
                    stderr = ("\n".join(lines) + "\n").encode()
                    (path.parent / "stderr").write_bytes(stderr)
                    process["stderr_sha256"] = AUDIT.sha(stderr)
                    path.write_text(json.dumps(process))
                    fixture[0]["raw_tool_records"][key] = AUDIT.sha(path.read_bytes())
                with self.assertRaises(AssertionError): self.reconstruct(directory, fixture)

    def test_changed_fingerprint_and_rehashed_counter_summary_are_rejected(self):
        for mutation in ("fingerprint", "counter", "N2N-marker"):
            with tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                fixture = self.fixture(directory)
                key = next(iter(fixture[0]["raw_tool_records"]))
                path = directory / key
                process = json.loads(path.read_bytes())
                stderr = (path.parent / "stderr").read_bytes()
                if mutation == "fingerprint":
                    stderr = stderr.replace(fixture[2][next(iter(fixture[2]))]["input_wire_fingerprint"].encode(), b"fnv1a64:altered")
                elif mutation == "counter":
                    stderr = stderr.replace(b"I   refs: 1160", b"I   refs: 1161")
                else:
                    stderr = stderr.replace(b'"iters": 16', b'"iters": 32')
                (path.parent / "stderr").write_bytes(stderr)
                process["stderr_sha256"] = AUDIT.sha(stderr)
                path.write_text(json.dumps(process))
                fixture[0]["raw_tool_records"][key] = AUDIT.sha(path.read_bytes())
                with self.assertRaises(AssertionError):
                    self.reconstruct(directory, fixture)

    def complete_archive(self, directory, current=False):
        """Exercise full audit coverage/provenance apart from separately tested counters."""
        root = HERE.parents[1]
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
        inventory_path = "docs/evidence/sb-32/preflight-inventory.json"
        inventory_bytes = (root / inventory_path).read_bytes()
        inventory = json.loads(inventory_bytes)
        if current:
            inventory, _ = self.inventory_fixture(True)
            inventory_path = "synthetic/current-preflight-inventory.json"
            inventory_bytes = json.dumps(inventory).encode()
            (directory / "inventory.json").write_bytes(inventory_bytes)
        oracle = {r["id"]: r for r in inventory["preflight_rows"]}
        sources = {path: AUDIT.sha(AUDIT.git_file(commit, path)) for path in AUDIT.source_paths(commit)}
        build = {"source_sha256": {path: checksum for path, checksum in sources.items() if not path.endswith(".py")},
                 "binary_sha256": "synthetic", "profile": "release",
                 "build_argv": ["synthetic-build"], "features": [], "tools": {"synthetic": True}, "schema_sha256": {}}
        (directory / "schemas").mkdir()
        for schema in DRIVER.SCHEMAS:
            data = (root / schema).read_bytes()
            destination = directory / "schemas" / schema
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
            build["schema_sha256"][schema] = AUDIT.sha(data)
        build_bytes = json.dumps(build).encode()
        (directory / "release-build.json").write_bytes(build_bytes)
        record = {"schema": "pbrs-sb32-measurement/1", "status": "captured; independent audit pending",
                  "source_commit": commit, "source_sha256": sources, "binary": "/fake/absent-devloop",
                  "binary_sha256": "synthetic", "build_record_sha256": AUDIT.sha(build_bytes),
                  "schema_sha256": build["schema_sha256"],
                  "inventory_path": inventory_path, "inventory_sha256": AUDIT.sha(inventory_bytes),
                  "iters": 16, "double_iters": 32, "common_prepare_iters": 32, "repeats": 3,
                  "warmup": 100, "runtime_workers": 2, "report_parallelism": 1,
                  "tools": {"valgrind": "/fake/absent-valgrind"}, "tool_pins": {},
                  "processes": {}, "cells": {}, "reports": {}, "syscall_scope": AUDIT.SYSCALL_SCOPE}
        if current:
            record.update(specimen_inventory_path=DRIVER.QUALIFIED_SPECIMEN_INVENTORY,
                          specimen_inventory_sha256=AUDIT.sha((root / DRIVER.QUALIFIED_SPECIMEN_INVENTORY).read_bytes()),
                          input_wire_bytes_required=True)
        def process(relative, argv, stdout=b"", stderr=b"", exit_code=0, tool_records=None):
            path = directory / relative
            path.mkdir(parents=True)
            (path / "stdout").write_bytes(stdout)
            (path / "stderr").write_bytes(stderr)
            state = {"argv": argv, "exit_code": exit_code,
                     "stdout_sha256": AUDIT.sha(stdout), "stderr_sha256": AUDIT.sha(stderr)}
            (path / "process.json").write_text(json.dumps(state))
            if tool_records is not None:
                state["raw_tool_records"] = tool_records
            record["processes"][relative] = state
        process("raw/tool-version-valgrind", ["/fake/absent-valgrind", "--version"], stdout=b"synthetic-valgrind")
        record["tool_pins"]["valgrind"] = {"executable_sha256": "synthetic", "version_process": "raw/tool-version-valgrind"}
        frozen = json.loads((root / "bench/devloop/adoption/evidence/rpc-inventory.json").read_bytes())
        old = [f"rpc.adoption.{p}.{r['specimen']}.{s}" for p in ("native_pbrs", "native_prost", "tonic_pbrs", "tonic_prost") for r in frozen for s in AUDIT.SHAPES]
        process("raw/registry", [record["binary"], "list"], stdout=("\n".join([*inventory["registered_ids"], *old])).encode())
        for cell in inventory["registered_ids"]:
            relative = "raw/preflight/" + cell
            argv = [record["binary"], "run-cell", cell, "--iters", "1", "--prepare-iters", "32", "--warmup", "0"]
            if cell in oracle:
                child = {"cell": cell, "iters": 1, "input_wire_fingerprint": oracle[cell]["input_wire_fingerprint"]}
                if current:
                    child["input_wire_bytes"] = oracle[cell]["request_bytes"]
                stderr = ("__QUALIFICATION__ " + json.dumps(oracle[cell]) + "\n__CHILD__ " + json.dumps(child) + "\n").encode()
                process(relative, argv, stderr=stderr)
                record["cells"][cell] = {"status": "qualified", "measurement_status": "captured", "network": oracle[cell], "preflight_process": relative}
            else:
                process(relative, argv, stderr=b"RPC encoded bytes must agree", exit_code=101)
                record["cells"][cell] = {"status": "not_run", "measurement_status": "not_run", "preflight_process": relative}
        batches = [(f"baseline-replay-{r}-{s}", "reference_reference", s) for r in (1, 2) for s in AUDIT.SHAPES]
        batches += [(f"matrix-{p}-{s}", p, s) for p in AUDIT.PROFILES for s in AUDIT.SHAPES]
        for name, profile, shape in batches:
            selected = [i for i in oracle if i.startswith(f"{AUDIT.PREFIX}{profile}.") and i.endswith("." + shape)]
            def row(cell):
                value = {"id": cell, **{metric: {"status": "measured", "data": {"value": 1}} for metric in (*AUDIT.PRIMARY, "wall_ns")}}
                if current:
                    value.update(input_wire_bytes=oracle[cell]["request_bytes"],
                                 input_wire_fingerprint=oracle[cell]["input_wire_fingerprint"])
                return value
            report = {"schema": "devloop/1", "devloop_commit": commit, "cells": [row(c) for c in selected]}
            filename = name + ".json"
            content = json.dumps(report).encode()
            (directory / filename).write_bytes(content)
            relative = "raw/collector/" + name
            process(relative, [record["binary"], "run", "--cells", ",".join(selected), "--iters", "16", "--repeats", "3", "--out", str(directory / filename)], tool_records={})
            record["reports"][name] = {"path": filename, "sha256": AUDIT.sha(content), "collector_process": relative, "raw_tool_records": {}}
        (directory / "measurement.json").write_text(json.dumps(record))
        return record

    def test_current_full_archive_covers_all512_without_dropping_map_states(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            record = self.complete_archive(directory, current=True)
            actual_git_file = functools.lru_cache(maxsize=None)(AUDIT.git_file)
            def git_file(commit, path):
                if path == record["inventory_path"]:
                    return (directory / "inventory.json").read_bytes()
                return actual_git_file(commit, path)
            with patch.object(AUDIT, "git_file", side_effect=git_file), patch.object(AUDIT, "reconstruct"):
                result = AUDIT.audit(directory)
                self.assertEqual((result["registered"], result["network_qualified"], result["blocked"]), (512, 512, 0))
                self.assertEqual(len(result["comparisons"]), 384)
                record.pop("input_wire_bytes_required")
                (directory / "measurement.json").write_text(json.dumps(record))
                with self.assertRaises(AssertionError): AUDIT.audit(directory)

    def test_complete_inventory_and_provenance_audit_rejects_blocked_measurement_and_hidden_replay_loss(self):
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(AUDIT, "git_file", wraps=functools.lru_cache(maxsize=None)(AUDIT.git_file)):
            directory = Path(temporary)
            record = self.complete_archive(directory)
            # Counter reconstruction has independent raw fixtures above. Here
            # exercise the 512-state/source/build/report/replay/ratio envelope.
            with patch.object(AUDIT, "reconstruct"):
                result = AUDIT.audit(directory)
                self.assertEqual((result["registered"], result["network_qualified"], result["blocked"]), (512, 488, 24))
                self.assertEqual(result["original_baseline_replay_gate"], "passed")
                self.assertEqual(len(result["comparisons"]), 366)
                changed = directory / "baseline-replay-2-unary.json"
                report = json.loads(changed.read_bytes())
                report["cells"][0]["instructions"]["data"]["value"] = 1.03
                changed.write_text(json.dumps(report))
                record["reports"]["baseline-replay-2-unary"]["sha256"] = AUDIT.sha(changed.read_bytes())
                (directory / "measurement.json").write_text(json.dumps(record))
                result = AUDIT.audit(directory)
                self.assertEqual(result["original_baseline_replay_gate"], "failed")
                self.assertTrue(result["baseline_replay_failures"])
                for relative in ("raw/registry", "raw/preflight/" + next(iter(record["cells"]))):
                    process = record["processes"][relative]
                    original = process["argv"][0]
                    process["argv"][0] = "/fake/foreign-devloop"
                    (directory / relative / "process.json").write_text(json.dumps(process))
                    (directory / "measurement.json").write_text(json.dumps(record))
                    with self.assertRaises(AssertionError):
                        AUDIT.audit(directory)
                    process["argv"][0] = original
                    (directory / relative / "process.json").write_text(json.dumps(process))
                blocked = next(c for c in record["cells"] if ".maps." in c)
                record["cells"][blocked]["measurement_status"] = "captured"
                (directory / "measurement.json").write_text(json.dumps(record))
                with self.assertRaises(AssertionError):
                    AUDIT.audit(directory)


if __name__ == "__main__":
    unittest.main()
