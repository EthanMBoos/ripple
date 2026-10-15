#!/usr/bin/env python3
"""Is a long-run C++/Rust divergence the port, or the scenario's own sensitivity?

    python3 reference/divergence.py runs/reference-checkNNN --output runs/divergence000

For each case of a completed reference check, nudges each entity group's initial
roll, each autonomy's commanded speed (Straight) or velocity (ConstantVelocity),
and each motion model's bank limit by one unit in the last place (changes
that spawn rounding cannot absorb). It also scales each PID controller loop's
proportional gain by 1 + 1e-12. One-ulp parameter changes perturb the state far
less than the ~1e-13 m by which C++ and Rust already differ from rounding, and
none reaches a controller loop directly; a flight that is sensitive only in its
altitude or heading loop would otherwise go unnoticed. The script runs Rust on
the nudged mission and compares two growth curves: C++ versus Rust, and Rust
versus the nudged Rust. If a scenario amplifies rounding differences, both
curves cross the comparison tolerance at about the same time: the divergence is
a property of the dynamics, and a rounding difference such as a math library's
produces it. Standard library only.
"""

import argparse
import json
import math
from pathlib import Path
import subprocess

from compare import POSITION_TOLERANCE_M, distance
from frames import read_frames
from project import ROOT, build_cli
import scenario as missions
GAIN_NUDGE = 1e-12
# Default gains per loop, from each Rust controller
# (crates/core/src/plugin/controller/; AircraftPIDController's are its `a4d`
# airframe gains, which generated missions name); a mission's own gains
# replace them.
PID_DEFAULTS = {
    "SimpleAircraftControllerPID": {
        "heading_pid": {"kp": 1.0, "ki": 0.01, "kd": 2.0, "integrate_within_deg": 9.0},
        "altitude_pid": {"kp": 1.0, "ki": 0.0, "kd": 0.8, "integrate_within_m": 1.0},
        "speed_pid": {"kp": 1.0, "ki": 0.1, "kd": 0.0, "integrate_within_mps": 1.0},
    },
    "AircraftPIDController": {
        "heading_pid": {"kp": 1.0, "ki": 0.2, "kd": 1.0, "integrate_within_deg": 45.0},
        "altitude_pid": {"kp": 0.1, "ki": 0.0001, "kd": 0.002, "integrate_within_m": 10.0},
        "speed_pid": {"kp": 1.0, "ki": 0.1, "kd": 0.03, "integrate_within_mps": 10.0},
        "pitch_pid": {"kp": 5.0, "ki": 0.01, "kd": 0.01, "integrate_within_deg": 9.0},
        "roll_pid": {"kp": 0.3, "ki": 0.1, "kd": 0.001, "integrate_within_deg": 9.0},
    },
}


def up(value):
    return math.nextafter(value, math.inf)


def nudged(scenario):
    """The scenario a run recorded (its manifest's), nudged in place."""
    for group in scenario["entities"]:
        group["roll_deg"] = up(group["roll_deg"])
        motion = group["motion_model"]["params"]
        if "max_roll_deg" in motion:
            motion["max_roll_deg"] = up(motion["max_roll_deg"])
        for controller in group["controller"]:
            for loop, default in PID_DEFAULTS.get(controller["name"], {}).items():
                gains = dict(controller["params"].get(loop, default))
                gains["kp"] *= 1 + GAIN_NUDGE
                controller["params"][loop] = gains
        for autonomy in group["autonomy"]:
            params = autonomy["params"]
            if "speed_mps" in params:
                params["speed_mps"] = up(params["speed_mps"])
            if "velocity_mps" in params:
                params["velocity_mps"] = [up(params["velocity_mps"][0]), *params["velocity_mps"][1:]]
    return scenario


def growth(reference, candidate):
    """Per-frame (time, largest position difference over shared entities)."""
    curve = []
    for expected, actual in zip(read_frames(reference), read_frames(candidate)):
        shared = expected.entities.keys() & actual.entities.keys()
        worst = max((distance(expected.entities[i].position, actual.entities[i].position)
                     for i in shared), default=0.0)
        curve.append((expected.time_s, worst))
    return curve


def crossing(curve):
    return next((t for t, error in curve if error > POSITION_TOLERANCE_M), None)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("check", type=Path)
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    binary = build_cli()
    cases = []
    for result in sorted(args.check.glob("*/result.json")):
        case = result.parent
        info = json.loads(result.read_text())
        mission = missions.write(args.output / f"{case.name}.yaml",
                                 missions.as_yaml(nudged(missions.recorded(case / "rust-1"))))
        run = args.output / case.name
        subprocess.run([binary, "run", mission, "--root", ROOT, "--output", run,
                        "--workers", "1", "--no-rerun"], check=True, capture_output=True)
        cpp = case / info["cpp_log_directory"] / "frames.bin"
        port, control = growth(cpp, case / "rust-1/frames.bin"), growth(
            case / "rust-1/frames.bin", run / "frames.bin")
        cases.append({
            "case": info.get("case", case.name),
            "end_s": port[-1][0],
            "cpp_vs_rust_exceeds_tolerance_at_s": crossing(port),
            "rust_vs_nudged_rust_exceeds_tolerance_at_s": crossing(control),
            "cpp_vs_rust_final_m": port[-1][1],
            "rust_vs_nudged_rust_final_m": control[-1][1],
        })
        print(json.dumps(cases[-1]), flush=True)
    (args.output / "report.json").write_text(json.dumps({"cases": cases}, indent=2) + "\n")


if __name__ == "__main__":
    main()
