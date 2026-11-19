#!/usr/bin/env python3
"""Check the Aerosonde against Beard and McLain's aircraft equations.

    python3 reference/aerosonde_check.py --output DIR [--fetch]

The pinned mavsim_public parameter file supplies the airframe numbers, but
its force and motor functions are stubs. This script independently transcribes
the book's aerodynamics, propeller, and equations of motion. Agreement with
Rust checks the two transcriptions, not flight accuracy.

The check compares loads and accelerations at 432 held states, then solves a
25 m/s longitudinal trim and flies it open loop for 10 s. The trim leaves a
reported lateral force; the flight must keep altitude and airspeed within
1 mm and 1 mm/s. Mission controls must match the trim, and fresh samples
must match the committed Rust test fixture.

Writes OUTPUT/results.json and OUTPUT/summary.md. --fetch verifies the
pinned parameter file; --update-fixture and --update-mission replace the
committed inputs. Needs Cargo; --fetch needs network access.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import urllib.request

from frames import read_frames
from jsbsim_check import ned_euler
from project import CORE_PACKAGE, ROOT, build_cli, source_identity

MAVSIM_COMMIT = "10363e0e"
MAVSIM_COMMIT_FULL = None  # resolved by --fetch from the short hash
PARAMETER_FILE = "mavsim_python/parameters/aerosonde_parameters.py"
PARAMETER_FILE_SHA256 = "edeb72d99766550e8ebb628a47d57453e1d64a968c3024888a2adb87da50997d"
FIXTURE = ROOT / "crates/core/tests/fixtures/aerosonde_book_samples.json"
MISSION = ROOT / "missions/aerosonde.yaml"
RUST_TEST = "plugin::motion::aerosonde::tests::book_samples"

# The file's numbers, by the file's names, SI. KV is converted from the
# datasheet's rpm/V as the file does, and KQ set equal to it, as the file does.
PARAMETERS = dict(
    mass=11.0, Jx=0.8244, Jy=1.135, Jz=1.759, Jxz=0.1204, S_wing=0.55, b=2.8956, c=0.18994,
    rho=1.2682, e=0.9, gravity=9.81,
    C_L_0=0.23, C_D_0=0.0424, C_m_0=0.0135, C_L_alpha=5.61, C_D_alpha=0.132, C_m_alpha=-2.74,
    C_L_q=7.95, C_D_q=0.0, C_m_q=-38.21, C_L_delta_e=0.13, C_D_delta_e=0.0135,
    C_m_delta_e=-0.99, M=50.0, alpha0=0.47, epsilon=0.16, C_D_p=0.043,
    C_Y_0=0.0, C_ell_0=0.0, C_n_0=0.0, C_Y_beta=-0.98, C_ell_beta=-0.13, C_n_beta=0.073,
    C_Y_p=0.0, C_ell_p=-0.51, C_n_p=0.069, C_Y_r=0.0, C_ell_r=0.25, C_n_r=-0.095,
    C_Y_delta_a=0.075, C_ell_delta_a=0.17, C_n_delta_a=-0.011, C_Y_delta_r=0.19,
    C_ell_delta_r=0.0024, C_n_delta_r=-0.069,
    D_prop=20 * 0.0254, KV_rpm_per_volt=145.0, R_motor=0.042, i0=1.5, ncells=12.0,
    C_Q2=-0.01664, C_Q1=0.004970, C_Q0=0.005230, C_T2=-0.1079, C_T1=-0.06044, C_T0=0.09357,
    u0=25.0, S_prop=0.2027)
PARAMETERS["KV"] = (1.0 / PARAMETERS["KV_rpm_per_volt"]) * 60.0 / (2.0 * math.pi)
PARAMETERS["KQ"] = PARAMETERS["KV"]
PARAMETERS["V_max"] = 3.7 * PARAMETERS["ncells"]
# The file's names this script does not use: C_D_0 and C_D_alpha are the
# chapter 5 linear drag, S_prop the printed book's propeller, epsilon unused.
UNUSED = ("C_D_0", "C_D_alpha", "S_prop", "epsilon")

# The mission's keys: the airframe in the file's air.
MISSION_KEYS = dict(airframe="aerosonde", air_density_kgpm3=PARAMETERS["rho"])

# Part A's grid: every airspeed, angle of attack (through the stall blend
# centered at alpha0 = 0.47 and past it), and sideslip, with each set of
# rates, surface angles, throttle, and attitude (roll, pitch, yaw in
# north/east/down).
AIRSPEEDS_MPS = [10.0, 25.0, 35.0]
ALPHAS_RAD = [-0.3, -0.1, 0.0, 0.05, 0.1, 0.3, 0.5, 0.8]
BETAS_RAD = [0.0, 0.1, -0.15]
CONTROLS = [  # (p, q, r), throttle, elevator, aileron, rudder, (phi, theta, psi)
    ((0.0, 0.0, 0.0), 0.0, 0.0, 0.0, 0.0, (0.0, 0.0, 0.0)),
    ((0.5, -0.3, 0.2), 0.4, 0.0, 0.0, 0.0, (0.3, -0.2, 1.1)),
    ((0.0, 0.0, 0.0), 0.8, 0.1, 0.0, 0.0, (0.0, 0.1, 0.0)),
    ((0.0, 0.0, 0.0), 1.0, -0.1, 0.0, 0.0, (-0.5, 0.4, -2.0)),
    ((0.5, -0.3, 0.2), 0.6, 0.1, 0.2, -0.15, (0.8, -0.6, 0.5)),
    ((-0.4, 0.6, -0.3), 0.2, -0.2, -0.3, 0.3, (0.1, 0.05, 3.0)),
]

# Part B: the trim and the flight.
AIRSPEED_MPS, ALTITUDE_M, END_S, DT_S, FINE = PARAMETERS["u0"], 100.0, 10.0, 0.01, 4
ALTITUDE_LIMIT_M, AIRSPEED_LIMIT_MPS = 0.001, 0.001


def sigma(alpha):
    M, alpha0 = PARAMETERS["M"], PARAMETERS["alpha0"]
    below, above = math.exp(-M * (alpha - alpha0)), math.exp(M * (alpha + alpha0))
    return (1.0 + below + above) / ((1.0 + below) * (1.0 + above))


def lift_coefficient(alpha):
    """C_L(alpha): the linear lift blended into a flat plate's past the stall."""
    P = PARAMETERS
    linear = P["C_L_0"] + P["C_L_alpha"] * alpha
    flat_plate = 2.0 * math.copysign(1.0, alpha) * math.sin(alpha) ** 2 * math.cos(alpha)
    s = sigma(alpha)
    return (1.0 - s) * linear + s * flat_plate


def drag_coefficient(alpha):
    """C_D(alpha): parasitic plus the induced drag of the linear lift."""
    P = PARAMETERS
    aspect_ratio = P["b"] ** 2 / P["S_wing"]
    return P["C_D_p"] + (P["C_L_0"] + P["C_L_alpha"] * alpha) ** 2 / (math.pi * P["e"] * aspect_ratio)


def aero_loads(air_velocity_frd, rates, elevator, aileron, rudder):
    """The aerodynamic force and moment in the body axes, chapter 4's equations."""
    P = PARAMETERS
    u, v, w = air_velocity_frd
    p, q, r = rates
    V = max(math.sqrt(u * u + v * v + w * w), 1e-5)
    alpha, beta = math.atan2(w, u), math.asin(max(-1.0, min(1.0, v / V)))
    qbar_s = 0.5 * P["rho"] * V * V * P["S_wing"]
    b, c = P["b"], P["c"]
    lift = qbar_s * (lift_coefficient(alpha) + P["C_L_q"] * c / (2 * V) * q + P["C_L_delta_e"] * elevator)
    drag = qbar_s * (drag_coefficient(alpha) + P["C_D_q"] * c / (2 * V) * q + P["C_D_delta_e"] * elevator)
    fy = qbar_s * (P["C_Y_0"] + P["C_Y_beta"] * beta + P["C_Y_p"] * b / (2 * V) * p
                   + P["C_Y_r"] * b / (2 * V) * r + P["C_Y_delta_a"] * aileron + P["C_Y_delta_r"] * rudder)
    fx = -math.cos(alpha) * drag + math.sin(alpha) * lift
    fz = -math.sin(alpha) * drag - math.cos(alpha) * lift
    roll = qbar_s * b * (P["C_ell_0"] + P["C_ell_beta"] * beta + P["C_ell_p"] * b / (2 * V) * p
                         + P["C_ell_r"] * b / (2 * V) * r + P["C_ell_delta_a"] * aileron
                         + P["C_ell_delta_r"] * rudder)
    pitch = qbar_s * c * (P["C_m_0"] + P["C_m_alpha"] * alpha + P["C_m_q"] * c / (2 * V) * q
                          + P["C_m_delta_e"] * elevator)
    yaw = qbar_s * b * (P["C_n_0"] + P["C_n_beta"] * beta + P["C_n_p"] * b / (2 * V) * p
                        + P["C_n_r"] * b / (2 * V) * r + P["C_n_delta_a"] * aileron
                        + P["C_n_delta_r"] * rudder)
    return (fx, fy, fz), (roll, pitch, yaw)


def propeller(airspeed, throttle):
    """The addendum's motor and propeller: (thrust, torque, speed)."""
    P = PARAMETERS
    rho, D, Va = P["rho"], P["D_prop"], airspeed
    V_in = P["V_max"] * throttle
    a = P["C_Q0"] * rho * D ** 5 / (2 * math.pi) ** 2
    b = P["C_Q1"] * rho * D ** 4 / (2 * math.pi) * Va + P["KQ"] * P["KV"] / P["R_motor"]
    c = P["C_Q2"] * rho * D ** 3 * Va ** 2 - P["KQ"] / P["R_motor"] * V_in + P["KQ"] * P["i0"]
    omega = max((-b + math.sqrt(b * b - 4 * a * c)) / (2 * a), 0.0)
    if omega > 0.0:
        J = 2 * math.pi * Va / (omega * D)
        C_T = P["C_T2"] * J ** 2 + P["C_T1"] * J + P["C_T0"]
        C_Q = P["C_Q2"] * J ** 2 + P["C_Q1"] * J + P["C_Q0"]
        n = omega / (2 * math.pi)
        return rho * n ** 2 * D ** 4 * C_T, rho * n ** 2 * D ** 5 * C_Q, omega
    # A standing propeller: the polynomials' J^2 terms alone remain.
    return rho * D ** 2 * P["C_T2"] * Va ** 2, rho * D ** 3 * P["C_Q2"] * Va ** 2, 0.0


def rotation_ned_from_frd(euler):
    """The body-to-world rotation matrix for roll, pitch, and yaw (ZYX)."""
    phi, theta, psi = euler
    cf, sf, ct, st, cy, sy = (math.cos(phi), math.sin(phi), math.cos(theta), math.sin(theta),
                              math.cos(psi), math.sin(psi))
    return [[ct * cy, sf * st * cy - cf * sy, cf * st * cy + sf * sy],
            [ct * sy, sf * st * sy + cf * cy, cf * st * sy - sf * cy],
            [-st, sf * ct, cf * ct]]


def apply(matrix, vector):
    return [sum(row[j] * vector[j] for j in range(3)) for row in matrix]


def transpose(matrix):
    return [[matrix[j][i] for j in range(3)] for i in range(3)]


def inverse(matrix):
    """A 3 x 3 inverse by cofactors."""
    (a, b, c), (d, e, f), (g, h, i) = matrix
    det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    return [[(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
            [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
            [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det]]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def derivative(air_velocity_frd, rates, throttle, elevator, aileron, rudder, euler):
    """Chapter 3's equations of motion in still air: the body acceleration
    (with the v x omega term), the angular acceleration through the inertia
    matrix with its product of inertia, and the world velocity."""
    P = PARAMETERS
    m = P["mass"]
    inertia = [[P["Jx"], 0.0, -P["Jxz"]], [0.0, P["Jy"], 0.0], [-P["Jxz"], 0.0, P["Jz"]]]
    rotation = rotation_ned_from_frd(euler)
    weight = apply(transpose(rotation), [0.0, 0.0, m * P["gravity"]])
    V = math.sqrt(sum(v * v for v in air_velocity_frd))
    force, moment = aero_loads(air_velocity_frd, rates, elevator, aileron, rudder)
    thrust, torque, _ = propeller(V, throttle)
    total_force = [weight[0] + thrust + force[0], weight[1] + force[1], weight[2] + force[2]]
    total_moment = [moment[0] - torque, moment[1], moment[2]]
    coriolis = cross(air_velocity_frd, rates)
    acceleration = [coriolis[i] + total_force[i] / m for i in range(3)]
    gyro = cross(rates, apply(inertia, rates))
    angular_acceleration = apply(inverse(inertia), [total_moment[i] - gyro[i] for i in range(3)])
    return acceleration, angular_acceleration, apply(rotation, air_velocity_frd)


def grid_samples():
    samples = []
    for V in AIRSPEEDS_MPS:
        for alpha in ALPHAS_RAD:
            for beta in BETAS_RAD:
                air = (V * math.cos(alpha) * math.cos(beta), V * math.sin(beta),
                       V * math.sin(alpha) * math.cos(beta))
                for rates, throttle, elevator, aileron, rudder, euler in CONTROLS:
                    force, moment = aero_loads(air, rates, elevator, aileron, rudder)
                    thrust, torque, omega = propeller(V, throttle)
                    acceleration, angular_acceleration, velocity_ned = derivative(
                        air, rates, throttle, elevator, aileron, rudder, euler)
                    samples.append(dict(
                        air_velocity_frd_mps=list(air), rates_frd_radps=list(rates), throttle=throttle,
                        elevator_rad=elevator, aileron_rad=aileron, rudder_rad=rudder,
                        attitude_ned_euler_rad=list(euler),
                        aero_force_frd_n=list(force), aero_moment_frd_nm=list(moment),
                        thrust_n=thrust, propeller_torque_nm=torque, propeller_speed_radps=omega,
                        acceleration_frd_mps2=acceleration,
                        angular_acceleration_frd_radps2=angular_acceleration,
                        velocity_ned_mps=velocity_ned))
    return samples


def fixture_text(samples):
    P = PARAMETERS
    head = dict(source=f"mavsim_public {MAVSIM_COMMIT} {PARAMETER_FILE}",
                parameter_file_sha256=PARAMETER_FILE_SHA256, mission_keys=MISSION_KEYS,
                vehicle=dict(mass_kg=P["mass"], wing_span_m=P["b"], wing_area_m2=P["S_wing"],
                             mean_chord_m=P["c"],
                             inertia_kgm2=[[P["Jx"], 0.0, -P["Jxz"]], [0.0, P["Jy"], 0.0],
                                           [-P["Jxz"], 0.0, P["Jz"]]]))
    text = json.dumps(head, indent=1)
    return text[:-2] + ',\n "samples": [\n  ' + ",\n  ".join(json.dumps(row) for row in samples) + "\n ]\n}\n"


def largest_relative_difference(left, right):
    """The largest |a - b| / max(|a|, |b|, 1) over every number in two equal
    structures, or infinity where they differ in shape."""
    if isinstance(left, dict) and isinstance(right, dict):
        if left.keys() != right.keys():
            return math.inf
        return max((largest_relative_difference(left[key], right[key]) for key in left), default=0.0)
    if isinstance(left, list) and isinstance(right, list):
        if len(left) != len(right):
            return math.inf
        return max((largest_relative_difference(a, b) for a, b in zip(left, right)), default=0.0)
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        return abs(left - right) / max(abs(left), abs(right), 1.0)
    return 0.0 if left == right else math.inf


def rust_test(samples_path, folder):
    """The Rust grid test on these samples; its printed errors, or None if it failed."""
    test = subprocess.run(
        ["cargo", "test", "-p", CORE_PACKAGE, "--lib", "book_samples", "--", "--nocapture", "--exact",
         RUST_TEST],
        cwd=ROOT, capture_output=True, text=True,
        env={**os.environ, "RIPPLE_AEROSONDE_SAMPLES": str(samples_path.resolve())})
    (folder / "rust-test.log").write_text(test.stdout + test.stderr)
    errors = None
    for line in test.stdout.splitlines():
        if line.startswith("AEROSONDE_ERRORS "):
            errors = json.loads(line.removeprefix("AEROSONDE_ERRORS "))
    return errors if test.returncode == 0 else None


def bisect(residual, low, high, iterations=200):
    """The root of a monotone residual between two values of opposite sign."""
    r_low, r_high = residual(low), residual(high)
    if r_low * r_high > 0:
        raise ValueError(f"no sign change between {low} and {high}: {r_low}, {r_high}")
    for _ in range(iterations):
        mid = 0.5 * (low + high)
        r_mid = residual(mid)
        if r_mid * r_low > 0:
            low, r_low = mid, r_mid
        else:
            high = mid
    return 0.5 * (low + high)


def trim(airspeed):
    """Longitudinal trim for level flight at this airspeed: wings level, no
    sideslip, no rates, pitch equal to the angle of attack. The ailerons and
    rudder balance the roll and yaw moments; the side force they leave is the
    lateral residual, reported in `residual["fy"]`."""
    P = PARAMETERS
    weight = P["mass"] * P["gravity"]

    def elevator_for(alpha):  # the pitching moment is linear in the elevator
        _, (_, m0, _) = aero_loads(air(alpha), (0, 0, 0), 0.0, 0.0, 0.0)
        _, (_, m1, _) = aero_loads(air(alpha), (0, 0, 0), 1.0, 0.0, 0.0)
        return -m0 / (m1 - m0)

    def air(alpha):
        return (airspeed * math.cos(alpha), 0.0, airspeed * math.sin(alpha))

    def vertical(alpha):  # body z: aerodynamic plus weight, with theta = alpha
        (_, _, fz), _ = aero_loads(air(alpha), (0, 0, 0), elevator_for(alpha), 0.0, 0.0)
        return fz + weight * math.cos(alpha)

    alpha = bisect(vertical, -0.2, 0.4)
    elevator = elevator_for(alpha)
    (fx, fy, _), (roll_aero, _, yaw_aero) = aero_loads(air(alpha), (0, 0, 0), elevator, 0.0, 0.0)

    def forward(throttle):
        return fx - weight * math.sin(alpha) + propeller(airspeed, throttle)[0]

    throttle = bisect(forward, 0.0, 1.0)
    _, torque, omega = propeller(airspeed, throttle)
    # The ailerons and rudder cancel the propeller's reaction torque and keep
    # yaw balanced: a 2 x 2 linear system in the two deflections.
    qbar_s_b = 0.5 * P["rho"] * airspeed ** 2 * P["S_wing"] * P["b"]
    a11, a12 = qbar_s_b * P["C_ell_delta_a"], qbar_s_b * P["C_ell_delta_r"]
    a21, a22 = qbar_s_b * P["C_n_delta_a"], qbar_s_b * P["C_n_delta_r"]
    r1, r2 = torque - roll_aero, -yaw_aero
    det = a11 * a22 - a12 * a21
    aileron, rudder = (r1 * a22 - a12 * r2) / det, (a11 * r2 - a21 * r1) / det
    force, moment = aero_loads(air(alpha), (0, 0, 0), elevator, aileron, rudder)
    thrust, torque, omega = propeller(airspeed, throttle)
    residual = dict(fx=force[0] - weight * math.sin(alpha) + thrust, fy=force[1],
                    fz=force[2] + weight * math.cos(alpha),
                    roll=moment[0] - torque, pitch=moment[1], yaw=moment[2])
    return dict(airspeed_mps=airspeed, alpha_rad=alpha, elevator_rad=elevator, throttle=throttle,
                aileron_rad=aileron, rudder_rad=rudder, thrust_n=thrust, propeller_torque_nm=torque,
                propeller_speed_radps=omega, residual=residual)


def mission_text(trimmed, dt_s=DT_S, name="aerosonde"):
    """The mission at this trim. Ripple's `pitch_deg` is positive nose down (the
    rotation is about the body's left axis), so the angle of attack goes in
    negated; `heading_deg` 0 is east."""
    keys = "".join(f"        {key}: {value}\n" for key, value in MISSION_KEYS.items())
    return (
        "# Beard and McLain's Aerosonde (the built-in `aerosonde` airframe) flown open\n"
        f"# loop for {END_S:g} s at its longitudinal trim for level flight at {AIRSPEED_MPS:g} m/s\n"
        "# in its parameter file's air. reference/aerosonde_check.py solves the trim\n"
        "# from the book's equations and writes this file (`--update-mission`), then\n"
        "# flies it and measures the drift in altitude and airspeed. The ailerons and\n"
        "# rudder hold the propeller's torque and leave a small side force, the lateral\n"
        "# residual, which drifts the aircraft sideways. Not compared with SCRIMMAGE,\n"
        "# which has no Aerosonde.\n"
        f"format_version: 1\nname: {name}\n\nrun:\n  end_s: {END_S:g}\n  dt_s: {dt_s!r}\n  seed: 1\n\n"
        "entities:\n  aerosonde:\n    team: 1\n"
        f"    position_m: [0, 0, {ALTITUDE_M:g}]\n    velocity_mps: [{AIRSPEED_MPS:g}, 0, 0]\n"
        f"    heading_deg: 0\n    pitch_deg: {-math.degrees(trimmed['alpha_rad'])!r}\n"
        "    visual_model: sphere\n    controller:\n      HeldControls:\n"
        f"        throttle: {trimmed['throttle']!r}\n        elevator_rad: {trimmed['elevator_rad']!r}\n"
        f"        aileron_rad: {trimmed['aileron_rad']!r}\n        rudder_rad: {trimmed['rudder_rad']!r}\n"
        "    motion_model:\n      Aerosonde:\n" + keys)


def run_ripple(binary, folder, name, text):
    path = folder / f"{name}.yaml"
    path.write_text(text)
    subprocess.run([binary, "run", path, "--root", ROOT, "--output", folder / name, "--headless"],
                   check=True, capture_output=True)
    return read_frames(folder / name / "run.mcap")


def flight_path(frames):
    path = []
    for frame in frames:
        entity = next(iter(frame.entities.values()))
        path.append(dict(time_s=frame.time_s, position_enu_m=list(entity.position),
                         velocity_enu_mps=list(entity.velocity),
                         euler_ned_rad=list(ned_euler(entity.orientation))))
    return path


def drift(path):
    """How far the flight left its start: the largest change in altitude and
    airspeed (still air, so ground speed), the lateral offset, and the
    attitude, with the values at the end."""
    start = path[0]
    altitude = max(abs(sample["position_enu_m"][2] - start["position_enu_m"][2]) for sample in path)
    speed = [math.sqrt(sum(v * v for v in sample["velocity_enu_mps"])) for sample in path]
    airspeed = max(abs(s - speed[0]) for s in speed)
    lateral = max(abs(sample["position_enu_m"][1] - start["position_enu_m"][1]) for sample in path)
    attitude = [max(abs(sample["euler_ned_rad"][axis] - start["euler_ned_rad"][axis]) for sample in path)
                for axis in range(3)]
    end = path[-1]
    return dict(altitude_m=altitude, airspeed_mps=airspeed, lateral_m=lateral,
                roll_rad=attitude[0], pitch_rad=attitude[1], yaw_rad=attitude[2],
                end=dict(time_s=end["time_s"], position_enu_m=end["position_enu_m"],
                         airspeed_mps=speed[-1], euler_ned_rad=end["euler_ned_rad"]))


def largest_difference(left, right):
    """The largest position and velocity difference between two flights at
    the times they share (the finer one has more frames)."""
    by_time = {round(sample["time_s"], 6): sample for sample in right}
    pairs = [(sample, by_time[round(sample["time_s"], 6)]) for sample in left
             if round(sample["time_s"], 6) in by_time]
    if len(pairs) < len(left) // 2:
        raise ValueError("the flights share too few times")
    return dict(
        position_m=max(math.dist(a["position_enu_m"], b["position_enu_m"]) for a, b in pairs),
        velocity_mps=max(math.dist(a["velocity_enu_mps"], b["velocity_enu_mps"]) for a, b in pairs))


def fetch_parameters():
    """The parameter file at the pinned commit, hash-checked, and its numbers
    compared with PARAMETERS. Returns the differences (none when they agree)."""
    url = f"https://raw.githubusercontent.com/randybeard/mavsim_public/{MAVSIM_COMMIT}/{PARAMETER_FILE}"
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != PARAMETER_FILE_SHA256:
        raise ValueError(f"{url}: SHA-256 {digest}, expected {PARAMETER_FILE_SHA256}")
    numbers = {}
    for match in re.finditer(r"^(\w+)\s*=\s*([-+]?[0-9.]+(?:[eE][-+]?\d+)?)\s*(?:#.*)?$", data.decode(),
                             re.MULTILINE):
        numbers[match.group(1)] = float(match.group(2))
    numbers["D_prop"] = 20 * 0.0254  # the file writes it as 20*(0.0254)
    differences = {name: (value, numbers.get(name)) for name, value in PARAMETERS.items()
                   if name in numbers and value != numbers[name]}
    missing = [name for name in PARAMETERS if name not in numbers
               and name not in ("KV", "KQ", "V_max", "D_prop")]
    return dict(differences=differences, missing=missing, file_sha256=digest)


def summary(report):
    grid, trimmed, flight = report["grid"], report["trim"], report["flight"]
    lines = ["# The Aerosonde against the book's equations", "",
             f"Passed: {report['passed']}", "",
             f"Numbers from mavsim_public {MAVSIM_COMMIT}, `{PARAMETER_FILE}` "
             f"(SHA-256 {PARAMETER_FILE_SHA256[:12]}...).", "",
             "## Held states", "",
             f"{grid['samples']} samples; the Rust test's largest errors "
             + (", ".join(f"{name} {value:.1e}" for name, value in grid["errors"]["largest"].items())
                if grid["errors"] else "(the test failed)")
             + f"; fixture difference {grid['fixture_largest_relative_difference']}.", "",
             "## Longitudinal trim for level flight at {:g} m/s".format(trimmed["airspeed_mps"]), "",
             f"alpha {math.degrees(trimmed['alpha_rad']):.3f} deg, elevator "
             f"{math.degrees(trimmed['elevator_rad']):.3f} deg, throttle {trimmed['throttle']:.4f} "
             f"(thrust {trimmed['thrust_n']:.3f} N, propeller {trimmed['propeller_speed_radps']:.1f} rad/s, "
             f"torque {trimmed['propeller_torque_nm']:.4f} N m), ailerons "
             f"{math.degrees(trimmed['aileron_rad']):.4f} deg, rudder {math.degrees(trimmed['rudder_rad']):.4f} deg; "
             f"lateral residual: side force {trimmed['residual']['fy']:.4f} N.", "",
             f"Mission equals the trim: {report['mission_equals_trim']}.", "",
             f"## The flight, {END_S:g} s open loop", ""]
    for name, measured in flight.items():
        if name == "quarter_step_difference":
            lines.append(f"- at a quarter step: position differs by {measured['position_m']:.2e} m, "
                         f"velocity by {measured['velocity_mps']:.2e} m/s")
        else:
            lines.append(f"- {name}: altitude drift {measured['altitude_m']:.1e} m (limit {ALTITUDE_LIMIT_M}), "
                         f"airspeed drift {measured['airspeed_mps']:.1e} m/s (limit {AIRSPEED_LIMIT_MPS}), "
                         f"lateral {measured['lateral_m']:.4f} m, roll {math.degrees(measured['roll_rad']):.4f} deg, "
                         f"pitch {math.degrees(measured['pitch_rad']):.4f} deg, yaw "
                         f"{math.degrees(measured['yaw_rad']):.4f} deg")
    if report.get("fetched"):
        fetched = report["fetched"]
        lines += ["", "## The parameter file, fetched", "",
                  f"SHA-256 {fetched['file_sha256']}; differences {fetched['differences']}; "
                  f"not found in the file {fetched['missing']}."]
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--update-fixture", action="store_true")
    parser.add_argument("--update-mission", action="store_true")
    parser.add_argument("--fetch", action="store_true", help="compare PARAMETERS with the pinned file")
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    fetched = fetch_parameters() if args.fetch else None

    grid_folder = args.output / "grid"
    grid_folder.mkdir()
    samples = grid_samples()
    samples_path = args.output / "samples.json"
    samples_path.write_text(fixture_text(samples))
    fixture_difference = None
    if args.update_fixture:
        FIXTURE.write_text(samples_path.read_text())
    else:
        fixture_difference = largest_relative_difference(json.loads(FIXTURE.read_text()),
                                                         json.loads(samples_path.read_text()))
    errors = rust_test(samples_path, grid_folder)
    grid_report = dict(samples=len(samples), errors=errors,
                       fixture_largest_relative_difference=fixture_difference)

    trimmed = trim(AIRSPEED_MPS)
    text = mission_text(trimmed)
    if args.update_mission:
        MISSION.write_text(text)
    mission_equals_trim = MISSION.read_text() == text

    binary = build_cli("release")
    flight_folder = args.output / "flight"
    flight_folder.mkdir()
    path = flight_path(run_ripple(binary, flight_folder, "trim", text))
    fine = flight_path(run_ripple(binary, flight_folder, "trim-fine",
                                  mission_text(trimmed, DT_S / FINE, "aerosonde-fine")))
    flight = dict(trim=drift(path), quarter_step_difference=largest_difference(path, fine))
    flight_passes = (flight["trim"]["altitude_m"] <= ALTITUDE_LIMIT_M
                     and flight["trim"]["airspeed_mps"] <= AIRSPEED_LIMIT_MPS)

    passed = (errors is not None and (fixture_difference is None or fixture_difference <= 1e-12)
              and mission_equals_trim and flight_passes
              and (fetched is None or (not fetched["differences"] and not fetched["missing"])))
    report = dict(passed=passed, mavsim_commit=MAVSIM_COMMIT, parameter_file=PARAMETER_FILE,
                  parameter_file_sha256=PARAMETER_FILE_SHA256, grid=grid_report, trim=trimmed,
                  mission_equals_trim=mission_equals_trim, flight=flight,
                  limits=dict(altitude_m=ALTITUDE_LIMIT_M, airspeed_mps=AIRSPEED_LIMIT_MPS),
                  fetched=fetched, source=source_identity())
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    (args.output / "summary.md").write_text(summary(report))
    print((args.output / "summary.md").read_text())
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
