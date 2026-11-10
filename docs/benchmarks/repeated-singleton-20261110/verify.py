#!/usr/bin/env python3
import json, math, pathlib, statistics, sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text())
cases = data["cases"]
expected = {"query_empty", "query4_construct", "query4_fresh", "query4_mutate", "query4_warm", "query4_decode", "query4_prost_fresh", "repeat_one_collect", "repeat_one_proxy", "repeat_one_into_vec", "repeat_eight_collect", "repeat_large_one", "repeat_large_many"}
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
    ("query4_fresh", (49, 1, 2949), (39, 0, 2573)),
    ("query4_prost_fresh", (22, 1, 2421), (22, 1, 2421)),
    ("repeat_one_proxy", (2, 0, 32), (2, 0, 40)),
    ("repeat_eight_collect", (2, 0, 88), (2, 0, 88)),
    ("repeat_large_many", (2, 0, 2072), (2, 0, 3080)),
]:
    per = cases[case]["allocations"][0]["per_op"]
    for arm, expected_row in (("a", a), ("b", b)):
        assert tuple(per[arm][field] for field in ("allocs", "reallocs", "bytes")) == expected_row
print(f"validated {len(cases)} cases, 130 paired blocks, 520 CPU samples and 39 allocation pairs; all measured identities and summaries match")
