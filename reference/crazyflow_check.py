#!/usr/bin/env python3
"""Compare Multirotor's Crazyflie and Firefly with Crazyflow's dynamics.

    python3 reference/crazyflow_check.py --output DIR [--python PATH]

At held states, both vehicles' loads without drag must match Crazyflow's
independent calculation and the committed Rust test fixture. Five open-loop
Crazyflie flights then compare trajectories with the same motor commands.

No-drag flights compare shared equations. The drag flights report the
difference between Ripple's per-rotor model and Crazyflow's lumped model;
that difference is not a pass criterion. For the hub-drag flights, Ripple's
drag force and moment are supplied to Crazyflow at every integration stage,
so the trajectories must agree within LIMITS. The stress flight also passes
force alone to isolate the missing moment. Each run checks its step-size
sensitivity against the same limits.

Writes OUTPUT/results.json and OUTPUT/samples.json. --update-fixture
replaces the committed samples. Crazyflow runs at a pinned commit via uv;
--python PATH selects an existing environment.
"""
import argparse
import json
import math
import os
from pathlib import Path
import subprocess
import sys

from frames import read_frames
from multirotor_grid import (VEHICLES, held_states, hover_speed_radps, largest_relative_difference,
                             samples_text)
from project import CORE_PACKAGE, ROOT, build_cli, source_identity

CRAZYFLOW_COMMIT = "9e288eaffcc65fa4b8b3291992b7a98e65dc176b"
CRAZYFLOW = f"crazyflow @ git+https://github.com/learnsyslab/crazyflow@{CRAZYFLOW_COMMIT}"
HELPER = ROOT / "reference/crazyflow/integrate.py"
LOADS_HELPER = ROOT / "reference/crazyflow/loads.py"
FIXTURE = ROOT / "crates/core/tests/fixtures/crazyflow_loads_samples.json"
RUST_TEST = "plugin::motion::multirotor::tests::crazyflow_loads_samples"
DT_S = 0.001
FINE_STEPS_PER_TICK = 10
# Step-size sensitivities: Ripple at a quarter step, Crazyflow at half.
RIPPLE_FINER = 4
CRAZYFLOW_FINER = 2

# The crazyflie2 preset (crates/core/src/plugin/motion/multirotor/airframe.rs),
# and Förster's drag coefficients.
CRAZYFLIE = VEHICLES["crazyflie2"]
DRAG_IN_PLANE, DRAG_AXIAL = CRAZYFLIE["rotors"]["rotor_drag_coefficient"], 10.311e-7
HOVER_RADPS = hover_speed_radps(CRAZYFLIE)  # 1900.85


def start(position_m, velocity_mps, roll_deg=0.0, pitch_deg=0.0, heading_deg=0.0):
    return dict(position_m=position_m, velocity_mps=velocity_mps, roll_deg=roll_deg,
                pitch_deg=pitch_deg, heading_deg=heading_deg)


# Rotor order: +x (CW), +y (CCW), -x (CW), -y (CCW). Commands as multiples
# of the hover speed, except the stress flight's, kept as first written.
FLIGHTS = {
    "hover": dict(end_s=4.0, start=start([0.0, 0.0, 5.0], [1.0, 0.3, 0.0]),
                  commands_radps=[HOVER_RADPS] * 4),
    "climb": dict(end_s=4.0, start=start([0.0, 0.0, 5.0], [0.5, 0.0, 0.2]),
                  commands_radps=[1.015 * HOVER_RADPS] * 4),
    # Each clockwise rotor faster, each counterclockwise slower: the net
    # thrust stays at hover and the reaction torques spin the vehicle up.
    "yaw": dict(end_s=4.0, start=start([0.0, 0.0, 5.0], [0.3, -0.2, 0.0]),
                commands_radps=[factor * HOVER_RADPS for factor in (1.0025, 0.9975, 1.0025, 0.9975)]),
    "tilt": dict(end_s=4.0, start=start([0.0, 0.0, 5.0], [0.0, 0.0, 0.0], 5.0, -3.0, 20.0),
                 commands_radps=[HOVER_RADPS] * 4),
    "stress": dict(end_s=3.0, start=start([0.0, 0.0, 2.0], [0.4, -0.3, 0.1], 4.0, -3.0, 20.0),
                   commands_radps=[1905.0, 1898.0, 1903.0, 1901.0]),
}
# Each variant: Ripple's rotor drag on or off, and Crazyflow's drag: none,
# its lumped matrix, or Ripple's hub drag fed in as a force or as force and
# torque.
VARIANTS = {"no-drag": (False, None), "drag": (True, "lumped"),
            "hub-drag-force": (True, "force"), "hub-drag": (True, "force-and-torque")}
PASS_VARIANTS = ("no-drag", "hub-drag")
# Set on 2026-11-03 from the sensitivities then measured over the five
# flights: Ripple at a quarter step at most 3.9e-12 m, 9.1e-12 m/s, 1.5e-13
# rad, and 2.1e-13 rad/s; Crazyflow at half its step at most 3.5e-13 m,
# 3.2e-13 m/s, 8.1e-14 rad, and 3.2e-14 rad/s (the stress flight sets every
# largest value but the attitude's, the yaw flight's). Each limit is a
# hundred times the larger of the two, rounded up to a power of ten.
LIMITS = dict(position_m=1e-9, velocity_mps=1e-9, attitude_rad=1e-10, angular_velocity_radps=1e-10)


def quaternion_wxyz(roll, pitch, yaw):
    cr, sr, cp, sp = math.cos(roll / 2), math.sin(roll / 2), math.cos(pitch / 2), math.sin(pitch / 2)
    cy, sy = math.cos(yaw / 2), math.sin(yaw / 2)
    return [cr * cp * cy + sr * sp * sy, sr * cp * cy - cr * sp * sy,
            cr * sp * cy + sr * cp * sy, cr * cp * sy - sr * sp * cy]


def mission(flight, drag, motion_multiplier=1):
    motion = "        airframe: crazyflie2\n"
    if drag:
        motion += (f"        rotor_drag_coefficient: {DRAG_IN_PLANE!r}\n"
                   f"        rotor_drag_coefficient_axial: {DRAG_AXIAL!r}\n")
    speeds = ", ".join(repr(value) for value in flight["commands_radps"])
    begin = flight["start"]
    return f"""format_version: 1
name: Crazyflow cross-check
run:
  end_s: {flight['end_s']}
  dt_s: {DT_S}
  motion_multiplier: {motion_multiplier}
  seed: 1
entities:
  crazyflie:
    position_m: {begin['position_m']}
    velocity_mps: {begin['velocity_mps']}
    roll_deg: {begin['roll_deg']}
    pitch_deg: {begin['pitch_deg']}
    heading_deg: {begin['heading_deg']}
    autonomy:
      ConstantVelocity:
    controller:
      MotorSpeeds:
        speeds_radps: [{speeds}]
    motion_model:
      Multirotor:
{motion}"""


def reaction_signs(vehicle):
    """Ripple's sign of each rotor's reaction torque about +z: a clockwise
    rotor (seen from above) twists the body counterclockwise."""
    return [1 if direction == "cw" else -1 for direction in vehicle["directions"]]


def crazyflow_vehicle(vehicle):
    """The vehicle's constants as crazyflow/integrate.py and loads.py take them."""
    return dict(mass_kg=vehicle["mass_kg"], gravity_mps2=vehicle["gravity_mps2"],
                inertia_kgm2=vehicle["inertia_kgm2"], arm_m=vehicle["arm_m"],
                rotor_offsets_m=vehicle["rotor_offsets_m"], reaction_signs=reaction_signs(vehicle),
                thrust_coefficient=vehicle["thrust_coefficient"],
                torque_coefficient=vehicle["torque_coefficient"])


def loads_grid(helper, folder, update_fixture):
    """Crazyflow's loads at every held state of every vehicle, the Rust test
    on them, and the samples against the committed fixture."""
    folder.mkdir()
    vehicles = {}
    for name, vehicle in VEHICLES.items():
        states = list(held_states(vehicle))
        case = dict(crazyflow_vehicle(vehicle), states=states)
        (folder / f"{name}-case.json").write_text(json.dumps(case, indent=1))
        subprocess.run([*helper, str(LOADS_HELPER), str(folder / f"{name}-case.json"),
                        str(folder / f"{name}-loads.json")], check=True, capture_output=True)
        loads = json.loads((folder / f"{name}-loads.json").read_text())
        c_t, c_q = vehicle["thrust_coefficient"], vehicle["torque_coefficient"]
        for state, load in zip(states, loads, strict=True):
            # Scale: the rotors' own loads, thrust and its moment about the
            # center of mass plus the reaction torque, summed over the rotors.
            thrusts = [c_t * speed * speed for speed in state["speeds_radps"]]
            state.update(load, force_scale_n=sum(thrusts),
                         moment_scale_nm=sum(thrusts) * (vehicle["arm_m"] + c_q / c_t))
        vehicles[name] = dict(mission_keys={"airframe": name}, samples=states)
    samples_path = folder / "samples.json"
    samples_path.write_text(samples_text(dict(crazyflow_commit=CRAZYFLOW_COMMIT,
                                              vehicles=vehicles)))
    fixture_difference = None
    if update_fixture:
        FIXTURE.write_text(samples_path.read_text())
    else:
        fixture_difference = largest_relative_difference(
            json.loads(FIXTURE.read_text()), json.loads(samples_path.read_text()))
    test = subprocess.run(
        ["cargo", "test", "-p", CORE_PACKAGE, "--lib", "crazyflow_loads_samples", "--",
         "--nocapture", "--exact", RUST_TEST],
        cwd=ROOT, capture_output=True, text=True,
        env={**os.environ, "RIPPLE_CRAZYFLOW_SAMPLES": str(samples_path.resolve())})
    (folder / "rust-test.log").write_text(test.stdout + test.stderr)
    errors = None
    for line in test.stdout.splitlines():
        if line.startswith("CRAZYFLOW_ERRORS "):
            errors = json.loads(line.removeprefix("CRAZYFLOW_ERRORS "))
    return dict(samples={name: len(vehicle["samples"]) for name, vehicle in vehicles.items()},
                rust_largest_errors=errors if test.returncode == 0 else None,
                fixture_largest_relative_difference=fixture_difference)


def angle_between(q, r):
    """The rotation angle between two unit quaternions. For an angle a,
    |q - r| = 2 sin(a/4) and |q + r| = 2 cos(a/4) (with r's sign chosen for
    the smaller angle), so this resolves small angles; acos of the dot
    product cannot tell angles under about 3e-8 rad apart."""
    same = math.dist(q, r)
    flipped = math.dist(q, [-value for value in r])
    return 4.0 * math.atan2(min(same, flipped), max(same, flipped))


def state_errors(left, right):
    """The four differences between two states (dicts with Crazyflow's keys)."""
    return dict(position_m=math.dist(left["position_m"], right["position_m"]),
                velocity_mps=math.dist(left["velocity_mps"], right["velocity_mps"]),
                attitude_rad=angle_between(left["orientation_wxyz"], right["orientation_wxyz"]),
                angular_velocity_radps=math.dist(left["angular_velocity_world_radps"],
                                                 right["angular_velocity_world_radps"]))


def ripple_states(frames):
    """Ripple's frames as states with Crazyflow's keys, by time."""
    states = {}
    for frame in frames:
        entity = next(iter(frame.entities.values()))
        states[round(frame.time_s, 9)] = dict(
            position_m=list(entity.position), velocity_mps=list(entity.velocity),
            orientation_wxyz=list(entity.orientation),
            angular_velocity_world_radps=list(entity.angular_velocity))
    return states


def largest_errors(name, end_s, left, right):
    """The largest of each difference over every tick from start to end;
    both sides must have every tick, so a short reference cannot pass."""
    errors = dict(position_m=0.0, velocity_mps=0.0, attitude_rad=0.0, angular_velocity_radps=0.0)
    for tick in range(round(end_s / DT_S) + 1):
        time_s = round(tick * DT_S, 9)
        if time_s not in left or time_s not in right:
            raise ValueError(f"{name}: no state at {time_s:.3f} s")
        for key, value in state_errors(left[time_s], right[time_s]).items():
            if not math.isfinite(value):
                raise ValueError(f"{name}: nonfinite state at {time_s} s")
            errors[key] = max(errors[key], value)
    return errors


def compare(name, end_s, ripple, crazyflow):
    """Ripple against Crazyflow over a flight: the largest errors, and at
    each whole second the position error and the spin on both sides."""
    result = dict(largest=largest_errors(name, end_s, ripple, crazyflow))
    for mark in range(1, int(end_s) + 1):
        result[f"position_error_at_{mark}s_m"] = state_errors(ripple[mark], crazyflow[mark])["position_m"]
        result[f"spin_at_{mark}s_radps"] = dict(
            ripple=math.hypot(*ripple[mark]["angular_velocity_world_radps"]),
            crazyflow=math.hypot(*crazyflow[mark]["angular_velocity_world_radps"]))
    return result


def reached(flight, ripple):
    """What the flight reached in Ripple: altitude, speed, rates, commands."""
    states = ripple.values()
    return dict(
        min_altitude_m=min(state["position_m"][2] for state in states),
        max_speed_mps=max(math.hypot(*state["velocity_mps"]) for state in states),
        max_rate_radps=max(math.hypot(*state["angular_velocity_world_radps"]) for state in states),
        commands_over_hover=[min(flight["commands_radps"]) / HOVER_RADPS,
                             max(flight["commands_radps"]) / HOVER_RADPS],
        distance_flown_m=math.dist(ripple[0.0]["position_m"],
                                   ripple[max(ripple)]["position_m"]))


def run_ripple(binary, folder, flight, drag, motion_multiplier=1):
    folder.mkdir()
    (folder / "mission.yaml").write_text(mission(flight, drag, motion_multiplier))
    subprocess.run([binary, "run", folder / "mission.yaml", "--root", ROOT, "--output",
                    folder / "ripple", "--headless"], check=True, capture_output=True)
    return ripple_states(read_frames(folder / "ripple/run.mcap"))


def run_crazyflow(helper, folder, flight, crazyflow_drag, fine_steps=FINE_STEPS_PER_TICK):
    folder.mkdir(exist_ok=True)
    speed_sum_radps = sum(flight["commands_radps"])
    drag_matrix = [[0.0] * 3 for _ in range(3)]
    if crazyflow_drag == "lumped":
        drag_matrix = [[-DRAG_IN_PLANE * speed_sum_radps, 0, 0],
                       [0, -DRAG_IN_PLANE * speed_sum_radps, 0],
                       [0, 0, -DRAG_AXIAL * speed_sum_radps]]
    hub_drag = None
    if crazyflow_drag in ("force", "force-and-torque"):
        hub_drag = dict(in_plane=DRAG_IN_PLANE, axial=DRAG_AXIAL,
                        torque=crazyflow_drag == "force-and-torque")
    begin = flight["start"]
    case = dict(crazyflow_vehicle(CRAZYFLIE), drag_matrix=drag_matrix, hub_drag=hub_drag,
                commands_radps=flight["commands_radps"], position_m=begin["position_m"],
                velocity_mps=begin["velocity_mps"],
                orientation_wxyz=quaternion_wxyz(math.radians(begin["roll_deg"]),
                                                 math.radians(begin["pitch_deg"]),
                                                 math.radians(begin["heading_deg"])),
                dt_s=DT_S / fine_steps, steps=round(flight["end_s"] / DT_S) * fine_steps,
                record_every=fine_steps)
    (folder / "crazyflow-case.json").write_text(json.dumps(case, indent=1))
    subprocess.run([*helper, str(HELPER), str(folder / "crazyflow-case.json"),
                    str(folder / "crazyflow.json")], check=True, capture_output=True)
    samples = json.loads((folder / "crazyflow.json").read_text())
    return {round(sample["time_s"], 9): sample for sample in samples}


def fly(binary, helper, folder, name, flight):
    """One flight in every variant, its sensitivities, and what it reached."""
    folder.mkdir()
    variants = {}
    ripple = {}
    for variant, (drag, crazyflow_drag) in VARIANTS.items():
        if variant == "hub-drag-force" and name != "stress":
            continue
        if drag not in ripple:
            ripple[drag] = run_ripple(binary, folder / f"ripple-{'drag' if drag else 'no-drag'}",
                                      flight, drag)
        crazyflow = run_crazyflow(helper, folder / variant, flight, crazyflow_drag)
        variants[variant] = compare(f"{name}/{variant}", flight["end_s"], ripple[drag], crazyflow)
        variants[variant]["crazyflow"] = crazyflow
    # Sensitivities on the hub-drag variant: each side at a finer step
    # against itself.
    finer_ripple = run_ripple(binary, folder / "ripple-finer", flight, True, RIPPLE_FINER)
    finer_crazyflow = run_crazyflow(helper, folder / "crazyflow-finer", flight,
                                    "force-and-torque", FINE_STEPS_PER_TICK * CRAZYFLOW_FINER)
    sensitivity = dict(
        ripple_quarter_step=largest_errors(f"{name}/ripple-finer", flight["end_s"],
                                           ripple[True], finer_ripple),
        crazyflow_half_step=largest_errors(f"{name}/crazyflow-finer", flight["end_s"],
                                           variants["hub-drag"]["crazyflow"], finer_crazyflow))
    if name == "stress":
        # Crazyflow's rotation under the force alone is its no-drag rotation,
        # the same arithmetic on the same numbers; a difference would mean
        # the feed reached the wrong input.
        error = dict(orientation=0.0, angular_velocity_radps=0.0)
        for time_s, fed in variants["hub-drag-force"]["crazyflow"].items():
            free = variants["no-drag"]["crazyflow"][time_s]
            error["orientation"] = max(error["orientation"],
                                       math.dist(fed["orientation_wxyz"], free["orientation_wxyz"]))
            error["angular_velocity_radps"] = max(
                error["angular_velocity_radps"],
                math.dist(fed["angular_velocity_world_radps"], free["angular_velocity_world_radps"]))
        variants["hub-drag-force"]["crazyflow_rotation_vs_no_drag"] = error
    for variant in variants.values():
        del variant["crazyflow"]
    within = all(variants[variant]["largest"][key] <= limit
                 for variant in PASS_VARIANTS for key, limit in LIMITS.items())
    resolved = all(side[key] <= limit for side in sensitivity.values()
                   for key, limit in LIMITS.items())
    identity = (name != "stress" or all(
        value <= 1e-12 for value in variants["hub-drag-force"]["crazyflow_rotation_vs_no_drag"].values()))
    return dict(passed=within and resolved and identity, end_s=flight["end_s"],
                start=flight["start"], commands_radps=flight["commands_radps"],
                reached=reached(flight, ripple[True]), sensitivity=sensitivity, variants=variants)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--python", help="an interpreter with Crazyflow installed")
    parser.add_argument("--update-fixture", action="store_true")
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    binary = build_cli("release")
    helper = [args.python, "-I"] if args.python else [
        "uv", "run", "--quiet", "--no-project", "--python", "3.12", "--with", CRAZYFLOW, "python", "-I"]

    grid = loads_grid(helper, args.output / "loads", args.update_fixture)
    flights = {name: fly(binary, helper, args.output / name, name, flight)
               for name, flight in FLIGHTS.items()}
    passed = (grid["rust_largest_errors"] is not None
              and (grid["fixture_largest_relative_difference"] is None
                   or grid["fixture_largest_relative_difference"] <= 1e-12)
              and all(flight["passed"] for flight in flights.values()))
    report = dict(passed=passed, crazyflow_commit=CRAZYFLOW_COMMIT, loads=grid, limits=LIMITS,
                  ripple_dt_s=DT_S, crazyflow_dt_s=DT_S / FINE_STEPS_PER_TICK,
                  hover_radps=HOVER_RADPS, flights=flights, source=source_identity())
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: report[key] for key in ("passed", "loads", "limits", "flights")},
                     indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
