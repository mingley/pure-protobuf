#!/usr/bin/env python3
import json, math, pathlib, statistics, sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text())
cases = data["cases"]
expected = {"query_empty", "query_operator_only", "query_cold_empty_proxy", "query_one_predicate", "query_mixed_10", "query3_fresh", "query4_construct", "query4_fresh", "query8_fresh", "query4_mutate", "query4_warm", "query4_clone_encode", "query4_decode", "query4_decode_read", "query4_prost_fresh", "repeat_eight_collect", "repeat_large_many"}
assert set(cases) == expected
identity = tuple(data["wire_identity"])
def same(x, y):
    assert math.isclose(x, y, rel_tol=1e-12, abs_tol=1e-12), (x, y)
def check_row(row, case, n):
    assert row["case"] == case and row["n"] == n and row["cpu_ns"] > 0
    assert (row["wire_len"], row["wire_fnv"]) == identity
for case, item in cases.items():
    n, allocation_n = item["n"], item["allocation_n"]
    assert len(item["blocks"]) == 10 and len(item["allocations"]) == 3
    check_row(item["calibration"], case, item["calibration"]["n"])
    for i, block in enumerate(item["blocks"]):
        assert block["order"] == ("abba" if i % 2 == 0 else "baab")
        assert len(block["rows"]) == 4
        for row in block["rows"]:
            check_row(row, case, n)
        timings = {arm: [row["cpu_ns"] for name, row in zip(block["order"], block["rows"]) if name == arm] for arm in "ab"}
        same(block["ba"], sum(timings["b"]) / sum(timings["a"]))
        for arm in "ab":
            same(block[arm + arm], timings[arm][1] / timings[arm][0])
    for metric in ("ba", "aa", "bb"):
        values = [block[metric] for block in item["blocks"]]
        reported = item["ba"] if metric == "ba" else item["controls"][metric]
        same(reported["median"], statistics.median(values))
        same(reported["range"][0], min(values))
        same(reported["range"][1], max(values))
        assert reported["slower"] == sum(value > 1 for value in values)
    assert item["min_process_cpu_ns"] == min(row["cpu_ns"] for block in item["blocks"] for row in block["rows"])
    for pair in item["allocations"]:
        for arm in "ab":
            for label, count in (("_n", allocation_n), ("_2n", allocation_n * 2)):
                check_row(pair["rows"][arm + label], case, count)
            for field in ("allocs", "reallocs", "bytes"):
                delta = (pair["rows"][arm + "_2n"][field] - pair["rows"][arm + "_n"][field]) / allocation_n
                same(delta, pair["per_op"][arm][field])
    assert all(pair["per_op"] == item["allocations"][0]["per_op"] for pair in item["allocations"])
for case, a, b in [
    ("query_cold_empty_proxy", (2, 0, 56), (1, 0, 96)),
    ("query_one_predicate", (4, 0, 137), (3, 0, 177)),
    ("query_mixed_10", (4, 0, 448), (3, 0, 488)),
    ("query4_fresh", (49, 1, 2949), (38, 1, 3069)),
    ("query8_fresh", (85, 1, 4949), (66, 1, 5133)),
    ("query4_decode", (39, 0, 4168), (29, 0, 4248)),
    ("query4_prost_fresh", (22, 1, 2421), (22, 1, 2421)),
    ("repeat_large_many", (2, 0, 2072), (2, 0, 2072)),
]:
    per = cases[case]["allocations"][0]["per_op"]
    for arm, expected_row in (("a", a), ("b", b)):
        assert tuple(per[arm][field] for field in ("allocs", "reallocs", "bytes")) == expected_row
print(f"validated {len(cases)} cases, 170 paired blocks, 680 CPU samples and 51 allocation blocks; all measured identities and summaries match")
