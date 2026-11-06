"""Negative controls for the proof runner's nonempty test requirement."""

import unittest

from run import validate_single_test_transcript


class RequiredTestTranscript(unittest.TestCase):
    NAME = "tests::expected_case"
    CASE = "test tests::expected_case ... ok\n"
    SUMMARY = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"

    def rejects(self, output):
        with self.assertRaises(ValueError):
            validate_single_test_transcript(output, self.NAME)

    def test_accepts_one_named_pass_with_cargo_json_noise(self):
        validate_single_test_transcript(
            '{"reason":"build-finished","success":true}\n\nrunning 1 test\n'
            + self.CASE + "\n" + self.SUMMARY, self.NAME,
        )

    def test_rejects_missing_transcript(self):
        self.rejects("")

    def test_rejects_zero_tests(self):
        self.rejects(self.SUMMARY.replace("1 passed", "0 passed"))

    def test_rejects_ignored_test(self):
        self.rejects(self.CASE.replace("... ok", "... ignored")
                     + self.SUMMARY.replace("1 passed", "0 passed").replace("0 ignored", "1 ignored"))

    def test_rejects_duplicate_test(self):
        self.rejects(self.CASE + self.CASE + self.SUMMARY)

    def test_rejects_duplicate_summary(self):
        self.rejects(self.CASE + self.SUMMARY + self.SUMMARY)

    def test_rejects_nonpassing_test(self):
        self.rejects(self.CASE.replace("... ok", "... FAILED")
                     + self.SUMMARY.replace("ok.", "FAILED.").replace("1 passed", "0 passed").replace("0 failed", "1 failed"))

    def test_rejects_filtered_test(self):
        self.rejects(self.CASE + self.SUMMARY.replace("0 filtered out", "1 filtered out"))

    def test_rejects_measured_test(self):
        self.rejects(self.CASE + self.SUMMARY.replace("0 measured", "1 measured"))

    def test_rejects_wrong_test_name(self):
        self.rejects(self.CASE.replace(self.NAME, "tests::wrong_case") + self.SUMMARY)

    def test_rejects_missing_result_line(self):
        self.rejects(self.SUMMARY)

    def test_rejects_missing_summary(self):
        self.rejects(self.CASE)

    def test_rejects_extra_test(self):
        self.rejects(self.CASE + "test tests::extra_case ... ok\n" + self.SUMMARY)


if __name__ == "__main__":
    unittest.main()
