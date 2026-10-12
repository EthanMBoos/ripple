#!/usr/bin/env python3
"""Bare-metal macOS run of benchmark.py's workloads against the native C++ build.

    python3 reference/native_benchmark.py --output runs/native-benchmark000

Uses benchmark.py's missions, checks, and statistics, but runs the native C++
build (native_build.sh, built on first use) and the Rust runner directly on the
host instead of in a container. Writes timings.json; exits non-zero if any
required check fails. macOS only.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import sys
import threading
import time

import benchmark
import native
from project import ROOT, build_runner


def run(variant, mission_path, directory, runner):
    if variant.startswith("cpp"):
        command, env = [native.CPP, mission_path], native.CPP_ENV
    else:
        command, env = [runner, mission_path, ROOT, directory / "rust", variant.split("-")[1]], os.environ
    with (directory / "console.log").open("w") as console:
        started = time.perf_counter()
        process = subprocess.Popen([str(p) for p in command], stdout=console, stderr=subprocess.STDOUT,
                                   env=env, start_new_session=True)
        watchdog = threading.Timer(benchmark.HANG_S, os.killpg, (process.pid, signal.SIGKILL))
        watchdog.start()
        _, status, usage = os.wait4(process.pid, 0)
        elapsed = time.perf_counter() - started
        watchdog.cancel()
    if elapsed >= benchmark.HANG_S:
        return None
    if status != 0:
        raise RuntimeError(f"{variant} failed in {directory}")
    return elapsed, usage.ru_maxrss / 2**20  # bytes on macOS; MiB like benchmark.py


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    parser.add_argument("--repetitions", type=int, default=3)
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True)  # Never overwrite an earlier benchmark.
    with (output / "cpp-build.log").open("w") as log:
        native.ensure_build(log)
    runner = build_runner()
    benchmark.run = lambda variant, mission_path, directory: run(variant, mission_path, directory, runner)
    result = {"platform": platform.platform(), "machine": platform.machine(),
              "cpu_count": os.cpu_count(), "repetitions": args.repetitions,
              "provenance": native.provenance(),
              "timing": "bare-metal macOS; whole process minus the median one-step run", "cases": []}
    for name, counts in benchmark.COUNTS.items():
        for agents in counts:
            case = output / f"{name}-{agents}"
            case.mkdir()
            result["cases"].append(benchmark.measure(name, agents, case, args.repetitions))
            (output / "timings.json").write_text(json.dumps(result, indent=2))
    result["passed"] = all(case["passed"] for case in result["cases"])
    (output / "timings.json").write_text(json.dumps(result, indent=2))
    print("passed", result["passed"])
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
