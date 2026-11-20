import json
import unittest

import summary


def fixture(name, pbrs, prost, upb, holdout=False):
    row = {"name": name, "detail": "", "samples": len(pbrs), "holdout": holdout}
    for column in summary.OPERATIONS:
        for codec, values in [("pbrs", pbrs), ("prost", prost), ("v4", upb)]:
            row[f"{codec}_{column}"] = values
    return row


class SurveySummaryTest(unittest.TestCase):
    def setUp(self):
        self.report = {
            "schema": "tonic-raw/1",
            "pbrs_prost_encode_output": "owned",
            "allocation_counts": False,
            "rows": [
                fixture("base", [3, 1, 2], [3, 3, 3], [2, 1, 3]),
                fixture("held", [1, 4], [3, 3], [5, 5], holdout=True),
                {
                    "name": "mutate", "detail": "append",
                    "pbrs_mutated_enc": [6], "prost_mutated_enc": [5], "v4_mutated_enc": [4],
                },
            ],
        }

    def test_saved_survey_and_json_report_count_the_same_fixtures(self):
        raw = json.dumps(self.report, indent=2)
        for text in (raw, "## Per-case tables\n\n## Raw paired samples (schema tonic-raw/1)\n" + raw):
            with self.subTest(raw_only=text == raw):
                rendered = summary.summarize(summary.read_report(text), losses=True)
                self.assertIn("2 codec fixtures.", rendered)
                self.assertIn("Allocation counters off", rendered)
                self.assertIn("| First encode | Prost | 1 | 0 | 1 |", rendered)
                self.assertIn("| Repeated encode | upb | 1 | 1 | 0 |", rendered)
                self.assertIn("| Full read | Prost | held | 4.000 | 3.000 |", rendered)
                self.assertNotIn("mutate", rendered)

    def test_missing_or_incomplete_column_is_an_error(self):
        self.report["rows"][1]["v4_construct"] = [1]
        with self.assertRaisesRegex(ValueError, "held.*v4_construct"):
            summary.summarize(self.report)

    def test_duplicate_fixture_is_an_error(self):
        self.report["rows"][1]["name"] = "base"
        with self.assertRaisesRegex(ValueError, "duplicate"):
            summary.summarize(self.report)
