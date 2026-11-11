#!/usr/bin/env python3
"""Check that a change left every output unchanged: two executables, byte for byte.

    python3 reference/equivalence.py OLD_EXECUTABLE NEW_EXECUTABLE

For refactors and renames. Build the old executable from any source you trust
(for example `cargo build --release` in an exported copy) and the new one from
this checkout. Runs every mission (reference/project.py), perf.py's workloads,
and a message-heavy mission at 1, 2, and 8 workers. Shipped missions use the
observation trace, as does a small generated contact-sensing case. The full
16-hub regional day and large generated workloads omit it to avoid recording
every agent's full contact list at every tick. The checked one-hub day keeps
its trace. Frame, entity, event, summary, and
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
from run import decode_file

CHANNELS = ("/entities", "/frames", "/events", "/summary", "/drawings",
            "/beliefs", "/readings", "/deliveries", "/outputs")
BASE_CHANNELS = CHANNELS[:5]
WORKERS = (1, 2, 8)


def cases(scratch):
    """(name, mission path, trace) for every mission compared."""
    # The regional full-day contact trace is too large for this in-memory reader.
    # Keep full trace coverage on the checked one-hub day and smaller missions.
    region = ROOT / "crates/delivery/missions/region.yaml"
    found = [(str(path.relative_to(ROOT)), path, path != region) for path in missions()]
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
    """The run's failure, as its last line of output, or None when it ran."""
    result = subprocess.run([executable, "run", mission, "--root", ROOT, "--output", output,
                             "--workers", str(workers), "--headless",
                             *(["--trace"] if trace else [])],
                            capture_output=True, text=True)
    if result.returncode == 0:
        return None
    return (result.stderr.strip().splitlines() or [f"exit {result.returncode}"])[-1]


def read_output(path):
    """Compare encoded output, including entities spawned from recipes.

    read_run also builds a static entity table, which cannot describe recipe
    groups. That decoding is unnecessary for byte-for-byte equivalence.
    """
    metadata, channels, _ = decode_file((path / "run.mcap").read_bytes())
    return channels, metadata.get("ripple.result", {}), json.loads(
        metadata["ripple.run"]["scenario"])


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
        skipped = None
        for workers in WORKERS:
            old, new = output / f"{index:02}-old-w{workers}", output / f"{index:02}-new-w{workers}"
            failures = (run(args.old, path, old, workers, trace),
                        run(args.new, path, new, workers, trace))
            if failures[0] is not None and failures[0] == failures[1]:
                # A mission neither can load (a model key missing from the
                # environment, say) proves nothing either way.
                skipped = failures[0]
                break
            if any(failures):
                raise RuntimeError(f"{name} at {workers} workers: {failures}")
            left_channels, left_metadata, left_scenario = read_output(old)
            right_channels, right_metadata, right_scenario = read_output(new)
            different += [f"w{workers}:{topic}" for topic in topics
                          if left_channels.get(topic, []) != right_channels.get(topic, [])]
            for key in sorted(left_metadata.keys() | right_metadata.keys()):
                if left_metadata.get(key) != right_metadata.get(key):
                    different.append(f"w{workers}:metadata.{key}")
            if left_scenario != right_scenario:
                different.append(f"w{workers}:scenario")
        if skipped is not None:
            results.append({"mission": name, "traced": trace, "identical": None,
                            "differences": [f"skipped: {skipped}"]})
            print(f"skip {name}: {skipped}", flush=True)
            continue
        differing += bool(different)
        results.append({"mission": name, "traced": trace, "identical": not different,
                        "differences": different})
        print(f"{'same' if not different else 'DIFF'} {name}"
              f"{' ' + ' '.join(different) if different else ''}", flush=True)
    report = {"old": str(args.old.resolve()), "new": str(args.new.resolve()),
              "channels_compared": CHANNELS, "workers": WORKERS,
              "traced_missions": sum(result["traced"] for result in results),
              "skipped": [result["mission"] for result in results if result["identical"] is None],
              "missions": len(results), "missions_differing": differing, "results": results}
    (output / "report.json").write_text(json.dumps(report, indent=1) + "\n")
    print(f"{len(results)} missions, {differing} differing: {output / 'report.json'}")
    return 1 if differing else 0


if __name__ == "__main__":
    sys.exit(main())
