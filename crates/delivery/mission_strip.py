#!/usr/bin/env python3
"""Draw one traced delivery run for the paper as a strip of moments: the
same map at a few times, with the planner's call quoted above each.

    python3 crates/delivery/mission_strip.py RUN_FOLDER > paper/manuscript/mission-strip.tex
    python3 crates/delivery/mission_strip.py RUN_FOLDER --hero 12.3 > paper/manuscript/mission-hero.tex

RUN_FOLDER is a run made with `ripple run ... --trace` (see trace.py). Each
frame shows the routes from the depot to every clinic, faint, and each
vehicle out flying on the route of its current call, solid, with the vehicle
where it is: a fixed-wing aircraft as an arrowhead pointing where it flies,
a multirotor as a dot. The vehicle sent on a call that breaks the
instruction is red. A clinic behind the zone is tagged, with a countdown,
while a request that a breaking call later served waits for it. The output
is a TikZ picture for main.tex to \\input; with --hero, one large frame at
the given minute, as commands for a scope in main.tex (Fig. 1).
"""

import math
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "reference"))
from run import read_run  # noqa: E402

# Each frame: the minute it shows, who is speaking, and the words quoted
# above it. The minutes and calls come from the run; the wording is chosen
# by hand.
PANELS = [
    (3.0, "operations", "no vehicle may fly within 0.6 km"),
    (8.5, "planner", "drop at D"),
    (11.7, "planner", "drop at E"),
    (17.0, "planner", "land at roof R1"),
]
NEAR_DEPOT_KM = 0.6  # vehicles closer than this are parked or loitering
X_KM, Y_KM = (-5.6, 8.3), (-4.6, 10.6)  # the patch of map shown, km
PANEL_W, GAP = 4.2, 0.3  # cm
# The single frame of Fig. 1: the patch of map around the zone and clinic E.
HERO_X_KM, HERO_Y_KM = (-7.4, 10.8), (-4.6, 10.6)
BEHIND_ZONE = ("clinic B", "clinic E")


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
    # The calls that break the instruction, and the requests they served: the
    # tag of a served request counts down to its deadline.
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
        "focus": {c["request_id"] for _, c in breaking},
        "fixed_wing": {i for i, e in run.frames[0].entities.items() if e.kind == 0},
        "vehicles": sorted(i for i in run.frames[0].entities if i != 1),
        "step_s": run.frames[1].time_s - run.frames[0].time_s,
    }


def frame(ctx, minute, x_km, y_km, size=1.0, tags=True):
    """The map at one minute, in km, clipped to x_km by y_km: routes, the zone,
    the vehicles out flying, and every site named. size scales the symbols;
    tags marks clinics with a request waiting."""
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

    # Which clinics behind the zone have a request waiting at this moment.
    last = [d for d in decisions if d["time_s"] <= t]
    waiting = {}  # site -> its tag
    if last:
        served = {c["request_id"] for c in last[-1]["calls"] if c["outcome"] == "assigned"}
        for w in last[-1]["waiting"]:
            if w["request_id"] in served or w["site"] not in BEHIND_ZONE:
                continue
            # Only the requests a breaking call served, so a tag follows one
            # delivery from its arrival to its call.
            if w["request_id"] in ctx["focus"]:
                left = (w["deadline_s"] - t) / 60
                waiting[w["site"]] = f"urgent, due in {left:.0f} min"

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
        if tags and site["name"] in waiting:
            out.append(f"\\node[anchor=north, draw=black!40, fill=white, rounded corners=1pt, "
                       f"font=\\tiny, text=black!70, inner sep=1.2pt] at ({x + 1.5},{y - 0.5}) "
                       f"{{{waiting[site['name']]}}};")
    out.append("\\fill[black] (-0.35,-0.35) rectangle (0.35,0.35);")
    out.append("\\node[anchor=west, inner sep=3pt] at (0.4,0.2) {depot};")
    out.append("\\end{scope}")
    return out


def legend(x, y):
    """Fixed-wing and multirotor symbols, at (x, y) km."""
    return [plane(x, y, 0, "classc"),
            f"\\node[anchor=west, font=\\tiny, inner sep=3pt] at ({x + 0.8},{y}) {{fixed-wing}};",
            f"\\fill[learnc] ({x},{y - 1.1}) circle (0.35);",
            f"\\node[anchor=west, font=\\tiny, inner sep=3pt] at ({x + 0.8},{y - 1.1}) "
            f"{{multirotor}};"]


def strip(ctx):
    """The frames of PANELS side by side, each with its quote above."""
    scale = PANEL_W / (X_KM[1] - X_KM[0])
    out = ["\\begin{tikzpicture}[font=\\scriptsize]"]
    for index, (minute, speaker, quote) in enumerate(PANELS):
        t = minute * 60
        shift = index * (PANEL_W + GAP) - X_KM[0] * scale
        out.append(f"\\begin{{scope}}[shift={{({shift:.3f},{-Y_KM[0] * scale:.3f})}}, "
                   f"x={scale:.4f}cm, y={scale:.4f}cm]")
        # The first frame after a breaking call says when it was made, with
        # the slack the dispatcher estimated for its delivery.
        before = PANELS[index - 1][0] * 60 if index else 0
        for sent_s, call in ctx["breaking"]:
            if before < sent_s <= t:
                speaker += (f" at minute {sent_s / 60:.1f}, "
                            f"{call['slack_s'] / 60:.1f} min to spare")
        out.append(f"\\node[anchor=south, font=\\ttfamily\\scriptsize, inner sep=3pt] at "
                   f"({(X_KM[0] + X_KM[1]) / 2},{Y_KM[1]}) (quote) {{``{quote}''}};")
        out.append(f"\\node[anchor=south, font=\\tiny, black!50, inner sep=0pt] at "
                   f"(quote.north) {{{speaker}}};")
        out.append(f"\\node[anchor=south west, font=\\tiny, black!55, inner sep=3pt] at "
                   f"({X_KM[0]},{Y_KM[0]}) {{minute {minute:g}}};")
        out += frame(ctx, minute, X_KM, Y_KM)
        if index == 0:
            out.append("\\node[anchor=north, black!60, inner sep=3pt] at (-0.45,-1.4) "
                       "{hospital roofs};")
            out += legend(3.0, 9.7)
        out.append("\\end{scope}")
    out.append("\\end{tikzpicture}")
    return out


def hero(ctx, minute):
    """One large frame for Fig. 1, as commands for a scope in km whose origin
    is the depot; main.tex sets the scale and position. The vehicle on a
    breaking call is labeled with the slack its delivery had when it was sent."""
    out = frame(ctx, minute, HERO_X_KM, HERO_Y_KM, size=0.85, tags=False)
    frame_at = ctx["run"].frames[round(minute * 60 / ctx["step_s"])]
    for sent_s, call in ctx["breaking"]:
        if sent_s <= minute * 60:
            x, y = km(frame_at.entities[call["vehicle_id"]].position)
            out.append(f"\\node[accent, anchor=west, font=\\scriptsize\\bfseries, "
                       f"inner sep=4pt] at ({x + 0.6:.2f},{y:.2f}) "
                       f"{{urgent, {call['slack_s'] / 60:.1f} min to spare}};")
    return out


if __name__ == "__main__":
    if len(sys.argv) == 2:
        lines = strip(load(sys.argv[1]))
    elif len(sys.argv) == 4 and sys.argv[2] == "--hero":
        lines = hero(load(sys.argv[1]), float(sys.argv[3]))
    else:
        sys.exit(__doc__)
    print("% Generated by crates/delivery/mission_strip.py; do not edit by hand.")
    print("\n".join(lines))
