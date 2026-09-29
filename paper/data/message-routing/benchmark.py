#!/usr/bin/env python3
"""Message routing cost in C++, in Rust before routing by index, and after.

    python3 paper/data/message-routing/benchmark.py OLD_RUNNER NEW_RUNNER OUTPUT_DIR \
        --old-revision 1ff73c0 --new-revision "working tree"

OLD_RUNNER and NEW_RUNNER are reference/benchmark_runner.rs built before and
after routing by index (see equivalence.py for building an older revision). C++
is the native build from paper/data/benchmark-native/build.sh, in
runs/native-cpp. Each case is perf.py's aircraft mission (Straight, which
subscribes to NoisyState's messages on LocalNetwork) with and without a NoisyState
sensor on every aircraft, so the difference is what those messages cost. A
variant's time per step is the whole process minus a one-step run of the same
mission, the median of three runs after a warm-up; variants alternate run by run.
The two Rust builds must write identical frames, events, and summaries.

On macOS, it also records a `sample` profile of the largest NoisyState case for
each variant (the "sort by top of stack" summary) in OUTPUT_DIR/profiles/.
Writes OUTPUT_DIR/timings.json. Standard library only.
"""

import argparse
import datetime
import filecmp
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import time
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "reference"))
import perf  # noqa: E402

NATIVE = ROOT / "runs/native-cpp"
CPP = NATIVE / "build/bin/scrimmage.app/Contents/MacOS/scrimmage"
CPP_ENV = {**os.environ,
           "SCRIMMAGE_PLUGIN_PATH": f"{NATIVE}/build/plugin_libs:{NATIVE}/source/include/scrimmage/plugins",
           "SCRIMMAGE_DATA_PATH": f"{NATIVE}/source/data",
           "SCRIMMAGE_CONFIG_PATH": f"{NATIVE}/source/config",
           "DYLD_LIBRARY_PATH": f"{NATIVE}/build/lib:{NATIVE}/build/plugin_libs"}
AGENTS = (128, 512, 2048, 4096)
END_S = 10  # 100 steps
NOISY_STATE = "    <sensor>NoisyState</sensor>"
RUST_FILES = ("frames.bin", "events.json", "summary.csv")


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


def command(variant, runners, path, output):
    if variant == "cpp":
        return [CPP, path], CPP_ENV
    return [runners[variant], path, ROOT, output, "1"], os.environ


def wall_seconds(variant, runners, path, output):
    shutil.rmtree(output, ignore_errors=True)
    arguments, environment = command(variant, runners, path, output)
    started = time.perf_counter()
    subprocess.run([str(part) for part in arguments], env=environment, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return time.perf_counter() - started


def profile(variant, runners, directory, agents, profiles):
    """Save `sample`'s top-of-stack summary for three seconds of stepping."""
    path = mission(directory / f"profile-{variant}", agents, NOISY_STATE, 120)
    arguments, environment = command(variant, runners, path, directory / f"profile-{variant}/rust")
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


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("old_runner")
    parser.add_argument("new_runner")
    parser.add_argument("output", type=Path)
    parser.add_argument("--old-revision", required=True)
    parser.add_argument("--new-revision", required=True)
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if not CPP.is_file():
        raise SystemExit(f"no native C++ build at {CPP}; run paper/data/benchmark-native/build.sh")
    args.output.mkdir(parents=True)  # never overwrite an earlier benchmark
    runners = {"rust-old": args.old_runner, "rust-new": args.new_runner}
    variants = ["cpp", "rust-old", "rust-new"]
    scratch = args.output / "scratch"
    cases = []
    for agents in AGENTS:
        for label, sensors in (("without messages", ""), ("NoisyState", NOISY_STATE)):
            directory = scratch / f"{agents}-{'state' if sensors else 'plain'}"
            full = mission(directory / "full", agents, sensors, END_S)
            one = mission(directory / "one-step", agents, sensors, 0.1)
            samples = {variant: {"full": [], "one": []} for variant in variants}
            for repeat in range(args.repeats + 1):
                shift = repeat % len(variants)
                for variant in variants[shift:] + variants[:shift]:
                    for kind, path in (("full", full), ("one", one)):
                        seconds = wall_seconds(variant, runners, path, directory / f"{variant}-{kind}")
                        if repeat:  # the first round is a warm-up
                            samples[variant][kind].append(seconds)
            for file in RUST_FILES:
                if not filecmp.cmp(directory / f"rust-old-full/{file}",
                                   directory / f"rust-new-full/{file}", shallow=False):
                    raise SystemExit(f"{agents} {label}: Rust builds wrote different {file}")
            steps = round(END_S / 0.1)
            case = {"agents": agents, "case": label, "variants": {}}
            for variant, runs in samples.items():
                per_step_ms = (statistics.median(runs["full"]) - statistics.median(runs["one"])) \
                    / (steps - 1) * 1e3
                case["variants"][variant] = {"ms_per_step": per_step_ms, "full_s": runs["full"],
                                             "one_step_s": runs["one"]}
            cases.append(case)
            print(agents, label, "  ".join(f"{v} {case['variants'][v]['ms_per_step']:.2f}"
                                           for v in variants), "ms/step", flush=True)
            shutil.rmtree(directory)

    profiled = shutil.which("sample") is not None
    if profiled:
        profiles = args.output / "profiles"
        profiles.mkdir()
        for variant in variants:
            profile(variant, runners, scratch, max(AGENTS) // 2, profiles)
    shutil.rmtree(scratch, ignore_errors=True)
    (args.output / "timings.json").write_text(json.dumps({
        "date": datetime.datetime.now().isoformat(timespec="seconds"),
        "machine": perf.machine(), "platform": platform.platform(),
        "old_revision": args.old_revision, "new_revision": args.new_revision,
        "cpp": "native build of Ubuntu-24.04 (paper/data/benchmark-native/build.sh)",
        "repeats": args.repeats, "steps": round(END_S / 0.1),
        "timing": "whole process minus a one-step run, median of repeats after a warm-up",
        "profiles": f"{max(AGENTS) // 2} agents with NoisyState" if profiled else None,
        "note": "The two Rust builds wrote identical frames, events, and summaries.",
        "cases": cases,
    }, indent=1) + "\n")


if __name__ == "__main__":
    main()
