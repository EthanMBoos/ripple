#!/usr/bin/env python3
"""How MultirotorVelocityController flies the Crazyflie.

    python3 reference/multirotor_control.py --output runs/multirotor-control000

Runs missions/multirotor-velocity.yaml (constant velocity commands from t = 0
on the crazyflie2 and firefly airframes, a 15 degree roll upset for each,
and a Firefly cruising at 12 m/s) and reports, per vehicle:

- step response of the commanded axis: 10-90% rise time, overshoot, time to
  stay within 5% of the command, and the error over the last second;
- the largest tilt, and the steady tilt over the last second. For the
  Crazyflie flying forward at 1 m/s, the steady tilt must match Förster's
  rotor drag: tan(tilt) = K_aero * (sum of rotor speeds) * v / (m g), with
  the rotor speeds of a hover. The same holds for the Firefly at 1 m/s with
  RotorS's rotor drag coefficient;
- the upset's largest height loss and its distance from the start after 10 s.

Then it reruns the mission with 10 and 20 motion substeps (half and a quarter
of the step; the controller runs at each substep, so this halves its period
too) and reports how far each trajectory lies from the finest one. The check
passes if every vehicle's whole velocity error stays within 5% of its
command over the last second (0.05 m/s when hovering), each steady tilt is
within 2% of its drag prediction, and the 5-substep trajectories of the
step-response vehicles lie within 5 mm of the 20-substep ones over the 10 s
(measured: 2.6 mm, and 0.9 mm at 10 substeps). The cruising Firefly flies
120 m, twelve times the others' distance, so its difference is reported but
not held to that 5 mm.
Writes OUTPUT/results.json and OUTPUT/summary.md. Standard library only;
needs Cargo.
"""
import argparse
import json
import math
from pathlib import Path
import subprocess
import sys

from frames import read_frames
from project import BINARY, ROOT, build_cli, source_identity

MISSION = ROOT / "missions/multirotor-velocity.yaml"
MULTIPLIERS = [5, 10, 20]
FINAL_WINDOW_S = 1.0
# Each preset (airframe.rs) as (mass, rotor count, c_T), with the in-plane
# rotor drag the mission switches on: Förster's K_aero, RotorS's
# rotor_drag_coefficient.
DRAG_TILT = {
    "crazyflie_forward": dict(mass_kg=0.028, rotors=4, c_t=1.900523877693446e-08,
                              drag_kg_per_rad=9.1785e-7),
    "firefly_forward": dict(mass_kg=1.545, rotors=6, c_t=8.54858e-06,
                            drag_kg_per_rad=8.06428e-5),
}


def tilt_deg(orientation):
    """Angle between body z and world up."""
    w, x, y, z = orientation
    return math.degrees(math.acos(max(-1.0, min(1.0, 1 - 2 * (x * x + y * y)))))


def commanded(name):
    """The mission's constant command for each entity label, in spawn order."""
    return {
        "forward": (1.0, 0.0, 0.0), "climb": (0.0, 0.0, 0.5),
        "fast": (2.0, 1.0, 0.0), "upset": (0.0, 0.0, 0.0), "cruise": (12.0, 0.0, 0.0),
    }[name.split("_", 1)[1]]


def step_metrics(times, speeds, command):
    """Rise, overshoot, and settling of a scalar response starting at zero."""
    rise_start = next((t for t, v in zip(times, speeds) if v >= 0.1 * command), None)
    rise_end = next((t for t, v in zip(times, speeds) if v >= 0.9 * command), None)
    overshoot = max(speeds) / command - 1.0
    settled = None
    for index in range(len(times)):
        if all(abs(v - command) <= 0.05 * command for v in speeds[index:]):
            settled = times[index]
            break
    return {
        "rise_s": None if rise_start is None or rise_end is None else rise_end - rise_start,
        "overshoot": overshoot,
        "settling_5pct_s": settled,
    }


def run(binary, output, multiplier):
    folder = output / f"substeps-{multiplier}"
    subprocess.run([binary, "run", MISSION, f"run.motion_multiplier:={multiplier}", "--root", ROOT,
                    "--output", folder, "--headless"], check=True, capture_output=True)
    return read_frames(folder / "run.mcap")


def analyze(frames, labels):
    results = {}
    end_s = frames[-1].time_s
    for entity_id, label in zip(sorted(frames[0].entities), labels):
        command = commanded(label)
        times, states = [], []
        for frame in frames:
            if entity_id in frame.entities and (not times or frame.time_s > times[-1]):
                times.append(frame.time_s)
                states.append(frame.entities[entity_id])
        final = [s for t, s in zip(times, states) if t >= end_s - FINAL_WINDOW_S]
        result = {
            "largest_tilt_deg": max(tilt_deg(s.orientation) for s in states),
            "final_tilt_deg": sum(tilt_deg(s.orientation) for s in final) / len(final),
            "final_velocity_error_mps": max(
                math.dist(s.velocity, command) for s in final),
        }
        speed = math.hypot(*command)
        if speed > 0:
            # Rise and overshoot along the command; settling is judged on the
            # whole velocity error, so sideways or vertical drift fails too.
            unit = [c / speed for c in command]
            along = [sum(u * v for u, v in zip(unit, s.velocity)) for s in states]
            result.update(step_metrics(times, along, speed))
            result["settled"] = result["final_velocity_error_mps"] <= 0.05 * speed
        else:
            start = states[0].position
            result["largest_height_loss_m"] = max(start[2] - s.position[2] for s in states)
            result["final_offset_m"] = math.dist(start, states[-1].position)
            result["settled"] = result["final_velocity_error_mps"] <= 0.05
        results[label] = result
    return results


def trajectory_distance(coarse, fine, entity_ids):
    """Largest position difference of the given entities, frames paired by
    index (the final time appears twice: before the last step and as the
    terminal frame)."""
    assert len(coarse) == len(fine)
    largest = 0.0
    for frame, other in zip(coarse, fine):
        assert abs(frame.time_s - other.time_s) < 1e-9
        for entity_id in entity_ids:
            largest = max(largest, math.dist(frame.entities[entity_id].position,
                                             other.entities[entity_id].position))
    return largest


def labels():
    """Entity labels in file order (the mission's spawn and ID order)."""
    names, inside = [], False
    for line in MISSION.read_text().splitlines():
        if line.startswith("entities:"):
            inside = True
        elif inside and line.startswith("  ") and not line.startswith("   ") and line.strip().endswith(":"):
            names.append(line.strip().rstrip(":"))
    return names


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    binary = build_cli("release")
    names = labels()
    runs = {multiplier: run(binary, args.output, multiplier) for multiplier in MULTIPLIERS}
    results = analyze(runs[MULTIPLIERS[0]], names)

    # Rotor drag at 1 m/s, level hover speeds: the tilt that cancels it.
    drag_tilt_deg = {}
    for label, vehicle in DRAG_TILT.items():
        weight_n = vehicle["mass_kg"] * 9.81
        hover_speed_radps = math.sqrt(weight_n / (vehicle["rotors"] * vehicle["c_t"]))
        drag_n = vehicle["drag_kg_per_rad"] * vehicle["rotors"] * hover_speed_radps * 1.0
        drag_tilt_deg[label] = {"predicted": math.degrees(math.atan(drag_n / weight_n)),
                                "measured": results[label]["final_tilt_deg"]}
    step_ids = [entity_id for entity_id, label in zip(sorted(runs[MULTIPLIERS[0]][0].entities), names)
                if not label.endswith("_cruise")]
    convergence = {
        f"substeps-{multiplier}": trajectory_distance(runs[multiplier], runs[MULTIPLIERS[-1]], step_ids)
        for multiplier in MULTIPLIERS[:-1]
    }
    cruise_ids = [entity_id for entity_id, label in zip(sorted(runs[MULTIPLIERS[0]][0].entities), names)
                  if label.endswith("_cruise")]
    cruise_convergence = {
        f"substeps-{multiplier}": trajectory_distance(runs[multiplier], runs[MULTIPLIERS[-1]], cruise_ids)
        for multiplier in MULTIPLIERS[:-1]
    }
    settled = all(result["settled"] for result in results.values())
    passed = (settled
              and all(abs(tilt["measured"] / tilt["predicted"] - 1.0) <= 0.02
                      for tilt in drag_tilt_deg.values())
              and convergence[f"substeps-{MULTIPLIERS[0]}"] <= 0.005)
    report = {
        "passed": passed,
        "mission": str(MISSION.relative_to(ROOT)),
        "source": source_identity(),
        "vehicles": results,
        "drag_tilt_deg": drag_tilt_deg,
        "largest_distance_from_20_substeps_m": convergence,
        "cruise_distance_from_20_substeps_m": cruise_convergence,
    }
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")

    lines = ["# Multirotor velocity control", "",
             f"`{report['mission']}`, 0.01 s ticks, {MULTIPLIERS[0]} substeps. Passed: {passed}.", "",
             "| Vehicle | Rise 10-90% (s) | Overshoot | Settles within 5% (s) | Largest tilt (deg) "
             "| Final tilt (deg) | Final error (m/s) |",
             "| --- | --- | --- | --- | --- | --- | --- |"]

    def number(value, digits=2):
        return "-" if value is None else f"{value:.{digits}f}"

    for label, result in results.items():
        overshoot = result.get("overshoot")
        lines.append(f"| {label} | {number(result.get('rise_s'))} | "
                     f"{'-' if overshoot is None else f'{100 * overshoot:.0f}%'} | "
                     f"{number(result.get('settling_5pct_s'))} | {number(result['largest_tilt_deg'], 1)} | "
                     f"{number(result['final_tilt_deg'], 2)} | {result['final_velocity_error_mps']:.4f} |")
    lines += ["", "Upsets (15 degree roll, hold zero velocity): "
              + "; ".join(f"{label} lost {result['largest_height_loss_m']:.3f} m and ended "
                          f"{result['final_offset_m']:.3f} m from its start"
                          for label, result in results.items() if "largest_height_loss_m" in result) + ".",
              "", "Steady tilt at 1 m/s against the rotor drag's prediction: "
              + "; ".join(f"{label.split('_')[0]} {tilt['measured']:.3f} deg, predicted "
                          f"{tilt['predicted']:.3f} deg" for label, tilt in drag_tilt_deg.items()) + ".", "",
              "Largest position difference from the 20-substep run, step responses: "
              + ", ".join(f"{name} {value * 1000:.3f} mm" for name, value in convergence.items())
              + "; the 12 m/s cruise (reported, not checked): "
              + ", ".join(f"{name} {value * 1000:.3f} mm" for name, value in cruise_convergence.items()) + "."]
    (args.output / "summary.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
