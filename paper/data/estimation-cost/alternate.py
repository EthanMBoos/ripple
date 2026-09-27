#!/usr/bin/env python3
"""Before/after stepping time of two builds of reference/benchmark_runner.rs.

    python3 paper/data/estimation-cost/alternate.py OLD_RUNNER NEW_RUNNER OUTPUT.json \
        --old-revision ab085bf --new-revision "working tree"

Runs every reference/perf.py workload at 1 and 8 workers, alternating the two
runners run by run (a warm-up pair, then --repeats pairs), so machine warm-up
affects both equally. Timing one build after the other overstated the cost of
the estimation stage. Reports the median stepping time of each and every raw
time. Standard library only.
"""

import argparse
import datetime
import json
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
        for name, (_, sizes, fields) in perf.WORKLOADS.items():
            for agents in sizes:
                mission = scratch / f"{name}-{agents}.xml"
                mission.write_text(perf.HEADER.format(name=name, agents=agents, **fields))
                for workers in perf.WORKERS:
                    times = {"old": [], "new": []}
                    for repeat in range(args.repeats + 1):
                        for label, runner in (("old", args.old_runner), ("new", args.new_runner)):
                            output = scratch / "run"
                            shutil.rmtree(output, ignore_errors=True)
                            done = subprocess.run([runner, mission, ROOT, output, str(workers)],
                                                  capture_output=True, text=True, check=True)
                            if repeat:  # the first pair is a warm-up
                                times[label].append(json.loads(done.stdout)["run_s"])
                    old, new = statistics.median(times["old"]), statistics.median(times["new"])
                    key = f"{name}-{agents}-w{workers}"
                    results[key] = {"old_median_s": old, "new_median_s": new,
                                    "change": new / old - 1, "old_s": times["old"],
                                    "new_s": times["new"]}
                    print(f"  {key:<22} {old:7.3f} {new:7.3f} {(new / old - 1) * 100:+6.1f}%",
                          flush=True)
    args.output.write_text(json.dumps({
        "date": datetime.datetime.now().isoformat(timespec="seconds"),
        "machine": perf.machine(),
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "old_revision": args.old_revision,
        "new_revision": args.new_revision,
        "method": f"reference/perf.py workloads, alternating old/new runs, one warm-up pair "
                  f"then {args.repeats} pairs, median stepping time (setup excluded)",
        "results": results,
    }, indent=1) + "\n")


if __name__ == "__main__":
    main()
