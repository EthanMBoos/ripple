"""The benchmark workloads: fixed missions of N aircraft shared by the timing,
determinism, and comparison checks (benchmark.py, native_benchmark.py,
determinism.py, spatial_benchmark.py, message_benchmark.py). Not a script.
Standard library only.
"""

import os
from pathlib import Path
import platform
import subprocess

from scenario import write as write_mission  # noqa: F401  re-exported for the checks


def mission(agents, end, multiplier=1, controller_rate=0, spread=0, interactions=None,
            spawn=None, sensors=None, autonomy=None):
    """A YAML mission (written as JSON, which YAML reads) of `agents` aircraft
    flying straight at 200 m, every one with its own random start."""
    aircraft = {"team": 1, "count": agents, "position_m": [0, 0, 200],
                "position_stddev_m": [spread, spread, 0]}
    if spawn:
        aircraft["spawn"] = spawn
    aircraft.update({
        "autonomy": {"Straight": {"speed_mps": 20}, **(autonomy or {})},
        "controller": {"SimpleAircraftControllerPID": {"loop_rate_hz": controller_rate}},
        "motion_model": {"SimpleAircraft": None},
        "sensors": sensors or {},
    })
    return {"format_version": 1, "name": "perf",
            "run": {"end_s": end, "dt_s": 0.1, "motion_multiplier": multiplier, "seed": 1},
            "interactions": interactions or {},
            "networks": {"GlobalNetwork": None, "LocalNetwork": None},
            "metrics": {"SimpleCollisionMetrics": None},
            "entities": {"aircraft": aircraft}}


COLLISION = {"SimpleCollision": {"collision_range_m": 5}}
NOISE_5_5_1 = {"position_stddev_m": [5, 5, 1], "velocity_stddev_mps": [5, 5, 1]}
# name: (description, agent counts, mission fields)
WORKLOADS = {
    "motion": ("aircraft flying straight", (64, 512), dict(end=200)),
    "substeps": ("ten controller and motion substeps per step", (64, 512), dict(
        end=60, multiplier=10)),
    "sensing": ("own-state and contact sensors on every aircraft", (32, 128), dict(
        end=60, sensors={"NoisyState": NOISE_5_5_1, "NoisyContacts": NOISE_5_5_1})),
    # Churn spreads its agents so collisions happen; the others above start together.
    "churn": ("scheduled spawning with collision removal", (64, 512), dict(
        end=200, spread=50,
        # Aircraft hold 200 m, so this ground check costs time but never removes.
        interactions={**COLLISION, "GroundCollision": {"ground_altitude_m": 150}},
        spawn={"rate_hz": 20, "batch_size": 4})),
    # Aircraft spread over about a kilometre, so few collide and the population
    # stays nearly constant: this measures the every-pair collision check itself.
    "collision": ("pairwise collision checks among active aircraft", (256, 1024), dict(
        end=20, spread=1000, interactions=COLLISION)),
    # The same mission, with spatial queries answered by a grid of 5 m cells
    # (`grid_cell_m`, passed to the runner) instead of a scan. Output is identical.
    "collision-grid": ("collision with a 5 m spatial grid", (256, 1024), dict(
        end=20, spread=1000, interactions=COLLISION, grid_cell_m=5)),
}


def workload(name, agents, **changes):
    """WORKLOADS[name] at `agents` aircraft as a mission, with any fields changed."""
    fields = {**WORKLOADS[name][2], **changes}
    fields.pop("grid_cell_m", None)  # a runner option, not part of the mission
    return mission(agents, **fields)


def cpu_model():
    try:
        if platform.system() == "Darwin":
            return subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"],
                                           text=True).strip()
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except (OSError, subprocess.SubprocessError):
        pass
    return platform.processor() or "unknown"


def environment():
    """What a timing depends on besides the code: the machine and the toolchain."""
    return {"machine": f"{platform.system()} {platform.machine()}, {cpu_model()}, "
                       f"{os.cpu_count()} CPUs",
            "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip()}
