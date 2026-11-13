#!/usr/bin/env python3
"""Print the per-day metrics behind the delivery paper as CSV.

Run from the repository root after the listed sweeps have finished:

    python3 paper/data/fleet-study/export.py \
        > paper/data/fleet-study/results.csv

Each source/case_id pair maps to a case in the matching committed sweep
file. The source JSONL files and model replies stay under ignored runs/ and
sweeps/; this compact extract carries the reported per-day outcomes.
"""

from __future__ import annotations

import csv
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
SOURCES = [
    (f"region-{hubs}-fixed", "run000")
    for hubs in (4, 16, 64, 256, 1024)
] + [
    ("region-16-models", "run000"),
    ("region-64-models", "run000"),
    ("region-surge-length", "run001"),
    ("region-guarded-replanner", "run000"),
    ("region-surge-informed", "run000"),
    ("region-surge-informed-240", "run000"),
    ("incident-gate", "run000"),
    ("incident-gate-64", "run000"),
    ("incident-gate-256", "run003"),
    ("incident-random-16", "run000"),
    ("incident-random-64", "run000"),
    ("incident-random-256", "run001"),
    ("incident-models-16", "run000"),
    ("incident-models-64", "run000"),
    ("incident-models-256", "run001"),
    ("incident-hide-16", "run000"),
]
METRICS = (
    "score", "requests", "on_time", "late", "undelivered",
    "moves", "vehicles_lost", "orders", "orders_applied",
    "orders_refused", "applied_reposition", "transfers",
    "calls", "prompt_chars",
)


def main():
    writer = csv.DictWriter(
        sys.stdout, fieldnames=("source", "case_id", "seed", *METRICS),
        lineterminator="\n",
    )
    writer.writeheader()
    for sweep, run in SOURCES:
        source = Path("sweeps") / sweep / run / "results.jsonl"
        path = ROOT / source
        if not path.exists():
            raise FileNotFoundError(path)
        for line in path.read_text().splitlines():
            row = json.loads(line)
            if row["error"] is not None:
                raise ValueError(f"{source}: {row['case_id']}: {row['error']}")
            metrics = next(iter(row["metrics"]["metrics/DeliveryScore:0"].values()))
            writer.writerow({
                "source": source,
                "case_id": row["case_id"],
                "seed": row["seed"],
                **{name: metrics.get(name, "") for name in METRICS},
            })


if __name__ == "__main__":
    main()
