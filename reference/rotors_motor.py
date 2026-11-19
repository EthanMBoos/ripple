#!/usr/bin/env python3
"""Compare Multirotor's rotor lag and loads with RotorS's motor code.

    python3 reference/rotors_motor.py --output DIR [--update-fixture]

The Docker harness compiles RotorS's pinned FirstOrderFilter and motor-load
code with small stand-ins for Gazebo. It tests command steps on individual
rotors, then sums RotorS loads at held Crazyflie and Firefly states. This
checks the assembled vehicle, hub offsets, and frame conversion as well as
each rotor. RotorS has no axial-drag term, so that term is omitted from the
matching Crazyflie samples.

Rust tests reproduce the fresh samples. They must also equal the committed
fixture within 1e-12 relative; --update-fixture replaces it. Writes
OUTPUT/samples.json and OUTPUT/report.json. Needs Docker and Cargo.
"""
import argparse
import json
import math
import os
from pathlib import Path
import subprocess
import sys

from multirotor_grid import (VEHICLES, held_states, largest_relative_difference, rotate,
                             samples_text)
from project import CORE_PACKAGE, IMAGE_PREFIX, ROOT, source_identity

ROTORS_COMMIT = "cd813b7a8c375d677352aa20ad20047feb661126"
FIXTURE = ROOT / "crates/core/tests/fixtures/rotors_motor_samples.json"
SAMPLES = 120
SLOWDOWN = 10.0  # RotorS's rotorVelocitySlowdownSim default



def preset_coefficients(name, speeds, dt_s):
    """A preset's c_T, c_Q / c_T, lag, and drag (multirotor_grid.py)."""
    vehicle = VEHICLES[name]
    return dict(motor_constant=vehicle["thrust_coefficient"],
                moment_constant=vehicle["torque_coefficient"] / vehicle["thrust_coefficient"],
                **vehicle["rotors"], speeds=speeds, dt_s=dt_s)


# RotorS's defaults (gazebo_motor_model.h); the crazyflie2 preset with
# Förster's lag and in-plane drag; and the firefly preset with its model
# file's lag, drag, and rolling moment.
COEFFICIENTS = {
    "rotors-defaults": dict(motor_constant=8.54858e-06, moment_constant=0.016,
                            time_constant_up=1 / 80, time_constant_down=1 / 40,
                            rotor_drag_coefficient=1.0e-4, rolling_moment_coefficient=1.0e-6,
                            speeds=(600.0, 900.0, 400.0), dt_s=0.001),
    "crazyflie2": preset_coefficients("crazyflie2", (1900.0, 2600.0, 1200.0), 0.002),
    "firefly": preset_coefficients("firefly", (540.0, 700.0, 300.0), 0.001),
}
# direction (1 = CCW, -1 = CW, RotorS's turning_direction), offset, tilt in degrees.
ROTORS = {
    "cw-level": dict(direction=-1, offset=(0.17, 0.0, 0.0), tilt_deg=(0.0, 0.0, 0.0)),
    "ccw-tilted": dict(direction=1, offset=(0.0, 0.17, 0.05), tilt_deg=(10.0, -5.0, 30.0)),
}


def quaternion(roll_rad, pitch_rad, yaw_rad):
    """World from body, Z-Y-X (as Ripple's Quaternion::from_euler), scalar first."""
    cr, sr = math.cos(roll_rad / 2), math.sin(roll_rad / 2)
    cp, sp = math.cos(pitch_rad / 2), math.sin(pitch_rad / 2)
    cy, sy = math.cos(yaw_rad / 2), math.sin(yaw_rad / 2)
    return (cr * cp * cy + sr * sp * sy, sr * cp * cy - cr * sp * sy,
            cr * sp * cy + sr * cp * sy, cr * cp * sy - sr * sp * cy)


def states(speeds, dt_s):
    """A command that steps up, down, and then varies, on a moving, rotating vehicle."""
    high, low = speeds[1], speeds[2]
    for k in range(SAMPLES):
        t = k * dt_s
        if k < 30:
            command = high
        elif k < 60:
            command = low
        else:
            command = 0.5 * (high + low) + 0.5 * (high - low) * math.sin(0.3 * k)
        yield dict(
            dt_s=dt_s, command_radps=command,
            attitude=quaternion(0.3 * math.sin(5 * t), 0.2 * math.cos(3 * t), 0.4 * t + 0.2),
            velocity_world_mps=(3 * math.cos(4 * t), 2 * math.sin(5 * t), 0.5 * math.sin(7 * t)),
            angular_velocity_world_radps=(0.8 * math.sin(6 * t), 0.5 * math.cos(4 * t), 0.4))


def run_harness(image, coefficients, direction, initial_speed, offset, tilt, samples):
    """One rotor through RotorS's code at each sample's state and command;
    per sample, the speed used, the force, the moment, and the speed after."""
    header = [coefficients[key] for key in ("motor_constant", "moment_constant",
                                            "time_constant_up", "time_constant_down",
                                            "rotor_drag_coefficient",
                                            "rolling_moment_coefficient")]
    numbers = header + [SLOWDOWN, direction, initial_speed, *offset, *tilt, len(samples)]
    for sample in samples:
        numbers += [sample["dt_s"], sample["command_radps"], *sample["attitude"],
                    *sample["velocity_world_mps"], *sample["angular_velocity_world_radps"]]
    # Integers stay integers: the harness reads the direction and count as int.
    text = " ".join(str(value) if isinstance(value, int) else repr(float(value))
                    for value in numbers) + "\n"
    result = subprocess.run(["docker", "run", "--rm", "-i", "--network", "none", image],
                            input=text, capture_output=True, text=True, check=True)
    lines = result.stdout.strip().splitlines()
    assert len(lines) == len(samples), result.stderr
    return [[float(value) for value in line.split()] for line in lines]


def run_case(image, coefficients, rotor):
    tilt = quaternion(*(math.radians(angle) for angle in rotor["tilt_deg"]))
    samples = list(states(coefficients["speeds"], coefficients["dt_s"]))
    rows = run_harness(image, coefficients, rotor["direction"], coefficients["speeds"][0],
                       rotor["offset"], tilt, samples)
    for sample, values in zip(samples, rows):
        sample.update(speed_used_radps=values[0], force_body_n=values[1:4],
                      moment_body_nm=values[4:7], speed_after_radps=values[7])
    return dict(direction="CCW" if rotor["direction"] == 1 else "CW",
                offset_body_m=list(rotor["offset"]), rotor_quaternion=list(tilt),
                thrust_coefficient=coefficients["motor_constant"],
                torque_coefficient=coefficients["motor_constant"] * coefficients["moment_constant"],
                time_constant_up_s=coefficients["time_constant_up"],
                time_constant_down_s=coefficients["time_constant_down"],
                rotor_drag_coefficient=coefficients["rotor_drag_coefficient"],
                rolling_moment_coefficient=coefficients["rolling_moment_coefficient"],
                samples=samples)


def run_vehicle(image, name, vehicle):
    """Every rotor of a preset through RotorS's code at the held states, the
    loads summed about the center of mass. With the filter's time constants
    zero, the speed a sample uses is the command before it, so each rotor is
    held at its state's speed; the speed RotorS used (rounded through its
    slowdown factor) replaces the prescribed one in the sample."""
    held = list(held_states(vehicle))
    coefficients = dict(motor_constant=vehicle["thrust_coefficient"],
                        moment_constant=vehicle["torque_coefficient"] / vehicle["thrust_coefficient"],
                        time_constant_up=0.0, time_constant_down=0.0,
                        rotor_drag_coefficient=vehicle["rotors"]["rotor_drag_coefficient"],
                        rolling_moment_coefficient=vehicle["rotors"]["rolling_moment_coefficient"])
    samples = [dict(state, force_body_n=[0.0] * 3, moment_body_nm=[0.0] * 3,
                    force_scale_n=0.0, moment_scale_nm=0.0) for state in held]
    for index, (offset, direction) in enumerate(zip(vehicle["rotor_offsets_m"],
                                                    vehicle["directions"])):
        speeds = [state["speeds_radps"][index] for state in held]
        inputs = [dict(dt_s=0.001, command_radps=speeds[min(k + 1, len(speeds) - 1)],
                       attitude=state["attitude_wxyz"],
                       velocity_world_mps=state["velocity_world_mps"],
                       angular_velocity_world_radps=rotate(state["attitude_wxyz"],
                                                           state["rates_body_radps"]))
                  for k, state in enumerate(held)]
        rows = run_harness(image, coefficients, 1 if direction == "ccw" else -1, speeds[0],
                           offset, (1.0, 0.0, 0.0, 0.0), inputs)
        for sample, values in zip(samples, rows):
            sample["speeds_radps"][index] = values[0]
            force, moment = values[1:4], values[4:7]
            sample["force_body_n"] = [a + b for a, b in zip(sample["force_body_n"], force)]
            sample["moment_body_nm"] = [a + b for a, b in zip(sample["moment_body_nm"], moment)]
            sample["force_scale_n"] += math.hypot(*force)
            sample["moment_scale_nm"] += math.hypot(*moment)
    return dict(mission_keys=dict(airframe=name, **{key: vehicle["rotors"][key] for key in (
        "rotor_drag_coefficient", "rolling_moment_coefficient")}), samples=samples)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--update-fixture", action="store_true")
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)

    image = f"{IMAGE_PREFIX}-rotors:{ROTORS_COMMIT[:12]}"
    with open(args.output / "build.log", "w") as log:
        subprocess.run(["docker", "build", "--build-arg", f"ROTORS_COMMIT={ROTORS_COMMIT}",
                        "-t", image, str(ROOT / "reference/rotors")],
                       stdout=log, stderr=subprocess.STDOUT, check=True)
    image_id = subprocess.run(["docker", "image", "inspect", "--format", "{{.Id}}", image],
                              capture_output=True, text=True, check=True).stdout.strip()

    cases = {}
    for set_name, coefficients in COEFFICIENTS.items():
        for rotor_name, rotor in ROTORS.items():
            cases[f"{set_name}/{rotor_name}"] = run_case(image, coefficients, rotor)
    vehicles = {name: run_vehicle(image, name, vehicle) for name, vehicle in VEHICLES.items()}
    samples = {"rotors_commit": ROTORS_COMMIT, "cases": cases, "vehicles": vehicles}
    samples_path = args.output / "samples.json"
    samples_path.write_text(samples_text(samples))

    fixture_difference = None
    if args.update_fixture:
        FIXTURE.write_text(samples_path.read_text())
    else:
        fixture_difference = largest_relative_difference(
            json.loads(FIXTURE.read_text()), json.loads(samples_path.read_text()))

    test = subprocess.run(
        ["cargo", "test", "-p", CORE_PACKAGE, "--lib", "rotors_", "--", "--nocapture", "--exact",
         "plugin::motion::multirotor::rotor::tests::rotors_motor_samples",
         "plugin::motion::multirotor::tests::rotors_vehicle_samples"],
        cwd=ROOT, capture_output=True, text=True,
        env={**os.environ, "RIPPLE_ROTORS_SAMPLES": str(samples_path.resolve())})
    (args.output / "rust-test.log").write_text(test.stdout + test.stderr)
    errors = vehicle_errors = None
    for line in test.stdout.splitlines():
        if line.startswith("ROTORS_ERRORS "):
            errors = json.loads(line.removeprefix("ROTORS_ERRORS "))
        if line.startswith("ROTORS_VEHICLE_ERRORS "):
            vehicle_errors = json.loads(line.removeprefix("ROTORS_VEHICLE_ERRORS "))
    passed = (test.returncode == 0 and errors is not None and vehicle_errors is not None
              and (fixture_difference is None or fixture_difference <= 1e-12))
    report = {
        "passed": passed,
        "rotors_commit": ROTORS_COMMIT,
        "image": image, "image_id": image_id,
        "source": source_identity(),
        "cases": {name: len(case["samples"]) for name, case in cases.items()},
        "vehicles": {name: len(vehicle["samples"]) for name, vehicle in vehicles.items()},
        "rust_largest_errors": errors,
        "rust_largest_vehicle_errors": vehicle_errors,
        "fixture_largest_relative_difference": fixture_difference,
    }
    (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
