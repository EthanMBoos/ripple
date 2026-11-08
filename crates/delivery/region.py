#!/usr/bin/env python3
"""Write matched jitter sweeps for the region mission.

    python3 crates/delivery/region.py --hubs 16 --classical-sweeps FOLDER
    python3 crates/delivery/region.py --hubs 16 --jitter-km 3 --classical-sweeps FOLDER

The mission itself is missions/region.yaml, written by hand: the Dispatcher
places the hubs on a grid and builds each hub's couriers, so the region's
size is one value. This script only writes sweep files for it: five matched
days (one demand seed each) with the six classical policy arms, on the grid
or with every hub displaced (`--jitter-km`, `--min-spacing-km`,
`--layout-seed`), the displaced layout written as `region.hub_positions_m`.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent

# The texts missions/region.yaml carries, for write_policy.py, which asks a
# model to write rules from them; keep the two the same.
SYSTEM_PROMPT = """\
You are the fleet manager for a regional blood delivery service. A classical
dispatcher assigns each request to the idle vehicle at the site's own hub that
has the stock and the range and arrives soonest; it does not read events.
Fixed-wing aircraft drop by parachute and cannot land; multirotors can also
land on hospital rooftops, which take only a landing, and have a short range.
Each hub holds stock of each blood type. Sites are named by hub number and
letter: 3A to 3F are hub 3's clinics, 3H1 and 3H2 its hospitals. You never
assign single requests: you
give orders that change what the dispatcher may do, where vehicles and stock
are, and what comes first. Keep trauma and urgent deliveries on time, keep
flights out of closed areas, and do not send vehicles beyond their endurance.
Reply with JSON only, no other text."""

EVENT_TEXT = {
    "surge": (
        "A mass-casualty incident near {hubs}: the hospitals there are sending "
        "trauma requests beyond what those hubs can fly alone, starting at "
        "minute {from}."
    ),
    "shortage": (
        "{item} has run out at {hubs} as of minute {from}. Requests for {item} "
        "at their sites can only be served from hubs that still hold it."
    ),
    "closure": (
        "Airspace is closed in {area} from minute {from} until minute {to}; "
        "no flights through it."
    ),
    "outage": (
        "{hubs} cannot launch from minute {from} until minute {to}; its sites "
        "must be covered by other hubs."
    ),
    "weather": (
        "Weather at {hubs}: no {class} launches from minute {from} until "
        "minute {to}."
    ),
    "priority": (
        "Hospital {site} takes priority over routine deliveries from minute "
        "{from} until {to}."
    ),
    "rooftop_closed": (
        "The rooftop at hospital {site} is closed from minute {from} until "
        "minute {to}; only drops at its ground pad are allowed."
    ),
    "corridor": (
        "A medevac helicopter corridor is active in {area} from minute {from} "
        "until minute {to}; no vehicle may cross it below 150 m."
    ),
}


def hub_position_m(hub: int, side: int, spacing_m: float) -> tuple[float, float]:
    """Hub `hub` (from 1) on a side x side grid centered on the origin."""
    index = hub - 1
    row, column = divmod(index, side)
    offset = (side - 1) / 2
    return ((column - offset) * spacing_m, (row - offset) * spacing_m)


def jittered_positions(hubs: int, spacing_m: float, radius_m: float,
                      min_spacing_m: float, seed: int) -> list[tuple[float, float]]:
    """Displace each grid point uniformly in a disk, rejecting close pairs.

    Each draw is keyed by layout seed, hub, attempt, and purpose. It never
    consumes the simulator's request/event random numbers.
    """
    side = math.isqrt(hubs)
    if hubs < 1 or side * side != hubs:
        raise ValueError("hubs must be a positive square number")
    if not all(math.isfinite(v) for v in (spacing_m, radius_m, min_spacing_m)):
        raise ValueError("spacing and jitter must be finite")
    if spacing_m <= 0 or radius_m < 0 or min_spacing_m <= 0:
        raise ValueError("spacing must be positive and jitter nonnegative")
    positions = []
    for hub in range(1, hubs + 1):
        x, y = hub_position_m(hub, side, spacing_m)
        for attempt in range(10_000):
            def uniform(purpose: str) -> float:
                key = f"ripple-layout:{seed}:{hub}:{attempt}:{purpose}".encode()
                bits = int.from_bytes(hashlib.sha256(key).digest()[:8], "big") >> 11
                return (bits + 0.5) / 2**53

            radius = radius_m * math.sqrt(uniform("radius"))
            angle = math.tau * uniform("angle")
            point = (x + radius * math.cos(angle), y + radius * math.sin(angle))
            if all(math.dist(point, other) >= min_spacing_m for other in positions):
                positions.append(point)
                break
        else:
            raise ValueError(f"cannot place hub {hub}; reduce minimum spacing or jitter")
    return positions


def write_classical_sweeps(args) -> None:
    """Matched days, each with one layout and six existing policy arms."""
    folder = args.classical_sweeps
    folder.mkdir(parents=True, exist_ok=True)
    layout = "jitter" if args.jitter_km else "grid"
    day_s = args.day_h * 3600
    for seed in args.seeds:
        layout_seed = args.layout_seed + seed - 1
        name = f"region-{args.hubs}-{layout}-seed{seed}"
        sweep = folder / f"{name}.sweep.yaml"
        if sweep.exists():
            raise FileExistsError(f"{sweep} already exists; use a new directory")
        parameters = [
            f"  run.end_s: {day_s + 1800:g}",
            f"  entities.region.autonomy.Dispatcher.requests.until_s: {day_s:g}",
            f"  entities.region.autonomy.Dispatcher.region.hubs: {args.hubs}",
            f"  entities.region.autonomy.Dispatcher.region.hub_spacing_m: {args.spacing_km * 1000:g}",
            "  entities.region.autonomy.Dispatcher.manager.offline: true",
        ]
        header = f"# Six classical arms on the {args.hubs}-hub grid, demand seed {seed}; no model calls.\n"
        if args.jitter_km:
            positions = jittered_positions(args.hubs, args.spacing_km * 1000,
                                          args.jitter_km * 1000, args.min_spacing_km * 1000,
                                          layout_seed)
            header = (f"# Six classical arms on one displaced layout and demand seed {seed}; no model\n"
                      f"# calls. Layout seed {layout_seed}; jitter radius {args.jitter_km:g} km; "
                      f"minimum hub spacing {args.min_spacing_km:g} km.\n")
            # One value that is a list: a bare list would be read as alternatives.
            parameters.append(
                "  entities.region.autonomy.Dispatcher.region.hub_positions_m:\n"
                f"    - {json.dumps([list(p) for p in positions])}")
        sweep.write_text(header + f"""\
name: {name}
base_scenario: {os.path.relpath(HERE / "missions" / "region.yaml", folder)}
seeds: [{seed}]
parameters:
""" + "\n".join(parameters) + """
prefix: entities.region.autonomy.Dispatcher
columns: [manager.kind, manager.policy]
cases:
  - [none, ""]
  - [replanner, ""]
  - [policy, crates/delivery/policies/local.yaml]
  - [policy, crates/delivery/policies/serve.yaml]
  - [policy, crates/delivery/policies/move.yaml]
  - [policy, crates/delivery/policies/return.yaml]
""")
        print(f"wrote {sweep} (6 episodes)")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--hubs", type=int, default=16, help="a square number: 1, 4, 16, 64, 256, 1024")
    parser.add_argument("--spacing-km", type=float, default=15.0)
    parser.add_argument("--day-h", type=float, default=8.0, help="hours of requests")
    parser.add_argument("--jitter-km", type=float, default=0, help="maximum hub displacement")
    parser.add_argument("--min-spacing-km", type=float, default=10)
    parser.add_argument("--layout-seed", type=int, default=1, help="independent of demand seed")
    parser.add_argument("--classical-sweeps", type=Path, required=True,
                        help="write six-policy sweeps to this folder")
    parser.add_argument("--seeds", type=int, nargs="+", default=list(range(1, 6)),
                        help="demand seeds for --classical-sweeps (default: 1 2 3 4 5)")
    args = parser.parse_args()
    write_classical_sweeps(args)


if __name__ == "__main__":
    main()
