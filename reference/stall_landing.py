#!/usr/bin/env python3
"""Does a stall change ArduPlane's landing of the Rascal? FixedWing6DOF with and without tables.

    python3 reference/stall_landing.py --output runs/stall-landing000

Flies crates/ardupilot/missions/rascal-runway.yaml (takeoff, circuit, and
landing under ArduPlane SITL, which `ripple run` starts in Docker) four times:
"linear", the mission as committed (C++'s linear lift and zero-lift drag);
"linear-repeat", the same again, which must write byte-identical frames,
events, and summaries, so differences between variants are effects of the
tables; "lift", with JSBSim's Rascal `lift_table` added; and "lift-drag", with
`lift_table` and `drag_table` (values in book/src/guides/ardupilot.md). The
tables go into copies of the mission. Writes OUTPUT/results.json and
OUTPUT/summary.md. Needs Docker and Cargo; builds `ripple` with
`--features ardupilot`.

Touchdown is the first frame after the highest point of the flight with the
centre within 5 cm of its resting height; ground height is taken as the start
height (the runway centreline has under 0.2 m of bumps). Angle of attack is
measured in still air from 15 s before touchdown to the end, above 5 m/s,
split into airborne and rolling on the wheels (the Rascal rests 14 degrees
nose-up).
"""
import argparse
import filecmp
import json
import math
from pathlib import Path
import subprocess
import sys

from frames import read_frames
from project import BINARY, PACKAGE, ROOT, source_identity

MISSION = ROOT / "crates/ardupilot/missions/rascal-runway.yaml"
LIFT_TABLE = "[[-0.2, -0.75], [0.0, 0.25], [0.23, 1.4], [0.6, 0.71]]"
DRAG_TABLE = "[[-1.57, 1.5], [-0.26, 0.056], [0.0, 0.028], [0.26, 0.056], [1.57, 1.5]]"
VARIANTS = {"linear": {}, "linear-repeat": {}, "lift": {"lift_table": LIFT_TABLE},
            "lift-drag": {"lift_table": LIFT_TABLE, "drag_table": DRAG_TABLE}}
FILES = ("frames.bin", "events.json", "summary.csv")
START_EN_M = (-121.657, -290.181)
REST_Z_M = -22.685
HEADING_RAD = math.radians(-24.0)  # runway direction, ENU heading
DT_S = 0.0025


def offset_m(position):
    """Distance right of the runway centreline."""
    dx, dy = position[0] - START_EN_M[0], position[1] - START_EN_M[1]
    return dx * math.sin(HEADING_RAD) - dy * math.cos(HEADING_RAD)


def alpha_deg(orientation, velocity):
    """Angle of attack in still air, body forward/left/up, world ENU."""
    w, x, y, z = orientation
    r = [[1 - 2 * (y * y + z * z), 2 * (x * y - w * z), 2 * (x * z + w * y)],
         [2 * (x * y + w * z), 1 - 2 * (x * x + z * z), 2 * (y * z - w * x)],
         [2 * (x * z - w * y), 2 * (y * z + w * x), 1 - 2 * (x * x + y * y)]]
    body = [sum(r[i][j] * velocity[i] for i in range(3)) for j in range(3)]
    return math.degrees(math.atan2(-body[2], body[0]))


def summarize(run_dir):
    rows = [(f.time_s, e) for f in read_frames(Path(run_dir) / "frames.bin")
            for e in f.entities.values()]
    peak = max(range(len(rows)), key=lambda i: rows[i][1].position[2])
    touchdown = next(i for i in range(peak, len(rows))
                     if rows[i][1].position[2] - REST_Z_M < 0.05)
    t, e = rows[touchdown]
    after = rows[touchdown:touchdown + int(10 / DT_S)]
    approach = rows[touchdown - int(15 / DT_S):]
    def alphas(airborne):
        return [alpha_deg(r[1].orientation, r[1].velocity) for r in approach
                if math.hypot(*r[1].velocity) > 5.0
                and (r[1].position[2] - REST_Z_M > 0.05) == airborne]
    return {
        "touchdown_time_s": round(t, 3),
        "touchdown_ground_speed_mps": round(math.hypot(e.velocity[0], e.velocity[1]), 2),
        "touchdown_sink_mps": round(-e.velocity[2], 2),
        "max_bounce_height_m": round(max(r[1].position[2] - REST_Z_M for r in after), 2),
        "max_rollout_offset_m": round(max(abs(offset_m(r[1].position)) for r in rows[touchdown:]), 2),
        "final_offset_m": round(offset_m(rows[-1][1].position), 2),
        # From 15 s before touchdown to the end, above 5 m/s; on the ground the
        # Rascal rests 14 degrees nose-up, so airborne and rolling are split.
        "max_alpha_airborne_deg": round(max(alphas(True)), 1),
        "max_alpha_on_ground_deg": round(max(alphas(False)), 1),
    }


def mission_copy(tables, path):
    """The runway mission with the given FixedWing6DOF parameters added."""
    text = MISSION.read_text()
    terrain = next(line for line in text.splitlines() if line.startswith("terrain: "))
    text = text.replace(terrain, f"terrain: {(MISSION.parent / terrain[9:]).resolve()}")
    anchor = "    motion_model:\n      FixedWing6DOF:\n"
    if text.count(anchor) != 1:
        raise RuntimeError(f"expected one FixedWing6DOF block in {MISSION}")
    added = "".join(f"        {key}: {value}\n" for key, value in tables.items())
    path.write_text(text.replace(anchor, anchor + added))
    return path


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True)
    with (output / "build.log").open("w") as log:
        subprocess.run(["cargo", "build", "--release", "--locked", "--package", PACKAGE,
                        "--bin", BINARY, "--features", "ardupilot"], cwd=ROOT, check=True,
                       stdout=log, stderr=subprocess.STDOUT)
    binary = ROOT / "target/release" / BINARY
    (output / "missions").mkdir()
    results = {}
    for variant, tables in VARIANTS.items():
        mission = mission_copy(tables, output / "missions" / f"{variant}.yaml")
        with (output / f"{variant}.log").open("w") as log:
            subprocess.run([binary, "run", mission, "--root", ROOT, "--output", output / variant,
                            "--headless", "--no-rerun"], cwd=ROOT, check=True,
                           stdout=log, stderr=subprocess.STDOUT)
        results[variant] = summarize(output / variant)
        print(variant, json.dumps(results[variant]), flush=True)
    repeatable = all(filecmp.cmp(output / "linear" / file, output / "linear-repeat" / file,
                                 shallow=False) for file in FILES)
    report = {"passed": repeatable, "repeat_byte_identical": repeatable,
              "mission": str(MISSION.relative_to(ROOT)), "tables": VARIANTS,
              "source_sha256": source_identity()["source_sha256"], "results": results}
    (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    rows = ["# Stall tables and the Rascal runway landing", "",
            "Generated by reference/stall_landing.py; see its docstring for the method.",
            f"The repeat of `linear` was {'byte-identical' if repeatable else 'DIFFERENT'}.", "",
            "| Variant | Touchdown speed (m/s) | Sink (m/s) | Highest bounce (m) | "
            "Largest rollout offset (m) | Airborne alpha (deg) | Rolling alpha (deg) |",
            "|---|---|---|---|---|---|---|"]
    rows += [f"| {variant} | {r['touchdown_ground_speed_mps']} | {r['touchdown_sink_mps']} | "
             f"{r['max_bounce_height_m']} | {r['max_rollout_offset_m']} | "
             f"{r['max_alpha_airborne_deg']} | {r['max_alpha_on_ground_deg']} |"
             for variant, r in results.items()]
    (output / "summary.md").write_text("\n".join(rows) + "\n")
    return 0 if repeatable else 1


if __name__ == "__main__":
    sys.exit(main())
