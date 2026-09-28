#!/usr/bin/env python3
"""Spatial lookup changes no output: previous revision against scan and grid.

    python3 paper/data/spatial-lookup/equivalence.py OLD_RUNNER NEW_RUNNER OUTPUT.json \
        --old-revision b280a82 --new-revision "working tree"

OLD_RUNNER is reference/benchmark_runner.rs built at a revision before spatial
queries (for example from `git archive b280a82`); NEW_RUNNER is the current build.
Runs every mission file under missions/ (not sweeps), perf.py's workloads, and
three crowded collision missions. Each runs once with the old runner at one
worker, then with the new runner using the scan at 1, 2, and 8 workers and
grids of several cell widths (the runner's optional cell-width argument). Every
new run's frames, summary, and events must be byte-identical to the old run's.
Writes a report and exits non-zero on any difference. Standard library only.
"""

import argparse
import datetime
import filecmp
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "reference"))
import perf  # noqa: E402

FILES = ("frames.bin", "summary.csv", "events.json")
# (label, workers, grid cell width in metres or None for the scan)
VARIANTS = [("scan-w1", 1, None), ("scan-w2", 2, None), ("scan-w8", 8, None),
            ("grid5-w1", 1, 5), ("grid5-w8", 8, 5), ("grid1-w1", 1, 1),
            ("grid50-w2", 2, 50), ("grid1000-w1", 1, 1000)]
CROWDED_VARIANTS = [("scan-w8", 8, None), ("grid5-w1", 1, 5), ("grid5-w8", 8, 5),
                    ("grid0.7-w2", 2, 0.7), ("grid40-w1", 1, 40)]
# Crowded collision missions: (position variance m^2, agents).
CROWDED = [(400, 1024), (2500, 2048), (100, 512)]


def missions(scratch):
    """(name, path, variants) for every mission compared."""
    found = [(str(path.relative_to(ROOT)), path, VARIANTS)
             for path in sorted((ROOT / "missions").rglob("*"))
             if path.suffix in (".xml", ".yaml") and "sweep" not in path.name]
    for name, (_, sizes, fields) in perf.WORKLOADS.items():
        if "grid_cell_m" in fields:
            continue  # the same mission as `collision`; the grid comes from the runner
        for agents in sizes:
            path = scratch / f"perf-{name}-{agents}.xml"
            path.write_text(perf.HEADER.format(name=name, agents=agents, **fields))
            found.append((f"perf {name}-{agents}", path, VARIANTS))
    for variance, agents in CROWDED:
        fields = {**perf.WORKLOADS["collision"][2], "variance": variance}
        path = scratch / f"crowded-{variance}-{agents}.xml"
        path.write_text(perf.HEADER.format(name="crowded", agents=agents, **fields))
        found.append((f"crowded variance {variance} m^2, {agents} agents", path,
                      CROWDED_VARIANTS))
    return found


def run(runner, mission, output, workers, grid):
    command = [runner, mission, ROOT, output, str(workers), *([] if grid is None else [str(grid)])]
    subprocess.run(command, capture_output=True, text=True, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("old_runner")
    parser.add_argument("new_runner")
    parser.add_argument("output", type=Path)
    parser.add_argument("--old-revision", required=True)
    parser.add_argument("--new-revision", required=True)
    args = parser.parse_args()

    results, differing = [], 0
    with tempfile.TemporaryDirectory() as scratch:
        scratch = Path(scratch)
        for index, (name, path, variants) in enumerate(missions(scratch)):
            base = scratch / f"{index}-old"
            run(args.old_runner, path, base, 1, None)
            events = json.loads((base / "events.json").read_text())
            collisions = sum(event["kind"] in ("TeamCollision", "NonTeamCollision")
                             for event in events)
            different = []
            for label, workers, grid in variants:
                output = scratch / f"{index}-{label}"
                run(args.new_runner, path, output, workers, grid)
                different += [f"{label}:{file}" for file in FILES
                              if not filecmp.cmp(base / file, output / file, shallow=False)]
            differing += bool(different)
            results.append({"mission": name, "collisions": collisions,
                            "variants": [label for label, _, _ in variants],
                            "identical": not different, "differences": different})
            print(f"{'same' if not different else 'DIFF'} {name} ({collisions} collisions)"
                  f"{' ' + ' '.join(different) if different else ''}", flush=True)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({
        "date": datetime.datetime.now().isoformat(timespec="seconds"),
        "machine": perf.machine(), "old_revision": args.old_revision,
        "new_revision": args.new_revision, "files_compared": FILES,
        "missions": len(results), "missions_differing": differing, "results": results,
    }, indent=1) + "\n")
    print(f"{len(results)} missions, {differing} differing")
    return 1 if differing else 0


if __name__ == "__main__":
    raise SystemExit(main())
