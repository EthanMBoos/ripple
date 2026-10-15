#!/usr/bin/env python3
"""Collision checking with the spatial scan and with grids, in the current build.

    python3 reference/spatial_benchmark.py --output runs/spatial-benchmark000

The scan tests every contact, the reference answer; a grid tests only contacts
in nearby cells (the runner's optional cell-width argument). Each case is
perf.py's collision mission with a different spread, collision range, or agent
count, on one worker (the collision check is a sequential phase). Variants
alternate run by run, in a rotating order, after a warm-up round, so machine
warm-up affects all equally. "none" is the same mission without
SimpleCollision; a variant's time minus it estimates collision's share (only an
estimate: without collisions no agents are removed, and spawn placement may
differ). Every variant of a case except "none" must write identical frames and
events; the script stops if not. Writes OUTPUT/timings.json with medians,
ranges, raw times, and peak memory. Standard library only.
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

import perf
from project import ROOT, build_runner, source_identity

SPREAD = 1_000_000  # position variance, m^2: perf.py's collision workload
CLUSTERED = 400     # 20 m standard deviation: agents crowd into a few cells

# name: (agents, position variance, collision range m, variants). A variant is
# "none", "scan", or a grid cell width in metres.
CASES = {
    "sparse-256": (256, SPREAD, 5, ["none", "scan", 5]),
    "sparse-1024": (1024, SPREAD, 5, ["none", "scan", 5]),
    "sparse-2048": (2048, SPREAD, 5, ["none", "scan", 5]),
    "sparse-4096": (4096, SPREAD, 5, ["none", "scan", 2.5, 5, 10, 20, 50]),
    "clustered-1024": (1024, CLUSTERED, 5, ["scan", 5, 50]),
    "clustered-4096": (4096, CLUSTERED, 5, ["scan", 5]),
    "large-range-4096": (4096, SPREAD, 50, ["scan", 5, 50]),
}


def mission(agents, variance, range_m, collide):
    interactions = {"SimpleCollision": {"collision_range": range_m}} if collide else {}
    return perf.workload("collision", agents, variance=variance, interactions=interactions)


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


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    parser.add_argument("--repeats", type=int, default=5)
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True)
    runner = build_runner()

    results = {}
    scratch = output / "scratch"
    scratch.mkdir()
    for name, (agents, variance, range_m, variants) in CASES.items():
        missions = {collide: perf.write_mission(scratch / f"{name}-{collide}.yaml",
                                                mission(agents, variance, range_m, collide))
                    for collide in (True, False)}
        samples = {str(variant): [] for variant in variants}
        case = {"agents": agents, "position_variance_m2": variance,
                "collision_range_m": range_m, "variants": {}}
        digests = set()
        for repeat in range(args.repeats + 1):
            shift = repeat % len(variants)
            for variant in variants[shift:] + variants[:shift]:
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

    shutil.rmtree(scratch)
    (output / "timings.json").write_text(json.dumps({
        "date": datetime.date.today().isoformat(), "platform": platform.platform(),
        "machine": perf.environment()["machine"], "rustc": perf.environment()["rustc"],
        "source_sha256": source_identity()["source_sha256"],
        "repeats": args.repeats, "workers": 1,
        "note": "Every variant of each case except none wrote identical frames and events.",
        "cases": results,
    }, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
