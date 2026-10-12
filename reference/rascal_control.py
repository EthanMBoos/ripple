#!/usr/bin/env python3
"""How AircraftPIDController's `rascal` settings fly FixedWing6DOF's Rascal.

    python3 reference/rascal_control.py --output runs/rascal-control000

The `rascal` settings are the `a4d` gains with ArduPlane's attitude limits
(pitch 20, roll 45 degrees); `a4d`'s are 45 and 60. This reruns the
measurements behind that choice, on the Rascal at 20 m/s with
`airframe: rascal` on both plugins, and writes OUTPUT/results.json and
OUTPUT/summary.md.

Part A, the altitude derivative gain: `a4d`'s 0.002 was chosen for a 220 m/s
jet. After a 5/3 and a 60/30 degree roll/pitch upset, at 0.01, 0.005, and
0.0025 s steps, with altitude D 0, 0.002, 0.01, and 0.02: the worst altitude
error after the first 5 s, and the worst heading error.

Part B, the attitude limits: heading (+90, +45 degrees), altitude (+-20 m),
and speed (25, 15 m/s) steps commanded by WaypointFollower toward a waypoint
5 km away, under 45/60 and 20/45 degree limits: rise, overshoot, settling
within 5%, and the largest pitch and roll. Heading is measured against the
bearing to the waypoint at each moment.

Passes when the derivative gain changes Part A's altitude error by under
1 cm, and under the 20/45 limits every Part B step settles within 5% and
pitch stays under 25 degrees. Standard library only; needs Cargo.
"""
import argparse
import json
import math
from pathlib import Path
import subprocess
import sys

from frames import read_frames
from project import ROOT, build_cli, source_identity

ALTITUDE_M = 200.0
SPEED_MPS = 20.0
UPSETS_DEG = [(5, 3), (60, 30)]
STEPS_S = [0.01, 0.005, 0.0025]
ALTITUDE_D = [0.0, 0.002, 0.01, 0.02]
LIMITS_DEG = {"a4d limits 45/60": (45, 60), "rascal limits 20/45": (20, 45)}
STEPS = {  # name: (waypoint, speed, what, commanded change)
    "heading +90": ([0, 5000, ALTITUDE_M], 20, "heading", 90.0),
    "heading +45": ([5000, 5000, ALTITUDE_M], 20, "heading", 45.0),
    "altitude +20": ([5000, 0, ALTITUDE_M + 20], 20, "altitude", 20.0),
    "altitude -20": ([5000, 0, ALTITUDE_M - 20], 20, "altitude", -20.0),
    "speed 25": ([5000, 0, ALTITUDE_M], 25, "speed", 5.0),
    "speed 15": ([5000, 0, ALTITUDE_M], 15, "speed", -5.0),
}


def mission(dt_s, end_s, entities):
    lines = [f"format_version: 1\nname: rascal-control\nrun:\n  end_s: {end_s}\n  dt_s: {dt_s}\n"
             "  seed: 1\nnetworks:\n  LocalNetwork:\n  GlobalNetwork:\nentities:"]
    for index, (label, body) in enumerate(entities):
        lines.append(f"  e{index:02}:  # {label}\n    position_m: [0, {400 * index}, {ALTITUDE_M}]\n{body}")
    return "\n".join(lines) + "\n"


def controller(altitude_d=None, limits=None):
    text = "    controller:\n      AircraftPIDController:\n        airframe: rascal\n"
    if altitude_d is not None:
        text += f"        altitude_pid: [0.1, 0.0001, {altitude_d!r}, 10]\n"
    if limits is not None:
        text += f"        max_pitch: {limits[0]}\n        max_roll: {limits[1]}\n"
    return text + "    motion_model:\n      FixedWing6DOF:\n        airframe: rascal\n"


def run(binary, output, name, text):
    path = output / f"{name}.yaml"
    path.write_text(text)
    subprocess.run([binary, "run", path, "--root", ROOT, "--output", output / name, "--no-rerun"],
                   check=True, capture_output=True)
    return read_frames(output / name / "frames.bin")


def yaw_deg(q):
    w, x, y, z = q
    return math.degrees(math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z)))


def roll_pitch_deg(q):
    w, x, y, z = q
    return (math.degrees(math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y))),
            math.degrees(math.asin(max(-1.0, min(1.0, 2 * (w * y - z * x))))))


def wrap_deg(angle):
    return (angle + 180) % 360 - 180


def track(frames, entity_id):
    # The last frame repeats the final time.
    return [(f.time_s, f.entities[entity_id]) for f in frames[:-1] if entity_id in f.entities]


def part_a(binary, output):
    rows = []
    for dt_s in STEPS_S:
        entities, keys = [], []
        for upset in UPSETS_DEG:
            for d in ALTITUDE_D:
                body = (f"    velocity_mps: [{SPEED_MPS * math.cos(math.radians(30))!r}, "
                        f"{SPEED_MPS * math.sin(math.radians(30))!r}, 0]\n"
                        f"    heading_deg: 30\n    roll_deg: {upset[0]}\n    pitch_deg: {upset[1]}\n"
                        f"    autonomy:\n      Straight:\n        speed: {SPEED_MPS}\n" + controller(d))
                entities.append((f"upset {upset} D {d}", body))
                keys.append((upset, d))
        frames = run(binary, output, f"upsets-dt{dt_s}", mission(dt_s, 45, entities))
        for entity_id, (upset, d) in enumerate(keys, start=1):
            points = track(frames, entity_id)
            rows.append(dict(
                upset_deg=list(upset), dt_s=dt_s, altitude_d=d,
                altitude_peak_m=max(abs(e.position[2] - ALTITUDE_M) for t, e in points),
                altitude_after_5s_m=max(abs(e.position[2] - ALTITUDE_M) for t, e in points if t >= 5),
                heading_after_5s_deg=max(abs(wrap_deg(yaw_deg(e.orientation) - 30))
                                         for t, e in points if t >= 5)))
    spread = 0.0
    for upset in UPSETS_DEG:
        for dt_s in STEPS_S:
            errors = [r["altitude_after_5s_m"] for r in rows
                      if r["upset_deg"] == list(upset) and r["dt_s"] == dt_s]
            spread = max(spread, max(errors) - min(errors))
    return rows, spread


def part_b(binary, output):
    entities, keys = [], []
    for limit_name, limits in LIMITS_DEG.items():
        for step_name, (waypoint, speed, what, change) in STEPS.items():
            # Waypoints are relative to the vehicle's start (entities sit 400 m apart).
            target = [waypoint[0], waypoint[1] + 400 * len(entities), waypoint[2]]
            body = (f"    velocity_mps: [{SPEED_MPS}, 0, 0]\n    heading_deg: 0\n"
                    f"    autonomy:\n      WaypointFollower:\n        waypoints: [{target}]\n"
                    f"        arrival_radius_m: 40\n        speed: {speed}\n" + controller(limits=limits))
            entities.append((f"{limit_name}, {step_name}", body))
            keys.append((limit_name, step_name))
    frames = run(binary, output, "steps", mission(0.01, 60, entities))
    rows = []
    for entity_id, (limit_name, step_name) in enumerate(keys, start=1):
        waypoint, speed, what, change = STEPS[step_name]
        points = track(frames, entity_id)
        start_y = points[0][1].position[1]

        def error(e):
            """Remaining error toward the command, in the command's units."""
            if what == "heading":
                bearing = math.degrees(math.atan2(waypoint[1] + start_y - e.position[1],
                                                  waypoint[0] - e.position[0]))
                return wrap_deg(yaw_deg(e.orientation) - bearing)
            if what == "altitude":
                return e.position[2] - waypoint[2]
            return math.hypot(*e.velocity) - speed

        start_error = -change
        progress = [(t, 1 - error(e) / start_error) for t, e in points]
        rise10 = next((t for t, p in progress if p >= 0.1), None)
        rise90 = next((t for t, p in progress if p >= 0.9), None)
        settled = next((progress[k][0] for k in range(len(progress))
                        if all(abs(p - 1) <= 0.05 for _, p in progress[k:])), None)
        rows.append(dict(
            limits=limit_name, step=step_name,
            rise_s=None if rise10 is None or rise90 is None else rise90 - rise10,
            overshoot=max(p for _, p in progress) - 1,
            settling_5pct_s=settled,
            largest_pitch_deg=max(abs(roll_pitch_deg(e.orientation)[1]) for _, e in points),
            largest_roll_deg=max(abs(roll_pitch_deg(e.orientation)[0]) for _, e in points)))
    return rows


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    binary = build_cli("release")

    upsets, spread_m = part_a(binary, args.output)
    steps = part_b(binary, args.output)
    rascal = [r for r in steps if r["limits"] == "rascal limits 20/45"]
    passed = (spread_m < 0.01
              and all(r["settling_5pct_s"] is not None for r in rascal)
              and all(r["largest_pitch_deg"] < 25 for r in rascal))
    report = dict(passed=passed, source=source_identity(), upsets=upsets,
                  altitude_d_spread_m=spread_m, steps=steps)
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")

    def number(value, digits=1):
        return "-" if value is None else f"{value:.{digits}f}"

    lines = ["# Rascal control", "",
             f"FixedWing6DOF and AircraftPIDController with `airframe: rascal`, 20 m/s. "
             f"Passed: {passed}.", "",
             "## Part A: altitude derivative gain after upsets", "",
             "| Upset (roll/pitch deg) | Step (s) | Altitude D | Worst altitude error (m) "
             "| After 5 s (m) | Heading after 5 s (deg) |",
             "| --- | --- | --- | --- | --- | --- |"]
    for r in upsets:
        lines.append(f"| {r['upset_deg'][0]}/{r['upset_deg'][1]} | {r['dt_s']} | {r['altitude_d']} | "
                     f"{r['altitude_peak_m']:.3f} | {r['altitude_after_5s_m']:.3f} | "
                     f"{r['heading_after_5s_deg']:.2f} |")
    lines += ["", f"Largest change in the after-5 s altitude error across D: {spread_m * 100:.2f} cm.",
              "", "## Part B: steps under each attitude limit", "",
              "| Limits | Step | Rise 10-90% (s) | Overshoot | Settles within 5% (s) "
              "| Largest pitch (deg) | Largest roll (deg) |",
              "| --- | --- | --- | --- | --- | --- | --- |"]
    for r in steps:
        lines.append(f"| {r['limits']} | {r['step']} | {number(r['rise_s'])} | "
                     f"{100 * r['overshoot']:.0f}% | {number(r['settling_5pct_s'])} | "
                     f"{r['largest_pitch_deg']:.0f} | {r['largest_roll_deg']:.0f} |")
    (args.output / "summary.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
