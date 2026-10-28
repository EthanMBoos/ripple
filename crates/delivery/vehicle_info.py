#!/usr/bin/env python3
"""Summarize vehicle-info.sweep.yaml results as CSV (means per run).

    python3 crates/delivery/vehicle_info.py sweeps/vehicle-info/run000

Score differences are paired with full information using the same seed,
reply sample, model, and other recorded settings. These are descriptive
means, not significance tests. Refusal counts count calls, not requests.
"""

import argparse
import collections
import csv
import json
import statistics
import sys
from pathlib import Path

DISPATCHER = "entities.depot.autonomy.Dispatcher."
OMISSIONS = {DISPATCHER + "prompt.limits", DISPATCHER + "prompt.omit"}


def summarize(folder):
    groups = collections.defaultdict(dict)
    for line in (Path(folder) / "results.jsonl").read_text().splitlines():
        case = json.loads(line)
        if case["error"]:
            raise ValueError(f"{case['case_id']} failed: {case['error']}")
        parameters = case["parameters"]
        condition = (
            "+".join(sorted(parameters[DISPATCHER + "prompt.omit"])) or "full"
            if parameters[DISPATCHER + "prompt.limits"] else "all_hidden"
        )
        # Include all other recorded settings, so a different mission length
        # or added prompt sentence cannot silently become the baseline.
        key = (case["seed"], json.dumps(
            {k: v for k, v in parameters.items() if k not in OMISSIONS},
            sort_keys=True,
        ))
        model = parameters[DISPATCHER + "model"]
        runs = groups[model, condition]
        if key in runs:
            raise ValueError(f"Duplicate seed/sample/settings: {case['case_id']}")
        (values,) = [teams["1"] for teams in case["metrics"].values()]
        if values["requests"] == 0:
            raise ValueError(f"No requests in {case['case_id']}")
        runs[key] = values

    if not groups:
        raise ValueError("No results found")
    summary = []
    for (model, condition), runs in sorted(groups.items()):
        baseline = groups.get((model, "full"), {})
        if runs.keys() != baseline.keys():
            raise ValueError(f"{model}/{condition}: seed/sample/settings differ from full")
        values = list(runs.values())
        row = {
            "model": model,
            "omitted": condition,
            "runs": len(runs),
            "weighted_score": statistics.fmean(v["score"] for v in values),
            "paired_score_delta": statistics.fmean(
                v["score"] - baseline[k]["score"] for k, v in runs.items()
            ),
            "on_time_share": statistics.fmean(
                v["on_time"] / v["requests"] for v in values
            ),
            "undelivered": statistics.fmean(
                v["requests"] - v["delivered"] for v in values
            ),
        }
        for name in ("late", "proposed", "refused", "refused_wrong_skill",
                     "refused_out_of_range", "parse_failures", "vehicles_lost"):
            row[name] = statistics.fmean(v.get(name, 0) for v in values)
        summary.append(row)
    return summary


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sweep", type=Path)
    args = parser.parse_args()
    try:
        rows = summarize(args.sweep)
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, f"{error}\n")
    writer = csv.DictWriter(sys.stdout, fieldnames=list(rows[0]))
    writer.writeheader()
    writer.writerows(rows)
