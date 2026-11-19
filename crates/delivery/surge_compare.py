#!/usr/bin/env python3
"""Compare informed model managers with fixed rules on the same surge days.

    python3 crates/delivery/surge_compare.py \
        sweeps/region-surge-length/run001/results.jsonl \
        sweeps/region-surge-informed/run000/results.jsonl \
        sweeps/region-surge-informed-240/run000/results.jsonl

The fixed sweep covers five seeds at each duration. Print a row only when
all five corresponding model days finished without an error.
"""

from __future__ import annotations

import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path


PREFIX = "entities.region.autonomy.Dispatcher."


def read(path):
    days = {}
    for line in Path(path).read_text().splitlines():
        row = json.loads(line)
        if row["error"] is not None:
            raise ValueError(f"{path}: {row['case_id']}: {row['error']}")
        config = row["parameters"]
        length = config[PREFIX + "events.surge_window_min"]
        manager = config[PREFIX + "manager.kind"]
        if manager == "model":
            arm = config[PREFIX + "manager.model"].removeprefix("claude-")
        elif manager == "policy":
            arm = Path(config[PREFIX + "manager.policy"]).stem
        else:
            arm = manager
        metrics = next(iter(row["metrics"]["metrics/DeliveryScore:0"].values()))
        key = (length, row["seed"], arm)
        if key in days:
            raise ValueError(f"duplicate day {key} in {path}")
        days[key] = metrics
    return days


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    fixed = read(sys.argv[1])
    models = {}
    for path in sys.argv[2:]:
        for key, metrics in read(path).items():
            if key in models:
                raise ValueError(f"duplicate model day {key}")
            models[key] = metrics
    grouped = defaultdict(dict)
    for (length, seed, arm), metrics in models.items():
        grouped[(length, arm)][seed] = metrics
    print("length model days score delta_local delta_return moves lost wins_local")
    for (length, arm), by_seed in sorted(grouped.items()):
        if len(by_seed) != 5:
            continue
        seeds = sorted(by_seed)
        local = [fixed[length, seed, "local"]["score"] for seed in seeds]
        returned = [fixed[length, seed, "return"]["score"] for seed in seeds]
        scores = [by_seed[seed]["score"] for seed in seeds]
        moves = [by_seed[seed].get("moves", 0) for seed in seeds]
        lost = [by_seed[seed].get("vehicles_lost", 0) for seed in seeds]
        print(
            length, arm, len(seeds),
            f"{statistics.mean(scores):.4f}",
            f"{statistics.mean(a - b for a, b in zip(scores, local)):+.4f}",
            f"{statistics.mean(a - b for a, b in zip(scores, returned)):+.4f}",
            f"{statistics.mean(moves):.1f}",
            f"{sum(lost):.0f}",
            sum(a > b for a, b in zip(scores, local)),
        )


if __name__ == "__main__":
    main()
