#!/usr/bin/env python3
"""Routing by index changes no output: previous revision against the current one.

    python3 paper/data/message-routing/equivalence.py OLD_SCRIMMAGE NEW_SCRIMMAGE OUTPUT.json \
        --old-revision 1ff73c0 --new-revision "working tree"

OLD_SCRIMMAGE is the `scrimmage` executable built at a revision before routing by
index (for example from `git archive 1ff73c0`, then
`cargo build --release -p scrimmage-rs --bin scrimmage`); NEW_SCRIMMAGE is the
current build. Runs every mission file under missions/ (not sweeps), perf.py's
workloads, and a message-heavy mission (NoisyState on every aircraft), at 1, 2,
and 8 workers, with the C++ comparison trace on. Every run's frames, events,
summary, and trace (which records every delivery: network, topic, sender,
receiver, in order) must be byte-identical between the two executables. Writes a
report and exits non-zero on any difference. Standard library only.
"""

import argparse
import datetime
import filecmp
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "reference"))
import perf  # noqa: E402

FILES = ("frames.bin", "events.json", "summary.csv", "trace.jsonl")
WORKERS = (1, 2, 8)
NOISY_STATE = "    <sensor>NoisyState</sensor>"


def missions(scratch):
    """(name, path) for every mission compared."""
    found = [(str(path.relative_to(ROOT)), path)
             for path in sorted((ROOT / "missions").rglob("*"))
             if path.suffix in (".xml", ".yaml") and "sweep" not in path.name]
    for name, (_, sizes, fields) in perf.WORKLOADS.items():
        if "grid_cell_m" in fields:
            continue  # the same mission as `collision`
        for agents in sizes:
            path = scratch / f"perf-{name}-{agents}.xml"
            path.write_text(perf.HEADER.format(name=name, agents=agents, **fields))
            found.append((f"perf {name}-{agents}", path))
    fields = {**perf.WORKLOADS["motion"][2], "end": 10, "sensors": NOISY_STATE}
    path = scratch / "noisy-state-512.xml"
    path.write_text(perf.HEADER.format(name="messages", agents=512, **fields))
    found.append(("NoisyState on 512 aircraft", path))
    return found


def run(executable, mission, output, workers):
    subprocess.run([executable, "run", mission, "--root", ROOT, "--output", output,
                    "--workers", str(workers), "--no-rerun", "--trace"],
                   capture_output=True, text=True, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("old_scrimmage")
    parser.add_argument("new_scrimmage")
    parser.add_argument("output", type=Path)
    parser.add_argument("--old-revision", required=True)
    parser.add_argument("--new-revision", required=True)
    args = parser.parse_args()

    results, differing = [], 0
    with tempfile.TemporaryDirectory() as scratch:
        scratch = Path(scratch)
        for index, (name, path) in enumerate(missions(scratch)):
            different, deliveries = [], 0
            for workers in WORKERS:
                old, new = scratch / f"{index}-old-w{workers}", scratch / f"{index}-new-w{workers}"
                run(args.old_scrimmage, path, old, workers)
                run(args.new_scrimmage, path, new, workers)
                different += [f"w{workers}:{file}" for file in FILES
                              if not filecmp.cmp(old / file, new / file, shallow=False)]
                if workers == 1:
                    with (old / "trace.jsonl").open() as trace:
                        deliveries = sum('"delivery"' in line for line in trace)
            differing += bool(different)
            results.append({"mission": name, "trace_deliveries": deliveries,
                            "identical": not different, "differences": different})
            print(f"{'same' if not different else 'DIFF'} {name} ({deliveries} deliveries)"
                  f"{' ' + ' '.join(different) if different else ''}", flush=True)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({
        "date": datetime.datetime.now().isoformat(timespec="seconds"),
        "machine": perf.machine(), "old_revision": args.old_revision,
        "new_revision": args.new_revision, "files_compared": FILES, "workers": WORKERS,
        "missions": len(results), "missions_differing": differing, "results": results,
    }, indent=1) + "\n")
    print(f"{len(results)} missions, {differing} differing")
    return 1 if differing else 0


if __name__ == "__main__":
    raise SystemExit(main())
