"""Catch task deadlocks hidden behind legacy-to-current reconciliation."""

import contextlib
import importlib.util
import io
from pathlib import Path
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "plan_lint", Path(__file__).resolve().parents[1] / "scripts/plan-lint.py"
)
PLAN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PLAN)


def card(name, dependencies=()):
    return {"id": name, "title": name, "depends_on": list(dependencies)}


class ReconciliationCycleTests(unittest.TestCase):
    def run_lint(self, old, new):
        output = io.StringIO()
        with mock.patch.object(PLAN, "load", side_effect=[old, new]):
            with contextlib.redirect_stdout(output):
                status = PLAN.main()
        return status, output.getvalue()

    def test_campaign_cannot_wait_on_legacy_work_it_must_itself_close(self):
        # The September audit found this exact deadlock: the ordinary
        # depends_on graph was acyclic, but SB-15 could never become ready.
        old = {"tasks": [card("BM-03")]}
        new = {
            "tasks": [card("SB-05"), card("SB-08"), card("SB-15", ["BM-03"])],
            "reconciliation": [{
                "legacy": "BM-03", "disposition": "executed_by",
                "by": ["SB-05", "SB-08", "SB-15"],
            }],
        }
        status, output = self.run_lint(old, new)
        self.assertEqual(status, 1)
        self.assertIn("dependency cycle: BM-03 -> SB-15 -> BM-03", output)

    def test_nested_replacements_cannot_close_through_their_consumer(self):
        cards = {name: card(name) for name in ("legacy", "middle", "consumer")}
        cards["consumer"]["depends_on"] = ["legacy"]
        reconciliation = {
            "legacy": {"disposition": "executed_by", "by": ["middle"]},
            "middle": {"disposition": "executed_by", "by": ["consumer"]},
        }
        self.assertEqual(PLAN.dependency_cycles(cards, reconciliation), [
            "dependency cycle: legacy -> middle -> consumer -> legacy"
        ])

    def test_reconciliation_gate_can_create_a_cycle(self):
        cards = {name: card(name) for name in ("legacy", "implementation", "gate")}
        cards["gate"]["depends_on"] = ["legacy"]
        for disposition in ("executed_by", "retained"):
            with self.subTest(disposition=disposition):
                reconciliation = {"legacy": {
                    "disposition": disposition, "by": ["implementation"],
                    "gate": ["gate"],
                }}
                self.assertEqual(PLAN.dependency_cycles(cards, reconciliation), [
                    "dependency cycle: legacy -> gate -> legacy"
                ])

    def test_retained_by_is_context_not_a_closure_dependency(self):
        old = {"tasks": [card("BM-03")]}
        new = {
            "tasks": [card("SB-15", ["BM-03"])],
            "reconciliation": [{
                "legacy": "BM-03", "disposition": "retained", "by": ["SB-15"],
            }],
        }
        status, output = self.run_lint(old, new)
        self.assertEqual(status, 0, output)

    def test_carried_card_uses_replacement_instead_of_obsolete_dependencies(self):
        # Unioning old and replacement edges would invent a cycle here.
        cards = {
            "legacy": card("legacy", ["consumer"]),
            "implementation": card("implementation"),
            "consumer": card("consumer", ["legacy"]),
        }
        reconciliation = {"legacy": {
            "disposition": "executed_by", "by": ["implementation"],
        }}
        self.assertEqual(PLAN.dependency_cycles(cards, reconciliation), [])

    def test_missing_replacement_still_fails_validation(self):
        old = {"tasks": [card("legacy")]}
        new = {"tasks": [], "reconciliation": [{
            "legacy": "legacy", "disposition": "executed_by", "by": ["missing"],
        }]}
        status, output = self.run_lint(old, new)
        self.assertEqual(status, 1)
        self.assertIn("reconciliation legacy by missing does not exist", output)

    def test_direct_dependencies_still_reject_cycles(self):
        cards = {"a": card("a", ["b"]), "b": card("b", ["a"])}
        self.assertEqual(PLAN.dependency_cycles(cards, {}), [
            "dependency cycle: a -> b -> a"
        ])


if __name__ == "__main__":
    unittest.main()
