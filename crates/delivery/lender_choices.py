#!/usr/bin/env python3
"""Compare cached compact-report choices with the spare rule on each prompt.

Read the frozen help-manager calls for the 16-, 64-, and 256-hub model
runs. The same demand seed can lead to different later offers under a
model and a rule, so this measures agreement on the offers the model
actually saw; it does not assign a score change to one disagreement.

    python3 crates/delivery/lender_choices.py
"""

from __future__ import annotations

import csv
import hashlib
import json
import re
import sys
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / "runs" / "llm-cache"
# SHA-256 prefix of the frozen system prompt in incident.yaml. The hide
# experiments changed that prompt and must not enter this comparison.
SYSTEM = "40eb781c"
MODELS = ("claude-haiku-5-5", "claude-opus-5-5")
SIZES = (16, 64, 256)

ASK = re.compile(r"^- hub (\d+): .*?\((\d+) trauma\).*?Offers, nearest first:")
OFFER = re.compile(r"^  - hub (\d+), .*?: (.*)\.$")
AIRCRAFT = re.compile(r"(\d+) (?:fixed-wing|multirotor) for \d+ of \d+ sites")
BACKLOG = re.compile(r"(\d+) waiting of its own")
ARRIVAL = re.compile(r"nearest site in (\d+) min")
NEW_HUBS = re.compile(r"near hubs ([0-9, ]+):")


def newly_affected(prompt):
    if "\nNew events:\n" not in prompt:
        return set()
    events = prompt.split("\nNew events:\n", 1)[1].split("\n\n", 1)[0]
    return {
        int(hub)
        for match in NEW_HUBS.finditer(events)
        for hub in match[1].split(", ")
    }


def offers_in(prompt):
    """Asking hub -> (trauma waiting, offers in the displayed order)."""
    requests = {}
    hub = None
    for line in prompt.splitlines():
        ask = ASK.match(line)
        if ask:
            hub = int(ask[1])
            requests[hub] = (int(ask[2]), [])
            continue
        offer = OFFER.match(line) if hub is not None else None
        if offer:
            facts = offer[2]
            aircraft = sum(int(n) for n in AIRCRAFT.findall(facts))
            backlog = BACKLOG.search(facts)
            arrival = ARRIVAL.search(facts)
            if not aircraft or backlog is None or arrival is None:
                raise ValueError(f"incomplete offer: {line}")
            requests[hub][1].append(
                (int(offer[1]), aircraft - int(backlog[1]), int(arrival[1]))
            )
            continue
        if line and not line.startswith("  "):
            hub = None
    return requests


def spare_choices(requests):
    """Mirror help::grant's ordering and shared-lender constraint."""
    chosen = {}
    granted = set()
    for hub in sorted(requests, key=lambda h: (-requests[h][0], h)):
        offers = [o for o in requests[hub][1] if o[0] not in granted]
        if not offers:
            continue
        lender, _, _ = min(offers, key=lambda o: (-o[1], o[2], o[0]))
        chosen[hub] = lender
        granted.add(lender)
    return chosen


def model_choices(reply):
    try:
        data = json.loads(reply)
    except json.JSONDecodeError:
        return None
    choices = {}
    for order in data.get("orders", []):
        help_order = order.get("help")
        if not isinstance(help_order, dict):
            continue
        hub, lender = help_order.get("hub"), help_order.get("from_hub")
        if isinstance(hub, int) and isinstance(lender, int):
            choices.setdefault(hub, lender)
    return choices


def model_calls():
    for path in CACHE.glob("*.json"):
        call = json.loads(path.read_text())
        if call.get("model") not in MODELS:
            continue
        system = call.get("system", "")
        if hashlib.sha256(system.encode()).hexdigest()[:8] != SYSTEM:
            continue
        prompt = call.get("prompt", "")
        if "You see new surges" not in prompt:
            continue
        if "spare over its own work" in prompt:
            continue  # the extra-number pilot at 64 hubs
        size = next((n for n in SIZES if f"across {n} hubs" in prompt), None)
        if size is not None:
            yield size, call["model"], prompt, call.get("reply", "")


def main():
    stats = Counter()
    for size, model, prompt, reply in model_calls():
        key = (size, model)
        stats[key, "calls"] += 1
        requests = offers_in(prompt)
        rule = spare_choices(requests)
        new_hubs = newly_affected(prompt)
        chosen = model_choices(reply)
        if chosen is None:
            stats[key, "unreadable"] += 1
            continue
        for hub, lender in rule.items():
            if hub not in new_hubs:
                continue
            offers = requests[hub][1]
            if len(offers) < 2:
                continue
            stats[key, "choices"] += 1
            if chosen.get(hub) == lender:
                stats[key, "matches"] += 1
            if offers[0][0] != lender:
                stats[key, "nearest_differs"] += 1
                if chosen.get(hub) == lender:
                    stats[key, "matches_when_nearest_differs"] += 1
        stats[key, "rule_grants"] += len(set(rule) & new_hubs)
        stats[key, "model_grants"] += len(set(chosen) & new_hubs)
    fields = (
        "hubs", "model", "calls", "choices", "matches", "nearest_differs",
        "matches_when_nearest_differs", "rule_grants", "model_grants",
        "unreadable_calls",
    )
    writer = csv.DictWriter(sys.stdout, fieldnames=fields, lineterminator="\n")
    writer.writeheader()
    for size in SIZES:
        for model in MODELS:
            key = (size, model)
            writer.writerow({
                "hubs": size,
                "model": model,
                **{
                    field: stats[key, field]
                    for field in fields[2:-1]
                },
                "unreadable_calls": stats[key, "unreadable"],
            })


if __name__ == "__main__":
    main()
