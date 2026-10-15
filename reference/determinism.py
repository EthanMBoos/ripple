#!/usr/bin/env python3
"""Rust determinism: worker counts, recording, tracing, and spatial lookup change no output.

    python3 reference/determinism.py --output runs/determinism000

Runs every mission (reference/project.py), perf.py's workloads, and three
crowded collision missions with the command-line executable at 1, 2, and 8
workers, with an eight-worker headless Rerun recording. Shipped missions and a
small generated contact-sensing case also run with the observation trace; large
generated workloads do not duplicate every agent's contacts in the check
output. Entity, frame, event, and summary
channel payloads must be byte-identical across the variants used, and the
optional RRD export nonempty.
The core-only runner then answers
spatial queries with a scan at 1 and 8 workers and with grids of several cell
widths (0.7 to 1,000 m); those must all write identical output too (the
runner has only the stock plugins, so user-crate missions skip this). Writes
report.json; exits non-zero on any difference. Standard library only.
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys

import perf
from project import ROOT, build_cli, build_runner, missions
from run import read_run

CHANNELS = ("/entities", "/frames", "/events", "/summary")
CLI_VARIANTS = [("w1", ["--workers", "1", "--headless"]),
                ("w2", ["--workers", "2", "--headless"]),
                ("w8", ["--workers", "8", "--headless"]),
                ("w8-recording", ["--workers", "8", "--headless", "--export-rrd"]),
                ("w1-trace", ["--workers", "1", "--headless", "--trace"])]
# (label, workers, grid cell width in metres or None for the scan)
LOOKUP_VARIANTS = [("scan-w1", 1, None), ("scan-w8", 8, None), ("grid5-w1", 1, 5),
                   ("grid5-w8", 8, 5), ("grid0.7-w2", 2, 0.7), ("grid50-w2", 2, 50),
                   ("grid1000-w1", 1, 1000)]
# Crowded collision missions: (position variance m^2, agents).
CROWDED = [(100, 512), (400, 1024), (2500, 2048)]


def cases(scratch):
    """(name, mission path, stock plugins only, trace) for each case."""
    found = [(str(path.relative_to(ROOT)), path,
              path.relative_to(ROOT).parts[0] == "missions", True)
             for path in missions()]
    for name, (_, sizes, fields) in perf.WORKLOADS.items():
        if "grid_cell_m" in fields:
            continue  # the same mission as `collision`; grids are checked below
        for agents in sizes:
            path = perf.write_mission(scratch / f"perf-{name}-{agents}.yaml",
                                      perf.workload(name, agents))
            found.append((f"perf {name}-{agents}", path, True, False))
    for variance, agents in CROWDED:
        path = perf.write_mission(scratch / f"crowded-{variance}-{agents}.yaml",
                                  perf.workload("collision", agents, variance=variance))
        found.append((f"crowded collision, variance {variance} m^2, {agents} agents",
                      path, True, False))
    path = perf.write_mission(scratch / "traced-contacts-16.yaml",
                              perf.workload("sensing", 16, end=0.5))
    found.append(("traced contacts on 16 aircraft", path, True, True))
    return found


def differences(base, other, label, compare_scenario=True):
    left, right = read_run(base), read_run(other)
    different = [f"{label}:{topic}" for topic in CHANNELS
                 if left.channels.get(topic, []) != right.channels.get(topic, [])]
    if left.metadata != right.metadata or (compare_scenario and left.scenario != right.scenario):
        different.append(f"{label}:metadata")
    return different


def check(cli, runner, name, mission, stock, trace, folder):
    folder.mkdir()
    different = []
    for label, options in CLI_VARIANTS:
        if label == "w1-trace" and not trace:
            continue
        subprocess.run([cli, "run", mission, "--root", ROOT, "--output", folder / label, *options],
                       capture_output=True, text=True, check=True)
        if label != "w1":
            different += differences(folder / "w1", folder / label, label)
    recording = folder / "w8-recording/recording.rrd"
    if not recording.is_file() or not recording.stat().st_size:
        different.append("w8-recording: no recording")
    for label, workers, grid in LOOKUP_VARIANTS if stock else []:
        subprocess.run([runner, mission, ROOT, folder / label, str(workers),
                        *([] if grid is None else [str(grid)])],
                       capture_output=True, text=True, check=True)
        if label != "scan-w1":
            different += differences(folder / "scan-w1", folder / label, label,
                                     compare_scenario=False)
    events = read_run(folder / "w1").events
    collisions = sum(event["kind"] in ("TeamCollision", "NonTeamCollision") for event in events)
    return {"mission": name, "traced": trace, "collisions": collisions,
            "identical": not different,
            "differences": different}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True)
    scratch = output / "missions"
    scratch.mkdir()
    cli, runner = build_cli("release"), build_runner()
    results = []
    for index, (name, mission, stock, trace) in enumerate(cases(scratch)):
        result = check(cli, runner, name, mission, stock, trace, output / f"{index:02}")
        results.append(result)
        print(f"{'same' if result['identical'] else 'DIFF'} {name}"
              f"{' ' + ' '.join(result['differences']) if result['differences'] else ''}",
              flush=True)
    report = {"passed": all(result["identical"] for result in results),
              "channels_compared": CHANNELS, "cli_variants": [label for label, _ in CLI_VARIANTS],
              "lookup_variants": [label for label, _, _ in LOOKUP_VARIANTS],
              "missions": len(results),
              "traced_missions": sum(result["traced"] for result in results),
              "missions_differing": sum(not result["identical"] for result in results),
              "results": results}
    (output / "report.json").write_text(json.dumps(report, indent=1) + "\n")
    print(f"{len(results)} missions, {report['missions_differing']} differing")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
