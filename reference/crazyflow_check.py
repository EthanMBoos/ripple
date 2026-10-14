#!/usr/bin/env python3
"""Does Multirotor's Crazyflie fly like Crazyflow's, given the same numbers?

    python3 reference/crazyflow_check.py --output runs/crazyflow000 [--python PATH]

Crazyflow (learnsyslab/crazyflow, MIT; Schuck et al. 2026) is an independent
Crazyflie simulator. Its `first_principles` dynamics take the same physics in
its own conventions: rotor speeds in RPM, x-y-z-w quaternions, body angular
velocity, a mixing matrix instead of rotor offsets, and a constant linear
drag matrix instead of per-rotor drag. This check gives it Ripple's
`crazyflie2` constants (reference/crazyflow/integrate.py maps them) and the
same constant, unequal motor commands from the same tilted, moving start, and
compares the two trajectories over 3 s.

- "no-drag": rotor drag off on both sides. The equations are the same, so
  the trajectories must agree to integration precision (pass: within 1e-6 m
  and 1e-6 rad).
- "drag": Ripple's per-rotor (RotorS) drag with Förster's coefficients, and
  Crazyflow's drag matrix set to Förster's lumped form at the commanded speeds,
  K_aero * (sum of speeds). These differ by design (Ripple's acts at each hub,
  so it also makes moments, and sees the body's rotation); the difference is
  reported, not judged.

Crazyflow runs in a uv environment with CRAZYFLOW (a pinned commit) unless
--python names an interpreter that has it. Writes OUTPUT/results.json.
"""
import argparse
import json
import math
from pathlib import Path
import subprocess
import sys

from frames import read_frames
from project import ROOT, build_cli, source_identity

CRAZYFLOW_COMMIT = "9e288eaffcc65fa4b8b3291992b7a98e65dc176b"
CRAZYFLOW = f"crazyflow @ git+https://github.com/learnsyslab/crazyflow@{CRAZYFLOW_COMMIT}"
HELPER = ROOT / "reference/crazyflow/integrate.py"
END_S = 3.0
DT_S = 0.001
FINE_STEPS_PER_TICK = 10
# Unequal speeds around the Crazyflie's hover (1900.85 rad/s): it climbs
# slowly while rolling, pitching, and yawing.
COMMANDS_RADPS = [1905.0, 1898.0, 1903.0, 1901.0]
START = dict(position_m=[0.0, 0.0, 2.0], velocity_mps=[0.4, -0.3, 0.1],
             roll_deg=4.0, pitch_deg=-3.0, heading_deg=20.0)

# The crazyflie2 preset (crates/core/src/plugin/motion/multirotor/airframe.rs).
MASS_KG = 0.028
C_T = 1.900523877693446e-08
INERTIA = [[16.571710e-6, 0.830806e-6, 0.718277e-6],
           [0.830806e-6, 16.655602e-6, 1.800197e-6],
           [0.718277e-6, 1.800197e-6, 29.261652e-6]]
ARM_M = 0.046
OFFSETS = [[ARM_M, 0, 0], [0, ARM_M, 0], [-ARM_M, 0, 0], [0, -ARM_M, 0]]
SIGNS = [1, -1, 1, -1]  # CW, CCW, CW, CCW
DRAG_IN_PLANE, DRAG_AXIAL = 9.1785e-7, 10.311e-7


def quaternion_wxyz(roll, pitch, yaw):
    cr, sr, cp, sp = math.cos(roll / 2), math.sin(roll / 2), math.cos(pitch / 2), math.sin(pitch / 2)
    cy, sy = math.cos(yaw / 2), math.sin(yaw / 2)
    return [cr * cp * cy + sr * sp * sy, sr * cp * cy - cr * sp * sy,
            cr * sp * cy + sr * cp * sy, cr * cp * sy - sr * sp * cy]


def mission(drag):
    motion = "        airframe: crazyflie2\n"
    if drag:
        motion += (f"        rotor_drag_coefficient: {DRAG_IN_PLANE!r}\n"
                   f"        rotor_drag_coefficient_axial: {DRAG_AXIAL!r}\n")
    speeds = ", ".join(repr(value) for value in COMMANDS_RADPS)
    return f"""format_version: 1
name: Crazyflow cross-check
run:
  end_s: {END_S}
  dt_s: {DT_S}
  seed: 1
entities:
  crazyflie:
    position_m: {START['position_m']}
    velocity_mps: {START['velocity_mps']}
    roll_deg: {START['roll_deg']}
    pitch_deg: {START['pitch_deg']}
    heading_deg: {START['heading_deg']}
    autonomy:
      ConstantVelocity:
    controller:
      MotorSpeeds:
        speeds: [{speeds}]
    motion_model:
      Multirotor:
{motion}"""


def angle_between(q, r):
    dot = abs(sum(a * b for a, b in zip(q, r)))
    return 2.0 * math.acos(min(1.0, dot))


def compare(variant, ripple_frames, crazyflow_samples):
    by_time = {round(sample["time_s"], 9): sample for sample in crazyflow_samples}
    errors = dict(position_m=0.0, velocity_mps=0.0, attitude_rad=0.0, angular_velocity_radps=0.0)
    compared = 0
    at = {}
    for frame in ripple_frames:
        # A frame labeled t holds the state at t.
        sample = by_time.get(round(frame.time_s, 9))
        if sample is None:
            raise ValueError(f"{variant}: no Crazyflow sample at {frame.time_s:.3f} s")
        values = [*sample["position_m"], *sample["velocity_mps"], *sample["orientation_wxyz"],
                  *sample["angular_velocity_world_radps"]]
        if not all(math.isfinite(value) for value in values):
            raise ValueError(f"{variant}: nonfinite Crazyflow state at {sample['time_s']} s")
        entity = next(iter(frame.entities.values()))
        position = math.dist(entity.position, sample["position_m"])
        errors["position_m"] = max(errors["position_m"], position)
        errors["velocity_mps"] = max(errors["velocity_mps"], math.dist(entity.velocity, sample["velocity_mps"]))
        errors["attitude_rad"] = max(errors["attitude_rad"],
                                     angle_between(entity.orientation, sample["orientation_wxyz"]))
        errors["angular_velocity_radps"] = max(
            errors["angular_velocity_radps"],
            math.dist(entity.angular_velocity, sample["angular_velocity_world_radps"]))
        for mark in (1.0, 2.0):
            if abs(frame.time_s - mark) < 1e-9:
                at[f"position_error_at_{mark:g}s_m"] = position
        compared += 1
    # Every state from the start to the end must be compared; a short or empty
    # reference cannot pass.
    expected = round(END_S / DT_S) + 1
    if compared != expected:
        raise ValueError(f"{variant}: compared {compared} frames, expected {expected}")
    travel = math.dist(ripple_frames[0].entities[next(iter(ripple_frames[0].entities))].position,
                       next(iter(ripple_frames[-1].entities.values())).position)
    return dict(variant=variant, frames_compared=compared, largest=errors, distance_flown_m=travel, **at)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--python", help="an interpreter with Crazyflow installed")
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    binary = build_cli("release")
    helper = [args.python, "-I"] if args.python else [
        "uv", "run", "--quiet", "--no-project", "--python", "3.12", "--with", CRAZYFLOW, "python", "-I"]

    results = {}
    for variant, drag in (("no-drag", False), ("drag", True)):
        folder = args.output / variant
        folder.mkdir()
        (folder / "mission.yaml").write_text(mission(drag))
        subprocess.run([binary, "run", folder / "mission.yaml", "--root", ROOT, "--output",
                        folder / "ripple", "--no-rerun"], check=True, capture_output=True)
        ripple_frames = read_frames(folder / "ripple/frames.bin")

        speed_sum_radps = sum(COMMANDS_RADPS)
        drag_matrix = [[0.0] * 3 for _ in range(3)]
        if drag:
            drag_matrix = [[-DRAG_IN_PLANE * speed_sum_radps, 0, 0],
                           [0, -DRAG_IN_PLANE * speed_sum_radps, 0],
                           [0, 0, -DRAG_AXIAL * speed_sum_radps]]
        case = dict(mass_kg=MASS_KG, gravity_mps2=9.81, inertia_kgm2=INERTIA, arm_m=ARM_M,
                    rotor_offsets_m=OFFSETS, reaction_signs=SIGNS, thrust_coefficient=C_T,
                    torque_coefficient=0.005964552 * C_T, drag_matrix=drag_matrix,
                    commands_radps=COMMANDS_RADPS, position_m=START["position_m"],
                    velocity_mps=START["velocity_mps"],
                    orientation_wxyz=quaternion_wxyz(math.radians(START["roll_deg"]),
                                                     math.radians(START["pitch_deg"]),
                                                     math.radians(START["heading_deg"])),
                    dt_s=DT_S / FINE_STEPS_PER_TICK, steps=round(END_S / DT_S) * FINE_STEPS_PER_TICK,
                    record_every=FINE_STEPS_PER_TICK)
        (folder / "crazyflow-case.json").write_text(json.dumps(case, indent=1))
        subprocess.run([*helper, str(HELPER), str(folder / "crazyflow-case.json"),
                        str(folder / "crazyflow.json")], check=True, capture_output=True)
        results[variant] = compare(variant, ripple_frames,
                                   json.loads((folder / "crazyflow.json").read_text()))

    no_drag = results["no-drag"]["largest"]
    passed = no_drag["position_m"] <= 1e-6 and no_drag["attitude_rad"] <= 1e-6
    report = dict(passed=passed, crazyflow_commit=CRAZYFLOW_COMMIT, commands_radps=COMMANDS_RADPS,
                  start=START, end_s=END_S, ripple_dt_s=DT_S,
                  crazyflow_dt_s=DT_S / FINE_STEPS_PER_TICK, results=results,
                  source=source_identity())
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: report[key] for key in ("passed", "results")}, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
