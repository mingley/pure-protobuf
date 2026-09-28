#!/usr/bin/env python3
"""Inventory the official rust_out kernel ABI against pbrs (UK-01).

Generates `protoc --rust_out kernel=upb` output with the pinned v35.1
protoc and a current v36.x protoc for every upstream Rust test proto
plus the SB-05 corpora, compiles each output against pbrs as the
`protobuf` runtime crate, and records every `__internal::runtime` item
gencode names as implemented/stub/missing with use sites and the
compile-error count.

Outputs: docs/evidence/rust-out-abi.json, docs/evidence/rust-out-abi.md.
Scratch crates live under target/rust-out-abi/ (gitignored).

Usage:
    python3 scripts/rust-out-abi.py [--protoc35 P] [--protoc36 P] [--verify]
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
JSON_OUT = os.path.join(ROOT, "docs", "evidence", "rust-out-abi.json")
MD_OUT = os.path.join(ROOT, "docs", "evidence", "rust-out-abi.md")
SCRATCH = os.path.join(ROOT, "target", "rust-out-abi")
PB_ROOT = os.path.join(ROOT, "third_party", "protobuf")
PB_SRC = os.path.join(PB_ROOT, "src")

# Curated stubs: (path, reason). Pattern-detected stubs (unimplemented! /
# todo! / panic! / unreachable! in the body) are found automatically; these
# need human judgment (null/constant/behavioral stubs).
CURATED_STUBS = {
    "build_enum_mini_table": "returns null; enum tables unimplemented "
        "(link_mini_table also ignores _subenums)",
    "message_eq": "constant false",
    "debug_string": 'constant "<msg>"',
    "MiniTableExtensionPtr": "type alias only; no extension table/link support",
    "ExtensionRegistryPtr": "type alias only; no extension registry",
    "MessageViewInterop::__unstable_wrap_raw_message": "unimplemented! default",
    "MessageViewInterop::__unstable_wrap_raw_message_unchecked_lifetime":
        "unimplemented! default",
    "parse_into": "private kernel parser drops unknown fields via skip_field "
        "instead of retaining them (decode.rs)",
}

def locate_def(fname, pattern):
    lines = open(os.path.join(ROOT, "src", "runtime", fname),
                 encoding="utf-8").read().splitlines()
    for i, ln in enumerate(lines, 1):
        if re.search(pattern, ln):
            return [f"src/runtime/{fname}", i]
    return [f"src/runtime/{fname}", 0]


IMPORT_RE = re.compile(r'^\s*import\s+"([^"]+)"\s*;', re.M)
USE_RE = re.compile(r"protobuf::__internal::runtime::([A-Za-z_]\w*)"
                    r"(?:::([A-Za-z_]\w*))?")
TURBOFISH_RE = re.compile(r"::\s*<[^;(){}\"]*>")


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def protoc_version(path):
    r = run([path, "--version"], check=True)
    return r.stdout.strip()


def closure(roots, inc_dirs):
    """Transitive import closure; returns {import_path: abs_path}."""
    seen = {}
    stack = list(roots)
    while stack:
        path = stack.pop()
        if path in seen:
            continue
        for d in inc_dirs:
            cand = os.path.join(d, path)
            if os.path.isfile(cand):
                break
        else:
            raise SystemExit(f"proto not found: {path}")
        data = open(cand, encoding="utf-8").read()
        seen[path] = cand
        for imp in IMPORT_RE.findall(data):
            if imp not in seen:
                stack.append(imp)
    return seen


def collect_sets():
    """{set_name: (proto_roots, [include dirs])}"""
    sets = {}
    # Every upstream Rust test proto, including nested dirs.
    rust_tests = []
    for dirpath, _, files in os.walk(os.path.join(PB_ROOT, "rust", "test")):
        for f in sorted(files):
            if f.endswith(".proto"):
                full = os.path.join(dirpath, f)
                rust_tests.append(os.path.relpath(full, PB_ROOT))
    sets["rust-test"] = (sorted(rust_tests), [PB_ROOT, PB_SRC])
    # SB-05 corpora: files lists straight from the manifest.
    manifest = json.load(open(os.path.join(ROOT, "bench", "corpora", "manifest.json")))
    for name in ("otlp", "xds", "googleapis", "grpc-testing", "google-messages"):
        entry = manifest["corpora"][name]
        roots = sorted(entry["files"])
        inc = [os.path.join(ROOT, "bench", "corpora", name, "protos")]
        sets[name] = (roots, inc)
    return sets


def gen_one(protoc, workdir, import_path, abs_path, inc_dirs, mapping):
    """One protoc invocation for a single proto; returns (ok, stderr, outdir)."""
    outdir = os.path.join(workdir, "gen")
    os.makedirs(outdir, exist_ok=True)
    cmd = [protoc, import_path,
           *[f"--proto_path={d}" for d in inc_dirs],
           f"--rust_out={outdir}",
           "--rust_opt=experimental-codegen=enabled,kernel=upb,"
           f"crate_mapping={mapping}"]
    r = run(cmd)
    return r.returncode == 0, (r.stdout + r.stderr).strip(), outdir


def find_generated_rs(outdir):
    found = []
    for dirpath, _, files in os.walk(outdir):
        if "generated.rs" in files:
            found.append(os.path.join(dirpath, "generated.rs"))
    return sorted(found)


def build_crate(label, mods, crate_dir):
    """mods: [(mod_name, generated_rs_abs)]. Writes + cargo checks."""
    os.makedirs(os.path.join(crate_dir, "src"), exist_ok=True)
    open(os.path.join(crate_dir, "Cargo.toml"), "w").write(
        '[package]\nname = "abi-probe-%s"\nversion = "0.0.0"\n'
        'edition = "2021"\npublish = false\n\n[workspace]\n\n'
        '[dependencies]\npbrs = { path = "%s" }\n' % (label, ROOT))
    lib = ["extern crate pbrs as protobuf;", ""]
    for mod_name, gen_rs in mods:
        lib.append("#[allow(dead_code, unused, nonstandard_style)]")
        lib.append("#[allow(clippy::all, unreachable_pub, static_mut_refs)]")
        lib.append(f'#[path = "{gen_rs}"]')
        lib.append(f"pub mod {mod_name};")
    open(os.path.join(crate_dir, "src", "lib.rs"), "w").write("\n".join(lib) + "\n")
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(SCRATCH, "target"))
    r = run(["cargo", "check", "--offline", "--message-format=json"],
            cwd=crate_dir, env=env)
    if r.returncode != 0 and not r.stdout.strip():
        # No JSON at all: resolution failed offline, retry online.
        r = run(["cargo", "check", "--message-format=json"], cwd=crate_dir, env=env)
    errors = []
    for line in r.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") != "compiler-message":
            continue
        diag = msg.get("message", {})
        if diag.get("level") != "error":
            continue
        spans = diag.get("spans") or [{}]
        errors.append({
            "file": os.path.relpath(spans[0].get("file_name", "?"), ROOT)
            if spans[0].get("file_name", "").startswith(ROOT) else
            spans[0].get("file_name", "?"),
            "line": spans[0].get("line_start", 0),
            "code": (diag.get("code") or {}).get("code"),
            "message": diag.get("message", "").split("\n")[0][:300],
        })
    return errors


def extract_uses(rs_path):
    """{full_path: count} for runtime:: paths in one generated file."""
    src = open(rs_path, encoding="utf-8").read()
    src = TURBOFISH_RE.sub("", src)
    uses = {}
    for m in USE_RE.finditer(src):
        item, assoc = m.group(1), m.group(2)
        if item == "":
            continue
        full = f"{item}::{assoc}" if assoc else item
        uses[full] = uses.get(full, 0) + 1
    # use-group imports: runtime::{A, B::C}
    for m in re.finditer(r"runtime::\{([^}]*)\}", src):
        for part in m.group(1).split(","):
            part = part.strip()
            if re.fullmatch(r"[A-Za-z_]\w*(::[A-Za-z_]\w*)?", part):
                uses[part] = uses.get(part, 0) + 1
    return uses


def parse_surface():
    """pbrs runtime surface: {path: (kind, file, line)} + assoc map."""
    rtdir = os.path.join(ROOT, "src", "runtime")
    hub = open(os.path.join(ROOT, "src", "runtime.rs"), encoding="utf-8").read()
    items = {}   # "Name" -> (kind, file, line)
    assoc = {}   # "Type::member" -> (kind, file, line)
    for m in re.finditer(r"^pub use (\w+)::(\w+);", hub, re.M):
        mod, name = m.group(1), m.group(2)
        src = open(os.path.join(rtdir, mod + ".rs"), encoding="utf-8").read().splitlines()
        kind, line = "reexport", 0
        for i, ln in enumerate(src, 1):
            mm = re.match(rf"pub (?:unsafe )?(struct|enum|trait|type|mod|fn|const) {re.escape(name)}\b", ln)
            if mm:
                kind, line = mm.group(1), i
                break
        items[name] = (kind, f"src/runtime/{mod}.rs", line)
    # Inherent assoc fns/consts + enum variants + trait-default fns.
    for fname in sorted(os.listdir(rtdir)):
        if not fname.endswith(".rs"):
            continue
        lines = open(os.path.join(rtdir, fname), encoding="utf-8").read().splitlines()
        i = 0
        while i < len(lines):
            m = re.match(r"^impl(<[^;{]*>)?\s+(\w+)", lines[i])
            if m and " for " not in lines[i].split("{")[0]:
                typ = m.group(2)
                depth = 0
                j = i
                while j < len(lines):
                    depth += lines[j].count("{") - lines[j].count("}")
                    fm = re.match(r"\s+pub (?:const )?(?:unsafe )?fn (\w+)", lines[j])
                    cm = re.match(r"\s+pub const (\w+)", lines[j])
                    if fm:
                        assoc[f"{typ}::{fm.group(1)}"] = ("fn", f"src/runtime/{fname}", j + 1)
                    if cm:
                        assoc[f"{typ}::{cm.group(1)}"] = ("const", f"src/runtime/{fname}", j + 1)
                    j += 1
                    if depth <= 0 and j > i:
                        break
                i = j
                continue
            tm = re.match(r"^impl\b.*\bfor\s+(\w+)", lines[i])
            if tm:
                typ = tm.group(1)
                depth = 0
                j = i
                while j < len(lines):
                    fm = re.match(r"\s+(?:unsafe )?fn (\w+)", lines[j])
                    if fm and depth == 1:
                        assoc.setdefault(f"{typ}::{fm.group(1)}",
                                         ("trait-impl-fn", f"src/runtime/{fname}", j + 1))
                    depth += lines[j].count("{") - lines[j].count("}")
                    j += 1
                    if depth <= 0 and j > i:
                        break
                i = j
                continue
            mm = re.match(r"^pub mod (\w+)", lines[i])
            if mm:
                mod = mm.group(1)
                depth = 0
                j = i
                while j < len(lines):
                    im = re.match(r"\s+pub (struct|enum|trait|type|fn|const|mod) (\w+)",
                                  lines[j])
                    if im and depth == 1:
                        assoc[f"{mod}::{im.group(2)}"] = (
                            im.group(1), f"src/runtime/{fname}", j + 1)
                    depth += lines[j].count("{") - lines[j].count("}")
                    j += 1
                    if depth <= 0 and j > i:
                        break
                i = j
                continue
            m = re.match(r"^pub enum (\w+)", lines[i])
            if m:
                typ = m.group(1)
                depth = 0
                j = i
                while j < len(lines):
                    if depth == 1:
                        vm = re.match(r"\s+([A-Z]\w*)", lines[j])
                        if vm:
                            assoc[f"{typ}::{vm.group(1)}"] = (
                                "variant", f"src/runtime/{fname}", j + 1)
                    depth += lines[j].count("{") - lines[j].count("}")
                    j += 1
                    if depth <= 0 and j > i:
                        break
                i = j
                continue
            m = re.match(r"^pub trait (\w+)", lines[i])
            if m:
                typ = m.group(1)
                depth = 0
                j = i
                while j < len(lines):
                    fm = re.match(r"\s+(?:unsafe )?fn (\w+)", lines[j])
                    if fm and depth == 1:
                        assoc.setdefault(f"{typ}::{fm.group(1)}",
                                         ("trait-fn", f"src/runtime/{fname}", j + 1))
                    depth += lines[j].count("{") - lines[j].count("}")
                    j += 1
                    if depth <= 0 and j > i:
                        break
                i = j
                continue
            i += 1
    return items, assoc


STUB_MACROS = re.compile(r"\b(unimplemented!|todo!|panic!|unreachable!)\b")


def find_body_stubs():
    """{path: (file, line)} for fns whose body hits a stub macro."""
    rtdir = os.path.join(ROOT, "src", "runtime")
    found = {}
    for fname in sorted(os.listdir(rtdir)):
        if not fname.endswith(".rs"):
            continue
        lines = open(os.path.join(rtdir, fname), encoding="utf-8").read().splitlines()
        # top-level fns
        for i, ln in enumerate(lines):
            m = re.match(r"^pub (?:unsafe )?fn (\w+)", ln)
            if not m:
                continue
            body, j = [], i
            depth = 0
            started = False
            while j < len(lines):
                depth += lines[j].count("{") - lines[j].count("}")
                if "{" in lines[j]:
                    started = True
                if started:
                    code = lines[j].split("//")[0]
                    body.append((j + 1, code))
                j += 1
                if started and depth <= 0:
                    break
            for ln_no, code in body:
                if STUB_MACROS.search(code):
                    found[m.group(1)] = (f"src/runtime/{fname}", ln_no)
                    break
        # assoc + trait-default fns: reuse parse via assoc map scan
        _, assoc = parse_surface_cached(fname, lines)
        for full, (kind, _, ln_no) in assoc.items():
            if kind not in ("fn", "trait-fn"):
                continue
            body = []
            depth = 0
            started = False
            j = ln_no - 1
            while j < len(lines):
                depth += lines[j].count("{") - lines[j].count("}")
                if "{" in lines[j]:
                    started = True
                if started:
                    body.append((j + 1, lines[j].split("//")[0]))
                j += 1
                if started and depth <= 0 or j - ln_no > 400:
                    break
            for b_no, code in body:
                if STUB_MACROS.search(code):
                    found[full] = (f"src/runtime/{fname}", b_no)
                    break
    return found


def parse_surface_cached(fname, lines):
    assoc = {}
    i = 0
    while i < len(lines):
        m = re.match(r"^impl(<[^;{]*>)?\s+(\w+)", lines[i])
        if m and " for " not in lines[i].split("{")[0]:
            typ = m.group(2)
            depth = 0
            j = i
            while j < len(lines):
                depth += lines[j].count("{") - lines[j].count("}")
                fm = re.match(r"\s+pub (?:const )?(?:unsafe )?fn (\w+)", lines[j])
                if fm:
                    assoc[f"{typ}::{fm.group(1)}"] = ("fn", fname, j + 1)
                j += 1
                if depth <= 0 and j > i:
                    break
            i = j
            continue
        m = re.match(r"^pub trait (\w+)", lines[i])
        if m:
            typ = m.group(1)
            depth = 0
            j = i
            while j < len(lines):
                fm = re.match(r"\s+(?:unsafe )?fn (\w+)", lines[j])
                if fm and depth == 1:
                    assoc.setdefault(f"{typ}::{fm.group(1)}", ("trait-fn", fname, j + 1))
                depth += lines[j].count("{") - lines[j].count("}")
                j += 1
                if depth <= 0 and j > i:
                    break
            i = j
            continue
        i += 1
    return None, assoc


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--protoc35", default=os.path.join(ROOT, "target",
                    "pinned-protoc-build", "protoc"))
    ap.add_argument("--protoc36", default="protoc")
    ap.add_argument("--verify", action="store_true")
    args = ap.parse_args()

    protocs = {"v35": args.protoc35, "v36": args.protoc36}
    versions = {k: protoc_version(p) for k, p in protocs.items()}
    print(f"protocs: {versions}", flush=True)
    sets = collect_sets()
    print(f"sets: { {k: len(v[0]) for k, v in sets.items()} }", flush=True)

    items_surface, assoc_surface = parse_surface()
    surface = dict(items_surface)
    surface.update(assoc_surface)
    body_stubs = find_body_stubs()

    runs = []
    uses = {}  # path -> {(protoc, proto): count}
    compile_errors = {}
    for label, protoc in protocs.items():
        mods = []
        for set_name, (roots, inc_dirs) in sorted(sets.items()):
            files = closure(roots, inc_dirs)
            # One mapping entry per file (mirrors rust_out_shared LIBS).
            # Entry names match the scratch-crate mod names exactly so
            # cross-file `crate::<mod>::` references resolve.
            ordered = sorted(files)
            prefix = set_name.replace("-", "_")
            mapping = os.path.join(SCRATCH, label, set_name, "crate_mapping.txt")
            os.makedirs(os.path.dirname(mapping), exist_ok=True)
            with open(mapping, "w") as f:
                for idx, imp in enumerate(ordered):
                    f.write(f"crate::{prefix}_f{idx:03d}\n1\n{imp}\n")
            for idx, imp in enumerate(ordered):
                workdir = os.path.join(SCRATCH, label, set_name, f"f{idx:03d}")
                ok, err, outdir = gen_one(protoc, workdir, imp, files[imp],
                                          inc_dirs, mapping)
                runs.append({"protoc": label, "set": set_name, "proto": imp,
                             "gen_ok": ok, "gen_error": err[:500] if not ok else ""})
                if not ok:
                    continue
                gens = find_generated_rs(outdir)
                if not gens:
                    runs[-1]["gen_ok"] = False
                    runs[-1]["gen_error"] = "no generated.rs emitted"
                    continue
                mod_name = f"{prefix}_f{idx:03d}"
                mods.append((mod_name, gens[0], imp))
                for dirpath, _, filenames in os.walk(outdir):
                    for fn in filenames:
                        if fn.endswith(".rs"):
                            for path, cnt in extract_uses(
                                    os.path.join(dirpath, fn)).items():
                                key = (label, f"{set_name}:{imp}")
                                uses.setdefault(path, {}).setdefault(key, 0)
                                uses[path][key] += cnt
        crate_dir = os.path.join(SCRATCH, label, "crate")
        errors = build_crate(label, [(m, g) for m, g, _ in mods], crate_dir)
        compile_errors[label] = errors
        print(f"{label}: {len(mods)} mods, {len(errors)} errors", flush=True)

    # Classify.
    records = []
    for path in sorted(set(uses) | set(CURATED_STUBS)):
        rec = {"name": path, "used_by": [
            {"protoc": p, "proto": f, "count": c}
            for (p, f), c in sorted(uses.get(path, {}).items())]}
        if path == "parse_into":
            # Private kernel fn: locate dynamically for evidence.
            rec["status"] = "stub"
            rec["reason"] = "curated: " + CURATED_STUBS[path]
            rec["evidence"] = locate_def("decode.rs", r"^fn parse_into\b")
            rec["kind"] = "fn (private)"
            records.append(rec)
            continue
        if path in CURATED_STUBS:
            rec["status"] = "stub"
            rec["reason"] = "curated: " + CURATED_STUBS[path]
            rec["evidence"] = surface.get(path, ("?", "?", 0))[1:]
        elif path not in surface:
            rec["status"] = "missing"
            rec["reason"] = "named by gencode, not in pbrs runtime"
            rec["evidence"] = []
        elif path in body_stubs:
            rec["status"] = "stub"
            rec["reason"] = "pattern: stub macro in body"
            rec["evidence"] = list(body_stubs[path])
        else:
            rec["status"] = "implemented"
            rec["reason"] = ""
            rec["evidence"] = list(surface[path][1:])
        rec["kind"] = surface.get(path, ("unknown",))[0]
        records.append(rec)

    doc = {
        "schema": "uk01/1",
        "protoc": {k: {"path": protocs[k], "version": v}
                   for k, v in versions.items()},
        "sets": {k: {"roots": len(v[0])} for k, v in sets.items()},
        "gen_runs": runs,
        "compile_errors": compile_errors,
        "items": records,
    }
    if args.verify:
        old = json.load(open(JSON_OUT))
        def sig(d):
            return (sorted((i["name"], i["status"]) for i in d["items"]),
                    {k: len(v) for k, v in d["compile_errors"].items()},
                    sum(1 for r in d["gen_runs"] if not r["gen_ok"]))
        if sig(old) != sig(doc):
            raise SystemExit("inventory drifted: items/errors differ from recorded JSON")
        print("inventory matches recorded JSON")
        return
    os.makedirs(os.path.dirname(JSON_OUT), exist_ok=True)
    json.dump(doc, open(JSON_OUT, "w"), indent=1, sort_keys=True)
    write_md(doc)
    print(f"wrote {JSON_OUT} and {MD_OUT}")


def write_md(doc):
    from collections import Counter
    items = doc["items"]
    by_status = Counter(i["status"] for i in items)
    n_protos = sum(v["roots"] for v in doc["sets"].values())
    gen_fail = [r for r in doc["gen_runs"] if not r["gen_ok"]]
    L = ["# rust_out kernel ABI inventory (UK-01)",
         "",
         f"protoc v35: `{doc['protoc']['v35']['version']}`; "
         f"protoc v36: `{doc['protoc']['v36']['version']}`. "
         f"{n_protos} upstream + corpus protos generated per protoc, "
         "compiled against pbrs as the `protobuf` crate.",
         "",
         "## Item status",
         "",
         "| Status | Count |",
         "|---|---|",
         f"| implemented | {by_status.get('implemented', 0)} |",
         f"| stub | {by_status.get('stub', 0)} |",
         f"| missing | {by_status.get('missing', 0)} |",
         "",
         "## Known stubs (acceptance checklist)",
         ""]
    for name in ["build_enum_mini_table", "ExtensionRegistryPtr",
                 "MiniTableExtensionPtr",
                 "MessageViewInterop::__unstable_wrap_raw_message",
                 "message_eq", "debug_string", "parse_into"]:
        hit = next((i for i in items if i["name"] == name), None)
        if hit:
            ev = ":".join(str(x) for x in hit["evidence"]) if hit["evidence"] else "curated"
            nuse = sum(u["count"] for u in hit["used_by"])
            L.append(f"- `{name}`: {hit['status']} ({hit['reason']}; {ev}; "
                     f"{nuse} gencode uses)")
        else:
            L.append(f"- `{name}`: NOT CAPTURED")
    L += ["",
          "## Compile errors (cargo check per protoc)",
          ""]
    for label in ("v35", "v36"):
        errs = doc["compile_errors"][label]
        L.append(f"- {label}: {len(errs)} errors")
        seen = set()
        for e in errs[:15]:
            key = (e["code"], e["message"][:80])
            if key in seen:
                continue
            seen.add(key)
            L.append(f"  - [{e['code']}] {e['file']}:{e['line']}: {e['message'][:120]}")
        if len(errs) > 15:
            L.append(f"  - ... and {len(errs) - 15} more (see JSON)")
    L += ["",
          "## Generation failures",
          ""]
    if gen_fail:
        for r in gen_fail:
            L.append(f"- {r['protoc']} {r['set']}:{r['proto']}: {r['gen_error'][:200]}")
    else:
        L.append("None: every proto generated under both protocs.")
    L += ["",
          "## Missing items (named by gencode, absent from pbrs)",
          ""]
    missing = [i for i in items if i["status"] == "missing"]
    if missing:
        for i in missing:
            users = ", ".join(f"{u['protoc']}:{u['proto']}" for u in i["used_by"][:3])
            L.append(f"- `{i['name']}` (used by {users})")
    else:
        L.append("None.")
    L += ["",
          "## Regenerating",
          "",
          "```bash",
          "python3 scripts/rust-out-abi.py",
          "python3 scripts/rust-out-abi.py --verify   # fail on drift",
          "```",
          ""]
    open(MD_OUT, "w").write("\n".join(L))


if __name__ == "__main__":
    main()
