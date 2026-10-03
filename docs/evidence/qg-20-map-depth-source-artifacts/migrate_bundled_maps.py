#!/usr/bin/env python3
"""Exact source-pinned map migration. No generator/compiler is invoked."""
import argparse
import bisect
import collections
import difflib
import hashlib
import json
import re
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--apply", action="store_true")
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
proof = root / "work/qg20"
baseline = "ad04010b166325ec09bb9b403afd47338a38ab1f"
guard = 'if depth >= pbrs::RECURSION_LIMIT { return Err(ParseError::new("recursion limit exceeded")); }'
entry_guard = 'if depth > pbrs::RECURSION_LIMIT { return Err(ParseError::new("recursion limit exceeded")); }'
value_guard = 'if entry_depth >= pbrs::RECURSION_LIMIT { return Err(ParseError::new("recursion limit exceeded")); }'
function_re = re.compile(r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?fn[ \t]+(\w+)[ \t]*\(")
impl_re = re.compile(r"(?m)^[ \t]*impl[ \t]+(\w+)[ \t]*\{")
len_arm = re.compile(r"pbrs::rt::WIRE_LEN\s*=>\s*\{")
field_arm = re.compile(r"(?m)^[ \t]*(\d+)\s*=>\s*match\s+w\s*\{")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canon(source):
    return re.sub(r"\s+", "", source)


def close(source, opening):
    # The selected generated codec regions use ordinary string literals and
    # comments; this lexical scan is not a Rust AST/typechecker.
    assert source[opening] == "{"
    level, pos = 1, opening + 1
    while level:
        if source.startswith("//", pos):
            pos = source.index("\n", pos) + 1
        elif source[pos] == '"':
            pos += 1
            while source[pos] != '"':
                pos += 2 if source[pos] == "\\" else 1
            pos += 1
        else:
            level += (source[pos] == "{") - (source[pos] == "}")
            pos += 1
    return pos


outputs, operations = [], []
totals = collections.Counter()
for path in sorted((root / "src/generated").glob("*.rs")):
    relative = str(path.relative_to(root))
    original = subprocess.run(["git", "show", baseline + ":" + relative], cwd=root,
                              check=True, capture_output=True).stdout
    source = original.decode()
    functions = list(function_re.finditer(source))
    decoders = [f for f in functions if f[1].startswith("decode_map_entry_")]
    if not decoders:
        continue
    assert path.read_bytes() == original, relative
    function_starts = [f.start() for f in functions]
    impls = list(impl_re.finditer(source))
    replacements, metadata = [], {}

    def add(start, end, after, kind, **details):
        before = source[start:end]
        replacements.append((start, end, after))
        totals[kind] += 1
        operations.append({"path": relative, "start": start, "end": end,
                           "original_line": source.count("\n", 0, start) + 1,
                           "kind": kind, "before": before, "after": after, **details})

    for function in decoders:
        opening = source.index("{", function.end())
        end = close(source, opening)
        body = source[opening + 1:end - 1]
        message_value = bool(re.search(r"\bval\.merge_inner\s*\(", body))
        value_type = re.search(r"let mut val = ([^;\n]+)::default\(\);", body)[1]
        assert body.count("let _ = depth;") == 1
        revised = body.replace("let _ = depth;", entry_guard, 1)
        if message_value:
            revised = revised.replace("let mut val = " + value_type + "::default();",
                                      "let mut val: Option<" + value_type + "> = None;", 1)
            arm = re.search(r"\(2, pbrs::rt::WIRE_LEN\)\s*=>\s*\{", revised)
            arm_open = revised.index("{", arm.start())
            arm_end = close(revised, arm_open)
            expected = "let (s, e) = pbrs::rt::read_len_span(data, &mut pos)?; let mut ip = 0; let mut sw = None; val.merge_inner(&data[s..e], &mut sw, &mut ip, depth, true, None)?;"
            assert canon(revised[arm_open + 1:arm_end - 1]) == canon(expected), relative
            after = guard + " " + expected.replace(
                "val.merge_inner", "val.get_or_insert_with(" + value_type + "::default).merge_inner"
            ).replace("ip, depth, true", "ip, depth + 1, true")
            revised = revised[:arm_open + 1] + after + revised[arm_end - 1:]
            assert revised.count("Ok((key, val))") == 1
            revised = revised.replace("Ok((key, val))", "Ok((key, val.unwrap_or_default()))", 1)
        add(opening + 1, end - 1, revised, "private-map-decoder",
            function=function[1], message_value=message_value, value_type=value_type)
        metadata[function[1]] = {"message_value": message_value, "value_type": value_type}
        totals["message-value-decoder" if message_value else "scalar-value-decoder"] += 1

    calls = list(re.finditer(r"let\s*\(kk,\s*vv\)\s*=\s*(decode_map_entry_\w+)\s*\(", source))
    assert len(calls) == len(decoders), relative
    entries = {}
    for call in calls:
        function = functions[bisect.bisect_right(function_starts, call.start()) - 1]
        assert function[1] in {"merge_inner", "merge_heavy"}, function[1]
        owner = next(impl for impl in reversed(impls) if impl.start() < function.start())[1]
        number = int(call[1].rsplit("_", 1)[1])
        key = (owner, number)
        assert key not in entries
        entries[key] = {**metadata[call[1]], "decoder": call[1]}
        arm = list(len_arm.finditer(source, function.start(), call.start()))[-1]
        opening = source.index("{", arm.start())
        assert call.end() < close(source, opening)
        prefix = source[opening + 1:call.start()]
        assert canon(prefix) == "let(s,e)=pbrs::rt::read_len_span(data,pos)?;", (relative, call[1])
        add(opening + 1, opening + 1, guard + "\n", "map-entry-before-wire",
            owner=owner, number=number, function=function[1], decoder=call[1])

    validations = 0
    for function in functions:
        if function[1] != "validate_until":
            continue
        opening = source.index("{", function.end())
        end = close(source, opening)
        owner = next(impl for impl in reversed(impls) if impl.start() < function.start())[1]
        for loop in re.finditer(r"while ip < d\.len\(\)", source[opening:end]):
            loop_start = opening + loop.start()
            arm = list(len_arm.finditer(source, opening, loop_start))[-1]
            arm_open = source.index("{", arm.start())
            arm_end = close(source, arm_open)
            field = list(field_arm.finditer(source, opening, arm.start()))[-1]
            number = int(field[1])
            entry = entries[(owner, number)]
            loop_open = source.index("{", loop_start)
            loop_end = close(source, loop_open)
            before = source[arm_open + 1:loop_end]
            expected = "let (s, e) = pbrs::rt::read_len_span(data, pos)?; let mut ip = 0; let w = wire.window(s, e); let d = w.as_slice(); while ip < d.len() { let (_, ww) = pbrs::rt::decode_tag(d, &mut ip)?; pbrs::rt::skip_field_with_depth(d, &mut ip, ww, depth + 1)?; }"
            assert canon(before) == canon(expected), (relative, owner, number)
            preamble = guard + " let entry_depth = depth + 1; let (s, e) = pbrs::rt::read_len_span(data, pos)?; let mut ip = 0; let w = wire.window(s, e); let d = w.as_slice(); "
            if entry["message_value"]:
                after = preamble + "while ip < d.len() { let (nn, ww) = pbrs::rt::decode_tag(d, &mut ip)?; match (nn, ww) { (2, pbrs::rt::WIRE_LEN) => { " + value_guard + " let (vs, ve) = pbrs::rt::read_len_span(d, &mut ip)?; let mut vp = 0; " + entry["value_type"] + "::validate_inner(&w.window(vs, ve), &mut vp, entry_depth + 1)?; }, _ => pbrs::rt::skip_field_with_depth(d, &mut ip, ww, entry_depth)?, } }"
            else:
                after = preamble + "while ip < d.len() { let (_, ww) = pbrs::rt::decode_tag(d, &mut ip)?; pbrs::rt::skip_field_with_depth(d, &mut ip, ww, entry_depth)?; }"
            add(arm_open + 1, loop_end, after, "map-entry-validation",
                owner=owner, number=number, message_value=entry["message_value"],
                value_type=entry["value_type"], decoder=entry["decoder"])
            totals["message-value-validator" if entry["message_value"] else "scalar-value-validator"] += 1
            validations += 1
    assert validations == len(decoders), relative
    ordered = sorted(replacements)
    assert all(a[1] <= b[0] for a, b in zip(ordered, ordered[1:])), relative
    candidate = source
    for start, end, after in reversed(ordered):
        candidate = candidate[:start] + after + candidate[end:]
    staged = proof / "migrated-map-callers" / relative
    staged.parent.mkdir(parents=True, exist_ok=True)
    staged.write_text(candidate)
    outputs.append({"path": relative, "original_sha256": digest(original),
                    "unformatted_migrated_sha256": digest(candidate.encode()),
                    "map_decoders": len(decoders), "map_callers": len(calls),
                    "map_validators": validations,
                    "message_value_maps": sum(row["message_value"] for row in metadata.values())})

assert len(outputs) == 7 and totals["private-map-decoder"] == 109
assert totals["map-entry-before-wire"] == totals["map-entry-validation"] == 109
assert totals["message-value-decoder"] == totals["message-value-validator"] == 17
if args.apply:
    for row in outputs:
        assert digest((root / row["path"]).read_bytes()) == row["original_sha256"]
    for row in outputs:
        (root / row["path"]).write_bytes((proof / "migrated-map-callers" / row["path"]).read_bytes())
record = {"kind": "exact provisional source migration; actual generator/protoc/Cargo NOT_RUN",
          "baseline": baseline, "applied": args.apply, "counts": dict(totals),
          "files": outputs, "operations": operations}
(proof / "map-migration.json").write_text(json.dumps(record, indent=2) + "\n")
print(json.dumps({key: record[key] for key in ["kind", "baseline", "applied", "counts"]}))
