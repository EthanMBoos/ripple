#!/usr/bin/env python3
"""Check that a change left every output unchanged: two executables, byte for byte.

    python3 reference/equivalence.py OLD_EXECUTABLE NEW_EXECUTABLE

For refactors and renames. Build the old executable from any source you trust
(for example `cargo build --release` in an exported copy) and the new one from
this checkout. Runs every mission (reference/project.py), perf.py's workloads,
and a message-heavy mission at 1, 2, and 8 workers with the comparison trace on.
Frames, events, summaries, and traces must be byte-identical. Manifests must be
equal except for METADATA, the keys that record where a run came from rather
than what it computed; each differing metadata key is reported, never hidden.
Writes runs/equivalenceNNN/report.json and exits non-zero on any difference.
Standard library only.
"""

import argparse
import filecmp
import json
from pathlib import Path
import subprocess
import sys

import perf
from project import ROOT, RUNS, allocate, missions

FILES = ("frames.bin", "events.json", "summary.csv", "trace.jsonl")
WORKERS = (1, 2, 8)
# Manifest keys that describe provenance, not results: the mission's absolute path.
METADATA = ("source",)


def cases(scratch):
    """(name, mission path) for every mission compared."""
    found = [(str(path.relative_to(ROOT)), path) for path in missions()]
    for name, (_, sizes, fields) in perf.WORKLOADS.items():
        if "grid_cell_m" in fields:
            continue  # the same mission as `collision`; the grid is a runner option
        for agents in sizes:
            path = perf.write_mission(scratch / f"perf-{name}-{agents}.yaml",
                                      perf.workload(name, agents))
            found.append((f"perf {name}-{agents}", path))
    path = perf.write_mission(scratch / "noisy-state-512.yaml", perf.workload(
        "motion", 512, end=10, sensors={"NoisyState": perf.NOISE_5_5_1}))
    found.append(("NoisyState on 512 aircraft", path))
    return found


def run(executable, mission, output, workers):
    subprocess.run([executable, "run", mission, "--root", ROOT, "--output", output,
                    "--workers", str(workers), "--no-rerun", "--trace"],
                   capture_output=True, text=True, check=True)


def manifest_differences(old, new):
    old = json.loads((old / "manifest.json").read_text())
    new = json.loads((new / "manifest.json").read_text())
    return sorted(key for key in old.keys() | new.keys() if old.get(key) != new.get(key))


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
    for index, (name, path) in enumerate(cases(scratch)):
        different, metadata = [], set()
        for workers in WORKERS:
            old, new = output / f"{index:02}-old-w{workers}", output / f"{index:02}-new-w{workers}"
            run(args.old, path, old, workers)
            run(args.new, path, new, workers)
            different += [f"w{workers}:{file}" for file in FILES
                          if not filecmp.cmp(old / file, new / file, shallow=False)]
            for key in manifest_differences(old, new):
                if key in METADATA:
                    metadata.add(key)
                else:
                    different.append(f"w{workers}:manifest.{key}")
        differing += bool(different)
        results.append({"mission": name, "identical": not different,
                        "differences": different, "metadata_differences": sorted(metadata)})
        print(f"{'same' if not different else 'DIFF'} {name}"
              f"{' ' + ' '.join(different) if different else ''}"
              f"{' (metadata: ' + ', '.join(sorted(metadata)) + ')' if metadata else ''}",
              flush=True)
    report = {"old": str(args.old.resolve()), "new": str(args.new.resolve()),
              "files_compared": FILES, "metadata_keys": METADATA, "workers": WORKERS,
              "missions": len(results), "missions_differing": differing, "results": results}
    (output / "report.json").write_text(json.dumps(report, indent=1) + "\n")
    print(f"{len(results)} missions, {differing} differing: {output / 'report.json'}")
    return 1 if differing else 0


if __name__ == "__main__":
    sys.exit(main())
