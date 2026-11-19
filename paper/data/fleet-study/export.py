#!/usr/bin/env python3
"""Print the per-day metrics behind the delivery paper as CSV.

Run from the repository root after the listed sweeps have finished:

    python3 paper/data/fleet-study/export.py \
        > paper/data/fleet-study/results.csv
    python3 paper/data/fleet-study/export.py --hubs \
        > paper/data/fleet-study/hubs.csv
    python3 paper/data/fleet-study/export.py --refresh-luna-hubs
    python3 paper/data/fleet-study/export.py --refresh-luna-results
The refresh commands replace only Luna's rows in the named retained CSV,
preserving other campaigns when their raw folders are not available locally.
`--hubs` writes one row per hub per day for the sweeps behind Figure 2,
with the manager settings that name the arm, so the figure can be redrawn
without the campaign folders.

Each source/case_id pair identifies a case in its saved campaign. Use the
campaign's configuration, since shipped sweeps may change. The source
JSONL files and model replies stay under ignored runs/ and
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
    ("region-256-return", "run000"),
    ("region-1024-return", "run000"),
    ("region-16-models", "run000"),
    ("region-64-models", "run000"),
    ("region-64-models-seed3", "run000"),
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
# Twelve fresh API runs used by Figures 2 and 3 and the retained outcomes.
LUNA_CASES = {
    ("region-16-luna-api", "run003"): {f"case-{i:04}" for i in range(6)},
    ("region-64-luna-api", "run004"): {f"case-{i:04}" for i in range(6)},
}
LUNA_HUB_SOURCES = list(LUNA_CASES)
# The sweeps Figure 2 reads, per hub.
HUB_SOURCES = [
    ("region-16-fixed", "run000"),
    ("region-16-models", "run000"),
    ("region-64-fixed", "run000"),
    ("region-64-models", "run000"),
    ("region-64-models-seed3", "run000"),
] + LUNA_HUB_SOURCES
MANAGER = "entities.region.autonomy.Dispatcher.manager."
HUB_COLUMNS = ("source", "case_id", "seed", "manager", "policy", "model", "hub",
               "requests", "on_time", "late", "undelivered", "gave", "got")
METRICS = (
    "score", "requests", "on_time", "late", "undelivered",
    "moves", "vehicles_lost", "orders", "orders_applied",
    "orders_refused", "applied_reposition", "transfers",
    "calls", "prompt_chars",
)


def selected_runs(sweep, run):
    source = Path("sweeps") / sweep / run / "results.jsonl"
    rows = [json.loads(line) for line in (ROOT / source).read_text().splitlines()]
    selected = LUNA_CASES.get((sweep, run))
    if selected is not None:
        rows = [row for row in rows if row["case_id"] in selected]
        if len(rows) != len(selected) or {r["case_id"] for r in rows} != selected:
            raise ValueError(f"{source}: missing or duplicate selected Luna cases")
    elif sweep in ("region-16-models", "region-64-models"):
        rows = [row for row in rows if row["parameters"][
            "entities.region.autonomy.Dispatcher.manager.model"] != "gpt-6-luna"]
    for row in rows:
        if row["error"] is not None:
            raise ValueError(f"{source}: {row['case_id']}: {row['error']}")
    return source, rows


def hub_rows(source, row):
    """One row per hub: the day, its manager settings, and that hub's counts."""
    config = row["parameters"]
    day = next(iter(row["metrics"]["metrics/DeliveryScore:0"].values()))
    hubs = {}
    for key, value in day.items():
        if key.startswith("hub_"):
            _, hub, metric = key.split("_", 2)
            hubs.setdefault(int(hub), {})[metric] = value
    for hub, counts in sorted(hubs.items()):
        yield {
            "source": source,
            "case_id": row["case_id"],
            "seed": row["seed"],
            "manager": config.get(MANAGER + "kind", ""),
            "policy": Path(config.get(MANAGER + "policy", "")).stem,
            "model": config.get(MANAGER + "model", ""),
            "hub": hub,
            **counts,
        }


def write_hubs():
    writer = csv.DictWriter(sys.stdout, fieldnames=HUB_COLUMNS, lineterminator="\n")
    writer.writeheader()
    for sweep, run in HUB_SOURCES:
        source, rows = selected_runs(sweep, run)
        for row in rows:
            writer.writerows(hub_rows(source, row))


def refresh_luna_hubs():
    """Replace Luna's hub rows without needing the other raw campaigns."""
    added_hubs = []
    for sweep, run in LUNA_HUB_SOURCES:
        source, rows = selected_runs(sweep, run)
        if len(rows) != 6 or {(r["seed"], r["parameters"][MANAGER + "kind"])
                              for r in rows} != {
                (seed, role) for seed in (1, 2, 3) for role in ("model", "veto")}:
            raise ValueError(f"{source}: expected three seeds in both roles")
        for row in rows:
            added_hubs.extend(hub_rows(source, row))
    path = Path(__file__).resolve().with_name("hubs.csv")
    with path.open() as stream:
        retained = [row for row in csv.DictReader(stream)
                    if row["model"] != "gpt-6-luna"]
    with path.open("w") as stream:
        writer = csv.DictWriter(stream, fieldnames=HUB_COLUMNS, lineterminator="\n")
        writer.writeheader()
        writer.writerows(retained)
        writer.writerows(added_hubs)
    print(f"hubs.csv: retained {len(retained)} rows, added {len(added_hubs)} Luna rows")


def refresh_luna_results():
    """Replace Luna's day-level outcomes, preserving other retained studies."""
    added = []
    for sweep, run in LUNA_CASES:
        source, rows = selected_runs(sweep, run)
        for row in rows:
            metrics = row["metrics"]["metrics/DeliveryScore:0"]["1"]
            added.append({
                "source": str(source), "case_id": row["case_id"],
                "seed": row["seed"],
                **{name: metrics.get(name, "") for name in METRICS},
            })
    path = Path(__file__).resolve().with_name("results.csv")
    with path.open() as stream:
        retained = [row for row in csv.DictReader(stream)
                    if "luna-api" not in row["source"]]
    with path.open("w") as stream:
        writer = csv.DictWriter(stream, fieldnames=("source", "case_id", "seed", *METRICS),
                                lineterminator="\n")
        writer.writeheader()
        writer.writerows(retained)
        writer.writerows(added)
    print(f"results.csv: retained {len(retained)} rows, added {len(added)} Luna rows")


def main():
    if sys.argv[1:] == ["--refresh-luna-results"]:
        refresh_luna_results()
        return
    if sys.argv[1:] == ["--refresh-luna-hubs"]:
        refresh_luna_hubs()
        return
    if sys.argv[1:] == ["--hubs"]:
        write_hubs()
        return
    writer = csv.DictWriter(
        sys.stdout, fieldnames=("source", "case_id", "seed", *METRICS),
        lineterminator="\n",
    )
    writer.writeheader()
    for sweep, run in [*SOURCES, *LUNA_CASES]:
        source, rows = selected_runs(sweep, run)
        for row in rows:
            metrics = next(iter(row["metrics"]["metrics/DeliveryScore:0"].values()))
            writer.writerow({
                "source": source,
                "case_id": row["case_id"],
                "seed": row["seed"],
                **{name: metrics.get(name, "") for name in METRICS},
            })


if __name__ == "__main__":
    main()
