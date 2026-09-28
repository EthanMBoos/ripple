#!/usr/bin/env python3
"""Collision checking with the every-pair loop, the scan, and the grid.

    python3 paper/data/spatial-lookup/benchmark.py OLD_RUNNER NEW_RUNNER OUTPUT.json \
        --old-revision b280a82 --new-revision "working tree"

OLD_RUNNER is reference/benchmark_runner.rs built at a revision before spatial
queries (SimpleCollision's own every-pair loop); NEW_RUNNER is the current build,
run with the default scan and with grids (its optional cell-width argument).
Each case is perf.py's collision mission with a different spread, collision
range, or agent count, on one worker (the collision check is a sequential
phase). Variants alternate run by run, in a rotating order, after a warm-up
round, so machine warm-up affects all equally. "none" is the same mission
without SimpleCollision; a variant's time minus it estimates collision's
share (only an estimate: without collisions no agents are removed, and spawn
placement may differ). Every variant of a case except "none" must write
identical frames and events; the script stops if not. Reports medians, ranges, raw times, and peak memory.
Standard library only.
"""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "reference"))
import perf  # noqa: E402

SPREAD = 1_000_000  # position variance, m^2: perf.py's collision workload
CLUSTERED = 400     # 20 m standard deviation: agents crowd into a few cells

# name: (agents, position variance, collision range m, variants). A variant is
# "none", "old", "scan", or a grid cell width in metres.
CASES = {
    "sparse-256": (256, SPREAD, 5, ["none", "old", "scan", 5]),
    "sparse-1024": (1024, SPREAD, 5, ["none", "old", "scan", 5]),
    "sparse-2048": (2048, SPREAD, 5, ["none", "old", "scan", 5]),
    "sparse-4096": (4096, SPREAD, 5, ["none", "old", "scan", 2.5, 5, 10, 20, 50]),
    "clustered-1024": (1024, CLUSTERED, 5, ["old", "scan", 5]),
    "clustered-4096": (4096, CLUSTERED, 5, ["old", "scan", 5]),
    "large-range-4096": (4096, SPREAD, 50, ["old", "scan", 5, 50]),
}


def mission_text(agents, variance, range_m, collide):
    fields = {**perf.WORKLOADS["collision"][2], "variance": variance}
    fields["world"] = (f'  <entity_interaction collision_range="{range_m}">SimpleCollision'
                       '</entity_interaction>') if collide else ""
    return perf.HEADER.format(name="spatial", agents=agents, **fields)


def run(runner, mission, output, grid):
    """(seconds stepping, steps, peak MB, collisions, output hash) for one run."""
    shutil.rmtree(output, ignore_errors=True)
    command = [runner, mission, ROOT, output, "1", *([] if grid is None else [str(grid)])]
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    _, status, usage = os.wait4(process.pid, 0)
    stdout, stderr = process.communicate()
    if status:
        raise RuntimeError(f"{command} failed: {stderr.strip()[-300:]}")
    result = json.loads(stdout.strip().splitlines()[-1])
    events = (output / "events.json").read_bytes()
    collisions = sum(event["kind"] in ("TeamCollision", "NonTeamCollision")
                     for event in json.loads(events))
    digest = hashlib.sha256((output / "frames.bin").read_bytes() + events).hexdigest()
    scale = 1 if platform.system() == "Darwin" else 1024  # ru_maxrss: bytes on macOS
    return result["run_s"], result["steps"], usage.ru_maxrss * scale / 1e6, collisions, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("old_runner")
    parser.add_argument("new_runner")
    parser.add_argument("output", type=Path)
    parser.add_argument("--old-revision", required=True)
    parser.add_argument("--new-revision", required=True)
    parser.add_argument("--repeats", type=int, default=5)
    args = parser.parse_args()

    results = {}
    with tempfile.TemporaryDirectory() as scratch:
        scratch = Path(scratch)
        for name, (agents, variance, range_m, variants) in CASES.items():
            missions = {collide: scratch / f"{name}-{collide}.xml" for collide in (True, False)}
            for collide, path in missions.items():
                path.write_text(mission_text(agents, variance, range_m, collide))
            samples = {str(variant): [] for variant in variants}
            case = {"agents": agents, "position_variance_m2": variance,
                    "collision_range_m": range_m, "variants": {}}
            digests = set()
            for repeat in range(args.repeats + 1):
                shift = repeat % len(variants)
                for variant in variants[shift:] + variants[:shift]:
                    runner = args.old_runner if variant == "old" else args.new_runner
                    grid = variant if isinstance(variant, (int, float)) else None
                    seconds, steps, peak_mb, collisions, digest = run(
                        runner, missions[variant != "none"], scratch / "run", grid)
                    if variant != "none":
                        digests.add(digest)
                        case["collisions"] = collisions
                    if repeat:  # the first round is a warm-up
                        samples[str(variant)].append((seconds, peak_mb))
            if len(digests) != 1:
                raise RuntimeError(f"{name}: variants wrote different output")
            for variant, runs in samples.items():
                times = [seconds for seconds, _ in runs]
                case["variants"][variant] = {
                    "median_ms_per_step": statistics.median(times) / steps * 1e3,
                    "min_ms_per_step": min(times) / steps * 1e3,
                    "max_ms_per_step": max(times) / steps * 1e3,
                    "peak_mb": round(max(mb for _, mb in runs), 1),
                    "run_s": times,
                }
            case["steps"] = steps
            results[name] = case
            print(name, "  ".join(f"{variant} {value['median_ms_per_step']:.3f}"
                                  for variant, value in case["variants"].items()),
                  "ms/step", flush=True)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({
        "date": datetime.datetime.now().isoformat(timespec="seconds"),
        "machine": perf.machine(), "platform": platform.platform(),
        "old_revision": args.old_revision, "new_revision": args.new_revision,
        "repeats": args.repeats, "workers": 1,
        "note": "Every variant of each case except none wrote identical frames and events.",
        "cases": results,
    }, indent=1) + "\n")


if __name__ == "__main__":
    main()
