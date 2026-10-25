#!/usr/bin/env python3
"""Draw the title figure's map from one traced delivery run.

    python3 crates/delivery/mission_hero.py RUN_FOLDER 11.7 > paper/manuscript/mission-hero.tex

RUN_FOLDER is a run made with `ripple run ... --trace` (see trace.py). The
output is TikZ commands for the map in Fig. 1 of main.tex. The vehicle on a
call that breaks the instruction is red.
"""

import math
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "reference"))
from run import read_run  # noqa: E402

NEAR_DEPOT_KM = 0.6  # vehicles closer than this are parked or loitering
HERO_X_KM, HERO_Y_KM = (-7.4, 10.8), (-4.6, 10.6)


def plane(x, y, heading, color, size=1.0):
    """A fixed-wing aircraft: an arrowhead at (x, y) km pointing along heading."""
    a, b, c = 0.7 * size, 0.5 * size, 0.45 * size
    return (f"\\fill[{color}, rotate around={{{heading:.1f}:({x:.2f},{y:.2f})}}] "
            f"({x + a:.2f},{y:.2f}) -- ({x - b:.2f},{y + c:.2f}) -- "
            f"({x - b / 2:.2f},{y:.2f}) -- ({x - b:.2f},{y - c:.2f}) -- cycle;")


def km(point):
    return point[0] / 1000.0, point[1] / 1000.0


def load(folder):
    """What the frames need from a traced run."""
    run = read_run(folder)
    (dispatcher,) = [a["params"] for e in run.scenario["entities"] for a in e["autonomy"]
                     if a["name"] == "Dispatcher"]
    received = [t for t in run.traces if t.get("kind") == "delivery"
                and t["receiver"]["plugin"].startswith("metrics/")]
    decisions = [t["payload"] for t in received if t["topic"] == "DispatchDecision"]
    # Calls that break the instruction are highlighted in the map.
    breaking = [(d["time_s"], c) for d in decisions for c in d["calls"]
                if c["breaks_instruction"]]
    return {
        "run": run,
        "dispatcher": dispatcher,
        "directive": dispatcher["directive"],
        "decisions": decisions,
        "requests": {t["payload"]["id"]: t["payload"] for t in received
                     if t["topic"] == "DeliveryRequest"},
        "breaking": breaking,
        "fixed_wing": {i for i, e in run.frames[0].entities.items() if e.kind == 0},
        "vehicles": sorted(i for i in run.frames[0].entities if i != 1),
        "step_s": run.frames[1].time_s - run.frames[0].time_s,
    }


def frame(ctx, minute, x_km, y_km, size=1.0):
    """Map one minute of routes, vehicles, sites, and the no-fly zone."""
    run, directive, decisions = ctx["run"], ctx["directive"], ctx["decisions"]
    t = minute * 60
    frame_at = run.frames[round(t / ctx["step_s"])]
    cx, cy = km(directive["center_m"])
    radius = directive["radius_m"] / 1000.0

    def inside(x, y):
        return x_km[0] <= x <= x_km[1] and y_km[0] <= y <= y_km[1]

    out = [f"\\draw[black!25] ({x_km[0]},{y_km[0]}) rectangle ({x_km[1]},{y_km[1]});",
           "\\begin{scope}",
           f"\\clip ({x_km[0]},{y_km[0]}) rectangle ({x_km[1]},{y_km[1]});"]
    # Every route from the depot, faint, and the zone once announced.
    for site in ctx["dispatcher"]["sites"]:
        x, y = km(site["position_m"])
        if inside(x, y) and not site.get("rooftop"):
            out.append(f"\\draw[black!20, dashed, line width=0.4pt] (0,0) -- ({x},{y});")
    if t >= directive["announce_s"]:
        out.append(f"\\fill[accent!20, draw=accent] ({cx},{cy}) circle ({radius});")
        out.append(f"\\node[accent, anchor=east, font=\\tiny, inner sep=2pt] at "
                   f"({cx - radius},{cy}) {{no-fly zone}};")

    # Each vehicle out flying: the route of its current call, solid, and the
    # vehicle where it is. Those at the depot are left out.
    for vehicle in ctx["vehicles"]:
        entity = frame_at.entities[vehicle]
        x, y = km(entity.position)
        if math.hypot(x, y) < NEAR_DEPOT_KM or not inside(x, y):
            continue
        calls = [c for d in decisions if d["time_s"] <= t for c in d["calls"]
                 if c["vehicle_id"] == vehicle and c["outcome"] == "assigned"]
        broke = calls[-1]["breaks_instruction"]
        color = ("accent" if broke
                 else "classc" if vehicle in ctx["fixed_wing"] else "learnc")
        sx, sy = km(ctx["requests"][calls[-1]["request_id"]]["site_world_m"])
        out.append(f"\\draw[{color}, line width={1.0 if broke else 0.7}pt] "
                   f"(0,0) -- ({sx},{sy});")
        if vehicle in ctx["fixed_wing"]:
            heading = math.degrees(math.atan2(entity.velocity[1], entity.velocity[0]))
            out.append(plane(x, y, heading, color, size))
        else:
            out.append(f"\\fill[{color}] ({x:.2f},{y:.2f}) circle ({0.35 * size});")

    # Sites and the depot on top, every one named.
    for site in ctx["dispatcher"]["sites"]:
        x, y = km(site["position_m"])
        if not inside(x, y):
            continue
        if site.get("rooftop"):
            out.append(f"\\draw[black!50, fill=white] ({x - 0.3},{y - 0.3}) "
                       f"rectangle ({x + 0.3},{y + 0.3});")
            continue
        label = site["name"]
        out.append(f"\\fill[black!70] ({x},{y}) circle (1.4pt);")
        side = "east" if x + 0.5 * len(label) > x_km[1] else "west"  # stay inside
        out.append(f"\\node[black!80, anchor={side}, inner sep=2pt] at ({x},{y}) "
                   f"{{{label}}};")
    out.append("\\fill[black] (-0.35,-0.35) rectangle (0.35,0.35);")
    out.append("\\node[anchor=west, inner sep=3pt] at (0.4,0.2) {depot};")
    out.append("\\end{scope}")
    return out


def hero(ctx, minute):
    """One frame for Fig. 1, with the breaking call's deadline slack."""
    out = frame(ctx, minute, HERO_X_KM, HERO_Y_KM, size=0.85)
    frame_at = ctx["run"].frames[round(minute * 60 / ctx["step_s"])]
    for sent_s, call in ctx["breaking"]:
        if sent_s <= minute * 60:
            x, y = km(frame_at.entities[call["vehicle_id"]].position)
            out.append(f"\\node[accent, anchor=west, font=\\scriptsize\\bfseries, "
                       f"inner sep=4pt] at ({x + 0.6:.2f},{y:.2f}) "
                       f"{{urgent, {call['slack_s'] / 60:.1f} min to spare}};")
    return out


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    lines = hero(load(sys.argv[1]), float(sys.argv[2]))
    print("% Generated by crates/delivery/mission_hero.py; do not edit by hand.")
    print("\n".join(lines))
