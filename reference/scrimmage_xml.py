"""SCRIMMAGE's XML mission for a scenario Ripple loaded from YAML.

Ripple records the scenario it loaded (templates expanded, overrides applied,
every default filled in) in each run's run.mcap. `write` turns that record
into the XML SCRIMMAGE reads, so both simulators run the same mission:

    write(scenario.recorded(run), "mission.xml")

Every entity value is written, since SCRIMMAGE's defaults differ from Ripple's
(spawn variance 100 m² against 0, sensor noise 5 m against none). Plugin
parameters are written as the mission gives them; a parameter left out takes
each simulator's own default, so a mission writes out any whose defaults differ
(see docs/REFERENCE_NOTES.md). SCRIMMAGE has no estimators or terrain, so a
scenario must use the default PassThrough estimators and no terrain.

SCRIMMAGE has no airframe profiles either: it ignores `airframe`, and its
FixedWing6DOF defaults are the A-4. A mission that names Ripple's `rascal`
gets the Rascal's numbers written out (AIRFRAMES), under the mission's own
values. They copy `airframe.rs`; if the two drift apart, the fixed-wing-rascal
comparison fails.
"""

import xml.etree.ElementTree as ET

NOISY_SENSORS = ("NoisyState", "NoisyContacts")
# Ripple's built-in airframes SCRIMMAGE's defaults don't already match, in
# SCRIMMAGE's keys. The Rascal is ArduPilot's JSBSim model (airframe.rs).
AIRFRAMES = {
    ("FixedWing6DOF", "rascal"): {
        "gravity_magnitude": 9.81, "air_density": 1.225,
        "inertia_matrix_slug_ft_sq": "[ 1.95 0.0 0.0 ] [ 0.0 1.55 0.0 ] [ 0.0 0.0 1.91 ]",
        "thrust_min": 0.0, "thrust_max": 25.0,
        "delta_elevator_min": -0.30, "delta_elevator_max": 0.30,
        "delta_aileron_min": -0.35, "delta_aileron_max": 0.35,
        "delta_rudder_min": -0.35, "delta_rudder_max": 0.35,
        "mass": 5.91, "wing_span": 2.7, "surface_area_of_wing": 0.982, "chord_length": 0.351,
        "C_D0": 0.028, "C_D_alpha": 0.0,
        "C_L0": 0.25, "C_L_alpha": 5, "C_LQ": 0.0, "C_L_alpha_dot": 0.0,
        "C_L_delta_elevator": 0.2, "C_Y_beta": -1.0, "C_Y_delta_rudder": 0.0,
        "C_L_beta": -0.1, "C_LP": -0.4, "C_LR": 0.15, "C_L_delta_aileron": 0.13,
        "C_L_delta_rudder": 0.01, "C_M0": 0.0, "C_MQ": -12.0, "C_M_alpha": -0.5,
        "C_M_alpha_dot": -7.0, "C_M_delta_elevator": -0.5, "C_N_beta": 0.12, "C_NP": 0.0,
        "C_NR": -0.15, "C_N_delta_aileron": -0.03, "C_N_delta_rudder": -0.05,
    },
}
# SCRIMMAGE's noise keys: `<prefix>_<axis>="bias stddev"`, from the named fields.
NOISE_KEYS = (("pos_noise", "position_bias_m", "position_stddev_m"),
              ("vel_noise", "velocity_bias_mps", "velocity_stddev_mps"),
              ("orient_noise", "attitude_bias_rad", "attitude_stddev_rad"))


def text(value):
    """A parameter as SCRIMMAGE text: lists separated by spaces, routes as x,y,z;x,y,z."""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, list):
        if value and all(isinstance(item, list) for item in value):
            return ";".join(",".join(text(x) for x in point) for point in value)
        return " ".join(text(item) for item in value)
    return str(value) if isinstance(value, str) else repr(value)


def rows(matrix):
    """A 3x3 matrix as SCRIMMAGE's bracketed rows: "[a b c] [d e f] [g h i]"."""
    return " ".join("[" + text(row) + "]" for row in matrix)


def rotor_rows(rotors):
    """Rotors as SCRIMMAGE's rotor_config rows: "[CW x y z roll pitch yaw] ..."."""
    return " ".join(
        f"[{rotor['direction'].upper()} {text(rotor['position_flu_m'])} "
        f"{text(rotor.get('tilt_deg', [0, 0, 0]))}]" for rotor in rotors)


MULTIROTOR_KEYS = {
    "mass_kg": "mass", "gravity_mps2": "gravity_magnitude",
    "thrust_coefficient": "c_T", "torque_coefficient": "c_Q",
    "min_rotor_speed_radps": "omega_min", "max_rotor_speed_radps": "omega_max",
}
# Ripple's key -> SCRIMMAGE's, per plugin. PIDs ({kp, ki, kd, integrate_within_*})
# become SCRIMMAGE's "P I D band" lists and wind_mps its three wind keys, a zero
# component left to SCRIMMAGE's default of zero (`values`).
KEYS = {
    "AuctionAssign": {"network": "network_name"},
    "Straight": {"speed_mps": "speed"},
    "WaypointFollower": {"waypoints_m": "waypoints", "speed_mps": "speed"},
    "ConstantVelocity": {"velocity_mps": "velocity"},
    "AircraftPIDController": {"max_pitch_deg": "max_pitch", "max_roll_deg": "max_roll",
                              "enable_roll_control": "use_roll_control"},
    "SimpleAircraftControllerPID": {"altitude_pid": "alt_pid", "speed_pid": "vel_pid"},
    "MultirotorVelocityController": {
        **MULTIROTOR_KEYS, "velocity_kp": "velocity_p", "velocity_ki": "velocity_i",
        "velocity_integral_limit_mps2": "velocity_integral_limit",
        "attitude_kp": "attitude_p", "body_rate_kp": "rate_p", "max_tilt_deg": "max_tilt",
        "max_vertical_acceleration_mps2": "max_vertical_acceleration"},
    "MotorSpeeds": {"speeds_radps": "speeds"},
    "Boundary": {"center_m": "center", "lengths_m": "lengths", "network": "network_name"},
    "GroundCollision": {"ground_altitude_m": "ground_collision_z"},
    "SimpleCollision": {"collision_range_m": "collision_range",
                        "startup_collision_range_m": "startup_collision_range",
                        "startup_altitude_only": "init_alt_deconflict"},
    "WaypointBroadcast": {"waypoints_m": "waypoints", "update_waypoints_m": "update_waypoints"},
    "SimpleCollisionMetrics": {"flight_time_weight": "flight_time_w",
                               "team_collision_weight": "team_collisions_w",
                               "non_team_collision_weight": "non_team_collisions_w"},
    "Multirotor": {**MULTIROTOR_KEYS, "body_drag_coefficient": "c_D",
                   "motor_time_constant_up_s": "motor_time_constant_up",
                   "motor_time_constant_down_s": "motor_time_constant_down"},
    "SimpleAircraft": {"min_speed_mps": "min_velocity", "max_speed_mps": "max_velocity",
                       "max_roll_deg": "max_roll", "max_pitch_deg": "max_pitch",
                       "max_roll_rate_degps": "max_roll_rate",
                       "max_pitch_rate_degps": "max_pitch_rate"},
    "SingleIntegrator": {"max_speed_mps": "max_speed"},
    "SphereNetwork": {"range_m": "range", "transmit_probability": "prob_transmit",
                      "enable_altitude_split": "filter_comms_plane",
                      "split_altitude_m": "comms_boundary_altitude",
                      "split_margin_m": "comms_boundary_epsilon"},
    "FixedWing6DOF": {
        "mass_kg": "mass", "gravity_mps2": "gravity_magnitude",
        "air_density_kgpm3": "air_density", "wing_span_m": "wing_span",
        "wing_area_m2": "surface_area_of_wing", "mean_chord_m": "chord_length",
        "oswald_efficiency": "efficiency_factor",
        "min_throttle": "throttle_input_min", "max_throttle": "throttle_input_max",
        "min_thrust_n": "thrust_min", "max_thrust_n": "thrust_max",
        "min_elevator_rad": "delta_elevator_min", "max_elevator_rad": "delta_elevator_max",
        "min_aileron_rad": "delta_aileron_min", "max_aileron_rad": "delta_aileron_max",
        "min_rudder_rad": "delta_rudder_min", "max_rudder_rad": "delta_rudder_max",
        "c_drag_0": "C_D0", "c_drag_alpha": "C_D_alpha", "c_drag_elevator": "C_D_delta_elevator_",
        "c_lift_0": "C_L0", "c_lift_alpha": "C_L_alpha", "c_lift_q": "C_LQ",
        "c_lift_elevator": "C_L_delta_elevator",
        "c_side_beta": "C_Y_beta", "c_side_rudder": "C_Y_delta_rudder",
        "c_roll_beta": "C_L_beta", "c_roll_p": "C_LP", "c_roll_r": "C_LR",
        "c_roll_aileron": "C_L_delta_aileron", "c_roll_rudder": "C_L_delta_rudder",
        "c_pitch_0": "C_M0", "c_pitch_alpha": "C_M_alpha", "c_pitch_q": "C_MQ",
        "c_pitch_elevator": "C_M_delta_elevator",
        "c_yaw_beta": "C_N_beta", "c_yaw_p": "C_NP", "c_yaw_r": "C_NR",
        "c_yaw_aileron": "C_N_delta_aileron", "c_yaw_rudder": "C_N_delta_rudder",
        "enable_ground_spring": "use_ground_model", "enable_launcher": "use_launcher",
        "launch_at_s": "launch_time", "launch_acceleration_mps2": "launch_accel",
        "launch_speed_mps": "launch_speed"},
}
# Ripple's end condition -> SCRIMMAGE's.
END_CONDITIONS = {"no_entities": "all_dead", "one_team_left": "one_team"}
PID_BANDS = ("integrate_within_deg", "integrate_within_m", "integrate_within_mps")


def values(key, value):
    """SCRIMMAGE's (key, value) pairs for one Ripple parameter."""
    if key == "wind_mps":
        return [(name, speed) for name, speed in zip(("wind_E", "wind_N", "wind_U"), value) if speed]
    if isinstance(value, dict) and "kp" in value:
        band = next(value[name] for name in PID_BANDS if name in value)
        return [(key, [value["kp"], value["ki"], value["kd"], band])]
    return [(key, value)]


def scrimmage_keys(name, params):
    """Ripple's parameters under SCRIMMAGE's keys, sorted by them."""
    renames = KEYS.get(name, {})
    params = dict(sorted((renames.get(key, key), value) for given_key, given in params.items()
                         for key, value in values(given_key, given)))
    for key, scrimmage_key in (("inertia_kgm2", "inertia_matrix"),
                               ("inertia_slug_ft2", "inertia_matrix_slug_ft_sq")):
        if key in params:
            params[scrimmage_key] = rows(params.pop(key))
    if "rotors" in params:
        params["rotor_config"] = rotor_rows(params.pop("rotors"))
    return params


def plugin(parent, tag, config, label_attribute="instance"):
    profile = AIRFRAMES.get((config["name"], config["params"].get("airframe")), {})
    params = {**profile, **scrimmage_keys(config["name"], config["params"])}
    if tag == "sensor" and config["name"] in NOISY_SENSORS:
        for prefix, bias, stddev in NOISE_KEYS:
            biases = params.pop(bias, [0, 0, 0])
            deviations = params.pop(stddev, [0, 0, 0])
            for axis in range(3):
                params[f"{prefix}_{axis}"] = f"{text(biases[axis])} {text(deviations[axis])}"
    if config["instance"] is not None:
        params[label_attribute] = config["instance"]
    if config["loop_rate_hz"]:
        params["loop_rate"] = config["loop_rate_hz"]
    node = ET.SubElement(parent, tag, {key: text(value) for key, value in params.items()})
    node.text = config["name"]


def element(parent, tag, value):
    ET.SubElement(parent, tag).text = text(value)


def write(scenario, path):
    ET.ElementTree(mission(scenario)).write(path, encoding="utf-8", xml_declaration=True)


def mission(scenario):
    """The XML tree, for a caller that adds SCRIMMAGE's own settings before writing."""
    if scenario["terrain"] is not None:
        raise ValueError("SCRIMMAGE missions cannot name terrain")
    run = scenario["run"]
    root = ET.Element("runscript", {"name": "mission"})
    # Headless and unpaced, like a Ripple run.
    ET.SubElement(root, "run", {
        "start": text(run["start_s"]), "end": text(run["end_s"]), "dt": text(run["dt_s"]),
        "motion_multiplier": text(run["motion_multiplier"]),
        "enable_gui": "false", "network_gui": "false", "start_paused": "false", "time_warp": "0",
    })
    element(root, "seed", run["seed"])
    ends = [END_CONDITIONS.get(name, name)
            for name, on in scenario["end_conditions"].items() if on]
    element(root, "end_condition", ", ".join(sorted(ends)) or "none")
    # The spatial lookup changes only how fast Ripple answers range queries.
    for config in scenario["networks"]:
        plugin(root, "network", config, label_attribute="name")
    for config in scenario["interactions"]:
        plugin(root, "entity_interaction", config)
    for config in scenario["metrics"]:
        plugin(root, "metrics", config)
    for group in scenario["entities"]:
        for estimator in ("localization", "tracking"):
            config = group[estimator]
            if config["name"] != "PassThrough" or config["params"]:
                raise ValueError(f"entity {group['label']}: SCRIMMAGE has no {estimator} plugins")
        entity = ET.SubElement(root, "entity")
        element(entity, "team_id", group["team"])
        element(entity, "visual_model", group["visual_model"])
        element(entity, "health", group["health"])
        if group["id"] is not None:
            element(entity, "id", group["id"])
        element(entity, "count", group["count"])
        if group["spawn"] is not None:
            spawn = group["spawn"]
            element(entity, "generate_rate", spawn["rate_hz"])
            element(entity, "generate_count", spawn["batch_size"])
            element(entity, "generate_start_time", spawn["start_s"])
            # SCRIMMAGE calls this a variance, but reads it as a standard deviation.
            element(entity, "generate_time_variance", spawn["time_stddev_s"])
        for axis, position, variance, velocity in zip(
                "xyz", group["position_m"], group["position_variance_m2"], group["velocity_mps"]):
            element(entity, axis, position)
            element(entity, f"variance_{axis}", variance)
            element(entity, f"v{axis}", velocity)
        element(entity, "heading", group["heading_deg"])
        element(entity, "variance_heading", group["heading_variance_deg2"])
        element(entity, "roll", group["roll_deg"])
        element(entity, "pitch", group["pitch_deg"])
        element(entity, "use_variance_all_ents", group["randomize_every_spawn"])
        for slot, tag in (("autonomy", "autonomy"), ("controller", "controller"),
                          ("sensors", "sensor")):
            for config in group[slot]:
                plugin(entity, tag, config)
        plugin(entity, "motion_model", group["motion_model"])
    ET.indent(root)
    return root
