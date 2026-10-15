#!/usr/bin/env python3
"""Check that a change left every output unchanged: two executables, byte for byte.

    python3 reference/equivalence.py OLD_EXECUTABLE NEW_EXECUTABLE

For refactors and renames. Build the old executable from any source you trust
(for example `cargo build --release` in an exported copy) and the new one from
this checkout. Runs every mission (reference/project.py), perf.py's workloads,
and a message-heavy mission at 1, 2, and 8 workers. Shipped missions use the
observation trace, as does a small generated contact-sensing case. Large
generated workloads omit it to avoid recording every agent's full contact list
at every tick. Frame, entity, event, summary, and
recorded trace channel payloads must be identical, as must the scenario and
result metadata. Both executables must write MCAP.
Writes runs/equivalenceNNN/report.json and exits non-zero on any difference.
Standard library only.
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys

import perf
from project import ROOT, RUNS, allocate, missions
from run import read_run

CHANNELS = ("/entities", "/frames", "/events", "/summary",
            "/beliefs", "/readings", "/deliveries", "/outputs")
BASE_CHANNELS = CHANNELS[:4]
WORKERS = (1, 2, 8)


def cases(scratch):
    """(name, mission path, trace) for every mission compared."""
    found = [(str(path.relative_to(ROOT)), path, True) for path in missions()]
    for name, (_, sizes, fields) in perf.WORKLOADS.items():
        if "grid_cell_m" in fields:
            continue  # the same mission as `collision`; the grid is a runner option
        for agents in sizes:
            path = perf.write_mission(scratch / f"perf-{name}-{agents}.yaml",
                                      perf.workload(name, agents))
            found.append((f"perf {name}-{agents}", path, False))
    path = perf.write_mission(scratch / "noisy-state-512.yaml", perf.workload(
        "motion", 512, end=10, sensors={"NoisyState": perf.NOISE_5_5_1}))
    found.append(("NoisyState on 512 aircraft", path, False))
    path = perf.write_mission(scratch / "traced-contacts-16.yaml",
                              perf.workload("sensing", 16, end=0.5))
    found.append(("traced contacts on 16 aircraft", path, True))
    return found


def run(executable, mission, output, workers, trace):
    subprocess.run([executable, "run", mission, "--root", ROOT, "--output", output,
                    "--workers", str(workers), "--headless",
                    *(["--trace"] if trace else [])],
                   capture_output=True, text=True, check=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("old", type=Path)
    parser.add_argument("new", type=Path)
    args = parser.parse_args(argv)
    output = allocate(RUNS, "equivalence")
    scratch = output / "missions"
    scratch.mkdir()
    results, differing = [], 0
    for index, (name, path, trace) in enumerate(cases(scratch)):
        different = []
        topics = CHANNELS if trace else BASE_CHANNELS
        for workers in WORKERS:
            old, new = output / f"{index:02}-old-w{workers}", output / f"{index:02}-new-w{workers}"
            run(args.old, path, old, workers, trace)
            run(args.new, path, new, workers, trace)
            left, right = read_run(old), read_run(new)
            different += [f"w{workers}:{topic}" for topic in topics
                          if left.channels.get(topic, []) != right.channels.get(topic, [])]
            for key in sorted(left.metadata.keys() | right.metadata.keys()):
                if left.metadata.get(key) != right.metadata.get(key):
                    different.append(f"w{workers}:metadata.{key}")
            if left.scenario != right.scenario:
                different.append(f"w{workers}:scenario")
        differing += bool(different)
        results.append({"mission": name, "traced": trace, "identical": not different,
                        "differences": different})
        print(f"{'same' if not different else 'DIFF'} {name}"
              f"{' ' + ' '.join(different) if different else ''}", flush=True)
    report = {"old": str(args.old.resolve()), "new": str(args.new.resolve()),
              "channels_compared": CHANNELS, "workers": WORKERS,
              "traced_missions": sum(result["traced"] for result in results),
              "missions": len(results), "missions_differing": differing, "results": results}
    (output / "report.json").write_text(json.dumps(report, indent=1) + "\n")
    print(f"{len(results)} missions, {differing} differing: {output / 'report.json'}")
    return 1 if differing else 0


if __name__ == "__main__":
    sys.exit(main())
