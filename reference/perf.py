#!/usr/bin/env python3
"""Track Rust simulation performance against your own history.

    python3 reference/perf.py                    # measure and compare with the last record
    python3 reference/perf.py --record --label "before spatial index"
    python3 reference/perf.py --history          # list recorded runs on this machine
    python3 reference/perf.py --quick            # smallest sizes only, about a minute

Runs a fixed set of workloads natively (release build, core-only runner
reference/benchmark_runner.rs, no viewer), each at a few agent counts and at 1
and 8 workers: one warm-up, then --repeats timed runs. It reports the median
stepping time (setup excluded), its min-max spread, and peak memory.

Results are compared with the most recent record from the same machine (or
--against LABEL_OR_COMMIT). A change is flagged only when the new median falls
outside the old min-max range and differs by more than 5%. --record appends the
run to reference/perf_history.jsonl, which is committed, so git keeps the history.
Compare only like with like: same machine, same power settings, idle system.
Standard library only; needs Cargo.
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
import tempfile

ROOT = Path(__file__).resolve().parents[1]
HISTORY = ROOT / "reference/perf_history.jsonl"
NOISE = 0.05

HEADER = """<?xml version="1.0"?>
<runscript name="perf-{name}">
  <run start="0" end="{end}" dt="0.1" motion_multiplier="{multiplier}"
       enable_gui="false" network_gui="false" start_paused="false" time_warp="0" />
  <seed>1</seed><end_condition>time</end_condition>
  <network>GlobalNetwork</network><network>LocalNetwork</network>
  <metrics>SimpleCollisionMetrics</metrics>
{world}
  <entity>
    <team_id>1</team_id><count>{agents}</count>
    <x>0</x><y>0</y><z>200</z>
    <variance_x>{variance}</variance_x><variance_y>{variance}</variance_y><variance_z>0</variance_z>
    <use_variance_all_ents>true</use_variance_all_ents>
{spawn}
    <autonomy speed="20">Straight</autonomy>
    <controller loop_rate="{controller_rate}">SimpleAircraftControllerPID</controller>
    <motion_model>SimpleAircraft</motion_model>
{sensors}
  </entity>
</runscript>
"""

# name: (description, agent counts, mission fields). Keep these stable: changing
# a workload makes its history incomparable, so add a new name instead.
WORKLOADS = {
    "motion": ("aircraft flying straight", (64, 512), dict(
        end=200, multiplier=1, controller_rate=0, world="", spawn="", sensors="")),
    "substeps": ("ten controller and motion substeps per step", (64, 512), dict(
        end=60, multiplier=10, controller_rate=0, world="", spawn="", sensors="")),
    "sensing": ("own-state and contact sensors on every aircraft", (32, 128), dict(
        end=60, multiplier=1, controller_rate=0, world="",
        spawn="", sensors="    <sensor>NoisyState</sensor>\n    <sensor>NoisyContacts</sensor>")),
    "churn": ("scheduled spawning with collision removal", (64, 512), dict(
        end=200, multiplier=1, controller_rate=0,
        world='  <entity_interaction collision_range="5">SimpleCollision</entity_interaction>\n'
              # Aircraft hold 200 m, so this ground check costs time but never removes.
              '  <entity_interaction ground_collision_z="150">GroundCollision</entity_interaction>',
        spawn="    <generate_rate>20</generate_rate><generate_count>4</generate_count>\n"
              "    <generate_start_time>0</generate_start_time>", sensors="")),
}
WORKERS = (1, 8)


def build_runner():
    """Build the core-only runner and return its path."""
    output = subprocess.check_output(
        ["cargo", "bench", "--locked", "-p", "scrimmage-core", "--bench", "reference",
         "--no-run", "--message-format=json"], cwd=ROOT, text=True, stderr=subprocess.DEVNULL)
    for line in output.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-artifact" and message.get("executable") \
                and message["target"]["name"] == "reference":
            return message["executable"]
    raise RuntimeError("could not find the benchmark runner")


def run_once(runner, mission, output, workers):
    """(stepping seconds, steps, peak resident memory in MB) for one run."""
    process = subprocess.Popen([runner, mission, ROOT, output, str(workers)],
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
            mission = scratch / f"{name}-{agents}.xml"
            # Churn spreads its agents so collisions happen; others start together.
            variance = 2500 if name == "churn" else 0
            mission.write_text(HEADER.format(name=name, agents=agents, variance=variance,
                                             **fields))
            for workers in WORKERS:
                key = f"{name}-{agents}-w{workers}"
                times, memory = [], 0.0
                for repeat in range(repeats + 1):  # the first run is a warm-up
                    seconds, steps, peak_mb = run_once(
                        runner, mission, scratch / f"{key}-{repeat}", workers)
                    if repeat:
                        times.append(seconds)
                        memory = max(memory, peak_mb)
                results[key] = {"median_s": statistics.median(times), "min_s": min(times),
                                "max_s": max(times), "steps": steps, "peak_mb": round(memory, 1)}
                print(f"  {key:<22} {results[key]['median_s']:8.3f} s   "
                      f"[{min(times):.3f}, {max(times):.3f}]   {memory:7.1f} MB", flush=True)
    return results


def machine():
    host = hashlib.sha256(platform.node().encode()).hexdigest()[:8]
    return f"{platform.system()}-{platform.machine()}-{os.cpu_count()}cpu-{host}"


def git(*arguments):
    return subprocess.check_output(["git", *arguments], cwd=ROOT, text=True).strip()


def load_history():
    if not HISTORY.is_file():
        return []
    return [json.loads(line) for line in HISTORY.read_text().splitlines() if line.strip()]


def compare(results, baseline):
    print(f"\nCompared with {baseline['label'] or baseline['commit'][:10]} "
          f"({baseline['date']}, commit {baseline['commit'][:10]}"
          f"{', uncommitted changes' if baseline['dirty'] else ''}):")
    print(f"  {'workload':<22} {'before':>9} {'after':>9} {'change':>8}")
    flagged = 0
    for key, new in results.items():
        old = baseline["results"].get(key)
        if old is None:
            print(f"  {key:<22} {'-':>9} {new['median_s']:9.3f}      new")
            continue
        change = new["median_s"] / old["median_s"] - 1
        outside = new["median_s"] < old["min_s"] or new["median_s"] > old["max_s"]
        note = ""
        if outside and abs(change) > NOISE:
            flagged += 1
            note = "  faster" if change < 0 else "  SLOWER"
        print(f"  {key:<22} {old['median_s']:9.3f} {new['median_s']:9.3f} {change:+8.1%}{note}")
    print(f"  {flagged} workload(s) changed beyond run-to-run noise.")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--record", action="store_true", help="append this run to the history")
    parser.add_argument("--label", default="", help="short name for a recorded run")
    parser.add_argument("--against", help="compare with the record with this label or commit")
    parser.add_argument("--repeats", type=int, default=5, help="timed runs per case (default 5)")
    parser.add_argument("--quick", action="store_true", help="smallest agent counts only")
    parser.add_argument("--history", action="store_true", help="list this machine's records")
    args = parser.parse_args(argv)

    this_machine = machine()
    history = [entry for entry in load_history() if entry["machine"] == this_machine]
    if args.history:
        for entry in history:
            print(f"{entry['date']}  {entry['commit'][:10]}{'+' if entry['dirty'] else ' '}  "
                  f"{entry['label']}")
        return 0

    print(f"Building the release runner ({this_machine})...", flush=True)
    runner = build_runner()
    with tempfile.TemporaryDirectory() as scratch:
        results = measure(runner, args.repeats, args.quick, Path(scratch))

    if args.against:
        matches = [entry for entry in history
                   if entry["label"] == args.against or entry["commit"].startswith(args.against)]
        baseline = matches[-1] if matches else None
        if baseline is None:
            print(f"\nNo record matching {args.against!r} on this machine.")
    else:
        baseline = history[-1] if history else None
    if baseline is not None:
        compare(results, baseline)
    elif not args.against:
        print("\nNo earlier record on this machine; use --record to start the history.")

    if args.record:
        entry = {
            "date": datetime.datetime.now().isoformat(timespec="seconds"),
            "commit": git("rev-parse", "HEAD"),
            "dirty": bool(git("status", "--porcelain", "--untracked-files=no")),
            "label": args.label,
            "machine": this_machine,
            "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
            "repeats": args.repeats,
            "results": results,
        }
        with HISTORY.open("a") as stream:
            stream.write(json.dumps(entry) + "\n")
        print(f"\nRecorded in {HISTORY.relative_to(ROOT)}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
