#!/usr/bin/env python3
"""Show the current work queue without copying task status into Markdown."""

import argparse
from collections import Counter
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PLAN_PATHS = ("docs/plan/tasks.json", "docs/plan/world-class/tasks.json")


def read_plans():
    return [json.loads((ROOT / path).read_text()) for path in PLAN_PATHS]


def index_plans(plans):
    cards = {}
    reconciliation = {}
    for plan in plans:
        for task in plan["tasks"]:
            card = {**plan.get("task_defaults", {}), **task}
            cards[card["id"]] = card
        reconciliation.update(
            (entry["legacy"], entry) for entry in plan.get("reconciliation", [])
        )
    return cards, reconciliation


def dependency_state(cards, reconciliation):
    carried = {
        key for key, entry in reconciliation.items()
        if entry["disposition"] == "executed_by"
    }
    done = {
        key for key, card in cards.items()
        if card.get("status", "pending") in ("done", "superseded")
        and key not in carried
    }
    # Reconciled legacy cards can depend on other reconciled cards.
    while True:
        newly_done = {
            key for key in carried - done
            if all(dep in done for dep in (
                reconciliation[key].get("by", [])
                + reconciliation[key].get("gate", [])
            ))
        }
        if not newly_done:
            return done, carried
        done.update(newly_done)


def open_dependencies(card, reconciliation, done):
    needs = card.get("depends_on", []) + reconciliation.get(
        card["id"], {}
    ).get("gate", [])
    return [dep for dep in needs if dep not in done]


def ready_cards(cards, reconciliation):
    done, carried = dependency_state(cards, reconciliation)
    return [
        card for key, card in cards.items()
        if card.get("status", "pending") == "pending"
        and key not in carried
        and not open_dependencies(card, reconciliation, done)
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    view = parser.add_mutually_exclusive_group()
    view.add_argument("--ready", action="store_true", help="unclaimed cards with closed dependencies")
    view.add_argument("--card", metavar="ID", help="one card, defaults and open dependencies")
    view.add_argument("--summary", action="store_true", help="status counts (the default)")
    view.add_argument("--lane", metavar="PREFIX", help="all cards in a lane, e.g. RX or QG")
    args = parser.parse_args()
    plans = read_plans()
    cards, reconciliation = index_plans(plans)
    done, carried = dependency_state(cards, reconciliation)
    if args.card:
        if args.card not in cards:
            parser.error(f"unknown card: {args.card}")
        card = cards[args.card]
        print(json.dumps({
            **card,
            "open_dependencies": open_dependencies(card, reconciliation, done),
            "reconciliation": reconciliation.get(args.card),
        }, indent=2))
    elif args.ready or args.lane:
        rows = ready_cards(cards, reconciliation) if args.ready else [
            card for card in cards.values()
            if card["id"].split("-", 1)[0] == args.lane.upper()
        ]
        for card in sorted(rows, key=lambda row: (row.get("priority", "P1"), row["id"])):
            pending = open_dependencies(card, reconciliation, done)
            detail = card.get("blocked_reason") or (
                "needs " + ", ".join(pending) if pending else ""
            )
            print(f'{card["id"]:8} {card.get("priority", "P1")} '
                  f'{card.get("status", "pending"):11} {card["title"]}'
                  + (f" [{detail}]" if detail else ""))
        if args.ready:
            print("\nReady means dependencies are closed. Read the card and decision records before starting.")
    else:
        for path, plan in zip(PLAN_PATHS, plans):
            counts = Counter(cards[task["id"]].get("status", "pending") for task in plan["tasks"])
            print(f"{path}: {len(plan['tasks'])} cards")
            print("  " + ", ".join(f"{key}={value}" for key, value in sorted(counts.items())))
        print(f"{len(ready_cards(cards, reconciliation))} ready; {len(carried)} legacy cards carried by newer work.")
        print("Counts describe task records, not production or performance qualification.")


if __name__ == "__main__":
    main()
