#!/usr/bin/env python3
"""Ask a model to write a policy once, from the mission's own description.

    python3 crates/delivery/write_policy.py --model claude-haiku-5-5 \
        --out crates/delivery/policies/haiku.yaml
    python3 crates/delivery/write_policy.py --model claude-haiku-5-5 \
        --revise crates/delivery/policies/haiku.yaml \
        --day runs/region-16/run047 --day runs/region-64/run021 \
        --out crates/delivery/policies/haiku-revised.yaml

The second form shows the model its policy and what happened on each day
it was run (the run's summary values), and asks for one revision.

The prompt is the mission's system prompt (the one the in-the-loop manager
reads), the event kinds with their texts, and policies/SYNTAX.md. Nothing
about the fleet's state: the model writes rules, not orders. The reply is
saved as written, so a policy that does not parse is a result too
(`ripple run` reports why). The model is reached through the `claude`
command, like the in-the-loop manager with `via: cli`.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from region import EVENT_TEXT, SYSTEM_PROMPT  # noqa: E402

TASK = """\
Write the rules the dispatcher will follow all day, once, in the policy form
below. You will not be asked again: the rules must cover every event kind you
want acted on, and the check-in. Each hub starts with {units} units of each
blood type and holds {fixed_wing} fixed-wing aircraft and {multirotor}
multirotors; fixed-wing aircraft reach any hub, multirotors only neighbours.

The event kinds and the text each one arrives with:
{kinds}

{syntax}
Reply with the YAML list only, no other text."""


REVISE = """\
Here is the policy you wrote, in the policy form below, and what happened on
each day the dispatcher ran it. Revise it once; you will not see another day.
A delivery is on time if it meets its deadline; the score is the share of
requests delivered on time, weighted by urgency, with deliveries that broke a
rule not counted. Each hub starts with {units} units of each blood type and
holds {fixed_wing} fixed-wing aircraft and {multirotor} multirotors.

Your policy:
{policy}

What happened:
{days}

{syntax}
Reply with the revised YAML list only, no other text."""

# The summary values shown for a day, in this order.
SHOWN = [
    "score", "requests", "on_time", "late", "delivered_in_breach", "missed_no_vehicle",
    "trauma_requests", "trauma_on_time", "urgent_requests", "urgent_on_time",
    "orders", "orders_refused", "moves", "lends", "applied_reposition", "applied_transfer",
    "applied_serve_from", "relocations", "vehicles_lost",
]


def day_lines(folder: Path) -> str:
    values = json.loads((folder / "summary.json").read_text())
    hubs = folder.parent.name.replace("region-", "")
    parts = []
    for name in SHOWN:
        value = values.get(name)
        if value is None:
            continue
        parts.append(f"{name} {value:.3f}" if name == "score" else f"{name} {int(value)}")
    return f"- a day at {hubs} hubs: " + ", ".join(parts)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--model", default="claude-haiku-5-5")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--units", type=int, default=100)
    parser.add_argument("--fixed-wing", type=int, default=3)
    parser.add_argument("--multirotor", type=int, default=2)
    parser.add_argument("--revise", type=Path, help="a policy to revise once from its days")
    parser.add_argument("--day", type=Path, action="append", default=[], help="a run of that policy")
    args = parser.parse_args()

    kinds = "\n".join(f"- {kind}: {text}" for kind, text in EVENT_TEXT.items())
    syntax = (HERE / "policies" / "SYNTAX.md").read_text()
    if args.revise:
        prompt = REVISE.format(
            units=args.units,
            fixed_wing=args.fixed_wing,
            multirotor=args.multirotor,
            policy=args.revise.read_text(),
            days="\n".join(day_lines(day) for day in args.day),
            syntax=syntax,
        )
    else:
        prompt = TASK.format(
            units=args.units,
            fixed_wing=args.fixed_wing,
            multirotor=args.multirotor,
            kinds=kinds,
            syntax=syntax,
        )
    reply = subprocess.run(
        [
            "claude", "-p", "--model", args.model, "--output-format", "text",
            "--no-session-persistence", "--tools", "", "--system-prompt", SYSTEM_PROMPT,
        ],
        input=prompt,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    # Keep the YAML; drop a code fence if the model wrote one.
    fenced = re.search(r"```[a-z]*\n(.*?)```", reply, re.S)
    text = fenced.group(1) if fenced else reply
    header = (
        f"# Revised once by {args.model} from {len(args.day)} days of {args.revise} (write_policy.py).\n"
        if args.revise
        else f"# Written by {args.model} once from the mission description (write_policy.py).\n"
    )
    args.out.write_text(header + text.strip() + "\n")
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
