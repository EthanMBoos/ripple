#!/usr/bin/env python3
"""Do Ripple's MPE point agents step exactly as MPE2's simple_spread_v3?

    python3 reference/mpe_parity.py --output runs/mpe-parity000 [--python PATH]

The bottom rung of the learning study (docs/TODO.md) is MPE2 v1.1.1's
`simple_spread_v3` with default settings. This runs EPISODES seeded episodes
in MPE2 with random continuous actions (reference/mpe2/rollout.py), replays
each in Ripple from the same starts with the same forces (MpeParticle,
MpeContact, SimpleSpread, ForceReplay), and compares every agent's position
and velocity after every step, and each episode's mean return (the episode
total only: summary.csv does not hold per-step rewards).

Passes when every position and velocity matches MPE2's bit for bit, with and
without contact, and every mean return agrees to the 6 decimals of
summary.csv.

MPE2 runs in a uv environment with MPE2 (pinned) unless --python names an
interpreter that has it. Writes OUTPUT/results.json.
"""
import argparse
import json
import math
from pathlib import Path
import subprocess
import sys

from frames import read_frames
from project import ROOT, build_cli, source_identity

MPE2 = "mpe2==1.1.1"
MPE2_COMMIT = "147a8e88b669de9f0b06d1947c44d5c380c570bb"
HELPER = ROOT / "reference/mpe2/rollout.py"
EPISODES = 100
STEPS = 25
RADIUS_M = 0.15
# MPE's push is soft: at 2 radii plus d it changes velocity by about
# 0.01 * exp(-d / 1 mm) m/s per step. Beyond 40 mm that is under 1e-19 m/s,
# below any velocity's last bit here, so only closer approaches count as
# contact (reported, not part of the criterion).
CONTACT_RANGE_M = 2 * RADIUS_M + 0.040
RETURN_TOLERANCE = 1e-6  # summary.csv keeps 6 decimals


def mission(episode):
    lines = ["format_version: 1", f"name: MPE parity, seed {episode['seed']}",
             "run:", f"  end_s: {STEPS * 0.1}", "  dt_s: 0.1", "  seed: 1",
             "interactions:", "  MpeContact:", "    teams: [1]", f"    radius_m: {RADIUS_M}",
             "metrics:", "  SimpleSpread:", "    agent_team: 1", "    landmark_team: 2",
             f"    agent_radius_m: {RADIUS_M}", "entities:"]
    for index, (x, y) in enumerate(episode["agents"]):
        forces = [step[index] for step in episode["forces"]]
        lines += [f"  agent{index}:", "    team: 1", f"    position_m: [{x!r}, {y!r}, 0]",
                  "    velocity_mps: [0, 0, 0]", "    autonomy:", "      ForceReplay:",
                  f"        forces: {forces!r}", "    motion_model:", "      MpeParticle:"]
    for index, (x, y) in enumerate(episode["landmarks"]):
        lines += [f"  landmark{index}:", "    team: 2", f"    position_m: [{x!r}, {y!r}, 0]",
                  "    autonomy:", "      ConstantVelocity:", "    motion_model:",
                  "      SingleIntegrator:"]
    return "\n".join(lines) + "\n"


def mean_return(folder):
    header, row = (folder / "summary.csv").read_text().splitlines()[:2]
    return float(dict(zip(header.split(","), row.split(",")))["return"])


def compare(episode, frames, ripple_return):
    # Frame k holds the state at step k, before that step; the last holds the
    # state after the last step.
    if len(frames) != STEPS + 1:
        raise ValueError(f"seed {episode['seed']}: {len(frames)} frames, expected {STEPS + 1}")
    agents = len(episode["agents"])
    worst, exact = 0.0, True
    for k, frame in enumerate(frames):
        for index in range(agents):
            entity = frame.entities[index + 1]
            position = episode["agents"][index] if k == 0 else episode["positions"][k - 1][index]
            velocity = [0.0, 0.0] if k == 0 else episode["velocities"][k - 1][index]
            for ours, theirs in zip([*entity.position[:2], *entity.velocity[:2]], [*position, *velocity]):
                if not math.isfinite(ours):
                    raise ValueError(f"seed {episode['seed']}: nonfinite Ripple state")
                worst = max(worst, abs(ours - theirs))
                exact = exact and ours == theirs
    returns = [sum(step[index] for step in episode["rewards"]) for index in range(agents)]
    mpe_return = sum(returns) / agents
    # Any pair within contact range at the start of a step.
    starts = [episode["agents"], *episode["positions"][:-1]]
    contact = any(math.dist(step[a], step[b]) < CONTACT_RANGE_M
                  for step in starts for a in range(agents) for b in range(a + 1, agents))
    return dict(seed=episode["seed"], contact=contact, bit_identical=exact,
                largest_state_error=worst, mpe2_return=mpe_return, ripple_return=ripple_return,
                return_error=abs(ripple_return - mpe_return))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--python", help="an interpreter with MPE2 installed")
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    binary = build_cli("release")
    helper = [args.python, "-I"] if args.python else [
        "uv", "run", "--quiet", "--no-project", "--python", "3.12", "--with", MPE2, "python", "-I"]

    subprocess.run([*helper, str(HELPER), ",".join(str(seed) for seed in range(EPISODES)),
                    str(args.output / "mpe2.json")], check=True, capture_output=True)
    episodes = json.loads((args.output / "mpe2.json").read_text())
    results = []
    for episode in episodes:
        folder = args.output / f"seed{episode['seed']:03}"
        folder.mkdir()
        (folder / "mission.yaml").write_text(mission(episode))
        subprocess.run([binary, "run", folder / "mission.yaml", "--root", ROOT, "--output",
                        folder / "ripple", "--no-rerun"], check=True, capture_output=True)
        results.append(compare(episode, read_frames(folder / "ripple/frames.bin"),
                               mean_return(folder / "ripple")))

    passed = (len(results) == EPISODES
              and all(r["bit_identical"] for r in results)
              and all(r["return_error"] <= RETURN_TOLERANCE for r in results))
    summary = dict(
        episodes=len(results),
        episodes_with_contact=sum(r["contact"] for r in results),
        bit_identical=sum(r["bit_identical"] for r in results),
        largest_state_error=max(r["largest_state_error"] for r in results),
        largest_return_error=max(r["return_error"] for r in results))
    report = dict(passed=passed, mpe2=MPE2, mpe2_commit=MPE2_COMMIT, steps=STEPS,
                  criterion=dict(state="bit identical", return_=RETURN_TOLERANCE),
                  summary=summary, results=results, source=source_identity())
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(dict(passed=passed, **summary), indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
