#!/usr/bin/env python3
"""Does Multirotor's motor and rotor model reproduce RotorS, sample by sample?

    python3 reference/rotors_motor.py --output runs/rotors-motor000 [--update-fixture]

Builds reference/rotors/Dockerfile: RotorS's own `FirstOrderFilter` and
`GazeboMotorModel::UpdateForcesAndMoments` (ethz-asl/rotors_simulator at
ROTORS_COMMIT, copied verbatim by rotors/extract.py), compiled against small
stand-ins for the Gazebo links and joint (rotors/harness.cpp). The harness
steps one rotor through prescribed vehicle states and motor commands and
prints, per sample, the shaft speed the loads used, the rotor's force and
moment on the body (moment about the center of mass, body frame), and the
filtered speed for the next sample.

Cases: two coefficient sets (RotorS's defaults, and Ripple's `crazyflie2`
preset with its axial drag left out, since RotorS has none) on two rotors
(clockwise and level; counterclockwise, raised, and tilted), with command
steps up and down and a moving, rotating vehicle.

Ripple's side is the unit test `rotors_motor_samples` in
crates/core/src/plugin/motion/multirotor/rotor.rs. It reads the committed
fixture crates/core/tests/fixtures/rotors_motor_samples.json, or the file in
RIPPLE_ROTORS_SAMPLES, and prints its largest errors. This script runs that
test on the samples it just produced (pass: every sample within the test's
tolerance) and checks that they equal the committed fixture to 1e-12
relative (`--update-fixture` replaces it instead). Writes OUTPUT/samples.json
and OUTPUT/report.json. Needs Docker and Cargo.
"""
import argparse
import json
import math
import os
from pathlib import Path
import subprocess
import sys

from project import CORE_PACKAGE, IMAGE_PREFIX, ROOT, source_identity

ROTORS_COMMIT = "cd813b7a8c375d677352aa20ad20047feb661126"
FIXTURE = ROOT / "crates/core/tests/fixtures/rotors_motor_samples.json"
SAMPLES = 120
SLOWDOWN = 10.0  # RotorS's rotorVelocitySlowdownSim default

# RotorS's defaults (gazebo_motor_model.h); and the crazyflie2 preset
# (airframe.rs): c_T, c_Q / c_T, Förster's lag and in-plane drag.
CRAZYFLIE_C_T = 1.900523877693446e-08
CRAZYFLIE_TAU_S = -0.002 / math.log(0.9695404)
COEFFICIENTS = {
    "rotors-defaults": dict(motor_constant=8.54858e-06, moment_constant=0.016,
                            time_constant_up=1 / 80, time_constant_down=1 / 40,
                            rotor_drag_coefficient=1.0e-4, rolling_moment_coefficient=1.0e-6,
                            speeds=(600.0, 900.0, 400.0), dt_s=0.001),
    "crazyflie2": dict(motor_constant=CRAZYFLIE_C_T, moment_constant=0.005964552,
                       time_constant_up=CRAZYFLIE_TAU_S, time_constant_down=CRAZYFLIE_TAU_S,
                       rotor_drag_coefficient=9.1785e-7, rolling_moment_coefficient=0.0,
                       speeds=(1900.0, 2600.0, 1200.0), dt_s=0.002),
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


def run_case(image, coefficients, rotor):
    tilt = quaternion(*(math.radians(angle) for angle in rotor["tilt_deg"]))
    samples = list(states(coefficients["speeds"], coefficients["dt_s"]))
    header = [coefficients[key] for key in ("motor_constant", "moment_constant",
                                            "time_constant_up", "time_constant_down",
                                            "rotor_drag_coefficient",
                                            "rolling_moment_coefficient")]
    numbers = header + [SLOWDOWN, rotor["direction"], coefficients["speeds"][0],
                        *rotor["offset"], *tilt, len(samples)]
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
    for sample, line in zip(samples, lines):
        values = [float(value) for value in line.split()]
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


def flatten(value):
    if isinstance(value, dict):
        for key in sorted(value):
            yield from flatten(value[key])
    elif isinstance(value, list):
        for item in value:
            yield from flatten(item)
    elif isinstance(value, (int, float)):
        yield float(value)


def largest_relative_difference(left, right):
    largest = 0.0
    for a, b in zip(flatten(left), flatten(right), strict=True):
        largest = max(largest, abs(a - b) / max(abs(a), abs(b), 1e-300))
    return largest


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
    samples = {"rotors_commit": ROTORS_COMMIT, "cases": cases}
    samples_path = args.output / "samples.json"
    samples_path.write_text(json.dumps(samples, indent=1) + "\n")

    fixture_difference = None
    if args.update_fixture:
        FIXTURE.write_text(samples_path.read_text())
    else:
        fixture_difference = largest_relative_difference(
            json.loads(FIXTURE.read_text()), json.loads(samples_path.read_text()))

    test = subprocess.run(
        ["cargo", "test", "-p", CORE_PACKAGE, "--lib", "rotors_motor_samples", "--",
         "--nocapture", "--exact", "plugin::motion::multirotor::rotor::tests::rotors_motor_samples"],
        cwd=ROOT, capture_output=True, text=True,
        env={**os.environ, "RIPPLE_ROTORS_SAMPLES": str(samples_path.resolve())})
    (args.output / "rust-test.log").write_text(test.stdout + test.stderr)
    errors = None
    for line in test.stdout.splitlines():
        if line.startswith("ROTORS_ERRORS "):
            errors = json.loads(line.removeprefix("ROTORS_ERRORS "))
    passed = (test.returncode == 0 and errors is not None
              and (fixture_difference is None or fixture_difference <= 1e-12))
    report = {
        "passed": passed,
        "rotors_commit": ROTORS_COMMIT,
        "image": image, "image_id": image_id,
        "source": source_identity(),
        "cases": {name: len(case["samples"]) for name, case in cases.items()},
        "rust_largest_errors": errors,
        "fixture_largest_relative_difference": fixture_difference,
    }
    (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
