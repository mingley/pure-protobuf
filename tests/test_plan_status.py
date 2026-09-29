"""Dependency and decision boundaries for the contributor queue."""

import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "plan_status", Path(__file__).resolve().parents[1] / "scripts/plan-status.py"
)
PLAN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PLAN)


class QueueTests(unittest.TestCase):
    def test_blocked_decision_is_not_ready_despite_completed_design(self):
        cards = {
            "design": {"id": "design", "status": "done"},
            "engine": {"id": "engine", "status": "blocked", "depends_on": ["design"]},
            "consumer": {"id": "consumer", "depends_on": ["engine"]},
        }
        self.assertEqual(PLAN.ready_cards(cards, {}), [])

    def test_reconciliation_resolves_transitively_and_honors_gates(self):
        cards = {key: {"id": key} for key in ("old", "middle", "leaf", "gate", "consumer")}
        cards["leaf"]["status"] = "done"
        cards["consumer"]["depends_on"] = ["old"]
        reconciliation = {
            "old": {"disposition": "executed_by", "by": ["middle"], "gate": ["gate"]},
            "middle": {"disposition": "executed_by", "by": ["leaf"]},
        }
        self.assertEqual([c["id"] for c in PLAN.ready_cards(cards, reconciliation)], ["gate"])
        cards["gate"]["status"] = "done"
        self.assertEqual([c["id"] for c in PLAN.ready_cards(cards, reconciliation)], ["consumer"])

    def test_retained_work_honors_reconciliation_gate(self):
        cards = {key: {"id": key} for key in ("legacy", "gate")}
        reconciliation = {"legacy": {"disposition": "retained", "gate": ["gate"]}}
        self.assertEqual([c["id"] for c in PLAN.ready_cards(cards, reconciliation)], ["gate"])

    def test_partial_implementation_does_not_unlock_claims(self):
        cards = {
            "measurement": {"id": "measurement", "status": "in_progress"},
            "claim": {"id": "claim", "depends_on": ["measurement"]},
        }
        self.assertEqual(PLAN.ready_cards(cards, {}), [])
        cards["measurement"]["status"] = "done"
        self.assertEqual([c["id"] for c in PLAN.ready_cards(cards, {})], ["claim"])


if __name__ == "__main__":
    unittest.main()
