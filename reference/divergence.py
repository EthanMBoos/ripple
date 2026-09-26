#!/usr/bin/env python3
"""Is a long-run C++/Rust divergence the port, or the scenario's own sensitivity?

    python3 reference/divergence.py runs/reference-checkNNN --output runs/divergence000

For each case of a completed reference check, nudges each entity group's initial
roll, each autonomy's commanded speed (Straight) or velocity (ConstantVelocity),
and each motion model's turning radius by one unit in the last place (changes
that spawn rounding cannot absorb), runs Rust on the nudged mission, and compares two growth curves: C++ versus Rust, and Rust versus the
nudged Rust. If a scenario amplifies rounding differences, both curves cross the
comparison tolerance at about the same time: the divergence is a property of
the dynamics, and any one-ulp difference (such as a math library's) produces it.
Standard library only.
"""

import argparse
import json
import math
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET

from compare import POSITION_TOLERANCE_M, distance
from frames import read_frames

ROOT = Path(__file__).resolve().parents[1]


def nudged(mission, output):
    tree = ET.parse(mission)
    for roll in tree.getroot().iter("roll"):
        roll.text = repr(math.nextafter(float(roll.text), math.inf))
    for motion in tree.getroot().iter("motion_model"):
        if "turning_radius" in motion.attrib:
            radius = float(motion.get("turning_radius"))
            motion.set("turning_radius", repr(math.nextafter(radius, math.inf)))
    for autonomy in tree.getroot().iter("autonomy"):
        if "speed" in autonomy.attrib:
            autonomy.set("speed", repr(math.nextafter(float(autonomy.get("speed")), math.inf)))
        if "velocity" in autonomy.attrib:
            first, *rest = autonomy.get("velocity").split()
            autonomy.set("velocity", " ".join([repr(math.nextafter(float(first), math.inf)),
                                               *rest]))
    tree.write(output, encoding="utf-8", xml_declaration=True)


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
    subprocess.run(["cargo", "build", "--locked", "--package", "scrimmage-rs", "--bin",
                    "scrimmage"], cwd=ROOT, check=True, capture_output=True)
    binary = ROOT / "target/debug/scrimmage"
    cases = []
    for result in sorted(args.check.glob("*/result.json")):
        case = result.parent
        info = json.loads(result.read_text())
        mission = args.output / f"{case.name}.xml"
        nudged(case / "input/input.xml", mission)
        run = args.output / case.name
        subprocess.run([binary, "run", mission, "--root", ROOT, "--output", run,
                        "--workers", "1", "--no-rerun"], check=True, capture_output=True)
        cpp = case / info["cpp_log_directory"] / "frames.bin"
        port, control = growth(cpp, case / "rust-1/frames.bin"), growth(
            case / "rust-1/frames.bin", run / "frames.bin")
        cases.append({
            "mission": Path(info["mission"]).name,
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
