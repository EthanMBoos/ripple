#!/usr/bin/env python3
"""Print one delivery run's decisions: what the model saw, what it proposed,
what became of each call, and when each request was delivered.

    python3 crates/delivery/trace.py run.seed:=2 \\
        entities.depot.autonomy.Dispatcher.directive.kind:=closure

Reruns the mission with the given overrides from the saved replies
(`offline: true`, so no model calls and the same run as before) with
--trace, and reads the record. Run from the repository root after
`cargo build --release`.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "reference"))
from run import read_run  # noqa: E402

MISSION = "crates/delivery/missions/delivery.yaml"
SAVED_ONLY = "entities.depot.autonomy.Dispatcher.offline:=true"


def minutes(seconds):
    return f"{seconds / 60:5.1f}"


def main(overrides):
    command = [str(ROOT / "target/release/ripple"), "run", MISSION, "--headless", "--trace",
               SAVED_ONLY, *overrides]
    output = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    if output.returncode != 0:
        sys.exit(output.stderr)
    folder = re.search(r"output (\S+)", output.stdout + output.stderr).group(1)
    run = read_run(folder)

    # Each message reaches every subscriber; keep the score's copy of each.
    received = [t for t in run.traces if t.get("kind") == "delivery"
                and t["receiver"]["plugin"].startswith("metrics/")]
    decisions = [t["payload"] for t in received if t["topic"] == "DispatchDecision"]
    delivered = {t["payload"]["request_id"]: t["payload"] for t in received
                 if t["topic"] == "Delivered"}
    requests = {t["payload"]["id"]: t["payload"] for t in received
                if t["topic"] == "DeliveryRequest"}

    print(f"Run {folder}")
    shown = None
    for decision in decisions:
        if decision["instructions"] != shown:
            shown = decision["instructions"]
            print(f"\n  Instructions now: {shown or 'none'}")
        waiting = ", ".join(f"{w['request_id']}{'!' if w['urgent'] else ''} {w['site']}"
                            for w in decision["waiting"])
        print(f"\nminute {minutes(decision['time_s'])}  idle {decision['idle']}  "
              f"waiting {waiting}" + ("  (unreadable reply)" if decision["parse_failed"] else ""))
        for call in decision["calls"]:
            slack = call["slack_s"]
            slack = f"slack {slack / 60:+.1f} min" if slack is not None else ""
            print(f"    vehicle {call['vehicle_id']} {call['skill'] or '?'} request "
                  f"{call['request_id']}: {call['outcome']} {slack}")

    print("\nRequests (! urgent):")
    for request_id, request in sorted(requests.items()):
        done = delivered.get(request_id)
        if done is None:
            outcome = "not delivered"
        else:
            late = done["time_s"] > request["deadline_s"]
            outcome = (f"delivered minute {minutes(done['time_s'])} by vehicle "
                       f"{done['vehicle_id']}{' LATE' if late else ''}")
        print(f"  {request_id:2}{'!' if request['urgent'] else ' '} {request['site']:12} "
              f"opened {minutes(request['opened_at_s'])} due {minutes(request['deadline_s'])}: "
              f"{outcome}")
    (team,) = list(run.metrics.values())[0].values()
    print("\n" + ", ".join(f"{k} {v:g}" for k, v in sorted(team.items())))


if __name__ == "__main__":
    main(sys.argv[1:])
