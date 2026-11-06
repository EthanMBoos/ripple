#!/usr/bin/env python3
"""Write the mission for a region of hubs.

    python3 crates/delivery/region.py --hubs 4             # missions/region-4.yaml
    python3 crates/delivery/region.py --hubs 1 --checked   # the Rascal and the Firefly

Each hub gets 3 fixed-wing aircraft and 2 multirotors (`--fixed-wing`,
`--multirotor`), placed with the same grid formula as
crates/delivery/src/region.rs. The committed missions are the small ones;
larger regions are written on demand and run the same way:

    cargo run --release -- run crates/delivery/missions/region-4.yaml --headless
"""

from __future__ import annotations

import argparse
import math
from pathlib import Path

HERE = Path(__file__).resolve().parent
# Few vehicles per hub, so that a surge makes vehicles scarce; the 2026-11-06
# probes found no fleet decision at 10 and 6 (docs/FLEET_MANAGER_PLAN.md,
# "Finding the problem").
FIXED_WING_PER_HUB = 3
MULTIROTOR_PER_HUB = 2

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


def quote(text: str, indent: int) -> str:
    """A folded YAML block scalar."""
    pad = " " * indent
    lines = [f"{pad}{line}" if line else "" for line in text.splitlines()]
    return ">-\n" + "\n".join(lines)


def templates(checked: bool) -> str:
    if checked:
        fixed_wing_models = """\
    controller: {AircraftPIDController: {airframe: rascal}}
    motion_model: {FixedWing6DOF: {airframe: rascal}}"""
        multirotor_models = """\
        # Brake at most 0.3 x 12 = 3.6 m/s^2, inside the controller's limits, so
        # the Firefly stops on a pad instead of overshooting it.
        approach_per_s: 0.3
    # RotorS's Firefly hexacopter under MultirotorVelocityController, with
    # RotorS's motor lag, rotor drag, and rolling moment.
    controller: {MultirotorVelocityController: {airframe: firefly}}
    motion_model:
      Multirotor:
        airframe: firefly
        motor_time_constant_up_s: 0.0125
        motor_time_constant_down_s: 0.025
        rotor_drag_coefficient: 8.06428e-5
        rolling_moment_coefficient: 1.0e-6"""
    else:
        fixed_wing_models = """\
    controller: {SimpleAircraftControllerPID: {}}
    motion_model: {SimpleAircraft: {}}"""
        multirotor_models = """\
    controller: {SingleIntegratorControllerSimple: {}}
    motion_model: {SingleIntegrator: {max_speed_mps: 12}}"""
    return f"""\
templates:
  fixed_wing:
    team: 1
    visual_model: aircraft
    autonomy:
      Courier:
        class: fixed_wing
        hub: 1
        hub_m: [0, 0, 0]
        cruise_speed_mps: 25
        endurance_s: 3600
        cruise_altitude_m: 120
        # Idle aircraft count as recovered on the ground, so their loiter
        # (drawn because SimpleAircraft can't stop) stays inside the hub
        # radius, where flight uses no endurance.
        hub_radius_m: 400
        arrival_radius_m: 75
        loiter_radius_m: 300
{fixed_wing_models}
  multirotor:
    team: 1
    visual_model: sphere
    autonomy:
      Courier:
        class: multirotor
        hub: 1
        hub_m: [0, 0, 0]
        cruise_speed_mps: 12
        endurance_s: 1500
        cruise_altitude_m: 80
        hub_radius_m: 200
        arrival_radius_m: 3
        dwell_s: 30
{multirotor_models}
"""


def dispatcher(hubs: int, spacing_m: float, day_s: float) -> str:
    texts = "\n".join(
        f"            {kind}: {quote(text, 14)}" for kind, text in EVENT_TEXT.items()
    )
    return f"""\
entities:
  region:
    team: 1
    visual_model: sphere
    position_m: [0, 0, 0]
    autonomy:
      Dispatcher:
        region:
          hubs: {hubs}
          hub_spacing_m: {spacing_m:g}
        stock:
          items: ["O-", "O+", "A+", "B+"]
          units_per_item: 100
          restock_interval_s: 3600
          restock_units: 30
          package_units: 4
        requests:
          per_hub_per_hour: 12
          # The day: requests open until then; the run ends 30 min later.
          until_s: {day_s:g}
          urgent_share: 0.3
          trauma_share: 0.1
          routine_deadline_s: 7200
          urgent_deadline_s: 1800
          trauma_deadline_s: 900
        events:
          # Expected events of each kind per hub per 8-hour day, scaled to the
          # day's length; the count is drawn from the seed, so a larger region
          # faces more events.
          per_hub_per_day:
            surge: 0.25
            shortage: 0.25
            closure: 0.25
            outage: 0.1
            weather: 0.5
            priority: 0.5
            rooftop_closed: 0.5
            corridor: 0.5
          # A fixed list instead, when not empty: for example
          # [{{kind: surge, at_min: 30, hubs: [1, 2]}}].
          scripted: []
          surge_requests_per_hub: 16
          surge_window_min: 15
          closure_radius_km: 6
          corridor_length_km: 10
          corridor_half_width_km: 0.5
          # One text per kind, written once. {{hubs}}, {{site}}, {{item}},
          # {{area}}, {{class}}, {{from}} and {{to}} are filled in.
          text:
{texts}
        manager:
          # none (dispatcher only), scripted, replanner, policy, model, or
          # veto (the replanner plans, the model drops what it objects to).
          kind: none
          check_in_s: 1800
          # The model reads events as text, or structured (their fields).
          events_as: text
          # The interface: view full_list, summaries, or queries; feedback
          # reports each order's estimate and what was flown since; checker
          # refuses orders that break a rule or cannot be flown.
          view: summaries
          feedback: false
          checker: false
          # With feedback: each move's donor hub, its idle vehicles now and
          # its late deliveries since, as one fixed line.
          cost_feedback: false
          # Refuse a reposition leaving the donor under half its vehicles of
          # the class, or a transfer leaving it under a quarter of its stock.
          guard: false
          max_queries: 6
          # A call budget: calls at least 3600 / calls_per_hour s apart, events
          # between calls batched; 0 is a call per event.
          calls_per_hour: 0
          # A fleet manager per square cluster of hubs this many a side; 0 is
          # one over the region.
          cluster_side: 0
          # Hand-written orders for kind scripted: [{{at_min: 30, order:
          # {{reposition: {{class: fixed_wing, count: 4, from_hub: 2, to_hub: 1}}}}}}].
          scripted: []
          # The rules file for kind policy, written by hand or by a model once
          # (src/policy.rs); a path from the working directory.
          policy: ""
          # The manager above the clusters (needs cluster_side): none,
          # replanner, or model; it sees cluster totals and moves vehicles
          # and stock between clusters. top_model empty means model.
          top_kind: none
          top_model: ""
          # Whose model and how it is reached (src/client.rs): provider
          # claude, openai, or ollama; via cli or api. Replies are kept in
          # cache_dir; offline: true uses only replies already there.
          provider: claude
          via: cli
          model: claude-haiku-5-5
          sample: 0
          prompt:
            system: {quote(SYSTEM_PROMPT, 14)}
          cache_dir: runs/llm-cache
          offline: false
          command: null
          host: null
        range_margin: 0.9
        dwell_s: 30
        confirm_timeout_s: 5
        decision_interval_s: 5
        retry_interval_s: 60
        detour_margin_m: 500
    controller: {{SingleIntegratorControllerSimple: {{}}}}
    motion_model: {{SingleIntegrator: {{}}}}
"""


def couriers(hubs: int, spacing_m: float) -> str:
    side = round(math.sqrt(hubs))
    assert side * side == hubs, "hubs must be a square number"
    groups = []
    for hub in range(1, hubs + 1):
        x, y = hub_position_m(hub, side, spacing_m)
        groups.append(
            f"""\
  hub{hub}_fixed_wing:
    template: fixed_wing
    count: {FIXED_WING_PER_HUB}
    position_m: [{x:g}, {y:g}, 120]
    position_stddev_m: [60, 60, 0]
    heading_deg: 90
    velocity_mps: [0, 25, 0]
    autonomy:
      Courier:
        class: fixed_wing
        hub: {hub}
        hub_m: [{x:g}, {y:g}, 0]
        cruise_speed_mps: 25
        endurance_s: 3600
        cruise_altitude_m: 120
        hub_radius_m: 400
        arrival_radius_m: 75
        loiter_radius_m: 300
  hub{hub}_multirotor:
    template: multirotor
    count: {MULTIROTOR_PER_HUB}
    position_m: [{x:g}, {y:g}, 0]
    position_stddev_m: [3, 3, 0]
    autonomy:
      Courier:
        class: multirotor
        hub: {hub}
        hub_m: [{x:g}, {y:g}, 0]
        cruise_speed_mps: 12
        endurance_s: 1500
        cruise_altitude_m: 80
        hub_radius_m: 200
        arrival_radius_m: 3
        dwell_s: 30
"""
        )
    return "".join(groups)


def mission(hubs: int, spacing_m: float, day_s: float, checked: bool, seed: int) -> str:
    vehicles = hubs * (FIXED_WING_PER_HUB + MULTIROTOR_PER_HUB)
    models = (
        "the Rascal (FixedWing6DOF) and the Firefly (Multirotor) at 500 Hz"
        if checked
        else "SimpleAircraft and point multirotors"
    )
    motion = "  motion_multiplier: 50\n" if checked else ""
    name = f"region-{hubs}" + ("-checked" if checked else "")
    return f"""\
# One day of blood delivery across {hubs} hub{'s' if hubs > 1 else ''} ({vehicles} vehicles) on
# {models}. Written by crates/delivery/region.py; edit that
# script, not this file. The Dispatcher on the region entity holds the hubs,
# stock, requests, events, and the fleet manager; see crates/delivery/README.md
# and docs/FLEET_MANAGER_PLAN.md.
#
# A group's Courier block repeats the template's vehicle values because a
# group's autonomy replaces the template's whole slot.
format_version: 1
name: {name}

run:
  end_s: {day_s + 1800:g}
  dt_s: 0.1
{motion}  seed: {seed}

networks: {{GlobalNetwork: {{}}}}
metrics: {{DeliveryScore: {{}}}}

{templates(checked)}
{dispatcher(hubs, spacing_m, day_s)}{couriers(hubs, spacing_m)}"""


def main() -> None:
    global FIXED_WING_PER_HUB, MULTIROTOR_PER_HUB
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--hubs", type=int, default=1, help="a square number: 1, 4, 16, 64, 256")
    parser.add_argument("--spacing-km", type=float, default=15.0)
    parser.add_argument("--day-h", type=float, default=8.0, help="hours of requests")
    parser.add_argument("--checked", action="store_true", help="the Rascal and the Firefly")
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--fixed-wing", type=int, default=FIXED_WING_PER_HUB, help="aircraft per hub")
    parser.add_argument("--multirotor", type=int, default=MULTIROTOR_PER_HUB, help="multirotors per hub")
    parser.add_argument("--out", type=Path, help="default: crates/delivery/missions/region-<hubs>.yaml")
    args = parser.parse_args()
    FIXED_WING_PER_HUB, MULTIROTOR_PER_HUB = args.fixed_wing, args.multirotor
    text = mission(args.hubs, args.spacing_km * 1000, args.day_h * 3600, args.checked, args.seed)
    out = args.out or HERE / "missions" / (
        f"region-{args.hubs}"
        + ("-checked" if args.checked else "")
        + (f"-fw{args.fixed_wing}-mr{args.multirotor}" if (args.fixed_wing, args.multirotor) != (3, 2) else "")
        + ".yaml"
    )
    out.write_text(text)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
