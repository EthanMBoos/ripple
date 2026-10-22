#!/usr/bin/env python3
"""The paper's result figure, drawn from the instruction sweeps.

    python3 crates/delivery/results_figs.py sweeps/directives-test/run002 \\
        sweeps/directives-6dof/run000 paper/manuscript

Writes fig-slope.tex, a TikZ picture for main.tex to \\input (like the
mission figures from mission_strip.py, in the paper's own fonts and
colours): per rule and wording, the share of decisions that broke the rule
where it could be broken against the share of missions broken, on one 0-100%
scale. Also prints the numbers the text quotes: the missions' 95% intervals,
the skill, the vehicles' limits, and the 6-DOF check.

Intervals are Wilson 95% intervals over missions; p-values are two-sided
Fisher exact tests on missions broken.
"""

import json
import math
import statistics
import sys
from pathlib import Path

D = "entities.depot.autonomy.Dispatcher."
WORDING = {"no_fly": (0, 3), "ground_vehicle": (0, 2), "reserve_multirotor": (0, 2)}


def load(folder):
    """Each case as (parameters without the prefix, seed, metric values)."""
    cases = []
    for line in (Path(folder) / "results.jsonl").open():
        case = json.loads(line)
        parameters = {k.removeprefix(D): v for k, v in case["parameters"].items()}
        (values,) = [teams["1"] for teams in case["metrics"].values()]
        cases.append((parameters, case["seed"], values))
    return cases


def select(cases, kind, wording=None, skills=2, enforce=False, limits=True):
    return [(p, seed, v) for p, seed, v in cases
            if p["directive.kind"] == kind and (wording is None or p["directive.phrasing"] == wording)
            and len(p["skills"]) == skills and p["enforce_instruction"] == enforce
            and p["limits_in_prompt"] == limits]


def broken(values):
    return values.get("directive_breaches", 0) > 0 or values.get("directive_breach_s", 0) > 0


def missions(runs):
    return sum(map(broken, (v for _, _, v in runs))), len(runs)


def on_time(runs):
    return statistics.fmean(v["score"] for _, _, v in runs)


def wilson(k, n, z=1.96):
    if n == 0:
        return 0.0, 0.0
    centre = (k + z * z / 2) / (n + z * z)
    half = z * math.sqrt(k * (n - k) / n + z * z / 4) / (n + z * z)
    return max(0.0, centre - half), min(1.0, centre + half)


def fisher(a, n1, b, n2):
    """Two-sided Fisher exact p for a of n1 against b of n2."""
    total, k = n1 + n2, a + b

    def prob(x):
        return math.comb(n1, x) * math.comb(n2, k - x) / math.comb(total, k)

    observed = prob(a)
    return min(1.0, sum(prob(x) for x in range(max(0, k - n2), min(k, n1) + 1)
                        if prob(x) <= observed * (1 + 1e-9)))


def slope(cases):
    """One decision against a whole mission, per rule and wording, on one
    0-100% scale: the share of decisions that broke the rule where it could
    be broken, and the share of missions broken. The missions' 95% intervals
    are printed, for the caption, rather than drawn."""
    span, height = 3.7, 4.0  # cm between the two columns, cm for 100%
    out = ["\\begin{tikzpicture}[x=1cm, y=1cm, font=\\scriptsize]"]
    for value in (0, 25, 50, 75, 100):
        y = height * value / 100
        out.append(f"  \\draw[black!10] (-0.15,{y:.2f}) -- ({span + 0.15:.2f},{y:.2f});")
        out.append(f"  \\node[anchor=east, text=black!55] at (-0.15,{y:.2f}) {{{value}\\%}};")
    out.append(f"  \\node[anchor=south, rotate=90] at (-0.95,{height / 2:.2f}) {{broke the rule}};")
    for x, label in ((0, "one decision"), (span, "a whole mission")):
        out.append(f"  \\node[anchor=north, font=\\footnotesize] at ({x:.2f},-0.12) {{{label}}};")
    lines = {}
    for kind, (a, b) in WORDING.items():
        for label, wording in (("A", a), ("B", b)):
            runs = select(cases, kind, wording)
            chances = sum(v.get("directive_opportunities", 0) for _, _, v in runs)
            slipped = sum(v.get("breaking_decisions", 0) for _, _, v in runs)
            k, n = missions(runs)
            lines[(kind, label)] = (slipped / chances, k, n)
            print(f"{kind} {label}: decisions {slipped:g}/{chances:g}, missions {k}/{n}")
        pa, pb = missions(select(cases, kind, a)), missions(select(cases, kind, b))
        print(f"  A vs B: p = {fisher(*pa, *pb):.4f}")
    # Lines that coincide are drawn once and labeled together.
    drawn = [(("no_fly", "B"), "accent", "no-fly zone, A and B"),
             (("reserve_multirotor", "B"), "learnc", "multirotor standby, B"),
             (("ground_vehicle", "B"), "classc", "grounded vehicle, B"),
             (("ground_vehicle", "A"), "black!45", "grounded and standby, A")]
    for key, colour, text in drawn:
        rate, k, n = lines[key]
        lo, hi = wilson(k, n)
        print(f"  {text}: missions {k}/{n}, 95% interval {lo:.0%}-{hi:.0%}")
        y0, y1 = height * rate, height * k / n
        out.append(f"  \\draw[{colour}, line width=2pt, line cap=round] (0,{y0:.3f}) -- "
                   f"({span:.2f},{y1:.3f});")
        for x, y in ((0, y0), (span, y1)):
            out.append(f"  \\filldraw[fill={colour}, draw=white, line width=0.9pt] "
                       f"({x:.2f},{y:.3f}) circle (2.7pt);")
        out.append(f"  \\node[anchor=west, inner sep=0pt] at ({span + 0.22:.2f},{y1:.3f}) {{{text}}};")
    out.append("\\end{tikzpicture}")
    return out


def other_results(cases, six):
    """The numbers the text quotes for the skill, the limits, and the 6-DOF
    check, printed for checking."""
    planner = [missions(select(cases, "no_fly", w)) for w in (0, 3)]
    via = [missions(select(cases, "no_fly", w, skills=3)) for w in (0, 3)]
    for w, (p, v) in zip("AB", zip(planner, via)):
        print(f"skill {w}: planner {p}, drop_via {v}, p = {fisher(*p, *v):.4f}")
    for stated in (True, False):
        runs = select(cases, "none", limits=stated)
        counts = [v.get("refused_wrong_skill", 0) for _, _, v in runs]
        print(f"limits stated={stated}: land calls {sum(counts):g}, range {min(counts):g}-"
              f"{max(counts):g}, on time {on_time(runs):.2f}")
    for w in (0, 3):
        kin, dof = missions(select(cases, "no_fly", w)), missions(select(six, "no_fly", w))
        print(f"6-DOF wording {w}: kinematic {kin}, 6-DOF {dof}, p = {fisher(*kin, *dof):.3f}")


def main(test, six, folder):
    cases, six_cases = load(test), load(six)
    text = "% Generated by crates/delivery/results_figs.py; do not edit by hand.\n"
    (Path(folder) / "fig-slope.tex").write_text(text + "\n".join(slope(cases)) + "\n")
    other_results(cases, six_cases)


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(*sys.argv[1:])
