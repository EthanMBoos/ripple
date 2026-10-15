"""A small scenario as a run's manifest.json records it, for tests that need no Cargo."""
import copy


def plugin(name, instance=None, **params):
    return {"name": name, "instance": instance, "loop_rate_hz": 0.0, "params": params}


_SCENARIO = {
    "run": {"start_s": 0.0, "end_s": 200.0, "dt_s": 0.1, "seed": 12345, "motion_multiplier": 1},
    "end_conditions": {"time": True, "all_dead": False, "one_team": False},
    "spatial_lookup": {"method": "scan"},
    "terrain": None,
    "interactions": [plugin("SimpleCollision", collision_range=5)],
    "networks": [plugin("GlobalNetwork"), plugin("SphereNetwork", "CommsNetwork", range=100)],
    "metrics": [plugin("SimpleCollisionMetrics")],
    "entities": [{
        "label": "blue", "team": 1, "color": [255, 255, 255], "visual_model": "aircraft",
        "health": 1, "id": None, "count": 64, "spawn": None,
        "position_m": [23.0, 0.0, 200.0], "position_variance_m2": [0.0, 0.0, 0.0],
        "heading_deg": 0.0, "heading_variance_deg2": 0.0, "roll_deg": 0.0, "pitch_deg": 0.0,
        "velocity_mps": [0.0, 0.0, 0.0], "randomize_every_spawn": False,
        "autonomy": [plugin("Straight", speed=27)],
        "controller": [plugin("SimpleAircraftControllerPID", heading_pid=[1, 0.01, 2, 9])],
        "motion_model": plugin("SimpleAircraft"),
        "sensors": [plugin("NoisyState", position_stddev_m=[1, 2, 3])],
        "localization": {**plugin("PassThrough"), "inputs": {"state": "NoisyState:0"}},
        "tracking": {**plugin("PassThrough"), "inputs": {"contacts": "perfect"}},
    }],
}


def scenario():
    """A fresh copy, free to edit."""
    return copy.deepcopy(_SCENARIO)
