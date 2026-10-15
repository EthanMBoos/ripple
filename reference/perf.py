#!/usr/bin/env python3
"""Rust speed and memory against the accepted baseline, on this machine.

    python3 reference/perf.py             # measure; compare with reference/perf_baseline.json
    python3 reference/perf.py --quick     # smallest agent counts only, about a minute
    python3 reference/perf.py --accept    # measure and make this the accepted baseline

Runs a fixed set of workloads natively (release build, core-only runner
reference/benchmark_runner.rs, no viewer), each at a few agent counts and at 1
and 8 workers: one warm-up, then --repeats timed runs. It reports the median
stepping time (setup excluded), its min-max spread, and peak memory.

The baseline is one record, not a history. It names the workloads (a hash of
their definitions), the machine, and the Rust toolchain, and a measurement is
compared only with a compatible one. Otherwise nothing is compared and the exit
code is 2: review the numbers, then --accept a new baseline. A workload is
SLOWER when its median lies outside the baseline's min-max range and is more
than 10% higher; any slower workload makes the exit code 1. Use an idle machine
with the same power settings. Runs one after another drift as the machine
warms, so for a before/after verdict on a change, alternate the two builds run
by run. Standard library only; needs Cargo.
"""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile

from project import ROOT, build_runner
from scenario import write as write_mission

BASELINE = ROOT / "reference/perf_baseline.json"
# DEVNOTE: identical code, timed in three sessions on an M2 Pro (2026-10-12),
# drifted up to 6.2% on single workloads (sensing-32-w1 +6.2%,
# collision-grid-1024-w1 +5.9%, sensing-128-w8 +6.1%), beyond each session's
# own min-max range. A 5% threshold failed on that noise; 10% does not.
NOISE = 0.10


def mission(agents, end, multiplier=1, controller_rate=0, variance=0, interactions=None,
            spawn=None, sensors=None, autonomy=None):
    """A YAML mission (written as JSON, which YAML reads) of `agents` aircraft
    flying straight at 200 m, every one with its own random start."""
    aircraft = {"team": 1, "count": agents, "position_m": [0, 0, 200],
                "position_variance_m2": [variance, variance, 0],
                "randomize_every_spawn": True}
    if spawn:
        aircraft["spawn"] = spawn
    aircraft.update({
        "autonomy": {"Straight": {"speed": 20}, **(autonomy or {})},
        "controller": {"SimpleAircraftControllerPID": {"loop_rate": controller_rate}},
        "motion_model": {"SimpleAircraft": None},
        "sensors": sensors or {},
    })
    return {"format_version": 1, "name": "perf",
            "run": {"end_s": end, "dt_s": 0.1, "motion_multiplier": multiplier, "seed": 1},
            "interactions": interactions or {},
            "networks": {"GlobalNetwork": None, "LocalNetwork": None},
            "metrics": {"SimpleCollisionMetrics": None},
            "entities": {"aircraft": aircraft}}


COLLISION = {"SimpleCollision": {"collision_range": 5}}
NOISE_5_5_1 = {"position_stddev_m": [5, 5, 1], "velocity_stddev_mps": [5, 5, 1]}
# name: (description, agent counts, mission fields). Changing a workload makes
# the accepted baseline incomparable, so perf.py stops comparing until a new
# baseline is accepted.
WORKLOADS = {
    "motion": ("aircraft flying straight", (64, 512), dict(end=200)),
    "substeps": ("ten controller and motion substeps per step", (64, 512), dict(
        end=60, multiplier=10)),
    "sensing": ("own-state and contact sensors on every aircraft", (32, 128), dict(
        end=60, sensors={"NoisyState": NOISE_5_5_1, "NoisyContacts": NOISE_5_5_1})),
    # Churn spreads its agents so collisions happen; the others above start together.
    "churn": ("scheduled spawning with collision removal", (64, 512), dict(
        end=200, variance=2500,
        # Aircraft hold 200 m, so this ground check costs time but never removes.
        interactions={**COLLISION, "GroundCollision": {"ground_collision_z": 150}},
        spawn={"rate_hz": 20, "batch_size": 4})),
    # Aircraft spread over about a kilometre, so few collide and the population
    # stays nearly constant: this measures the every-pair collision check itself.
    "collision": ("pairwise collision checks among active aircraft", (256, 1024), dict(
        end=20, variance=1_000_000, interactions=COLLISION)),
    # The same mission, with spatial queries answered by a grid of 5 m cells
    # (`grid_cell_m`, passed to the runner) instead of a scan. Output is identical.
    "collision-grid": ("collision with a 5 m spatial grid", (256, 1024), dict(
        end=20, variance=1_000_000, interactions=COLLISION, grid_cell_m=5)),
}

WORKERS = (1, 8)


def workload(name, agents, **changes):
    """WORKLOADS[name] at `agents` aircraft as a mission, with any fields changed."""
    fields = {**WORKLOADS[name][2], **changes}
    fields.pop("grid_cell_m", None)  # a runner option, not part of the mission
    return mission(agents, **fields)


def workloads_hash():
    missions = {name: workload(name, 1) for name in WORKLOADS}
    definition = json.dumps([missions, WORKLOADS, WORKERS], sort_keys=True)
    return hashlib.sha256(definition.encode()).hexdigest()[:16]


def run_once(runner, mission, output, workers, grid_cell_m=None):
    """(stepping seconds, steps, peak resident memory in MB) for one run."""
    grid = [] if grid_cell_m is None else [str(grid_cell_m)]
    process = subprocess.Popen([runner, mission, ROOT, output, str(workers), *grid],
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    _, status, usage = os.wait4(process.pid, 0)
    stdout, stderr = process.communicate()
    if status != 0:
        raise RuntimeError(f"{mission} failed: {stderr.strip()[-300:]}")
    result = json.loads(stdout.strip().splitlines()[-1])
    # ru_maxrss is bytes on macOS and kilobytes on Linux.
    scale = 1 if platform.system() == "Darwin" else 1024
    return result["run_s"], result["steps"], usage.ru_maxrss * scale / 1e6


def measure(runner, repeats, quick, scratch):
    results = {}
    for name, (_, sizes, fields) in WORKLOADS.items():
        for agents in sizes[:1] if quick else sizes:
            path = write_mission(scratch / f"{name}-{agents}.yaml", workload(name, agents))
            for workers in WORKERS:
                key = f"{name}-{agents}-w{workers}"
                times, memory = [], 0.0
                for repeat in range(repeats + 1):  # the first run is a warm-up
                    seconds, steps, peak_mb = run_once(
                        runner, path, scratch / f"{key}-{repeat}", workers,
                        fields.get("grid_cell_m"))
                    if repeat:
                        times.append(seconds)
                        memory = max(memory, peak_mb)
                results[key] = {"median_s": statistics.median(times), "min_s": min(times),
                                "max_s": max(times), "times_s": times, "steps": steps,
                                "peak_mb": round(memory, 1)}
                print(f"  {key:<22} {results[key]['median_s']:8.3f} s   "
                      f"[{min(times):.3f}, {max(times):.3f}]   {memory:7.1f} MB", flush=True)
    return results


def cpu_model():
    try:
        if platform.system() == "Darwin":
            return subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"],
                                           text=True).strip()
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except (OSError, subprocess.SubprocessError):
        pass
    return platform.processor() or "unknown"


def environment():
    """What a measurement depends on besides the code: workloads, machine, toolchain."""
    return {"workloads": workloads_hash(),
            "machine": f"{platform.system()} {platform.machine()}, {cpu_model()}, "
                       f"{os.cpu_count()} CPUs",
            "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip()}


def incompatibility(baseline, current, results):
    """Why the baseline cannot judge this measurement, or None."""
    if baseline is None:
        return f"no accepted baseline at {BASELINE.relative_to(ROOT)}"
    for key in ("workloads", "machine", "rustc"):
        if baseline["environment"][key] != current[key]:
            return (f"different {key}: baseline {baseline['environment'][key]!r}, "
                    f"now {current[key]!r}")
    missing = sorted(results.keys() - baseline["results"].keys())
    if missing:
        return f"the baseline has no {', '.join(missing)}"
    return None


def compare(results, baseline):
    """Print each workload against the baseline; return how many are slower."""
    print(f"\nCompared with the baseline accepted {baseline['date']}:")
    print(f"  {'workload':<22} {'baseline':>9} {'now':>9} {'change':>8}")
    slower = 0
    for key, new in results.items():
        old = baseline["results"][key]
        change = new["median_s"] / old["median_s"] - 1
        outside = new["median_s"] < old["min_s"] or new["median_s"] > old["max_s"]
        note = ""
        if outside and abs(change) > NOISE:
            note = "  faster" if change < 0 else "  SLOWER"
            slower += change > 0
        print(f"  {key:<22} {old['median_s']:9.3f} {new['median_s']:9.3f} {change:+8.1%}{note}")
    print(f"  {slower} workload(s) slower beyond run-to-run noise.")
    return slower


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--accept", action="store_true",
                        help="make this measurement the accepted baseline")
    parser.add_argument("--repeats", type=int, default=5, help="timed runs per case (default 5)")
    parser.add_argument("--quick", action="store_true", help="smallest agent counts only")
    parser.add_argument("--output", type=Path, help="also write the measurement to this file")
    args = parser.parse_args(argv)
    if args.accept and args.quick:
        parser.error("accept a full measurement, not --quick")

    current = environment()
    print(f"Building the release runner ({current['machine']}; {current['rustc']})...", flush=True)
    runner = build_runner()
    with tempfile.TemporaryDirectory() as scratch:
        results = measure(runner, args.repeats, args.quick, Path(scratch))
    record = {"date": datetime.date.today().isoformat(), "environment": current,
              "repeats": args.repeats, "results": results}
    baseline = json.loads(BASELINE.read_text()) if BASELINE.is_file() else None
    reason = incompatibility(baseline, current, results)
    if reason is None:
        record["baseline_date"] = baseline["date"]
        record["slower"] = compare(results, baseline)
    else:
        print(f"\nNot compared: {reason}. Review the numbers, then --accept a new baseline.")
    if args.output:
        args.output.write_text(json.dumps(record, indent=1) + "\n")
    if args.accept:
        BASELINE.write_text(json.dumps({key: record[key] for key in
                                        ("date", "environment", "repeats", "results")},
                                       indent=1) + "\n")
        print(f"\nAccepted as the baseline: {BASELINE.relative_to(ROOT)}.")
        return 0
    if reason is not None:
        return 2
    return 1 if record["slower"] else 0


if __name__ == "__main__":
    sys.exit(main())
