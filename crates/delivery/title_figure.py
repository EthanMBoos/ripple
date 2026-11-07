#!/usr/bin/env python3
"""Draw the title figure's two maps from the paired surge records.

    python3 crates/delivery/title_figure.py LENDING_RUN RELOCATING_RUN OUT_DIR

Each run is a directory holding a run.mcap of the region-16 surge day (a
scripted surge at hub 6 at minute 60, no other events) under the serve and
the move policy. The output is TikZ commands, in km, for the window around
hub 1 at minute 90 with the trails of hub 1's aircraft since minute 60:
OUT_DIR/title-lending.tex and OUT_DIR/title-relocating.tex. main.tex
inputs them in a scaled scope and adds the annotations. The day numbers
each record's summary holds are printed, to check against the typed ones.

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
from region import hub_position_m, sites  # noqa: E402
from run import MAGIC, blob, decode_summary, integer, records, string, zstd  # noqa: E402

SIDE, SPACING_M = 4, 15000.0
HUB = 1  # the hub that gives its aircraft
SURGE_HUBS = [6, 2, 5, 7, 10]  # hub 6 and its grid neighbors, as event.rs groups them
TRAIL_FROM_S, AT_S, STEP_S = 3600.0, 5400.0, 10.0
WINDOW_KM = (-37.0, -1.0)  # the same square on both axes
HOME_KM = 5.0  # near home: loitering or at one of its own sites


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
                    if time_s < next_sample - 1e-6 or time_s > AT_S + 1e-6:
                        continue
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
                    samples.append((time_s, frame))
    if summary is None or not samples:
        sys.exit(f"{run_dir}: no summary or no frames in the window")
    return described, samples, summary


def hub_of(group):
    """Each hub has two groups, fixed-wing then multirotor; group 0 is the region."""
    return (group + 1) // 2 if group > 0 else 0


def km(x_m, y_m):
    return x_m / 1000.0, y_m / 1000.0


def inside(x, y):
    low, high = WINDOW_KM
    return low <= x <= high and low <= y <= high


def plane(x, y, heading_deg, color, size_km):
    """A fixed-wing aircraft: an arrowhead at (x, y) km along its heading."""
    a, b, c = 0.7 * size_km, 0.5 * size_km, 0.45 * size_km
    return (f"\\fill[{color}, rotate around={{{heading_deg:.0f}:({x:.2f},{y:.2f})}}] "
            f"({x + a:.2f},{y:.2f}) -- ({x - b:.2f},{y + c:.2f}) -- "
            f"({x - b / 2:.2f},{y:.2f}) -- ({x - b:.2f},{y - c:.2f}) -- cycle;")


def draw(described, samples, color):
    low, high = WINDOW_KM
    out = [f"\\fill[white] ({low},{low}) rectangle ({high},{high});",
           "\\begin{scope}", f"\\clip ({low},{low}) rectangle ({high},{high});"]
    hubs_shown = [h for h in range(1, SIDE * SIDE + 1)
                  if inside(*km(*hub_position_m(h, SIDE, SPACING_M)))]
    # Every shown hub's sites; hub 1's with spokes, the others' faint.
    for hub in hubs_shown:
        hx, hy = km(*hub_position_m(hub, SIDE, SPACING_M))
        for _code, rooftop, x_m, y_m in sites(hub, SIDE, SPACING_M):
            x, y = km(x_m, y_m)
            shade = "black!70" if hub == HUB else "black!30"
            if hub == HUB:
                out.append(f"\\draw[black!20, dashed, line width=0.3pt] ({hx:.2f},{hy:.2f}) -- ({x:.2f},{y:.2f});")
            if rooftop:
                out.append(f"\\draw[{shade}, fill=white, line width=0.4pt] ({x:.2f},{y:.2f}) +(-1.2pt,-1.2pt) rectangle +(1.2pt,1.2pt);")
            else:
                out.append(f"\\fill[{shade}] ({x:.2f},{y:.2f}) circle (1.1pt);")
    # The surge, a dashed ring at each hub the event names.
    for hub in SURGE_HUBS:
        hx, hy = km(*hub_position_m(hub, SIDE, SPACING_M))
        out.append(f"\\draw[muted, dashed, line width=0.5pt] ({hx:.2f},{hy:.2f}) circle (3.2);")
    # Hub 1's aircraft: their trails since the surge began, then where they are.
    givers = sorted(e for e, (group, _) in described.items() if hub_of(group) == HUB)
    for entity in givers:
        points = [km(f[entity][0], f[entity][1]) for _, f in samples if entity in f]
        path = " -- ".join(f"({x:.2f},{y:.2f})" for x, y in points)
        out.append(f"\\draw[{color}, line width=0.45pt, opacity=0.7, line join=round] {path};")
    _, last = samples[-1]
    for entity, (group, model) in sorted(described.items()):
        if entity not in last or hub_of(group) == 0:
            continue
        x_m, y_m, vx, vy = last[entity]
        x, y = km(x_m, y_m)
        if not inside(x, y):
            continue
        giver = hub_of(group) == HUB
        fill = color if giver else "black!45"
        if model == "aircraft":
            heading = math.degrees(math.atan2(vy, vx)) if math.hypot(vx, vy) > 1.0 else 0.0
            out.append(plane(x, y, heading, fill, 1.7 if giver else 1.2))
        else:
            out.append(f"\\fill[{fill}] ({x:.2f},{y:.2f}) circle ({1.7 if giver else 1.2}pt);")
    # Hubs on top, named.
    for hub in hubs_shown:
        hx, hy = km(*hub_position_m(hub, SIDE, SPACING_M))
        out.append(f"\\fill[black] ({hx:.2f},{hy:.2f}) +(-2pt,-2pt) rectangle +(2pt,2pt);")
        anchor = "north west" if hub in SURGE_HUBS else "south east"
        out.append(f"\\node[font=\\tiny, anchor={anchor}, inner sep=1.5pt] at ({hx:.2f},{hy:.2f}) {{hub {hub}}};")
    out.append("\\end{scope}")
    out.append(f"\\draw[black!30, line width=0.4pt] ({low},{low}) rectangle ({high},{high});")
    return out


def report(name, described, samples, summary):
    score = summary["metrics/DeliveryScore:0"][1]
    at_home = 0
    _, last = samples[-1]
    hx, hy = km(*hub_position_m(HUB, SIDE, SPACING_M))
    for entity, (group, _) in described.items():
        if hub_of(group) == HUB and entity in last:
            x, y = km(last[entity][0], last[entity][1])
            at_home += math.hypot(x - hx, y - hy) <= HOME_KM
    print(f"{name}: score {score['score']:.3f}, hub {HUB} requests "
          f"{score.get(f'hub_{HUB:03}_requests', 0):.0f}, undelivered "
          f"{score.get(f'hub_{HUB:03}_undelivered', 0):.0f}, late "
          f"{score.get(f'hub_{HUB:03}_late', 0):.0f}; at minute {AT_S / 60:.0f}, "
          f"{at_home} of hub {HUB}'s aircraft within {HOME_KM:.0f} km of home; "
          f"{len(samples)} samples")


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    out_dir = Path(sys.argv[3])
    for name, run_dir, color in (("lending", sys.argv[1], "ink"), ("relocating", sys.argv[2], "orange")):
        described, samples, summary = read(Path(run_dir))
        lines = ["% Generated by crates/delivery/title_figure.py; do not edit by hand.",
                 *draw(described, samples, color)]
        out_dir.joinpath(f"title-{name}.tex").write_text("\n".join(lines) + "\n")
        report(name, described, samples, summary)


if __name__ == "__main__":
    main()
