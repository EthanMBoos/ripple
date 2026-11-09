#!/usr/bin/env python3
"""Draw the title figure's two maps from the paired surge records.

    python3 crates/delivery/title_figure.py LENDING_RUN RELOCATING_RUN OUT_DIR [HUB]

Each run is a directory holding a run.mcap of the region-16 surge day (a
scripted surge at hub 6 at minute 60, no other events) under the serve and
the move policy. The output is TikZ commands, in km, for a 36 km window
around HUB (default 3, which gave all five of its aircraft on seed 1) at
minute 90 with the trails of that hub's aircraft since minute 60:
OUT_DIR/title-lending.tex and OUT_DIR/title-relocating.tex. main.tex
inputs them in a scaled scope and adds the annotations; the scope shift
for the window is printed with the day numbers each record's summary
holds, to check against the typed ones.

The record is read straight from its chunks, decompressing only those in
the window and the last one (the summary), because reference/run.py's
read_run builds Python objects for every frame of a region day.
"""

from __future__ import annotations

import math
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "reference"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from frames import fields, values  # noqa: E402
from region import hub_position_m  # noqa: E402
from run import MAGIC, blob, decode_summary, integer, records, string, zstd  # noqa: E402

SIDE, SPACING_M = 4, 15000.0
HUB = 3  # the hub whose aircraft are drawn; the command line can change it
SURGE_HUBS = [6, 2, 5, 7, 10]  # hub 6 and its grid neighbors, as event.rs groups them
TRAIL_FROM_S, AT_S, STEP_S = 3600.0, 5400.0, 10.0
WINDOW_KM = 36.0  # a square this wide, centered between HUB and SURGE_HUB
SURGE_HUB = 6  # the scripted surge is at hub 6 and its neighbors
HOME_KM = 5.0  # near home: loitering or at one of its own sites

# Site geometry, the same as region.rs: clinics A to F at these radii on
# angles 60 degrees apart, hospitals H1 and H2 at theirs on 30 and 210
# degrees, all turned by 17 degrees per hub so neighbors' sites interleave.
CLINIC_RADII_KM = [4.0, 7.0, 10.0, 13.0, 6.0, 9.0]
HOSPITAL_RADII_KM = [1.5, 2.5]


def sites(hub: int, side: int, spacing_m: float) -> list[tuple[str, bool, float, float]]:
    """Hub `hub`'s sites as (code, rooftop, x_m, y_m), clinics then hospitals."""
    x, y = hub_position_m(hub, side, spacing_m)
    turn_deg = 17.0 * hub
    found = []
    for index, radius_km in enumerate(CLINIC_RADII_KM):
        angle = math.radians(60.0 * index + turn_deg)
        code = f"{hub}{chr(ord('A') + index)}"
        found.append((code, False, x + radius_km * 1000 * math.cos(angle),
                      y + radius_km * 1000 * math.sin(angle)))
    for index, radius_km in enumerate(HOSPITAL_RADII_KM):
        angle = math.radians(30.0 + 180.0 * index + turn_deg)
        found.append((f"{hub}H{index + 1}", True, x + radius_km * 1000 * math.cos(angle),
                      y + radius_km * 1000 * math.sin(angle)))
    return found


def window():
    """(low_x, low_y, high_x, high_y) in km: the window is centered between
    HUB and the surge's center, so the hub and the hubs its aircraft went to
    stay in view together."""
    hx, hy = km(*hub_position_m(HUB, SIDE, SPACING_M))
    sx, sy = km(*hub_position_m(SURGE_HUB, SIDE, SPACING_M))
    low_x = (hx + sx) / 2 - WINDOW_KM / 2
    low_y = (hy + sy) / 2 - WINDOW_KM / 2
    return low_x, low_y, low_x + WINDOW_KM, low_y + WINDOW_KM


def varint(item, field, default=0):
    found = values(item, field, 0)
    return found[-1] if found else default


def double(item, field):
    found = values(item, field, 1)
    return struct.unpack("<d", found[-1])[0] if found else 0.0


def text(item, field):
    found = values(item, field, 2)
    return found[-1].decode("utf-8") if found else ""


def read(run_dir: Path):
    """Entity descriptions, the sampled frames in the window, and the summary."""
    data = run_dir.joinpath("run.mcap").read_bytes()
    # The channel records and the entity descriptions are in the opening
    # chunks, at time zero; the frames and the summary follow in theirs.
    # Records outside any chunk are read too, in case a writer puts the
    # channels there.
    chunks, top = [], []
    for opcode, body in records(data[len(MAGIC):-len(MAGIC)]):
        if opcode == 0x06:
            start_ns, _ = integer(body, 0, 8)
            end_ns, _ = integer(body, 8, 8)
            chunks.append((start_ns / 1e9, end_ns / 1e9, body))
        elif opcode in (0x04, 0x05):
            top.append((opcode, body))
    channels, described, samples, summary = {}, {}, [], None
    first = None  # the first frame with the aircraft in it: each sits at its hub
    next_sample = TRAIL_FROM_S
    for index, (start_s, end_s, body) in enumerate([(0.0, 0.0, None)] + chunks):
        wanted = (start_s <= 1.0 or index == len(chunks)
                  or (end_s >= TRAIL_FROM_S - 1 and start_s <= AT_S + 1))
        if not wanted:
            continue
        if body is None:
            nested_records = top
        else:
            offset = 16
            _, offset = integer(body, offset, 8)
            offset += 4
            compression, offset = string(body, offset)
            nested, _ = blob(body, offset, 8)
            if compression == "zstd":
                nested = zstd.decompress(nested)
            nested_records = records(nested)
        for inner, inner_body in nested_records:
            if inner == 0x04:
                channel, offset = integer(inner_body, 0, 2)
                offset += 2
                topic, _ = string(inner_body, offset)
                channels[channel] = topic
            elif inner == 0x05:
                channel, offset = integer(inner_body, 0, 2)
                payload = inner_body[offset + 4 + 8 + 8:]
                topic = channels.get(channel)
                if topic == "/entities":
                    item = fields(payload)
                    described[varint(item, 1)] = (varint(item, 3), text(item, 5))
                elif topic == "/summary":
                    summary = decode_summary(payload)[0]
                elif topic == "/frames":
                    item = fields(payload)
                    time_s = double(item, 1)
                    # The couriers are built by spawn request in the first
                    # step, so the frame at time zero holds only the region.
                    at_start = first is None and time_s < 60.0
                    if not at_start and (time_s < next_sample - 1e-6 or time_s > AT_S + 1e-6):
                        continue
                    if not at_start:
                        next_sample += STEP_S
                    frame = {}
                    for raw in values(item, 2, 2):
                        row = fields(raw)
                        state = fields(b"".join(values(row, 2, 2)))
                        position = fields(b"".join(values(state, 1, 2)))
                        velocity = fields(b"".join(values(state, 3, 2)))
                        active = values(row, 3, 0)
                        if active and not active[-1]:
                            continue
                        frame[varint(row, 1)] = (double(position, 1), double(position, 2),
                                                 double(velocity, 1), double(velocity, 2))
                    if at_start:
                        if len(frame) > 1:
                            first = frame
                    else:
                        samples.append((time_s, frame))
    if summary is None or not samples or first is None:
        sys.exit(f"{run_dir}: no summary, no frame at time zero, or no frames in the window")
    return described, samples, summary, homes(described, first)


def homes(described, first):
    """Each aircraft's hub: the hub it sits at once built. Group 0 is
    the region entity, which has no hub. The recipes build one group per
    vehicle class, not per hub, so the hub is read from the position."""
    hubs = {h: km(*hub_position_m(h, SIDE, SPACING_M)) for h in range(1, SIDE * SIDE + 1)}
    found = {}
    for entity, (group, _) in described.items():
        if group == 0 or entity not in first:
            found[entity] = 0
            continue
        x, y = km(first[entity][0], first[entity][1])
        found[entity] = min(hubs, key=lambda h: math.dist((x, y), hubs[h]))
    return found


def km(x_m, y_m):
    return x_m / 1000.0, y_m / 1000.0


def inside(x, y):
    low_x, low_y, high_x, high_y = window()
    return low_x <= x <= high_x and low_y <= y <= high_y


def plane(x, y, heading_deg, color, size_km):
    """A fixed-wing aircraft: an arrowhead at (x, y) km along its heading,
    edged in white so it reads on top of a hub or a trail."""
    a, b, c = 0.7 * size_km, 0.5 * size_km, 0.45 * size_km
    return (f"\\filldraw[fill={color}, draw=white, line width=0.4pt, "
            f"rotate around={{{heading_deg:.0f}:({x:.2f},{y:.2f})}}] "
            f"({x + a:.2f},{y:.2f}) -- ({x - b:.2f},{y + c:.2f}) -- "
            f"({x - b / 2:.2f},{y:.2f}) -- ({x - b:.2f},{y - c:.2f}) -- cycle;")


def draw(described, samples, home, color):
    """The map: only what the story needs. The drawn hub's and the surge
    hubs' sites, the surge rings, the drawn hub's aircraft as faint trails
    since the surge began and a solid mark where each is now, and the hubs
    named. The other hubs' aircraft and sites are left out; they are the
    rest of the region going about its day."""
    low_x, low_y, high_x, high_y = window()
    out = [f"\\fill[white] ({low_x:.1f},{low_y:.1f}) rectangle ({high_x:.1f},{high_y:.1f});",
           "\\begin{scope}", f"\\clip ({low_x:.1f},{low_y:.1f}) rectangle ({high_x:.1f},{high_y:.1f});"]
    story = [h for h in [HUB, *SURGE_HUBS] if inside(*km(*hub_position_m(h, SIDE, SPACING_M)))]
    for hub in story:
        shade = "black!70" if hub == HUB else "black!35"
        for _code, rooftop, x_m, y_m in sites(hub, SIDE, SPACING_M):
            x, y = km(x_m, y_m)
            if rooftop:
                out.append(f"\\draw[{shade}, fill=white, line width=0.4pt] ({x:.2f},{y:.2f}) +(-1.2pt,-1.2pt) rectangle +(1.2pt,1.2pt);")
            else:
                out.append(f"\\fill[{shade}] ({x:.2f},{y:.2f}) circle (1.1pt);")
    # The surge, a dashed ring at each hub the event names.
    for hub in SURGE_HUBS:
        hx, hy = km(*hub_position_m(hub, SIDE, SPACING_M))
        out.append(f"\\draw[muted, dashed, line width=0.5pt] ({hx:.2f},{hy:.2f}) circle (3.2);")
    # The drawn hub's aircraft: faint trails that stack where sorties repeat,
    # then a solid mark where each one is now.
    givers = sorted(e for e in described if home[e] == HUB)
    for entity in givers:
        points = [km(f[entity][0], f[entity][1]) for _, f in samples if entity in f]
        path = " -- ".join(f"({x:.2f},{y:.2f})" for x, y in points)
        out.append(f"\\draw[{color}, line width=0.6pt, opacity=0.22, line cap=round, line join=round] {path};")
    # Hubs under the marks, named: the drawn hub below its square, the surge
    # hubs inside their rings.
    for hub in story:
        hx, hy = km(*hub_position_m(hub, SIDE, SPACING_M))
        out.append(f"\\fill[black] ({hx:.2f},{hy:.2f}) +(-2pt,-2pt) rectangle +(2pt,2pt);")
        if hub == HUB:  # to the right, clear of the aircraft parked at home and the rooftop below
            out.append(f"\\node[font=\\tiny, anchor=west, inner sep=1pt] at ({hx + 2.3:.2f},{hy - 0.3:.2f}) {{hub {hub}}};")
        else:
            out.append(f"\\node[font=\\tiny, anchor=north west, inner sep=2pt] at ({hx:.2f},{hy:.2f}) {{hub {hub}}};")
    _, last = samples[-1]
    for entity in givers:
        if entity not in last:
            continue
        x_m, y_m, vx, vy = last[entity]
        x, y = km(x_m, y_m)
        model = described[entity][1]
        if model == "aircraft":
            heading = math.degrees(math.atan2(vy, vx)) if math.hypot(vx, vy) > 1.0 else 0.0
            out.append(plane(x, y, heading, color, 1.7))
        else:
            out.append(f"\\filldraw[fill={color}, draw=white, line width=0.4pt] ({x:.2f},{y:.2f}) circle (1.8pt);")
    out.append("\\end{scope}")
    out.append(f"\\draw[black!30, line width=0.4pt] ({low_x:.1f},{low_y:.1f}) rectangle ({high_x:.1f},{high_y:.1f});")
    return out


def report(name, described, samples, summary, home):
    score = summary["metrics/DeliveryScore:0"][1]
    at_home = 0
    _, last = samples[-1]
    hx, hy = km(*hub_position_m(HUB, SIDE, SPACING_M))
    for entity in described:
        if home[entity] == HUB and entity in last:
            x, y = km(last[entity][0], last[entity][1])
            at_home += math.hypot(x - hx, y - hy) <= HOME_KM
    nearest = []
    for entity, (group, model) in sorted(described.items()):
        if home[entity] == HUB and entity in last:
            x, y = km(last[entity][0], last[entity][1])
            hub = min(range(1, SIDE * SIDE + 1),
                      key=lambda h: math.dist((x, y), km(*hub_position_m(h, SIDE, SPACING_M))))
            nearest.append(f"{model} near hub {hub}")
    low_x, low_y, _, _ = window()
    print(f"{name}: score {score['score']:.3f}, hub {HUB} requests "
          f"{score.get(f'hub_{HUB:03}_requests', 0):.0f}, undelivered "
          f"{score.get(f'hub_{HUB:03}_undelivered', 0):.0f}, late "
          f"{score.get(f'hub_{HUB:03}_late', 0):.0f}; at minute {AT_S / 60:.0f}, "
          f"{at_home} of hub {HUB}'s aircraft within {HOME_KM:.0f} km of home "
          f"({', '.join(nearest)}); {len(samples)} samples; window low corner "
          f"({low_x:.1f}, {low_y:.1f}) km, so a 4.4 cm scope anchored at (X, Y) cm "
          f"shifts by ({-low_x * 4.4 / WINDOW_KM:+.3f}, {-low_y * 4.4 / WINDOW_KM:+.3f}) cm")


def main():
    global HUB
    if len(sys.argv) not in (4, 5):
        sys.exit(__doc__)
    if len(sys.argv) == 5:
        HUB = int(sys.argv[4])
    out_dir = Path(sys.argv[3])
    for name, run_dir, color in (("lending", sys.argv[1], "ink"), ("relocating", sys.argv[2], "orange")):
        described, samples, summary, home = read(Path(run_dir))
        lines = ["% Generated by crates/delivery/title_figure.py; do not edit by hand.",
                 *draw(described, samples, home, color)]
        out_dir.joinpath(f"title-{name}.tex").write_text("\n".join(lines) + "\n")
        report(name, described, samples, summary, home)


if __name__ == "__main__":
    main()
