#!/usr/bin/env python3
"""Reconstruct the exact inventoried source changes; no compiler invocation."""
import hashlib
import json
import re
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[2]
proof = root / "work/qg20"
inventory = json.loads((proof / "map-migration.json").read_text())
base = inventory["baseline"]
literals = re.compile(r'"(?:\\.|[^"\\])*"')
new_call = re.compile(r"pbrs::rt::(?:skip_field|capture_unknown)_with_depth\s*\(")
counts = inventory["counts"]
assert inventory["applied"]
assert counts["private-map-decoder"] == counts["map-entry-before-wire"] == counts["map-entry-validation"] == 109
assert counts["message-value-decoder"] == counts["message-value-validator"] == 17
assert counts["scalar-value-decoder"] == counts["scalar-value-validator"] == 92
assert len(inventory["files"]) == 7 and len(inventory["operations"]) == 327


def digest(data):
    return hashlib.sha256(data).hexdigest()


def source(path):
    return subprocess.run(["git", "show", base + ":" + path], cwd=root,
                          capture_output=True, check=True).stdout


def normalized(text):
    # Allow only formatting whitespace and two Rustfmt optional-comma cases.
    # Quoted literals are compared separately, before this normalization.
    text = re.sub(r"\s+", "", text)
    text = re.sub(r",(?=\))", "", text)
    return text.replace("},_=>", "}_=>")


rows = []
for row in inventory["files"]:
    path = row["path"]
    original = source(path)
    assert digest(original) == row["original_sha256"], path
    text = original.decode()
    operations = sorted((op for op in inventory["operations"] if op["path"] == path),
                        key=lambda op: op["start"])
    assert all(a["end"] <= b["start"] for a, b in zip(operations, operations[1:])), path
    for op in reversed(operations):
        assert original.decode()[op["start"]:op["end"]] == op["before"], path
        text = text[:op["start"]] + op["after"] + text[op["end"]:]
    current = (root / path).read_text()
    assert literals.findall(text) == literals.findall(current), (path, "quoted literal change")
    assert normalized(text) == normalized(current), (path, "unexpected token change")
    assert len(new_call.findall(original.decode())) == len(new_call.findall(current)), path
    decoders = [op for op in operations if op["kind"] == "private-map-decoder"]
    callers = [op for op in operations if op["kind"] == "map-entry-before-wire"]
    validators = [op for op in operations if op["kind"] == "map-entry-validation"]
    assert len(decoders) == len(callers) == len(validators) == row["map_decoders"]
    for op in decoders:
        after = normalized(op["after"])
        assert after.startswith('ifdepth>pbrs::RECURSION_LIMIT{returnErr(ParseError::new("recursionlimitexceeded"));}')
        if op["message_value"]:
            assert "letmutval:Option<" + op["value_type"] + ">=None;" in after
            assert "val.get_or_insert_with(" + op["value_type"] + "::default).merge_inner(" in after
            assert "&mutip,depth+1,true,None)?;" in after
            assert "Ok((key,val.unwrap_or_default()))" in after
            assert after.index("ifdepth>=pbrs::RECURSION_LIMIT") < after.index("val.get_or_insert_with")
        else:
            # An entry guard precedes the old scalar/key/unknown field body.
            original_body = normalized(op["before"]).replace("let_=depth;", "", 1)
            guard_end = after.index('letdata=wire.as_slice();')
            assert after[guard_end:] == original_body
    for op in callers:
        assert op["before"] == "" and 'if depth >= pbrs::RECURSION_LIMIT' in op["after"]
    for op in validators:
        after = normalized(op["after"])
        assert after.index("ifdepth>=pbrs::RECURSION_LIMIT") < after.index("letentry_depth=depth+1;")
        if op["message_value"]:
            assert '(2,pbrs::rt::WIRE_LEN)=>{' in after
            assert op["value_type"] + '::validate_inner(&w.window(vs,ve),&mutvp,entry_depth+1)?;' in after
            assert after.index('ifentry_depth>=pbrs::RECURSION_LIMIT') < after.index('::validate_inner(')
            assert '_=>pbrs::rt::skip_field_with_depth(d,&mutip,ww,entry_depth)?' in after
    rows.append({**row, "candidate_sha256": digest(current.encode()),
                 "reconstruction": "PASS: exact operation stream plus formatting only"})

all_generated = sorted((root / "src/generated").glob("*.rs"))
affected = {row["path"] for row in rows}
unchanged_generated = []
for path in all_generated:
    relative = str(path.relative_to(root))
    if relative not in affected:
        assert path.read_bytes() == source(relative), relative
        unchanged_generated.append(relative)
assert len(unchanged_generated) == 7  # Six unaffected registered files plus the registry.

unchanged = ["Cargo.toml", "Cargo.lock", "src/wire.rs", "src/rt.rs", "src/table.rs",
             "src/runtime/decode.rs", "src/generated/mod.rs", "scripts/regen-generated.sh",
             "tests/generated_map_value_depth.rs", "tests/support/generated_map_value_depth.rs",
             "tests/fixtures/generated_map_value_depth_consumer.rs", "tests/fixtures/generated_map_value_depth.proto"]
command = ["git", "diff", "--exit-code", base, "--", *unchanged]
result = subprocess.run(command, cwd=root, capture_output=True)
assert result.returncode == 0 and not result.stdout

record = {"kind": "source textual reconstruction; Cargo/generator/AST/type/regeneration/runtime qualification NOT_RUN",
          "baseline": base, "files": rows, "operations": 327, "map_entries": 109,
          "message_value_maps": 17, "scalar_value_maps": 92,
          "unaffected_generated_byte_identical": unchanged_generated,
          "unchanged_scope_argv": command, "unchanged_scope_exit": result.returncode,
          "handwritten_generator_sha256": digest((root / "src/codegen/parse.rs").read_bytes()),
          "scope": "one generator file plus seven owned generated files; same15 oracle bytes and matched recorded driver"}
(proof / "source-reconstruction.json").write_text(json.dumps(record, indent=2) + "\n")
print(json.dumps({key: record[key] for key in ["kind", "baseline", "operations", "map_entries", "message_value_maps", "scalar_value_maps", "scope"]}))
