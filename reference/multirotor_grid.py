"""The multirotor vehicles and the held states their loads are checked at.

Shared by crazyflow_check.py (loads against Crazyflow's dynamics, common
physics) and rotors_motor.py (loads against the sum of RotorS's rotor
loads, drag on). Each vehicle's numbers repeat its preset in
crates/core/src/plugin/motion/multirotor/airframe.rs, which the Rust tests
configure by name, so a changed preset shows as a mismatch here.

A held state is an attitude, a world velocity, body rates, and every rotor's
speed, with unequal speeds among them. The Firefly's states include forward
motion to its 12 m/s cruise and a climb. Every sample's errors are scaled by
the individual rotor loads (the sum of their sizes), not the net, since at
hover the net moment is near zero.
"""
import itertools
import json
import math

# The crazyflie2 preset: Förster's measurements, c_T and c_Q derived there.
CRAZYFLIE_C_T = 1.900523877693446e-08
CRAZYFLIE_TAU_S = -0.002 / math.log(0.9695404)  # Förster's lag at 500 Hz
CRAZYFLIE_ARM_M = 0.046
# The firefly preset: RotorS's firefly.xacro at commit cd813b7.
FIREFLY_C_T = 8.54858e-06
FIREFLY_ARM_M = 0.215
FIREFLY_HUB_UP_M = 0.037
COS30, SIN30 = 0.866025403784, 0.5

VEHICLES = {
    "crazyflie2": dict(
        mass_kg=0.028, gravity_mps2=9.81,
        inertia_kgm2=[[16.571710e-6, 0.830806e-6, 0.718277e-6],
                      [0.830806e-6, 16.655602e-6, 1.800197e-6],
                      [0.718277e-6, 1.800197e-6, 29.261652e-6]],
        thrust_coefficient=CRAZYFLIE_C_T, torque_coefficient=0.005964552 * CRAZYFLIE_C_T,
        arm_m=CRAZYFLIE_ARM_M,
        rotor_offsets_m=[[CRAZYFLIE_ARM_M, 0, 0], [0, CRAZYFLIE_ARM_M, 0],
                         [-CRAZYFLIE_ARM_M, 0, 0], [0, -CRAZYFLIE_ARM_M, 0]],
        directions=["cw", "ccw", "cw", "ccw"],
        # RotorS's motor-plugin keys as the Crazyflie missions set them:
        # Förster's in-plane drag and lag; RotorS has no axial drag, and the
        # rolling moment was not measured.
        rotors=dict(rotor_drag_coefficient=9.1785e-7, rolling_moment_coefficient=0.0,
                    time_constant_up=CRAZYFLIE_TAU_S, time_constant_down=CRAZYFLIE_TAU_S),
        # Within Förster's wind-tunnel range, about 1.5 to 4 m/s.
        velocities_world_mps=[[0, 0, 0], [1.5, 0, 0], [0.5, -1.2, 0.3], [-2.0, 1.0, -0.8]],
    ),
    "firefly": dict(
        mass_kg=1.5 + 6 * 0.005 + 0.015, gravity_mps2=9.81,
        inertia_kgm2=[[0.0347563, 0, 0], [0, 0.0458929, 0], [0, 0, 0.0977]],
        thrust_coefficient=FIREFLY_C_T, torque_coefficient=0.016 * FIREFLY_C_T,
        arm_m=FIREFLY_ARM_M,
        rotor_offsets_m=[[forward * FIREFLY_ARM_M, left * FIREFLY_ARM_M, FIREFLY_HUB_UP_M]
                         for forward, left in ((COS30, SIN30), (0, 1), (-COS30, SIN30),
                                               (-COS30, -SIN30), (0, -1), (COS30, -SIN30))],
        directions=["ccw", "cw", "ccw", "cw", "ccw", "cw"],
        # firefly.xacro lines 37-38 and 42-43.
        rotors=dict(rotor_drag_coefficient=8.06428e-05, rolling_moment_coefficient=1.0e-6,
                    time_constant_up=0.0125, time_constant_down=0.025),
        # Hover, the 12 m/s cruise, a 2 m/s climb, and two oblique motions.
        velocities_world_mps=[[0, 0, 0], [12, 0, 0], [0, 0, 2], [6, -3, 1], [-4, 2, -1.5]],
    ),
}

ATTITUDES_DEG = [(0, 0, 0), (10, 0, 0), (0, -8, 0), (5, 3, 35)]
RATES_BODY_RADPS = [[0, 0, 0], [0.5, 0, 0], [0, -0.4, 0.3], [0.3, 0.2, -1.0]]
# Each rotor's speed as a multiple of the hover speed: equal, slightly
# unequal, and widely unequal.
SPEED_FACTORS = [[1.0] * 6, [1.02, 0.98, 1.01, 0.995, 1.015, 0.985],
                 [1.25, 0.6, 1.1, 0.8, 0.95, 1.3]]


def quaternion_wxyz(roll_rad, pitch_rad, yaw_rad):
    """World from body, Z-Y-X (as Ripple's Quaternion::from_euler), scalar first."""
    cr, sr = math.cos(roll_rad / 2), math.sin(roll_rad / 2)
    cp, sp = math.cos(pitch_rad / 2), math.sin(pitch_rad / 2)
    cy, sy = math.cos(yaw_rad / 2), math.sin(yaw_rad / 2)
    return [cr * cp * cy + sr * sp * sy, sr * cp * cy - cr * sp * sy,
            cr * sp * cy + sr * cp * sy, cr * cp * sy - sr * sp * cy]


def rotate(q, v):
    """q v q*: the body vector v in the world frame, for q world from body."""
    w, x, y, z = q
    tx, ty, tz = 2 * (y * v[2] - z * v[1]), 2 * (z * v[0] - x * v[2]), 2 * (x * v[1] - y * v[0])
    return [v[0] + w * tx + (y * tz - z * ty), v[1] + w * ty + (z * tx - x * tz),
            v[2] + w * tz + (x * ty - y * tx)]


def hover_speed_radps(vehicle):
    rotors = len(vehicle["rotor_offsets_m"])
    return math.sqrt(vehicle["mass_kg"] * vehicle["gravity_mps2"]
                     / (rotors * vehicle["thrust_coefficient"]))


def held_states(vehicle):
    """Every combination of attitude, velocity, rates, and speed set."""
    hover = hover_speed_radps(vehicle)
    rotors = len(vehicle["rotor_offsets_m"])
    for attitude, velocity, rates, factors in itertools.product(
            ATTITUDES_DEG, vehicle["velocities_world_mps"], RATES_BODY_RADPS, SPEED_FACTORS):
        yield dict(attitude_wxyz=quaternion_wxyz(*(math.radians(a) for a in attitude)),
                   velocity_world_mps=list(velocity), rates_body_radps=list(rates),
                   speeds_radps=[hover * factor for factor in factors[:rotors]])


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
    """Over every number of two fixtures with the same shape."""
    largest = 0.0
    for a, b in zip(flatten(left), flatten(right), strict=True):
        largest = max(largest, abs(a - b) / max(abs(a), abs(b), 1e-300))
    return largest


def samples_text(samples):
    """A fixture with one sample per line, so its diffs read."""
    head = json.dumps({key: value for key, value in samples.items() if key != "vehicles"}, indent=1)
    vehicles = []
    for name, vehicle in samples["vehicles"].items():
        rows = ",\n   ".join(json.dumps(sample) for sample in vehicle["samples"])
        keys = json.dumps(vehicle["mission_keys"])
        vehicles.append(f'  "{name}": {{\n   "mission_keys": {keys},\n   "samples": [\n   {rows}\n   ]\n  }}')
    return head[:-2] + ',\n "vehicles": {\n' + ",\n".join(vehicles) + "\n }\n}\n"
