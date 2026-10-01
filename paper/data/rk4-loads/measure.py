#!/usr/bin/env python3
"""What evaluating FixedWing6DOF and Multirotor forces at every RK4 stage
changed, against holding them at the start of the step as C++ did.

    python3 paper/data/rk4-loads/measure.py paper/data/rk4-loads/results.json

Builds `scrimmage` twice: BEFORE, the last revision with frozen loads, and the
current checkout (tracked files, including uncommitted changes), which
recomputes forces and moments from each stage's state. Each run writes
frames.bin, which this script compares. Standard library only; needs git and
Cargo. Run part C on an idle machine.

Part A, integration accuracy with constant controls: Multirotor
(missions/multirotor.xml, constant motor speeds) and FixedWing6DOF with every
controller gain zero (fixed-wing-open-loop.xml). Each runs with frozen and with
stage-evaluated loads at motion multipliers 1 to 16, against stage-evaluated
loads at multiplier 1024. With constant controls, the substep count changes
only the integration step.

Part B, a controlled flight at practical steps: fixed-wing-recovery.xml at
outer steps of 0.01, 0.05, and 0.1 s with one substep. Frozen loads are
compared with stage-evaluated loads under the same controller schedule.

Part C, speed: 128 FixedWing6DOF aircraft (AircraftPIDController, 30 s at
0.01 s) and 128 hovering Multirotors (4 s at 0.001 s), one worker, whole
`scrimmage run` wall time including writing frames: a warm-up, then the median
and range of five runs.

Part D, the altitude gain: paper/data/aircraft-pid-upset/upset.xml (the
default A-4, 45 s, 5 degree roll and 3 degree pitch upset) with altitude D
0.01 (upstream), 0.002 (the default), and 0, at 0.01 s (the setting that
motivated lowering the gain) and two halvings: the largest altitude error over
the run and after the first 5 s. It tests whether frozen loads caused the
oscillation at D 0.01.
"""

import datetime
import json
import math
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT / "reference"))
from compare import distance, orientation_distance_rad  # noqa: E402
from frames import read_frames  # noqa: E402

BEFORE = "b705319"  # the last revision with frozen loads
MULTIPLIERS = [1, 2, 4, 8, 16]
REFERENCE_MULTIPLIER = 1024
PART_A = [("multirotor", ROOT / "missions/multirotor.xml", 0.01),
          ("fixed-wing-open-loop", HERE / "fixed-wing-open-loop.xml", 0.05)]
PART_B = [("fixed-wing-recovery", HERE / "fixed-wing-recovery.xml", dt) for dt in (0.01, 0.05, 0.1)]
UPSET = ROOT / "paper/data/aircraft-pid-upset/upset.xml"
UPSET_GAINS = [0.01, 0.002, 0.0]
UPSET_STEPS_S = [0.01, 0.005, 0.0025]
UPSET_ALTITUDE_M = 1000.0
TIMED_AGENTS = 128
TIMED_RUNS = 5
TIMED_MISSIONS = {
    "fixed-wing-6dof": """<?xml version="1.0"?>
<runscript name="timed-fixed-wing">
  <run start="0" end="30" dt="0.01" enable_gui="false" start_paused="false" time_warp="0" />
  <seed>1</seed><end_condition>time</end_condition><output_type>all</output_type>
  <network>LocalNetwork</network><network>GlobalNetwork</network>
  <entity>
    <count>{count}</count><team_id>1</team_id><x>0</x><y>0</y><z>1000</z>
    <vx>220</vx><heading>0</heading><roll>5</roll><pitch>3</pitch>
    <variance_x>2000</variance_x><variance_y>2000</variance_y><variance_z>100</variance_z>
    <autonomy speed="220">Straight</autonomy><controller>AircraftPIDController</controller>
    <motion_model>FixedWing6DOF</motion_model><visual_model>sphere</visual_model>
  </entity>
</runscript>
""",
    "multirotor": """<?xml version="1.0"?>
<runscript name="timed-multirotor">
  <run start="0" end="4" dt="0.001" enable_gui="false" start_paused="false" time_warp="0" />
  <seed>1</seed><end_condition>time</end_condition><output_type>all</output_type>
  <network>LocalNetwork</network><network>GlobalNetwork</network>
  <entity>
    <count>{count}</count><team_id>1</team_id><x>0</x><y>0</y><z>30</z><heading>0</heading>
    <roll>3</roll><variance_x>100</variance_x><variance_y>100</variance_y>
    <autonomy speed="0">Straight</autonomy>
    <controller speeds="820 821 823 822">MotorSpeeds</controller>
    <motion_model>Multirotor</motion_model><visual_model>sphere</visual_model>
  </entity>
</runscript>
""",
}


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def build(revision, folder):
    """Export a revision and build its release `scrimmage`."""
    folder.mkdir()
    archive = subprocess.run(["git", "archive", revision], cwd=ROOT, check=True,
                             capture_output=True).stdout
    subprocess.run(["tar", "-x", "-C", folder], input=archive, check=True)
    subprocess.run(["cargo", "build", "--release", "--locked", "-p", "scrimmage-rs",
                    "--bin", "scrimmage"], cwd=folder, check=True)
    return folder / "target/release/scrimmage"


def run(binaries, mission, dt_s, multiplier, loads, scratch):
    """Run the mission with the given step and substeps; return its frames."""
    tree = ET.parse(mission)
    tree.getroot().find("run").set("dt", repr(dt_s))
    tree.getroot().find("run").set("motion_multiplier", str(multiplier))
    name = f"{mission.stem}-{dt_s}-{multiplier}-{loads}"
    edited = scratch / f"{name}.xml"
    tree.write(edited, encoding="utf-8", xml_declaration=True)
    output = scratch / name
    result = subprocess.run([binaries[loads], "run", edited, "--root", ROOT, "--output", output,
                             "--workers", "1", "--no-rerun"], capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"{name} failed:\n{result.stderr}")
    return read_frames(output / "frames.bin")


def timed(binaries, scratch):
    """Median and range of whole-run wall time for each model and loads version."""
    rows = []
    for name, text in TIMED_MISSIONS.items():
        mission = scratch / f"timed-{name}.xml"
        mission.write_text(text.format(count=TIMED_AGENTS))
        for loads, binary in binaries.items():
            seconds = []
            for attempt in range(TIMED_RUNS + 1):
                output = scratch / f"timed-{name}-{loads}-{attempt}"
                started = time.perf_counter()
                subprocess.run([binary, "run", mission, "--root", ROOT, "--output", output,
                                "--workers", "1", "--no-rerun"], check=True, capture_output=True)
                if attempt:  # the first run is a warm-up
                    seconds.append(time.perf_counter() - started)
            rows.append({"mission": name, "agents": TIMED_AGENTS, "loads": loads,
                         "median_s": statistics.median(seconds), "min_s": min(seconds),
                         "max_s": max(seconds)})
            print(json.dumps(rows[-1]), flush=True)
    return rows


def altitude_errors(binary, gain, dt_s, scratch, loads):
    """Largest altitude error (m) of the upset flight, over the run and after 5 s."""
    tree = ET.parse(UPSET)
    tree.getroot().find("run").set("dt", repr(dt_s))
    tree.getroot().find("entity/controller").set("altitude_pid", f"0.1, 0.0001, {gain!r}, 10")
    name = f"upset-{loads}-{gain}-{dt_s}"
    edited = scratch / f"{name}.xml"
    tree.write(edited, encoding="utf-8", xml_declaration=True)
    output = scratch / name
    subprocess.run([binary, "run", edited, "--root", ROOT, "--output", output, "--workers", "1",
                    "--no-rerun"], check=True, capture_output=True)
    whole = after_5_s = 0.0
    for frame in read_frames(output / "frames.bin"):
        for entity in frame.entities.values():
            error = abs(entity.position[2] - UPSET_ALTITUDE_M)
            whole = max(whole, error)
            if frame.time_s >= 5.0:
                after_5_s = max(after_5_s, error)
    return whole, after_5_s


def largest_difference(frames, reference):
    """Largest position (m) and attitude (rad) difference over every frame and entity."""
    position = attitude = 0.0
    for candidate, expected in zip(frames, reference, strict=True):
        for key, entity in expected.entities.items():
            other = candidate.entities[key]
            if not all(math.isfinite(value) for value in other.position):
                return math.inf, math.inf
            position = max(position, distance(other.position, entity.position))
            attitude = max(attitude, orientation_distance_rad(entity.orientation,
                                                              other.orientation))
    return position, attitude


def order(errors):
    """Observed order between successive halvings: log2(e(h) / e(h/2))."""
    return [round(math.log2(a / b), 2) if a > 0 and b > 0 and math.isfinite(a) else None
            for a, b in zip(errors, errors[1:])]


def main():
    output = Path(sys.argv[1]).resolve()
    snapshot = git("stash", "create") or git("rev-parse", "HEAD")
    with tempfile.TemporaryDirectory() as directory:
        scratch = Path(directory)
        binaries = {"frozen": build(BEFORE, scratch / "before"),
                    "stage": build(snapshot, scratch / "after")}
        part_a = []
        for name, mission, dt_s in PART_A:
            reference = run(binaries, mission, dt_s, REFERENCE_MULTIPLIER, "stage", scratch)
            check = largest_difference(run(binaries, mission, dt_s, REFERENCE_MULTIPLIER // 2,
                                           "stage", scratch), reference)[0]
            rows = {"frozen": [], "stage": []}
            for loads in rows:
                for multiplier in MULTIPLIERS:
                    frames = run(binaries, mission, dt_s, multiplier, loads, scratch)
                    position, attitude = largest_difference(frames, reference)
                    rows[loads].append({"multiplier": multiplier, "motion_dt_s": dt_s / multiplier,
                                        "position_m": position, "attitude_rad": attitude})
            part_a.append({
                "mission": name, "dt_s": dt_s, "reference_multiplier": REFERENCE_MULTIPLIER,
                "reference_self_check_m": check, **rows,
                "frozen_order": order([row["position_m"] for row in rows["frozen"]]),
                "stage_order": order([row["position_m"] for row in rows["stage"]]),
            })
            print(json.dumps(part_a[-1]), flush=True)
        part_b = []
        for name, mission, dt_s in PART_B:
            frozen = run(binaries, mission, dt_s, 1, "frozen", scratch)
            stage = run(binaries, mission, dt_s, 1, "stage", scratch)
            position, attitude = largest_difference(frozen, stage)
            part_b.append({"mission": name, "dt_s": dt_s, "frozen_vs_stage_position_m": position,
                           "frozen_vs_stage_attitude_rad": attitude})
            print(json.dumps(part_b[-1]), flush=True)
        part_c = timed(binaries, scratch)
        part_d = []
        for loads, binary in binaries.items():
            for gain in UPSET_GAINS:
                for dt_s in UPSET_STEPS_S:
                    whole, after = altitude_errors(binary, gain, dt_s, scratch, loads)
                    part_d.append({"loads": loads, "altitude_d": gain, "dt_s": dt_s,
                                   "altitude_error_m": whole, "after_5_s_m": after})
                    print(json.dumps(part_d[-1]), flush=True)
    output.write_text(json.dumps({
        "date": datetime.datetime.now().isoformat(timespec="seconds"),
        "machine": f"{platform.system()}-{platform.machine()}",
        "before": git("rev-parse", BEFORE), "after": git("rev-parse", "HEAD"),
        "after_dirty": bool(git("status", "--porcelain", "--untracked-files=no")),
        "part_a": part_a, "part_b": part_b, "part_c": part_c, "part_d": part_d}, indent=2) + "\n")


if __name__ == "__main__":
    main()
