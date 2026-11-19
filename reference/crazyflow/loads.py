"""Crazyflow's force and moment at held states, for reference/crazyflow_check.py.

    python -I loads.py CASE.json SAMPLES.json

CASE.json gives a vehicle (as integrate.py takes it, any number of rotors)
and a list of held states: attitude (w-x-y-z), world velocity, body rates,
and every rotor's speed in rad/s. For each, Crazyflow's `first_principles`
dynamics is evaluated once, with no drag, and its accelerations are turned
back into the rotors' force (the weight taken out) and moment about the
center of mass, both in the body frame: F = m (v_dot - g), M = J w_dot + w x
J w.
"""
import json
import math
import sys

# Crazyflow sets SCIPY_ARRAY_API on import, so it must come before scipy.
from crazyflow.dynamics.first_principles.dynamics import dynamics
import numpy as np
from scipy.spatial.transform import Rotation

RPM_PER_RADPS = 60.0 / (2.0 * math.pi)


def main():
    case = json.loads(open(sys.argv[1]).read())
    radps_per_rpm = 1.0 / RPM_PER_RADPS
    arm_m = case["arm_m"]
    offsets = np.array(case["rotor_offsets_m"])
    signs = np.array(case["reaction_signs"], dtype=float)
    mixing = np.stack([offsets[:, 1] / arm_m, -offsets[:, 0] / arm_m, signs])
    J = np.array(case["inertia_kgm2"])
    mass = case["mass_kg"]
    gravity_vec = np.array([0.0, 0.0, -case["gravity_mps2"]])
    params = dict(
        mass=mass, L=arm_m, prop_inertia=0.0, gravity_vec=gravity_vec,
        J=J, J_inv=np.linalg.inv(J),
        rpm2thrust=np.array([0.0, 0.0, case["thrust_coefficient"] * radps_per_rpm ** 2]),
        rpm2torque=np.array([0.0, 0.0, case["torque_coefficient"] * radps_per_rpm ** 2]),
        mixing_matrix=mixing,
        drag_matrix=np.zeros((3, 3)),
        rotor_dyn_coef=np.zeros(4),
    )
    samples = []
    for state in case["states"]:
        w, x, y, z = state["attitude_wxyz"]
        quat = np.array([x, y, z, w], dtype=float)
        rates = np.array(state["rates_body_radps"], dtype=float)
        _, _, vel_dot, ang_vel_dot, _ = dynamics(
            np.zeros(3), quat, np.array(state["velocity_world_mps"], dtype=float), rates,
            np.array(state["speeds_radps"]) * RPM_PER_RADPS, None, None, None, **params)
        rotation = Rotation.from_quat(quat)
        force_body = rotation.apply(mass * (np.asarray(vel_dot) - gravity_vec), inverse=True)
        moment_body = J @ np.asarray(ang_vel_dot) + np.cross(rates, J @ rates)
        samples.append(dict(force_body_n=force_body.tolist(), moment_body_nm=moment_body.tolist()))
    with open(sys.argv[2], "w") as output:
        json.dump(samples, output)


if __name__ == "__main__":
    import warnings
    warnings.simplefilter("ignore")
    main()
