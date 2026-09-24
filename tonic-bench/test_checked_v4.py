"""Offline integrity checks for the v35.1 benchmark-only Rust bindings."""

import hashlib
import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CHECKED = ROOT / "tonic-bench/checked_v4"
MANIFEST = json.loads((CHECKED / "manifest.json").read_text())


class CheckedV4Test(unittest.TestCase):
    def test_generator_version_and_all_artifact_hashes(self):
        self.assertEqual(
            MANIFEST["protobuf_source_revision"],
            "35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03",
        )
        self.assertEqual(MANIFEST["protoc_version"], "libprotoc 35.1")
        self.assertEqual(
            MANIFEST["protoc_sha256"],
            "e2b116ef44d4b7f3246945ceb1938c72f04e16040020e321ac601869135ab940",
        )
        self.assertEqual(MANIFEST["runtime_version"], "4.35.1-release")
        self.assertIs(MANIFEST["qualification"]["qualified"], False)
        for relative, expected in MANIFEST["artifacts"].items():
            with self.subTest(path=relative):
                actual = hashlib.sha256((ROOT / relative).read_bytes()).hexdigest()
                self.assertEqual(actual, expected)

    def test_source_and_generated_version_are_consistent(self):
        self.assertEqual(
            (ROOT / "proto/codec_cases.proto").read_bytes(),
            (CHECKED / "codec_cases.proto").read_bytes(),
        )
        generated = (CHECKED / "codec_cases.u.pb.rs").read_text()
        self.assertIn('assert_compatible_gencode_version("4.35.1-release")', generated)
        self.assertNotIn('"0.36.1-release"', generated)
        index = (CHECKED / "generated.rs").read_text()
        self.assertIn('#[path="codec_cases.u.pb.rs"]', index)
        for content in (generated, index):
            self.assertNotIn("/Users/", content)
            self.assertNotIn("/home/runner/", content)


if __name__ == "__main__":
    unittest.main()
