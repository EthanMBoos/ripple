"""Fly Crazyflow's first-principles quadrotor dynamics for reference/crazyflow_check.py.

    python -I integrate.py CASE.json TRAJECTORY.json

Runs in an environment with Crazyflow installed (crazyflow_check.py starts one
with uv). CASE.json gives Ripple's vehicle constants and constant motor
commands; this maps them onto Crazyflow's `first_principles.dynamics` (rotor
speeds in RPM, quaternions x-y-z-w, body angular velocity) and integrates it
with classical RK4 at a fine step, renormalizing the quaternion after each
step as Crazyflow's warning asks. Only Crazyflow's derivative is used; the
integrator here is plain RK4, so differences are the models', not the
integrators'.
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
    # Crazyflow's mixing matrix: rows are roll torque per (thrust * L), pitch
    # torque per (thrust * L), and yaw torque per rotor torque. From Ripple's
    # rotor offsets (all at distance L on the body axes) and directions.
    arm_m = case["arm_m"]
    offsets = np.array(case["rotor_offsets_m"])
    signs = np.array(case["reaction_signs"], dtype=float)
    mixing = np.stack([offsets[:, 1] / arm_m, -offsets[:, 0] / arm_m, signs])
    J = np.array(case["inertia_kgm2"])
    params = dict(
        mass=case["mass_kg"], L=arm_m, prop_inertia=0.0,
        gravity_vec=np.array([0.0, 0.0, -case["gravity_mps2"]]),
        J=J, J_inv=np.linalg.inv(J),
        rpm2thrust=np.array([0.0, 0.0, case["thrust_coefficient"] * radps_per_rpm ** 2]),
        rpm2torque=np.array([0.0, 0.0, case["torque_coefficient"] * radps_per_rpm ** 2]),
        mixing_matrix=mixing,
        drag_matrix=np.array(case["drag_matrix"]),
        rotor_dyn_coef=np.zeros(4),
    )
    command_rpm = np.array(case["commands_radps"]) * RPM_PER_RADPS

    w, x, y, z = case["orientation_wxyz"]
    state = dict(pos=np.array(case["position_m"], dtype=float),
                 quat=np.array([x, y, z, w], dtype=float),
                 vel=np.array(case["velocity_mps"], dtype=float),
                 ang_vel=np.zeros(3))

    def derivative(s):
        pos_dot, quat_dot, vel_dot, ang_vel_dot, _ = dynamics(
            s["pos"], s["quat"], s["vel"], s["ang_vel"], command_rpm, None, **params)
        return dict(pos=pos_dot, quat=quat_dot, vel=vel_dot, ang_vel=ang_vel_dot)

    def add(s, k, h):
        return {key: s[key] + h * k[key] for key in s}

    dt_s = case["dt_s"]
    record_every = case["record_every"]
    samples = []
    for step in range(case["steps"] + 1):
        if step % record_every == 0:
            rotation = Rotation.from_quat(state["quat"])
            qx, qy, qz, qw = state["quat"]
            samples.append(dict(time_s=step * dt_s, position_m=state["pos"].tolist(),
                                velocity_mps=state["vel"].tolist(),
                                orientation_wxyz=[qw, qx, qy, qz],
                                angular_velocity_world_radps=rotation.apply(state["ang_vel"]).tolist()))
        if step == case["steps"]:
            break
        k1 = derivative(state)
        k2 = derivative(add(state, k1, dt_s / 2))
        k3 = derivative(add(state, k2, dt_s / 2))
        k4 = derivative(add(state, k3, dt_s))
        state = {key: state[key] + dt_s / 6 * (k1[key] + 2 * k2[key] + 2 * k3[key] + k4[key])
                 for key in state}
        state["quat"] = state["quat"] / np.linalg.norm(state["quat"])
    with open(sys.argv[2], "w") as output:
        json.dump(samples, output)


if __name__ == "__main__":
    import warnings
    warnings.simplefilter("ignore")
    main()
