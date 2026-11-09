#!/usr/bin/env python3
"""Write the appendix tables for the model runs from their sweep results.

    python3 crates/delivery/counts_table.py sweeps/region-16-models/run000/results.jsonl \
        sweeps/region-64-models/run000/results.jsonl paper/appendix

Writes OUT_DIR/counts.tex, one row per run with the calls, orders, applied
and refused orders, unreadable replies, moves, lending permissions and
vehicles lost, and OUT_DIR/scores.tex, the per-seed on-time score and
moves for each model and role, which the paper's model table summarises
as means. Rows are ordered by size, role, model, seed.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

PREFIX = "entities.region.autonomy.Dispatcher."
MODELS = [("claude-haiku-5-5", "Haiku"), ("claude-sonnet-5-5", "Sonnet"),
          ("claude-opus-5-5", "Opus"), ("gpt-6-luna", "GPT-6 Luna")]
ROLES = [("model", "Live"), ("veto", "Veto")]


def rows(paths):
    out = []
    for path in paths:
        for line in Path(path).read_text().splitlines():
            run = json.loads(line)
            p = run["parameters"]
            m = run["metrics"]["metrics/DeliveryScore:0"]["1"]
            out.append((int(p[PREFIX + "region.hubs"]), p[PREFIX + "manager.kind"],
                        p[PREFIX + "manager.model"], run["seed"], m))
    return out


def ordered(runs):
    order = {name: i for i, (name, _) in enumerate(MODELS)}
    roles = {kind: i for i, (kind, _) in enumerate(ROLES)}
    return sorted(runs, key=lambda r: (r[0], roles[r[1]], order[r[2]], r[3]))


def counts(runs):
    names = dict(MODELS)
    roles = dict(ROLES)
    lines = ["\\begin{tabular}{llllrrrrrrrr}", "\\toprule",
             "Hubs & Role & Model & Seed & Calls & Orders & Applied & Refused & "
             "Unreadable & Moves & Lends & Lost \\\\", "\\midrule"]
    last = None
    for hubs, kind, model, seed, m in ordered(runs):
        if last is not None and last != (hubs, kind):
            lines.append("\\addlinespace")
        last = (hubs, kind)
        cells = [str(hubs), roles[kind], names[model], str(seed)]
        cells += [f"{m.get(k, 0):.0f}" for k in ("calls", "orders", "orders_applied", "orders_refused",
                                                 "parse_failures", "moves", "lends", "vehicles_lost")]
        lines.append(" & ".join(cells) + " \\\\")
    lines += ["\\bottomrule", "\\end{tabular}"]
    return "\n".join(lines) + "\n"


def scores(runs):
    """Per seed: score and moves (kept moves, for the veto), one cell per model."""
    names = dict(MODELS)
    roles = dict(ROLES)
    by = {}
    for hubs, kind, model, seed, m in runs:
        by.setdefault((hubs, kind, model), []).append((seed, m["score"], m.get("moves", 0)))
    lines = ["\\begin{tabular}{llcccc}", "\\toprule",
             "Hubs & Role & " + " & ".join(label for _, label in MODELS) + " \\\\", "\\midrule"]
    for hubs in sorted({r[0] for r in runs}):
        for kind, role in ROLES:
            cells = []
            for model, _ in MODELS:
                per = sorted(by.get((hubs, kind, model), []))
                cells.append(" / ".join(f"{s:.2f}" for _, s, _ in per) + ", "
                             + " / ".join(f"{mv:.0f}" for _, _, mv in per))
            lines.append(f"{hubs} & {role} & " + " & ".join(cells) + " \\\\")
    lines += ["\\bottomrule", "\\end{tabular}"]
    return "\n".join(lines) + "\n"


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    *paths, out_dir = sys.argv[1:]
    runs = rows(paths)
    out = Path(out_dir)
    out.joinpath("counts.tex").write_text(counts(runs))
    out.joinpath("scores.tex").write_text(scores(runs))
    print(f"{len(runs)} runs -> {out / 'counts.tex'}, {out / 'scores.tex'}")


if __name__ == "__main__":
    main()
