#!/usr/bin/env python3
"""Generate random comparison missions from retained models, for differential testing.

    python3 reference/generate_scenarios.py --count 50 --seed 1 --output runs/generated
    python3 reference/reference_check.py --mission-dir runs/generated

Each mission is a seeded draw over: timestep, horizon, motion substeps, entity
families (SimpleAircraft, SingleIntegrator, FixedWing6DOF), start states, update
rates, spawn schedules and positional variance, sensor biases (zero noise), and
interactions (SimpleCollision, GroundCollision, Boundary). Every choice is
comparable: spawn draws are keyed on both sides (SCRIMMAGE_KEYED_RANDOM=1),
sensor noise is zero, and FixedWing6DOF runs without substeps. Missions are
YAML, written as JSON (which YAML reads). Standard library only; the same seed
always writes the same files.
"""

import argparse
import json
from pathlib import Path
import random


def fmt(value):
    """Six significant digits, so missions stay short to read."""
    return float(f"{value:.6g}")


def sensor_bias(rng):
    """Nonzero biases with zero standard deviation: deterministic equations only."""
    scale = {"position_bias_m": 2.0, "velocity_bias_mps": 0.5, "attitude_bias_rad": 0.05}
    return {key: [fmt(rng.uniform(-1, 1) * size) for _ in range(3)] for key, size in scale.items()}


def aircraft_group(rng, fixed_wing):
    speed = rng.uniform(150, 250) if fixed_wing else rng.uniform(15, 30)
    group = {"heading_deg": fmt(rng.uniform(-180, 180)),
             "roll_deg": fmt(rng.uniform(-5, 5)), "pitch_deg": fmt(rng.uniform(-3, 3)),
             "autonomy": {"Straight": {"speed_mps": fmt(speed), "loop_rate_hz": rng.choice([0, 5, 10])}}}
    if fixed_wing:
        group["velocity_mps"] = [fmt(speed), 0, 0]  # FixedWing6DOF needs airspeed at start.
        group["controller"] = {"AircraftPIDController": {"airframe": "a4d"}}
        group["motion_model"] = {"FixedWing6DOF": {
            "airframe": "a4d", "wind_mps": [fmt(rng.uniform(-3, 3)), fmt(rng.uniform(-3, 3)), 0]}}
    else:
        group["controller"] = {"SimpleAircraftControllerPID": {"loop_rate_hz": rng.choice([0, 20, 50])}}
        group["motion_model"] = {"SimpleAircraft": {"max_roll_deg": fmt(rng.uniform(20, 45))}}
        if rng.random() < 0.4:
            sensor = rng.choice(["NoisyState", "NoisyContacts"])
            group["sensors"] = {sensor: sensor_bias(rng)}
    return group


def point_group(rng):
    velocity = [rng.uniform(-10, 10), rng.uniform(-10, 10), rng.uniform(-2, 2)]
    if rng.random() < 0.1:
        velocity = [0, 0, 0]
    max_speed = rng.choice([-1, -1, round(rng.uniform(1, 12), 3)])
    return {"autonomy": {"ConstantVelocity": {"velocity_mps": [fmt(value) for value in velocity],
                                              "loop_rate_hz": rng.choice([0, 2, 10])}},
            "controller": {"SingleIntegratorControllerSimple": None},
            "motion_model": {"SingleIntegrator": {"max_speed_mps": fmt(max_speed)}}}


def mission(rng, index, horizon=None):
    """Scenario `index` as a YAML mission, written as JSON (which YAML reads)."""
    fixed_wing = rng.random() < 0.2
    dt = 0.01 if fixed_wing else rng.choice([0.05, 0.1])
    multiplier = 1 if fixed_wing else rng.choice([1, 1, 2, 5])
    end = rng.choice([3, 5, 10]) if not fixed_wing else 3
    if horizon is not None:
        # Fixed-wing steps are 0.01 s; cap them at 10,000 steps.
        end = min(horizon, 100) if fixed_wing else horizon
    result = {"format_version": 1, "name": f"generated-{index:03}",
              "run": {"end_s": end, "dt_s": dt, "motion_multiplier": multiplier,
                      "seed": rng.randrange(1, 2**31 - 1)},
              "interactions": {},
              "networks": {"GlobalNetwork": None, "LocalNetwork": None},
              "metrics": {"SimpleCollisionMetrics": None},
              "entities": {}}
    interactions = result["interactions"]
    collisions = rng.random() < 0.5
    if collisions:
        interactions["SimpleCollision"] = {"collision_range_m": fmt(rng.uniform(1, 20))}
    if rng.random() < 0.3:
        # Below every start altitude, so startup placement never fails.
        interactions["GroundCollision"] = {"ground_altitude_m": fmt(rng.uniform(0, 90))}
    if not fixed_wing and rng.random() < 0.3:
        interactions["Boundary"] = {"center_m": [0, 0, 200], "lengths_m": [800, 800, 400]}
    for block in range(rng.randint(1, 4)):
        count = rng.randint(1, 3)
        position = [rng.uniform(-500, 500), rng.uniform(-500, 500),
                    rng.uniform(100, 300) if not fixed_wing else rng.uniform(800, 1200)]
        variance = [rng.choice([0, 0, 25, 100]), rng.choice([0, 0, 25, 100]), 0]
        if collisions:
            # Entities sharing a start need variance for collision-free placement.
            variance = [max(value, 400) for value in variance[:2]] + [0]
        group = {"team": rng.randint(1, 3), "count": count, "visual_model": "sphere",
                 "position_m": [fmt(value) for value in position],
                 "position_variance_m2": variance,
                 "heading_variance_deg2": rng.choice([0, 0, 25])}
        if rng.random() < 0.3:
            group["randomize_every_spawn"] = True
        if rng.random() < 0.35:
            group["spawn"] = {"rate_hz": fmt(rng.uniform(0.5, 4)), "batch_size": rng.randint(1, 2),
                              "start_s": fmt(rng.uniform(0, 1)),
                              "time_stddev_s": rng.choice([0, 0.05])}
        family = "fixed_wing" if fixed_wing else rng.choice(["aircraft", "point"])
        group.update(point_group(rng) if family == "point" else aircraft_group(rng, fixed_wing))
        result["entities"][f"group{block}"] = group
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--count", type=int, default=50)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    parser.add_argument("--horizon", type=float,
                        help="run every scenario for this many seconds (fixed-wing at most 100)")
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    rng = random.Random(args.seed)
    for index in range(args.count):
        text = json.dumps(mission(rng, index, args.horizon), indent=1)
        (args.output / f"generated-{index:03}.yaml").write_text(text + "\n")
    print(f"wrote {args.count} missions to {args.output}")


if __name__ == "__main__":
    main()
