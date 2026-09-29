#!/usr/bin/env python3
"""Differential driver for the GN-08 pinned frontend corpus.

Baseline mode (no --frontend): records pinned-protoc descriptor output for
every corpus entry in parse, link and full modes, verifies each outcome
against the corpus.json expectation, and exits nonzero on any mismatch.

Compare mode (--frontend BIN): additionally runs the frontend under test
with the same per-entry arguments and diffs verdicts (parse) and
descriptor bytes (link, full) against the recorded baseline.

Frontend contract: BIN --mode MODE -I DIR... [--out PATH] FILE...
  parse: exit 0 iff protoc would accept every FILE (verdict parity only).
  link:  write the FileDescriptorSet (no source info) to --out, exit 0.
  full:  write the FileDescriptorSet with source info and retained options
         (--include_source_info --retain_options equivalent) to --out.
"""
import argparse
import hashlib
import json
import os
import subprocess
import sys

MODES = ("parse", "link", "full")


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def safe_id(entry_id):
    return entry_id.replace("/", "_").replace(":", "_")


def run(cmd, cwd):
    try:
        cp = subprocess.run(cmd, capture_output=True, cwd=cwd)
    except OSError as e:
        return None, "", "exec failed: %s" % e
    return (cp.returncode,
            cp.stdout.decode("utf-8", "replace"),
            cp.stderr.decode("utf-8", "replace"))


def protoc_cmd(protoc, roots, files, out, mode):
    cmd = [protoc]
    for r in roots:
        cmd += ["-I", r]
    cmd += ["--descriptor_set_out=" + out]
    if mode == "full":
        cmd += ["--include_source_info", "--retain_options"]
    cmd += files
    return cmd


def main():
    ap = argparse.ArgumentParser(description="GN-08 frontend differential driver")
    ap.add_argument("--root", required=True)
    ap.add_argument("--protoc", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--mode", action="append", default=[], choices=MODES)
    ap.add_argument("--frontend", default=None)
    ap.add_argument("--entry", action="append", default=[],
                    help="only run these entry ids (repeatable)")
    ap.add_argument("--skip-source-verify", action="store_true")
    args = ap.parse_args()

    modes = tuple(args.mode) if args.mode else MODES
    corpus_path = os.path.join(args.root, "tests", "frontend_corpus", "corpus.json")
    with open(corpus_path, encoding="utf-8") as f:
        corpus = json.load(f)
    assert corpus["version"] == 1, "unsupported corpus.json version"

    root_dirs = {k: os.path.join(args.root, v) for k, v in corpus["roots"].items()}
    entries = corpus["entries"]
    if args.entry:
        want = set(args.entry)
        entries = [e for e in entries if e["id"] in want]
        missing = want - {e["id"] for e in entries}
        if missing:
            print("unknown --entry ids: %s" % sorted(missing), file=sys.stderr)
            return 2

    # Source integrity: vendored bytes must match the manifest before any
    # baseline is recorded, so a bitten file cannot silently move the oracle.
    if not args.skip_source_verify:
        bad = []
        for e in entries:
            first_root = root_dirs[e["roots"][0]]
            src = os.path.join(first_root, e["files"][0])
            if not os.path.isfile(src) or sha256_of(src) != e["sha256"]:
                bad.append(e["id"])
        if bad:
            print("source integrity failure (%d entries):" % len(bad), file=sys.stderr)
            for eid in bad[:20]:
                print("  %s" % eid, file=sys.stderr)
            print("re-run tests/frontend_corpus/fetch-corpus.sh", file=sys.stderr)
            return 2

    rc, protoc_version, _ = run([args.protoc, "--version"], args.root)
    protoc_version = (protoc_version or "").strip() or "unknown"
    if rc != 0:
        print("pinned protoc not runnable: %s" % args.protoc, file=sys.stderr)
        return 2

    if args.frontend and not (os.path.isfile(args.frontend) and os.access(args.frontend, os.X_OK)):
        print("frontend not executable: %s" % args.frontend, file=sys.stderr)
        return 2

    base_dir = os.path.join(args.out, "baseline")
    fe_dir = os.path.join(args.out, "frontend") if args.frontend else None
    os.makedirs(args.out, exist_ok=True)
    tmp_fds = os.path.join(args.out, "tmp.fds")

    results = []
    failures = []
    for e in entries:
        eid = e["id"]
        includes = [root_dirs[r] for r in e["roots"]]
        for mode in modes:
            out_path = os.path.join(base_dir, mode, safe_id(eid) + ".fds")
            os.makedirs(os.path.dirname(out_path), exist_ok=True)
            if os.path.exists(tmp_fds):
                os.unlink(tmp_fds)
            rc, _, err = run(protoc_cmd(args.protoc, includes, e["files"], tmp_fds, mode), args.root)
            if rc is None:
                print("protoc exec failed", file=sys.stderr)
                return 2
            outcome = "ok" if rc == 0 else "fail"
            digest = None
            if outcome == "ok" and mode != "parse":
                digest = sha256_of(tmp_fds)
                os.replace(tmp_fds, out_path)
            elif os.path.exists(out_path):
                os.unlink(out_path)
            with open(os.path.join(base_dir, mode, safe_id(eid) + ".status"), "w") as f:
                f.write("%s rc=%d\n" % (outcome, rc))
            rec = {"id": eid, "mode": mode, "expect": e["expect"],
                   "protoc": outcome, "protoc_rc": rc, "sha256": digest}
            if outcome != e["expect"]:
                rec["baseline_mismatch"] = True
                failures.append("baseline %s %s: protoc=%s expect=%s%s" % (
                    eid, mode, outcome, e["expect"],
                    (": " + err.strip().split("\n")[0][:200]) if err.strip() else ""))
            if args.frontend:
                fe_out = os.path.join(fe_dir, mode, safe_id(eid) + ".fds")
                os.makedirs(os.path.dirname(fe_out), exist_ok=True)
                fe_tmp = os.path.join(args.out, "tmp.frontend.fds")
                if os.path.exists(fe_tmp):
                    os.unlink(fe_tmp)
                cmd = [args.frontend, "--mode", mode]
                for inc in includes:
                    cmd += ["-I", inc]
                if mode != "parse":
                    cmd += ["--out", fe_tmp]
                cmd += e["files"]
                frc, _, ferr = run(cmd, args.root)
                if frc is None:
                    print("frontend exec failed", file=sys.stderr)
                    return 2
                foutcome = "ok" if frc == 0 else "fail"
                with open(os.path.join(fe_dir, mode, safe_id(eid) + ".status"), "w") as f:
                    f.write("%s rc=%d\n" % (foutcome, frc))
                rec["frontend"] = foutcome
                rec["frontend_rc"] = frc
                if foutcome != outcome:
                    rec["verdict_mismatch"] = True
                    failures.append("verdict %s %s: frontend=%s protoc=%s%s" % (
                        eid, mode, foutcome, outcome,
                        (": " + ferr.strip().split("\n")[0][:200]) if ferr.strip() else ""))
                elif foutcome == "ok" and mode != "parse":
                    fdigest = sha256_of(fe_tmp) if os.path.exists(fe_tmp) else None
                    if fdigest is None:
                        rec["bytes_mismatch"] = "missing-output"
                        failures.append("bytes %s %s: frontend produced no --out file" % (eid, mode))
                    else:
                        os.replace(fe_tmp, fe_out)
                        rec["frontend_sha256"] = fdigest
                        if fdigest != digest:
                            rec["bytes_mismatch"] = True
                            failures.append("bytes %s %s: frontend=%s protoc=%s" % (
                                eid, mode, fdigest[:16], digest[:16]))
            results.append(rec)

    summary = {
        "corpus": "pinned",
        "protoc": protoc_version,
        "protoc_pin": corpus["protoc"]["pin"],
        "protoc_sha": corpus["protoc"]["sha"],
        "modes": list(modes),
        "entries": len(entries),
        "frontend": args.frontend,
        "failures": len(failures),
        "results": results,
    }
    with open(os.path.join(args.out, "summary.json"), "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=1, sort_keys=False)
        f.write("\n")

    n_checks = len(results)
    if args.frontend:
        print("frontend-diff compare: %d entries x %s (%d checks, %d failures)" % (
            len(entries), ",".join(modes), n_checks, len(failures)))
    else:
        print("frontend-diff baseline: %d entries x %s (%d checks, %d failures)" % (
            len(entries), ",".join(modes), n_checks, len(failures)))
    print("protoc: %s (%s %s)" % (protoc_version, corpus["protoc"]["pin"], corpus["protoc"]["sha"][:12]))
    for line in failures[:40]:
        print("FAIL " + line)
    if len(failures) > 40:
        print("... and %d more (see summary.json)" % (len(failures) - 40))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
