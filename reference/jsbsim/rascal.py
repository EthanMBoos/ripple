"""Run ArduPilot's JSBSim Rascal for reference/jsbsim_check.py.

    python -I rascal.py grid CASE.json OUT.json
    python -I rascal.py glide CASE.json OUT.json

Runs in an environment with jsbsim installed (jsbsim_check.py starts one with
uv). CASE.json names the JSBSim root folder, which holds aircraft/Rascal/ as
fetched from ArduPilot's repository, and what to run. Output is in SI units
and JSBSim's own frames: forward/right/down body axes, north/east/down world
axes, and the wind axes' drag, side force, and lift. The propeller is stopped
throughout (throttle zero, where JSBSim's electric motor adds nothing).

grid: the aerodynamic loads at each given state (airspeed, angle of attack,
sideslip, body rates, surface angles), read from JSBSim's property tree after
its initial-condition run, which resolves the one-frame lag in its induced
drag, with the rate of change of angle of attack JSBSim used. Beside the
totals, each term beyond the textbook fixed-wing equations, computed from
JSBSim's own per-function values, so the check can report each one's size:
the moment transfer from the aerodynamic reference point to the center of
mass, the alpha-dot pitch damping, the constant yaw moment, the Mach
dependence of the aileron and elevator powers, the drag from sideslip, the
elevator drag's sign, and the exact wind-to-body rotation (against the
rotation SCRIMMAGE's FixedWing6DOF had until 2026-11-02).

glide: a steady glide at the given airspeed, trimmed by Newton's method on
JSBSim's held-state accelerations (JSBSim's own trim cannot: the propeller's
speed is a state), then flown open loop with the surfaces held at the trim
plus the given offsets, heading north at the equator so the Earth's rotation
adds no Coriolis acceleration. Also the numbers JSBSim resolved for the
Ripple mission: the mass and inertia with fuel, the air density and speed of
sound, and the acceleration a body at rest feels (gravity less the Earth's
centripetal term).
"""
import json
import math
import sys

import jsbsim
import numpy as np

FT = 0.3048
LBF = 4.4482216152605
SLUG = LBF / FT               # kg
SLUG_FT3 = SLUG / FT ** 3     # kg/m^3
PSF = LBF / FT ** 2           # Pa

# Rascal.xml: the elevator's drag coefficient is 0.03 times the normalized
# deflection (elevator-pos-rad / 0.30, sign kept); the aileron and elevator
# powers are tables against Mach, linear from Mach 0 to Mach 2.
ELEVATOR_DRAG, ELEVATOR_NORM_RAD = 0.03, 0.30
AILERON_POWER_MACH_0_2 = (0.13, 0.057)
ELEVATOR_POWER_MACH_0_2 = (-0.5, -0.275)
PITCH_ALPHA_DOT = -7.0
# Every aerodynamic function in the file, by axis: drag, side, lift (forces),
# then roll, pitch, yaw (moments).
FUNCTIONS = ["CD0", "CDi", "CDbeta", "CDde", "CYb", "CLalpha", "CLde",
             "Clb", "Clp", "Clr", "Clda", "Cldr", "Cmalpha", "Cmde", "Cmq", "Cmadot",
             "Cnb", "Cnr", "Cndr", "Cnda", "Cndi"]
FORCE_FUNCTIONS = {"CD0", "CDi", "CDbeta", "CDde", "CYb", "CLalpha", "CLde"}


def load(root, dt_s):
    fdm = jsbsim.FGFDMExec(root)
    fdm.set_debug_level(0)
    fdm.set_aircraft_path("aircraft")
    fdm.set_engine_path("engine")
    fdm.set_systems_path("systems")
    if not fdm.load_model("Rascal"):
        raise RuntimeError(f"could not load the Rascal from {root}")
    fdm.set_dt(dt_s)
    return fdm


def elevator_command(elevator_rad):
    """The normalized command the file's zero-centered -0.35 to 0.30 scale maps to this angle."""
    return elevator_rad / (0.30 if elevator_rad >= 0 else 0.35)


def hold(fdm, altitude_m, airspeed_mps, alpha_rad, beta_rad, rates_radps, surfaces_rad, gamma_rad=0.0):
    """Put JSBSim at this state, at the equator heading north, and resolve it."""
    elevator_rad, aileron_rad, rudder_rad = surfaces_rad
    fdm["ic/lat-geod-deg"] = 0.0
    fdm["ic/long-gc-deg"] = 0.0
    fdm["ic/psi-true-deg"] = 0.0
    fdm["ic/h-sl-ft"] = altitude_m / FT
    fdm["ic/vt-fps"] = airspeed_mps / FT
    fdm["ic/alpha-rad"] = alpha_rad
    fdm["ic/beta-rad"] = beta_rad
    fdm["ic/gamma-rad"] = gamma_rad
    fdm["ic/p-rad_sec"], fdm["ic/q-rad_sec"], fdm["ic/r-rad_sec"] = rates_radps
    fdm["fcs/throttle-cmd-norm"] = 0.0
    fdm["fcs/elevator-cmd-norm"] = elevator_command(elevator_rad)
    fdm["fcs/aileron-cmd-norm"] = aileron_rad / 0.35
    fdm["fcs/rudder-cmd-norm"] = rudder_rad / 0.35
    fdm.run_ic()
    for name, wanted in (("fcs/elevator-pos-rad", elevator_rad),
                         ("fcs/left-aileron-pos-rad", aileron_rad),
                         ("fcs/rudder-pos-rad", rudder_rad)):
        if abs(fdm[name] - wanted) > 1e-12:
            raise ValueError(f"{name} is {fdm[name]}, not {wanted}")
    if abs(fdm["propulsion/engine/thrust-lbs"]) > 1e-12:
        raise ValueError("the propeller is not stopped")


def close(actual, expected, what, tolerance=1e-9):
    """Raise unless the two agree to `tolerance` relative to their size (at least 1)."""
    actual, expected = np.atleast_1d(actual), np.atleast_1d(expected)
    scale = max(1.0, float(np.max(np.abs(actual))), float(np.max(np.abs(expected))))
    if np.max(np.abs(actual - expected)) > tolerance * scale:
        raise ValueError(f"{what}: {actual.tolist()} against {expected.tolist()}")


def wind_to_body_exact(alpha_rad, beta_rad):
    """JSBSim's wind-to-body rotation, on the wind-axis vector (-drag, side, -lift)."""
    ca, sa, cb, sb = math.cos(alpha_rad), math.sin(alpha_rad), math.cos(beta_rad), math.sin(beta_rad)
    return lambda x, y, z: np.array([ca * cb * x - ca * sb * y - sa * z,
                                     sb * x + cb * y,
                                     sa * cb * x - sa * sb * y + ca * z])


def wind_to_body_ripple(alpha_rad, beta_rad):
    """SCRIMMAGE's rotation, which drops the drag's sideslip terms (FixedWing6DOF's until 2026-11-02)."""
    ca, sa, cb, sb = math.cos(alpha_rad), math.sin(alpha_rad), math.cos(beta_rad), math.sin(beta_rad)
    return lambda x, y, z: np.array([ca * x - sb * y - sa * z, cb * y, sa * x + ca * z])


def mach_table(mach_0_2, mach):
    return mach_0_2[0] + (mach_0_2[1] - mach_0_2[0]) * mach / 2.0


def vector(fdm, names, factor=1.0):
    return np.array([fdm[name] for name in names]) * factor


def grid_sample(fdm, altitude_m, state):
    hold(fdm, altitude_m, state["airspeed_mps"], state["alpha_rad"], state["beta_rad"],
         state["rates_radps"], (state["elevator_rad"], state["aileron_rad"], state["rudder_rad"]))
    f = {name: fdm[f"aero/coefficient/{name}"] for name in FUNCTIONS}
    drag = f["CD0"] + f["CDi"] + f["CDbeta"] + f["CDde"]
    side = f["CYb"]
    lift = f["CLalpha"] + f["CLde"]
    body = vector(fdm, ["forces/fbx-aero-lbs", "forces/fby-aero-lbs", "forces/fbz-aero-lbs"])
    about_cg = vector(fdm, ["moments/l-aero-lbsft", "moments/m-aero-lbsft", "moments/n-aero-lbsft"])
    about_rp = np.asarray(fdm.get_aerodynamics().get_moments_MRC(), dtype=float).flatten()
    alpha, beta, mach = fdm["aero/alpha-rad"], fdm["aero/beta-rad"], fdm["velocities/mach"]
    qbar_s = fdm["aero/qbar-psf"] * fdm["metrics/Sw-sqft"]
    span, chord = fdm["metrics/bw-ft"], fdm["metrics/cbarw-ft"]
    elevator, aileron = fdm["fcs/elevator-pos-rad"], fdm["fcs/left-aileron-pos-rad"]
    alpha_dot, airspeed_fps = fdm["aero/alphadot-rad_sec"], fdm["velocities/vt-fps"]
    # The alpha-dot damping was computed from this frame's alpha-dot.
    close(f["Cmadot"], PITCH_ALPHA_DOT * alpha_dot * chord / (2.0 * airspeed_fps) * qbar_s * chord,
          "alpha-dot pitch damping")
    # The named functions are the whole of each axis, and the induced drag's
    # lift coefficient is this frame's, not the last one's.
    close([drag, side, lift],
          vector(fdm, ["forces/fwx-aero-lbs", "forces/fwy-aero-lbs", "forces/fwz-aero-lbs"]),
          "wind-axis forces")
    close(fdm["aero/cl-squared"], (lift / qbar_s) ** 2, "lift coefficient squared")

    exact, ripple = wind_to_body_exact(alpha, beta), wind_to_body_ripple(alpha, beta)
    # FixedWing6DOF's forms of the drag terms that differ: elevator drag from
    # |elevator| in radians, and no drag from sideslip.
    signed_elevator_drag = f["CDde"] - ELEVATOR_DRAG / ELEVATOR_NORM_RAD * abs(elevator) * qbar_s
    sideslip_drag = f["CDbeta"]
    ripple_drag = drag - sideslip_drag - signed_elevator_drag
    force_terms = {
        "sideslip_drag": exact(-sideslip_drag, 0.0, 0.0),
        "signed_elevator_drag": exact(-signed_elevator_drag, 0.0, 0.0),
        "wind_to_body": exact(-ripple_drag, side, -lift) - ripple(-ripple_drag, side, -lift),
    }
    close(body, ripple(-ripple_drag, side, -lift) + sum(force_terms.values()), "body forces")
    # The Mach tables, read at this sample's Mach, against the file's rows.
    aileron_power = mach_table(AILERON_POWER_MACH_0_2, mach)
    elevator_power = mach_table(ELEVATOR_POWER_MACH_0_2, mach)
    if aileron != 0.0:
        close(f["Clda"] / (qbar_s * span * aileron), aileron_power, "aileron power")
    if elevator != 0.0:
        close(f["Cmde"] / (qbar_s * chord * elevator), elevator_power, "elevator power")
    moment_terms = {
        "reference_point": about_cg - about_rp,
        "alpha_dot": np.array([0.0, f["Cmadot"], 0.0]),
        "constant_yaw": np.array([0.0, 0.0, f["Cndi"]]),
        "mach": np.array([f["Clda"] * (1.0 - AILERON_POWER_MACH_0_2[0] / aileron_power),
                          f["Cmde"] * (1.0 - ELEVATOR_POWER_MACH_0_2[0] / elevator_power), 0.0]),
    }
    return dict(
        airspeed_mps=fdm["velocities/vt-fps"] * FT, alpha_rad=alpha, beta_rad=beta,
        air_velocity_frd_mps=vector(fdm, [f"velocities/{axis}-aero-fps" for axis in "uvw"], FT).tolist(),
        rates_frd_radps=vector(fdm, [f"velocities/{axis}-aero-rad_sec" for axis in "pqr"]).tolist(),
        elevator_rad=elevator, aileron_rad=aileron, rudder_rad=fdm["fcs/rudder-pos-rad"],
        alpha_dot_radps=alpha_dot,
        mach=mach, dynamic_pressure_pa=fdm["aero/qbar-psf"] * PSF,
        air_density_kgpm3=fdm["atmosphere/rho-slugs_ft3"] * SLUG_FT3,
        speed_of_sound_mps=fdm["atmosphere/a-fps"] * FT,
        wind_axes_n=dict(drag=drag * LBF, side=side * LBF, lift=lift * LBF),
        force_frd_n=(body * LBF).tolist(),
        moment_cg_frd_nm=(about_cg * LBF * FT).tolist(),
        moment_rp_frd_nm=(about_rp * LBF * FT).tolist(),
        functions={name: value * (LBF if name in FORCE_FUNCTIONS else LBF * FT)
                   for name, value in f.items()},
        terms=dict(force_frd_n={name: (term * LBF).tolist() for name, term in force_terms.items()},
                   moment_frd_nm={name: (term * LBF * FT).tolist()
                                  for name, term in moment_terms.items()}),
    )


def grid(case):
    fdm = load(case["root"], 0.001)
    samples = [grid_sample(fdm, case["altitude_m"], state) for state in case["states"]]
    for sample in samples:
        close(sample["air_density_kgpm3"], samples[0]["air_density_kgpm3"], "air density")
        close(sample["speed_of_sound_mps"], samples[0]["speed_of_sound_mps"], "speed of sound")
    return dict(jsbsim_version=fdm.get_version(), altitude_m=case["altitude_m"],
                air_density_kgpm3=samples[0]["air_density_kgpm3"],
                speed_of_sound_mps=samples[0]["speed_of_sound_mps"], samples=samples)


def at_rest(fdm, altitude_m):
    """The downward acceleration of a body at rest: gravity less the centripetal term."""
    hold(fdm, altitude_m, 0.0, 0.0, 0.0, (0.0, 0.0, 0.0), (0.0, 0.0, 0.0))
    close([fdm["accelerations/udot-ft_sec2"], fdm["accelerations/vdot-ft_sec2"]], [0.0, 0.0],
          "at rest, level acceleration")
    return dict(gravity_mps2=fdm["accelerations/wdot-ft_sec2"] * FT,
                gravitational_attraction_mps2=fdm["accelerations/gravity-ft_sec2"] * FT)


def trim(fdm, altitude_m, airspeed_mps):
    """Angle of attack, elevator, and flight-path angle of the steady glide: zero
    forward, downward, and pitch acceleration at the held state, by Newton."""
    def residual(x):
        alpha_rad, elevator_rad, gamma_rad = x
        hold(fdm, altitude_m, airspeed_mps, alpha_rad, 0.0, (0.0, 0.0, 0.0),
             (elevator_rad, 0.0, 0.0), gamma_rad)
        return np.array([fdm["accelerations/udot-ft_sec2"], fdm["accelerations/wdot-ft_sec2"],
                         fdm["accelerations/qdot-rad_sec2"]])

    x = np.array([0.05, -0.05, -0.08])
    for iteration in range(20):
        r = residual(x)
        if np.max(np.abs(r)) < 1e-10:
            return x, iteration, float(np.max(np.abs(r)))
        step = 1e-6
        jacobian = np.column_stack([(residual(x + step * axis) - r) / step for axis in np.eye(3)])
        x = x + np.linalg.solve(jacobian, -r)
    raise RuntimeError("the glide did not trim in 20 iterations")


def signed_distance(distance_m, degrees_from_start):
    """JSBSim's distance-from-start properties are great-circle magnitudes; the
    sign is the coordinate's."""
    return -distance_m if degrees_from_start < 0.0 else distance_m


def record(fdm, time_s):
    return dict(time_s=time_s,
                north_m=signed_distance(fdm["position/distance-from-start-lat-mt"],
                                        fdm["position/lat-geod-deg"] - fdm["ic/lat-geod-deg"]),
                east_m=signed_distance(fdm["position/distance-from-start-lon-mt"],
                                       fdm["position/long-gc-deg"] - fdm["ic/long-gc-deg"]),
                altitude_m=fdm["position/h-sl-meters"],
                velocity_ned_mps=vector(fdm, ["velocities/v-north-fps", "velocities/v-east-fps",
                                              "velocities/v-down-fps"], FT).tolist(),
                euler_rad=vector(fdm, ["attitude/phi-rad", "attitude/theta-rad",
                                       "attitude/psi-rad"]).tolist(),
                rates_frd_radps=vector(fdm, [f"velocities/{axis}-rad_sec" for axis in "pqr"]).tolist(),
                airspeed_mps=fdm["velocities/vt-fps"] * FT,
                alpha_rad=fdm["aero/alpha-rad"], beta_rad=fdm["aero/beta-rad"])


def glide(case):
    fdm = load(case["root"], case["dt_s"])
    altitude_m, airspeed_mps = case["altitude_m"], case["airspeed_mps"]
    rest = at_rest(fdm, altitude_m)
    (alpha_rad, elevator_rad, gamma_rad), iterations, residual = trim(fdm, altitude_m, airspeed_mps)
    surfaces = (elevator_rad + case["elevator_offset_rad"], case["aileron_rad"], case["rudder_rad"])
    hold(fdm, altitude_m, airspeed_mps, alpha_rad, 0.0, (0.0, 0.0, 0.0), surfaces, gamma_rad)
    mass_balance = fdm.get_mass_balance()
    vehicle = dict(
        mass_kg=fdm["inertia/mass-slugs"] * SLUG,
        inertia_slug_ft2=np.asarray(mass_balance.get_J(), dtype=float).tolist(),
        center_of_mass_structural_in=np.asarray(mass_balance.get_xyz_cg(), dtype=float).flatten().tolist(),
        aero_reference_structural_in=np.asarray(fdm.get_aircraft().get_xyz_rp(), dtype=float).flatten().tolist(),
        air_density_kgpm3=fdm["atmosphere/rho-slugs_ft3"] * SLUG_FT3,
        speed_of_sound_mps=fdm["atmosphere/a-fps"] * FT, **rest)
    samples = [record(fdm, 0.0)]
    steps = round(case["end_s"] / case["dt_s"])
    for step in range(1, steps + 1):
        fdm.run()
        if step % case["record_every"] == 0:
            samples.append(record(fdm, step * case["dt_s"]))
    return dict(jsbsim_version=fdm.get_version(), altitude_m=altitude_m, airspeed_mps=airspeed_mps,
                dt_s=case["dt_s"], end_s=case["end_s"],
                trim=dict(alpha_rad=alpha_rad, elevator_rad=elevator_rad, gamma_rad=gamma_rad,
                          theta_rad=samples[0]["euler_rad"][1], iterations=iterations,
                          residual=residual),
                controls=dict(throttle=0.0, elevator_rad=surfaces[0], aileron_rad=surfaces[1],
                              rudder_rad=surfaces[2]),
                vehicle=vehicle, air_density_end_kgpm3=fdm["atmosphere/rho-slugs_ft3"] * SLUG_FT3,
                samples=samples)


def main():
    mode, case_path, out_path = sys.argv[1:4]
    with open(case_path) as stream:
        case = json.load(stream)
    result = {"grid": grid, "glide": glide}[mode](case)
    with open(out_path, "w") as output:
        json.dump(result, output)


if __name__ == "__main__":
    main()
