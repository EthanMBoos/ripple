#!/usr/bin/env python3
"""How much the frozen RK4 loads in FixedWing6DOF and Multirotor cost.

    python3 paper/data/rk4-loads/measure.py paper/data/rk4-loads/results.json

Exports the current checkout (tracked files, including uncommitted changes) to a
temporary folder, applies stage-loads.patch, and builds `scrimmage`. The patch
adds SCRIMMAGE_STAGE_LOADS: when set, both models recompute forces and moments
from each RK4 stage's state instead of holding the start-of-step values. With
it unset, output is unchanged. Each run writes frames.bin, which this script
compares. Standard library only; needs git and Cargo.

Part A, integration accuracy with constant controls: Multirotor
(missions/multirotor.xml, constant motor speeds) and FixedWing6DOF with every
controller gain zero (fixed-wing-open-loop.xml). Each runs with frozen and with
stage-evaluated loads at motion multipliers 1 to 16, against stage-evaluated
loads at multiplier 1024. With constant controls, the substep count changes
only the integration step.

Part B, a controlled flight at practical steps: fixed-wing-recovery.xml at
outer steps of 0.01, 0.05, and 0.1 s with one substep. Frozen loads are
compared with stage-evaluated loads under the same controller schedule.
"""

import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT / "reference"))
from compare import distance, orientation_distance_rad  # noqa: E402
from frames import read_frames  # noqa: E402

MULTIPLIERS = [1, 2, 4, 8, 16]
REFERENCE_MULTIPLIER = 1024
PART_A = [("multirotor", ROOT / "missions/multirotor.xml", 0.01),
          ("fixed-wing-open-loop", HERE / "fixed-wing-open-loop.xml", 0.05)]
PART_B = [("fixed-wing-recovery", HERE / "fixed-wing-recovery.xml", dt) for dt in (0.01, 0.05, 0.1)]


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def build(scratch):
    """Build the patched checkout; return the executable and its provenance."""
    snapshot = git("stash", "create") or git("rev-parse", "HEAD")
    source = scratch / "source"
    source.mkdir()
    archive = subprocess.run(["git", "archive", snapshot], cwd=ROOT, check=True,
                             capture_output=True).stdout
    subprocess.run(["tar", "-x", "-C", source], input=archive, check=True)
    # Hash the exported sources before patching, as reference_check.py does.
    files = [source / "Cargo.toml", source / "Cargo.lock"]
    files += [path for path in (source / "crates").rglob("*")
              if path.is_file() and path.suffix in {".rs", ".xml", ".toml"}]
    source_hashes = {str(path.relative_to(source)): hashlib.sha256(path.read_bytes()).hexdigest()
                     for path in sorted(files)}
    patch = HERE / "stage-loads.patch"
    subprocess.run(["git", "apply", patch], cwd=source, check=True)
    subprocess.run(["cargo", "build", "--release", "--locked", "-p", "scrimmage-rs",
                    "--bin", "scrimmage"], cwd=source, check=True)
    provenance = {"revision": git("rev-parse", "HEAD"), "dirty": bool(git("status", "--porcelain",
                                                                          "--untracked-files=no")),
                  "patch_sha256": hashlib.sha256(patch.read_bytes()).hexdigest(),
                  "source_hashes": source_hashes}
    return source / "target/release/scrimmage", provenance


def run(binary, mission, dt_s, multiplier, stage_loads, scratch):
    """Run the mission with the given step and substeps; return its frames."""
    tree = ET.parse(mission)
    tree.getroot().find("run").set("dt", repr(dt_s))
    tree.getroot().find("run").set("motion_multiplier", str(multiplier))
    name = f"{mission.stem}-{dt_s}-{multiplier}-{'stage' if stage_loads else 'frozen'}"
    edited = scratch / f"{name}.xml"
    tree.write(edited, encoding="utf-8", xml_declaration=True)
    env = {key: value for key, value in os.environ.items() if key != "SCRIMMAGE_STAGE_LOADS"}
    if stage_loads:
        env["SCRIMMAGE_STAGE_LOADS"] = "1"
    output = scratch / name
    result = subprocess.run([binary, "run", edited, "--root", ROOT, "--output", output,
                             "--workers", "1", "--no-rerun"], env=env, capture_output=True,
                            text=True)
    if result.returncode != 0:
        raise RuntimeError(f"{name} failed:\n{result.stderr}")
    return read_frames(output / "frames.bin")


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
    with tempfile.TemporaryDirectory() as directory:
        scratch = Path(directory)
        binary, provenance = build(scratch)
        part_a = []
        for name, mission, dt_s in PART_A:
            reference = run(binary, mission, dt_s, REFERENCE_MULTIPLIER, True, scratch)
            check = largest_difference(run(binary, mission, dt_s, REFERENCE_MULTIPLIER // 2, True,
                                           scratch), reference)[0]
            rows = {"frozen": [], "stage": []}
            for loads in rows:
                for multiplier in MULTIPLIERS:
                    frames = run(binary, mission, dt_s, multiplier, loads == "stage", scratch)
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
            frozen = run(binary, mission, dt_s, 1, False, scratch)
            stage = run(binary, mission, dt_s, 1, True, scratch)
            position, attitude = largest_difference(frozen, stage)
            part_b.append({"mission": name, "dt_s": dt_s, "frozen_vs_stage_position_m": position,
                           "frozen_vs_stage_attitude_rad": attitude})
            print(json.dumps(part_b[-1]), flush=True)
    output.write_text(json.dumps({
        "date": datetime.datetime.now().isoformat(timespec="seconds"),
        "machine": f"{platform.system()}-{platform.machine()}", **provenance,
        "part_a": part_a, "part_b": part_b}, indent=2) + "\n")


if __name__ == "__main__":
    main()
