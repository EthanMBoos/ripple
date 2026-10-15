#!/usr/bin/env python3
"""What evaluating FixedWing6DOF and Multirotor forces at every RK4 stage changes.

    python3 reference/rk4_loads.py --output runs/rk4-loads000 [--source ../scrimmage]

Upstream C++ (the unmodified Ubuntu-24.04 build, in Docker as for
reference_check.py) computes forces and moments once, from the start-of-step
state, and holds them through all four RK4 stages ("frozen"). This checkout,
like the C++ comparison branch, computes them from each stage's state
("stage"). The frozen runs come from that external reference, so no earlier
revision of this project is needed. Writes OUTPUT/results.json and a readable
OUTPUT/summary.md. Standard library only; needs Docker, Cargo, and the C++
checkout. C++ runs single-threaded and headless; its controller schedule
differs from Rust's only with more than one substep, and every multi-substep
case here has constant controls.

Part A, integration accuracy with constant controls: Multirotor
(missions/multirotor.yaml, constant motor speeds) and FixedWing6DOF with every
controller gain zero (inputs/fixed-wing-open-loop.yaml), at motion multipliers 1
to 16, against stage loads at multiplier 1024. With constant controls the
substep count changes only the integration step.

Part B, a controlled flight at practical steps: inputs/fixed-wing-recovery.yaml
at outer steps of 0.01, 0.05, and 0.1 s with one substep, frozen against stage.

Part C, the altitude gain: inputs/aircraft-pid-upset.yaml (the default A-4, 45 s
at 1000 m after a 5 degree roll and 3 degree pitch upset) with altitude D 0.01
(upstream's default), 0.002 (this project's), and 0, at 0.01 s and two
halvings: the largest altitude error over the run and after the first 5 s.

Each variant edits the scenario Ripple recorded for its mission; Ripple runs it
as YAML, and C++ as XML written from it (scrimmage_xml.py).
"""

import argparse
import json
import math
from pathlib import Path
import sys
from types import SimpleNamespace

from compare import distance, orientation_distance_rad
from frames import read_frames
from project import ROOT, build_cli, source_identity
from reference_check import BASELINE_BRANCH, build_reference, prepare_mission, run_cpp, run_logged
import scenario as missions

INPUTS = ROOT / "reference/inputs"
MULTIPLIERS = [1, 2, 4, 8, 16]
REFERENCE_MULTIPLIER = 1024
PART_A = [("multirotor", ROOT / "missions/multirotor.yaml", 0.01),
          ("fixed-wing-open-loop", INPUTS / "fixed-wing-open-loop.yaml", 0.05)]
PART_B = [("fixed-wing-recovery", INPUTS / "fixed-wing-recovery.yaml", dt) for dt in (0.01, 0.05, 0.1)]
UPSET = INPUTS / "aircraft-pid-upset.yaml"
UPSET_GAINS = [0.01, 0.002, 0.0]
UPSET_STEPS_S = [0.01, 0.005, 0.0025]
UPSET_ALTITUDE_M = 1000.0


class Runner:
    """Runs one mission variant in Rust ("stage") or upstream C++ ("frozen")."""

    def __init__(self, output, rust, image, settings):
        self.output, self.rust, self.image, self.settings = output, rust, image, settings
        self.count = 0
        self.scenarios = {}

    def run_rust(self, mission, folder):
        run_logged([self.rust, "run", mission, "--root", ROOT, "--output", folder / "rust",
                    "--workers", "1", "--no-rerun"], folder / "rust.log",
                   timeout=self.settings.timeout)
        return folder / "rust"

    def scenario(self, mission):
        """The scenario Ripple records for `mission`, from one run of it."""
        if mission not in self.scenarios:
            folder = self.output / f"base-{mission.stem}"
            folder.mkdir()
            self.scenarios[mission] = missions.recorded(self.run_rust(mission, folder))
        return json.loads(json.dumps(self.scenarios[mission]))  # a copy to edit

    def frames(self, mission, loads, dt_s=None, multiplier=None, altitude_d=None):
        scenario = self.scenario(mission)
        if dt_s is not None:
            scenario["run"]["dt_s"] = dt_s
        if multiplier is not None:
            scenario["run"]["motion_multiplier"] = multiplier
        if altitude_d is not None:
            scenario["entities"][0]["controller"][0]["params"]["altitude_pid"] = [
                0.1, 0.0001, altitude_d, 10]
        self.count += 1
        folder = self.output / f"{self.count:03}-{mission.stem}-{loads}"
        inputs = folder / "input"
        inputs.mkdir(parents=True)
        edited = missions.write(inputs / "input.yaml", missions.as_yaml(scenario))
        if loads == "stage":
            return read_frames(self.run_rust(edited, folder) / "frames.bin")
        prepare_mission(inputs, scenario)
        logs = run_cpp(self.settings, self.image, folder / "cpp", inputs / "effective.xml", {})
        return read_frames(logs / "frames.bin")


def largest_difference(frames, reference):
    """Largest position (m) and attitude (rad) difference over every frame and entity."""
    position = attitude = 0.0
    for candidate, expected in zip(frames, reference, strict=True):
        for key, entity in expected.entities.items():
            other = candidate.entities[key]
            if not all(math.isfinite(value) for value in other.position):
                return math.inf, math.inf
            position = max(position, distance(other.position, entity.position))
            attitude = max(attitude, orientation_distance_rad(entity.orientation,
                                                              other.orientation))
    return position, attitude


def order(errors):
    """Observed order between successive halvings: log2(e(h) / e(h/2))."""
    return [round(math.log2(a / b), 2) if a > 0 and b > 0 and math.isfinite(a) else None
            for a, b in zip(errors, errors[1:])]


def altitude_errors(frames):
    """Largest altitude error (m) over the run and after the first 5 s."""
    whole = after_5_s = 0.0
    for frame in frames:
        for entity in frame.entities.values():
            error = abs(entity.position[2] - UPSET_ALTITUDE_M)
            whole = max(whole, error)
            if frame.time_s >= 5.0:
                after_5_s = max(after_5_s, error)
    return whole, after_5_s


def summary(results):
    """The results as Markdown tables."""
    lines = ["# RK4 loads: frozen (upstream C++) against every stage (this checkout)", "",
             "Generated by reference/rk4_loads.py; see its docstring for the method.", "",
             "## A. Largest position error against stage loads at 1024 substeps", "",
             "| Substeps | " + " | ".join(
                 f"{case['mission']} frozen / stage (m)" for case in results["part_a"]) + " |",
             "|---|" + "---|" * len(results["part_a"])]
    for index, multiplier in enumerate(MULTIPLIERS):
        cells = [f"{case['frozen'][index]['position_m']:.2g} / {case['stage'][index]['position_m']:.2g}"
                 for case in results["part_a"]]
        lines.append(f"| {multiplier} | " + " | ".join(cells) + " |")
    lines += ["", "Reference self-check (512 against 1024 substeps): " + ", ".join(
        f"{case['mission']} {case['reference_self_check_m']:.2g} m" for case in results["part_a"]),
        "", "## B. Frozen against stage, one substep", "",
        "| Step (s) | Position (m) | Attitude (rad) |", "|---|---|---|"]
    lines += [f"| {row['dt_s']} | {row['frozen_vs_stage_position_m']:.3g} | "
              f"{row['frozen_vs_stage_attitude_rad']:.3g} |" for row in results["part_b"]]
    lines += ["", "## C. Largest altitude error of the upset flight, whole run (after 5 s)", "",
              "| Altitude D | Step (s) | Frozen (m) | Stage (m) |", "|---|---|---|---|"]
    rows = {(row["loads"], row["altitude_d"], row["dt_s"]): row for row in results["part_c"]}
    for gain in UPSET_GAINS:
        for dt_s in UPSET_STEPS_S:
            cells = [f"{rows[loads, gain, dt_s]['altitude_error_m']:.3g} "
                     f"({rows[loads, gain, dt_s]['after_5_s_m']:.3g})" for loads in ("frozen", "stage")]
            lines.append(f"| {gain} | {dt_s} | " + " | ".join(cells) + " |")
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    parser.add_argument("--source", type=Path, default=ROOT.parent / "scrimmage",
                        help="C++ checkout with a local Ubuntu-24.04 branch")
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True)
    settings = SimpleNamespace(source=args.source.resolve(), jobs=4, build_timeout=3600, timeout=600)
    cpp = build_reference(settings, output, BASELINE_BRANCH)
    with (output / "rust-build.log").open("w") as log:
        rust = build_cli("release", log=log)
    runs = output / "runs"
    runs.mkdir()
    runner = Runner(runs, rust, cpp["image"], settings)

    part_a = []
    for name, mission, dt_s in PART_A:
        reference = runner.frames(mission, "stage", dt_s, REFERENCE_MULTIPLIER)
        check = largest_difference(runner.frames(mission, "stage", dt_s, REFERENCE_MULTIPLIER // 2),
                                   reference)[0]
        rows = {"frozen": [], "stage": []}
        for loads in rows:
            for multiplier in MULTIPLIERS:
                position, attitude = largest_difference(
                    runner.frames(mission, loads, dt_s, multiplier), reference)
                rows[loads].append({"multiplier": multiplier, "motion_dt_s": dt_s / multiplier,
                                    "position_m": position, "attitude_rad": attitude})
        part_a.append({"mission": name, "dt_s": dt_s, "reference_multiplier": REFERENCE_MULTIPLIER,
                       "reference_self_check_m": check, **rows,
                       "frozen_order": order([row["position_m"] for row in rows["frozen"]]),
                       "stage_order": order([row["position_m"] for row in rows["stage"]])})
        print(json.dumps(part_a[-1]), flush=True)
    part_b = []
    for name, mission, dt_s in PART_B:
        position, attitude = largest_difference(runner.frames(mission, "frozen", dt_s, 1),
                                                runner.frames(mission, "stage", dt_s, 1))
        part_b.append({"mission": name, "dt_s": dt_s, "frozen_vs_stage_position_m": position,
                       "frozen_vs_stage_attitude_rad": attitude})
        print(json.dumps(part_b[-1]), flush=True)
    part_c = []
    for loads in ("frozen", "stage"):
        for gain in UPSET_GAINS:
            for dt_s in UPSET_STEPS_S:
                whole, after = altitude_errors(runner.frames(UPSET, loads, dt_s, altitude_d=gain))
                part_c.append({"loads": loads, "altitude_d": gain, "dt_s": dt_s,
                               "altitude_error_m": whole, "after_5_s_m": after})
                print(json.dumps(part_c[-1]), flush=True)
    results = {"frozen": {"implementation": "upstream C++", "branch": cpp["branch"],
                          "commit": cpp["commit"], "image": cpp["image"]},
               "stage": {"implementation": "this checkout",
                         "source_sha256": source_identity()["source_sha256"]},
               "part_a": part_a, "part_b": part_b, "part_c": part_c}
    (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    (output / "summary.md").write_text(summary(results))
    return 0


if __name__ == "__main__":
    sys.exit(main())
