"""Offline integrity and shape checks for the pinned C++ CLOSED-enum reference."""

import hashlib
import json
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent
CATALOG = json.loads((ROOT / "expectations.json").read_text())
CASE = re.compile(
    r"^(?P<name>[a-z0-9_]+) input=(?P<input>[0-9a-f]+) "
    r"generated=(?P<generated>[0-9a-f]+) dynamic=(?P<dynamic>[0-9a-f]+) "
    r"packed=\[(?P<packed>[0-9,]*)\] expanded=\[(?P<expanded>[0-9,]*)\] "
    r"map_size=(?P<map_size>[0-9]+) unknown=\[(?P<unknown>[0-9a-f:,]*)\]$"
)


class ClosedEnumReferenceTest(unittest.TestCase):
    def test_reference_files_match_checked_checksums(self):
        artifacts = CATALOG["artifacts"]
        self.assertEqual(
            CATALOG["closed_enum_reference"]["protobuf_revision"],
            "f377bfefc5e2cfab68b816903c25b23e091c439d",
        )
        self.assertIs(CATALOG["closed_enum_reference"]["qualified"], False)
        checksums = dict(artifacts["reference_checksums"])
        checksums["fds/closed_enum.fds"] = artifacts["fds_checksums"]["closed_enum.fds"]
        for path, expected in checksums.items():
            with self.subTest(path=path):
                self.assertEqual(hashlib.sha256((ROOT / path).read_bytes()).hexdigest(), expected)
        self.assertEqual(
            checksums["reference/observed.txt"],
            "89ca566019a8ca17b0e137c126cec0cd3880e0e5a2368b77c384ae2e09f731ee",
        )

    def test_generated_and_dynamic_reference_vectors_match(self):
        cases = {}
        for line in (ROOT / "reference/observed.txt").read_text().splitlines():
            match = CASE.fullmatch(line)
            self.assertIsNotNone(match, line)
            case = match.groupdict()
            name = case["name"]
            self.assertNotIn(name, cases)
            bytes.fromhex(case["input"])
            bytes.fromhex(case["generated"])
            bytes.fromhex(case["dynamic"])
            self.assertEqual(case["generated"], case["dynamic"], name)
            cases[name] = case

        self.assertEqual(
            set(cases),
            {
                "packed_negative",
                "unpacked_negative",
                "packed_highbit32",
                "unpacked_highbit32",
                "packed_highbit64",
                "mixed",
                "expanded_mixed",
                "map_mixed",
                "map_highbit64",
            },
        )
        self.assertEqual(
            cases["packed_negative"]["generated"], "0a02000108ffffffffffffffffff01"
        )
        self.assertEqual(
            cases["unpacked_negative"]["generated"], cases["unpacked_negative"]["input"]
        )
        self.assertEqual(cases["mixed"]["packed"], "1,0,1,0")
        self.assertEqual(
            cases["mixed"]["unknown"], "1:2,1:18446744073709551615,1:3,1:2"
        )
        self.assertEqual(cases["map_mixed"]["map_size"], "1")
        self.assertEqual(
            cases["map_mixed"]["unknown"],
            "3:08011002,3:080110ffffffffffffffffff01",
        )


if __name__ == "__main__":
    unittest.main()
