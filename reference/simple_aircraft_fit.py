#!/usr/bin/env python3
"""Fit SimpleAircraft's `rascal` limits to FixedWing6DOF's Rascal, and check them.

    python3 reference/simple_aircraft_fit.py --output runs/simple-aircraft-fit000

The two fixed-wing rungs should share their performance limits: speeds,
acceleration, climb, descent, and turn rate over these maneuvers. (They fly
under different controllers, so this is not all that separates them.) This
flies FixedWing6DOF's Rascal under AircraftPIDController (both `airframe:
rascal`, 0.01 s steps) through maneuvers that each reach one limit, and
measures:

- max speed: the steady level speed at full throttle (60 m/s asked);
- min speed: the slowest steady speed that still holds altitude (sink under
  0.05 m/s), over speeds asked from 7 to 14 m/s;
- acceleration and deceleration: the largest change of speed over 1 s while
  speeding up toward 60 m/s and slowing toward 5 m/s, from 20 m/s;
- climb and descent: the steady climb toward a waypoint 2.7 km above, and the
  steady descent toward one 2.7 km below, at 20 m/s asked;
- turn: the turn rate in a 180 degree turn while the bank is held (within half
  a degree of the largest bank held for 1 s), and the bank that gives that
  rate in a coordinated turn, atan(rate x speed / g). The Rascal turns about
  13% slower than a coordinated turn at its own bank; SimpleAircraft's bank
  limit is this effective bank, since SimpleAircraft's bank only sets its
  turn rate;
- roll and pitch rates: the largest change of each over 0.1 s in those runs.

The bank and pitch the Rascal holds are reported, not compared: SimpleAircraft
keeps its speed in a climb or dive (energy is not modeled), so its pitch limit
(30 degrees) only has to let the climb and descent limits bind at cruise.

It then flies SimpleAircraft with `airframe: rascal` under
SimpleAircraftControllerPID through the same maneuvers, measures the same
numbers. Passes when every
SimpleAircraft number is within 5% (or 0.2 in its unit, for small numbers) of
the Rascal's. Writes OUTPUT/results.json and OUTPUT/summary.md. Standard library
only; needs Cargo.
"""
import argparse
import json
import math
from pathlib import Path
import subprocess
import sys

from frames import read_frames
from project import ROOT, build_cli, source_identity

ALTITUDE_M = 300.0
MIN_SPEED_ASKED_MPS = [7, 8, 9, 10, 11, 12, 13, 14]
# name: (start altitude, waypoint relative to the start, speed asked, end time)
MANEUVERS = {
    "full throttle": (ALTITUDE_M, [20000, 0, 0], 60, 60),
    "idle": (ALTITUDE_M, [20000, 0, 0], 5, 60),
    "climb": (ALTITUDE_M, [20000, 0, 2700], 20, 60),
    "descent": (ALTITUDE_M + 2700, [20000, 0, -2700], 20, 60),
    "turn": (ALTITUDE_M, [-20000, 1, 0], 20, 60),
    **{f"slow {speed}": (ALTITUDE_M, [20000, 0, 0], speed, 120)
       for speed in MIN_SPEED_ASKED_MPS},
}
TOLERANCE = 0.05
SMALL = 0.2
GRAVITY_MPS2 = 9.81  # both models'
REPORTED = ("held_bank_deg", "held_climb_pitch_deg")


def vehicle(rung):
    if rung == "FixedWing6DOF":
        return ("    controller:\n      AircraftPIDController:\n        airframe: rascal\n"
                "    motion_model:\n      FixedWing6DOF:\n        airframe: rascal\n")
    return ("    controller:\n      SimpleAircraftControllerPID:\n"
            "    motion_model:\n      SimpleAircraft:\n        airframe: rascal\n")


def mission(rung):
    end_s = max(end for *_, end in MANEUVERS.values())
    lines = [f"format_version: 1\nname: simple-aircraft-fit\nrun:\n  end_s: {end_s}\n"
             "  dt_s: 0.01\n  seed: 1\nnetworks:\n  LocalNetwork:\n  GlobalNetwork:\nentities:"]
    for index, (name, (altitude, waypoint, speed, _)) in enumerate(MANEUVERS.items()):
        y = 1000 * index
        target = [waypoint[0], waypoint[1] + y, altitude + waypoint[2]]
        lines.append(f"  e{index:02}:  # {name}\n    position_m: [0, {y}, {altitude}]\n"
                     "    velocity_mps: [20, 0, 0]\n    heading_deg: 0\n"
                     f"    autonomy:\n      WaypointFollower:\n        waypoints_m: [{target}]\n"
                     f"        arrival_radius_m: 40\n        speed_mps: {speed}\n" + vehicle(rung))
    return "\n".join(lines) + "\n"


def roll_pitch_yaw_deg(q):
    w, x, y, z = q
    return (math.degrees(math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y))),
            math.degrees(math.asin(max(-1.0, min(1.0, 2 * (w * y - z * x))))),
            math.degrees(math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z))))


def fly(binary, output, rung):
    path = output / f"{rung}.yaml"
    path.write_text(mission(rung))
    subprocess.run([binary, "run", path, "--root", ROOT, "--output", output / rung, "--no-rerun"],
                   check=True, capture_output=True)
    frames = read_frames(output / rung / "frames.bin")
    tracks = {}
    for index, (name, (*_, end_s)) in enumerate(MANEUVERS.items(), start=1):
        tracks[name] = [(f.time_s, f.entities[index]) for f in frames
                        if index in f.entities and f.time_s <= end_s + 1e-9]
    return tracks


def speed(e):
    return math.sqrt(sum(v * v for v in e.velocity))


def mean(values):
    values = list(values)
    return sum(values) / len(values)


def last(track, seconds):
    end = track[-1][0]
    return [e for t, e in track if t >= end - seconds]


def largest_change(track, value, window_s):
    """Largest |change of value| per second over window_s."""
    steps = round(window_s / (track[1][0] - track[0][0]))
    return max(abs(value(track[k + steps][1]) - value(track[k][1])) / window_s
               for k in range(len(track) - steps))


def held(track, value, window_s):
    """The largest |value| held for window_s: the largest of each window's smallest."""
    steps = round(window_s / (track[1][0] - track[0][0]))
    values = [abs(value(e)) for _, e in track]
    return max(min(values[k:k + steps + 1]) for k in range(len(values) - steps))


def measure(tracks):
    roll = lambda e: roll_pitch_yaw_deg(e.orientation)[0]  # noqa: E731
    pitch = lambda e: roll_pitch_yaw_deg(e.orientation)[1]  # noqa: E731
    level = []
    for asked in MIN_SPEED_ASKED_MPS:
        steady = last(tracks[f"slow {asked}"], 20)
        sink_mps = -mean(e.velocity[2] for e in steady)
        level.append(dict(asked_mps=asked, speed_mps=mean(speed(e) for e in steady),
                          sink_mps=sink_mps, holds_altitude=abs(sink_mps) < 0.05))
    holding = [row["speed_mps"] for row in level if row["holds_altitude"]]
    turn = tracks["turn"]
    # The steady part of the turn: banked within half a degree of the held bank.
    bank_deg = held(turn, roll, 1.0)
    banked = [(t, e) for t, e in turn if abs(roll(e)) > bank_deg - 0.5]
    heading_rate_degps = effective_bank_deg = None
    if len(banked) > 10:
        (t0, e0), (t1, e1) = banked[0], banked[-1]
        turned = (roll_pitch_yaw_deg(e1.orientation)[2] - roll_pitch_yaw_deg(e0.orientation)[2]
                  + 180) % 360 - 180
        heading_rate_degps = abs(turned) / (t1 - t0)
        banked_speed_mps = mean(speed(e) for _, e in banked)
        effective_bank_deg = math.degrees(
            math.atan(math.radians(heading_rate_degps) * banked_speed_mps / GRAVITY_MPS2))
    return dict(
        min_speed_mps=min(holding) if holding else None,
        max_speed_mps=mean(speed(e) for e in last(tracks["full throttle"], 5)),
        max_acceleration_mps2=largest_change(tracks["full throttle"][:1000], speed, 1.0),
        max_deceleration_mps2=largest_change(tracks["idle"][:1000], speed, 1.0),
        max_climb_rate_mps=mean(e.velocity[2] for e in last(tracks["climb"], 20)),
        max_descent_rate_mps=-mean(e.velocity[2] for e in last(tracks["descent"], 20)),
        turn_rate_degps=heading_rate_degps,
        effective_bank_deg=effective_bank_deg,
        max_roll_rate_degps=largest_change(turn, roll, 0.1),
        max_pitch_rate_degps=max(largest_change(tracks[name], pitch, 0.1)
                                 for name in ("climb", "descent")),
        held_bank_deg=bank_deg,
        held_climb_pitch_deg=held(tracks["climb"], pitch, 1.0),
        level_flight=level)


def agrees(simple, rascal):
    if simple is None or rascal is None:
        return False
    return abs(simple - rascal) <= max(TOLERANCE * abs(rascal), SMALL)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    binary = build_cli("release")
    rascal = measure(fly(binary, args.output, "FixedWing6DOF"))
    simple = measure(fly(binary, args.output, "SimpleAircraft"))
    keys = [key for key in rascal if key != "level_flight"]
    rows = [dict(limit=key, rascal=rascal[key], simple_aircraft=simple[key],
                 compared=key not in REPORTED,
                 agrees=agrees(simple[key], rascal[key])) for key in keys]
    passed = all(row["agrees"] for row in rows if row["compared"])
    report = dict(passed=passed, source=source_identity(), tolerance=TOLERANCE, small=SMALL,
                  limits=rows, rascal_level_flight=rascal["level_flight"],
                  simple_aircraft_level_flight=simple["level_flight"])
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")

    def number(value):
        return "-" if value is None else f"{value:.2f}"

    lines = ["# SimpleAircraft's `rascal` limits", "",
             "FixedWing6DOF's Rascal under AircraftPIDController, and SimpleAircraft with "
             f"`airframe: rascal` under SimpleAircraftControllerPID. Passed: {passed}.", "",
             "| Limit | Rascal (FixedWing6DOF) | SimpleAircraft | Agrees |",
             "| --- | --- | --- | --- |"]
    lines += [f"| {row['limit']} | {number(row['rascal'])} | {number(row['simple_aircraft'])} | "
              f"{('yes' if row['agrees'] else 'no') if row['compared'] else 'reported'} |"
              for row in rows]
    lines += ["", "Level flight at each speed asked (last 20 s of 120 s):", "",
              "| Asked (m/s) | Rascal speed (m/s) | Rascal sink (m/s) |", "| --- | --- | --- |"]
    lines += [f"| {row['asked_mps']} | {row['speed_mps']:.2f} | {row['sink_mps']:.3f} |"
              for row in rascal["level_flight"]]
    (args.output / "summary.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
