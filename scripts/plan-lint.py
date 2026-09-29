#!/usr/bin/env python3
"""Lint the legacy and world-class task plans.

Checks, across docs/plan/tasks.json and docs/plan/world-class/tasks.json:
  - every depends_on, gate, relates, and reconciliation id exists
  - no effective dependency cycles, including executed_by reconciliation
  - every named check exists in one of the files' checks maps
  - required card fields are present and status values are known

Usage: python3 scripts/plan-lint.py
Exit 0 when the plans are consistent, 1 with errors on stdout otherwise.
"""

import json
import sys

OLD = "docs/plan/tasks.json"
NEW = "docs/plan/world-class/tasks.json"
REQUIRED = ("id", "title", "depends_on")
STATUSES = {"pending", "in_progress", "done", "superseded", "blocked"}


def load(path):
    with open(path) as fh:
        return json.load(fh)


def dependency_cycles(cards, reconciliation):
    """Find cycles in the dependency graph used to close queue prerequisites.

    A carried legacy card closes through its replacement cards and gates,
    not its obsolete depends_on list (see plan-status.py dependency_state).
    Retained cards keep depends_on and gates; their by list is explanatory.
    """
    edges = {}
    for cid, task in cards.items():
        entry = reconciliation.get(cid, {})
        dependencies = (
            entry.get("by", [])
            if entry.get("disposition") == "executed_by"
            else task.get("depends_on", [])
        )
        edges[cid] = list(dependencies) + entry.get("gate", [])

    errors = []
    color = {}
    stack = []

    def visit(node):
        color[node] = 1
        stack.append(node)
        for dep in edges[node]:
            if dep not in edges:
                continue  # Missing references are reported by main's validation.
            state = color.get(dep, 0)
            if state == 1:
                cycle = stack[stack.index(dep):] + [dep]
                errors.append("dependency cycle: " + " -> ".join(cycle))
            elif state == 0:
                visit(dep)
        stack.pop()
        color[node] = 2

    for cid in edges:
        if color.get(cid, 0) == 0:
            visit(cid)
    return errors


def main():
    errors = []
    old = load(OLD)
    new = load(NEW)
    cards = {}
    for plan in (old, new):
        for task in plan["tasks"]:
            if task["id"] in cards:
                errors.append(f"duplicate card id {task['id']}")
            cards[task["id"]] = task
    checks = {}
    for plan in (old, new):
        checks.update(plan.get("checks", {}))
    planned = set(new.get("planned_checks", {}))

    rec = {
        entry["legacy"]: entry
        for plan in (old, new)
        for entry in plan.get("reconciliation", [])
    }
    for legacy, entry in rec.items():
        if legacy not in cards:
            errors.append(f"reconciliation legacy {legacy} does not exist")
        for key in ("by", "gate"):
            for ref_id in entry.get(key, []):
                if ref_id not in cards:
                    errors.append(
                        f"reconciliation {legacy} {key} {ref_id} does not exist"
                    )
        if entry.get("disposition") not in ("executed_by", "retained"):
            errors.append(
                f"reconciliation {legacy} has bad disposition "
                f"{entry.get('disposition')!r}"
            )

    for cid, task in cards.items():
        for field in REQUIRED:
            if field not in task:
                errors.append(f"{cid} is missing field {field}")
        status = task.get("status", "pending")
        if status not in STATUSES:
            errors.append(f"{cid} has bad status {status!r}")
        deps = list(task.get("depends_on", []))
        deps += rec.get(cid, {}).get("gate", [])
        for dep in deps:
            if dep not in cards:
                errors.append(f"{cid} depends on missing {dep}")
        for rel in task.get("relates", []):
            if rel not in cards:
                errors.append(f"{cid} relates missing {rel}")
        for check in task.get("checks", []):
            if check not in checks and check not in planned:
                errors.append(f"{cid} cites unknown check {check}")

    errors.extend(dependency_cycles(cards, rec))

    if errors:
        print(f"{len(errors)} plan error(s):")
        for err in sorted(set(errors)):
            print(f"  - {err}")
        return 1
    print(f"plans OK: {len(cards)} cards, {len(checks)} checks, no cycles")
    return 0


if __name__ == "__main__":
    sys.exit(main())
