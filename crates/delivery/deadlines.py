#!/usr/bin/env python3
"""Were the calls that broke an instruction made under deadline pressure?

    python3 crates/delivery/deadlines.py sweeps/directives-test/run001

Reruns every instruction case of a sweep (none, and enforced, are skipped)
from the saved replies (`command: none`, so no model calls) with --trace,
and compares the calls that broke the instruction with the calls that kept
it: how many served an urgent request, how many had three minutes or less
to spare (the dispatcher's estimate of arrival against the deadline), and
the median slack. Breaking calls are listed one by one. Run from the
repository root after `cargo build --release`.
"""

import argparse
import json
import re
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "reference"))
from run import read_run  # noqa: E402

DISPATCHER = "entities.depot.autonomy.Dispatcher."
MISSION = "crates/delivery/missions/delivery.yaml"
TIGHT_MIN = 3.0


def override(name, value):
    """A sweep parameter as a `ripple run` override."""
    if isinstance(value, list):
        value = "[" + ",".join(str(v) for v in value) + "]"
    elif isinstance(value, bool):
        value = str(value).lower()
    return f"{name}:={value}"


def main(arguments):
    broke, kept, skipped = [], [], []
    for line in (Path(arguments.sweep) / "results.jsonl").open():
        case = json.loads(line)
        parameters = case["parameters"]
        if (parameters[DISPATCHER + "directive.kind"] == "none"
                or parameters.get(DISPATCHER + "enforce_instruction")):
            continue
        overrides = [override(name, value) for name, value in parameters.items()
                     if name != DISPATCHER + "command"]
        command = [str(ROOT / "target/release/ripple"), "run", MISSION, "--headless",
                   "--trace", f"run.seed:={case['seed']}", *overrides,
                   DISPATCHER + "command:=none"]
        output = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
        if output.returncode != 0:
            skipped.append(case["case_id"])
            continue
        run = read_run(re.search(r"output (\S+)", output.stdout + output.stderr).group(1))
        for trace in run.traces:
            if (trace.get("kind") != "delivery" or trace["topic"] != "DispatchDecision"
                    or not trace["receiver"]["plugin"].startswith("metrics/")):
                continue
            decision = trace["payload"]
            if not decision["instructions"]:
                continue
            urgent = {w["request_id"]: w["urgent"] for w in decision["waiting"]}
            for call in decision["calls"]:
                if call["outcome"] != "assigned" or call["slack_s"] is None:
                    continue
                row = (call["slack_s"] / 60, urgent.get(call["request_id"], False),
                       parameters[DISPATCHER + "directive.kind"],
                       parameters[DISPATCHER + "directive.wording"], case["case_id"],
                       decision["time_s"] / 60)
                (broke if call["breaks_instruction"] else kept).append(row)

    print("Calls that broke the instruction:")
    for slack, urgent, kind, wording, case_id, minute in sorted(broke):
        print(f"  {slack:6.1f} min to spare  {'urgent ' if urgent else 'routine'}  "
              f"{kind} wording {wording}  {case_id} minute {minute:.1f}")
    for name, rows in (("broke", broke), ("kept", kept)):
        if not rows:
            continue
        slack = [row[0] for row in rows]
        print(f"{name}: {len(rows)} calls, urgent {sum(row[1] for row in rows)}, "
              f"{TIGHT_MIN:g} min or less to spare {sum(s <= TIGHT_MIN for s in slack)}, "
              f"median slack {statistics.median(slack):.1f} min")
    if skipped:
        print(f"not replayable: {', '.join(skipped)}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("sweep")
    main(parser.parse_args())
