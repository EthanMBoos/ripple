#!/usr/bin/env python3
"""Message routing cost: native C++ against Rust, with and without all-agent messages.

    python3 reference/message_benchmark.py --output runs/message-benchmark000

Each case is perf.py's aircraft mission with and without a NoisyState sensor
and its messages on every aircraft, so the difference is what those messages
cost. In SCRIMMAGE, NoisyState publishes the reading on LocalNetwork and
Straight subscribes to it. Ripple's NoisyState sends no message (the reading
goes to the entity's estimators), so the Rust mission adds the same traffic
with a benchmark-only pair from the runner: StateBroadcast publishes each
aircraft's state on LocalNetwork every step, and StateListener subscribes and
discards it. Both simulators then route one message per aircraft per step to
every subscriber's network check. C++ is the
native build (native_build.sh, built on first use); Rust is the core-only runner
on one worker. A variant's time per step is the whole process minus a one-step
run of the same mission, the median of the repeats after a warm-up; variants
alternate run by run. The C++ comparison checks that both deliver the same
messages (reference_check.py); this measures what delivering them costs.

On macOS it also records a `sample` profile of the 2,048-agent NoisyState case
for each simulator (the "sort by top of stack" summary) in OUTPUT/profiles/.
Writes OUTPUT/timings.json. Standard library only; macOS only.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import sys
import time
import xml.etree.ElementTree as ET

import native
import perf
from project import ROOT, build_runner

AGENTS = (128, 512, 2048, 4096)
PROFILED_AGENTS = 2048
END_S = 10  # 100 steps
NOISY_STATE = "    <sensor>NoisyState</sensor>"
# Ripple's stand-in for NoisyState's message and Straight's subscription.
RUST_MESSAGES = ("    <sensor>StateBroadcast</sensor>\n"
                 "    <autonomy>StateListener</autonomy>")
VARIANTS = ("cpp", "rust")


def sensors_for(variant, sensors):
    """The entity's extra elements: Rust adds the message pair wherever NoisyState is."""
    if variant == "rust" and sensors:
        return sensors + "\n" + RUST_MESSAGES
    return sensors


def mission(directory, agents, sensors, end_s):
    """perf.py's aircraft mission, with C++ logging set so both simulators write frames."""
    directory.mkdir(parents=True)
    fields = {**perf.WORKLOADS["motion"][2], "end": end_s, "sensors": sensors}
    root = ET.fromstring(perf.HEADER.format(name="messages", agents=agents, **fields))
    for key, value in {"log_dir": directory / "logs", "create_latest_dir": "false",
                       "display_progress": "false", "output_type": "all",
                       "no_bin_logging": "false", "multi_threaded": "false"}.items():
        ET.SubElement(root, key).text = str(value)
    path = directory / "mission.xml"
    ET.ElementTree(root).write(path, encoding="utf-8", xml_declaration=True)
    return path


def command(variant, runner, path, output):
    if variant == "cpp":
        return [native.CPP, path], native.CPP_ENV
    return [runner, path, ROOT, output, "1"], os.environ


def wall_seconds(variant, runner, path, output):
    shutil.rmtree(output, ignore_errors=True)
    arguments, environment = command(variant, runner, path, output)
    started = time.perf_counter()
    subprocess.run([str(part) for part in arguments], env=environment, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return time.perf_counter() - started


def profile(variant, runner, directory, agents, profiles):
    """Save `sample`'s top-of-stack summary for three seconds of stepping."""
    path = mission(directory / f"profile-{variant}", agents, sensors_for(variant, NOISY_STATE),
                   120)
    arguments, environment = command(variant, runner, path, directory / f"profile-{variant}/rust")
    process = subprocess.Popen([str(part) for part in arguments], env=environment,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(1.5)  # past setup
        report = directory / f"profile-{variant}/sample.txt"
        subprocess.run(["sample", str(process.pid), "3", "-file", str(report)],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    finally:
        process.kill()
        process.wait()
    text = report.read_text(errors="replace")
    summary = text[text.index("Sort by top of stack"):text.index("Binary Images")]
    (profiles / f"{variant}-{agents}.txt").write_text(summary)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True)  # never overwrite an earlier benchmark
    with (output / "cpp-build.log").open("w") as log:
        native.ensure_build(log)
    runner = build_runner()
    scratch = output / "scratch"
    cases = []
    for agents in AGENTS:
        for label, sensors in (("without messages", ""), ("NoisyState", NOISY_STATE)):
            directory = scratch / f"{agents}-{'state' if sensors else 'plain'}"
            paths = {variant: {
                "full": mission(directory / variant / "full", agents,
                                sensors_for(variant, sensors), END_S),
                "one": mission(directory / variant / "one-step", agents,
                               sensors_for(variant, sensors), 0.1),
            } for variant in VARIANTS}
            samples = {variant: {"full": [], "one": []} for variant in VARIANTS}
            for repeat in range(args.repeats + 1):
                shift = repeat % len(VARIANTS)
                for variant in VARIANTS[shift:] + VARIANTS[:shift]:
                    for kind, path in paths[variant].items():
                        seconds = wall_seconds(variant, runner, path,
                                               directory / variant / f"{kind}-output")
                        if repeat:  # the first round is a warm-up
                            samples[variant][kind].append(seconds)
            steps = round(END_S / 0.1)
            case = {"agents": agents, "case": label, "variants": {}}
            for variant, runs in samples.items():
                per_step_ms = (statistics.median(runs["full"]) - statistics.median(runs["one"])) \
                    / (steps - 1) * 1e3
                case["variants"][variant] = {"ms_per_step": per_step_ms, "full_s": runs["full"],
                                             "one_step_s": runs["one"]}
            cases.append(case)
            print(agents, label, "  ".join(f"{v} {case['variants'][v]['ms_per_step']:.2f}"
                                           for v in VARIANTS), "ms/step", flush=True)
            shutil.rmtree(directory)

    profiled = shutil.which("sample") is not None
    if profiled:
        profiles = output / "profiles"
        profiles.mkdir()
        for variant in VARIANTS:
            profile(variant, runner, scratch, PROFILED_AGENTS, profiles)
    shutil.rmtree(scratch, ignore_errors=True)
    (output / "timings.json").write_text(json.dumps({
        "provenance": native.provenance(), "repeats": args.repeats, "steps": round(END_S / 0.1),
        "timing": "whole process minus a one-step run, median of repeats after a warm-up",
        "profiles": f"{PROFILED_AGENTS} agents with NoisyState" if profiled else None,
        "cases": cases,
    }, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
