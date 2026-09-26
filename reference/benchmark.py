#!/usr/bin/env python3
"""Time C++ against Rust in one Linux container for the paper.

    python3 reference/benchmark.py --output runs/new-benchmark
    python3 reference/benchmark.py --output runs/quick-benchmark --quick

Builds the unmodified C++ branch (Ubuntu-24.04) and the Rust core-only runner
(reference/benchmark_runner.rs, no viewer) for this machine's own architecture,
so neither is emulated. On Apple Silicon that is linux/arm64 in Docker's VM, and
C++ is built without JSBSim, which no workload uses.

Runs perf.py's workloads (motion, substeps, sensing, churn, collision) at three agent counts
with C++ single-threaded, C++ with its own multi_threaded mode at 8 threads, and
Rust at 1 and 8 workers: one warm-up, then --repetitions timed runs each. Times
are whole processes minus the median of three one-step runs of the same mission
(startup, parsing, and plugin loading), so they approximate stepping and logging.
Peak memory is each simulator's maximum resident set, from GNU time. A timed
run's output is deleted once it matches the warm-up's, and each build replaces
the previous benchmark image, so repeated benchmarks don't fill the disk.

Rust must write identical output (frames, summary, events) at 1 and 8 workers.
It must also match single-threaded C++ except in churn, sensing, and collision,
which depend on random spawn positions and sensor noise that the unmodified C++
build draws differently (the reference check matches them with its comparison build).
Every timed run must repeat its warm-up output; C++'s multithreaded mode is
reported, not required, to do so.
That mode sometimes deadlocks (SimControl::worker waits on its condition variable
without a predicate, so a wake-up can be lost): a C++ 8-thread run still going
after HANG_S is killed, counted, and retried. A hang in any other variant fails.
The paper's retained results are in paper/data/benchmark/.
"""
import argparse
from dataclasses import asdict
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import statistics
import subprocess
import threading
import time
from types import SimpleNamespace
import xml.etree.ElementTree as ET

from compare import compare_runs
from perf import HEADER, WORKLOADS
from reference_check import BASELINE_BRANCH, ROOT, build_reference, capture, run_logged, sha256, \
    write_json

COUNTS = {"motion": (64, 256, 1024), "substeps": (64, 256, 1024),
          "sensing": (32, 64, 128), "churn": (64, 256, 1024), "collision": (256, 1024, 2048)}
VARIANTS = ("cpp-1", "cpp-8", "rust-1", "rust-8")
HANG_S = 120  # far longer than any run here takes
HANG_RETRIES = 5
RUST_FILES = ("frames.bin", "summary.csv", "events.json")
RANDOM = {"churn", "sensing", "collision"}  # random spawns; noisy state that Straight steers by


def mission(name, agents, directory, variant, one_step=False):
    """The perf.py workload with C++ logging and threading set for one run."""
    _, _, fields = WORKLOADS[name]
    fields = {**fields, "end": 0.1} if one_step else fields
    root = ET.fromstring(HEADER.format(name=name, agents=agents, **fields))
    settings = {"log_dir": directory / "logs", "create_latest_dir": "false",
                "display_progress": "false", "output_type": "all", "no_bin_logging": "false",
                "multi_threaded": "true" if variant == "cpp-8" else "false"}
    for key, value in settings.items():
        ET.SubElement(root, key).text = str(value)
    if variant == "cpp-8":
        root.find("multi_threaded").set("num_threads", "8")
    path = directory / "mission.xml"
    ET.ElementTree(root).write(path, encoding="utf-8", xml_declaration=True)
    return path


def run(variant, mission_path, directory):
    """(wall seconds, peak resident MB) for one whole process, or None if it hung."""
    if variant.startswith("cpp"):
        command = ["/opt/build/bin/scrimmage", mission_path]
        environment = os.environ.copy()
    else:
        command = ["/usr/local/bin/scrimmage-benchmark", mission_path, ROOT,
                   directory / "rust", variant.split("-")[1]]
        environment = {**os.environ, "SCRIMMAGE_PLUGIN_PATH": ""}  # Rust uses its bundled defaults.
    peak = directory / "peak-kb.txt"
    command = ["/usr/bin/time", "-f", "%M", "-o", peak] + command
    with (directory / "console.log").open("w") as console:
        started = time.perf_counter()
        process = subprocess.Popen([str(part) for part in command], stdout=console,
                                   stderr=subprocess.STDOUT, env=environment,
                                   start_new_session=True)
        watchdog = threading.Timer(HANG_S, os.killpg, (process.pid, signal.SIGKILL))
        watchdog.start()
        status = process.wait()
        elapsed = time.perf_counter() - started
        watchdog.cancel()
    if elapsed >= HANG_S:
        return None
    if status != 0:
        raise RuntimeError(f"{variant} failed in {directory}; see console.log")
    return elapsed, int(peak.read_text().split()[-1]) / 1024


def run_retrying(variant, name, agents, directory, hangs, one_step=False):
    """Run once; retry only C++'s multithreaded mode when it deadlocks."""
    for attempt in range(HANG_RETRIES + 1):
        attempt_directory = directory / f"attempt-{attempt}"
        attempt_directory.mkdir(parents=True)
        result = run(variant, mission(name, agents, attempt_directory, variant, one_step),
                     attempt_directory)
        if result is not None:
            return result, attempt_directory
        hangs[variant] += 1
        if variant != "cpp-8":
            break
    raise RuntimeError(f"{variant} hung in {directory}")


def output_folder(directory):
    frames = list(directory.rglob("frames.bin"))
    if len(frames) != 1:
        raise RuntimeError(f"expected one frames.bin in {directory}")
    return frames[0].parent


def same_output(left, right, names=("frames.bin", "summary.csv")):
    return all(sha256(left / name) == sha256(right / name) for name in names)


def measure(name, agents, case, repetitions):
    samples = {variant: [] for variant in VARIANTS}
    memory = {variant: 0.0 for variant in VARIANTS}
    startup = {}
    warmups, repeatable = {}, {variant: True for variant in VARIANTS}
    hangs = {variant: 0 for variant in VARIANTS}
    for variant in VARIANTS:
        times = [run_retrying(variant, name, agents, case / f"startup-{index}-{variant}", hangs,
                              one_step=True)[0][0] for index in range(3)]
        startup[variant] = statistics.median(times)
    for repetition in range(repetitions + 1):
        # One untimed warm-up each; rotate the order to reduce ordering bias.
        shift = repetition % len(VARIANTS)
        for variant in VARIANTS[shift:] + VARIANTS[:shift]:
            (elapsed, peak_mb), directory = run_retrying(
                variant, name, agents, case / f"{repetition}-{variant}", hangs)
            output = output_folder(directory)
            memory[variant] = max(memory[variant], peak_mb)
            if repetition == 0:
                warmups[variant] = output
                continue
            samples[variant].append(elapsed - startup[variant])
            if same_output(output, warmups[variant]):
                # Only the warm-up's output is kept; a matching repeat adds nothing.
                shutil.rmtree(case / f"{repetition}-{variant}")
            else:
                repeatable[variant] = False
            print(f"{name}-{agents} {variant} {repetition}: {elapsed:.3f}s", flush=True)
    statistics_by_variant = {
        variant: {"samples_s": values, "median_s": statistics.median(values),
                  "min_s": min(values), "max_s": max(values),
                  "startup_s": startup[variant], "peak_mb": memory[variant]}
        for variant, values in samples.items()}
    comparisons = {variant: asdict(compare_runs(warmups["cpp-1"], warmups[variant]))
                   for variant in VARIANTS[1:]}
    rust_workers_match = same_output(warmups["rust-1"], warmups["rust-8"], RUST_FILES)
    return {"workload": name, "agents": agents, "steps": round(WORKLOADS[name][2]["end"] / 0.1),
            "statistics": statistics_by_variant, "repeatable": repeatable, "hangs": hangs,
            "comparisons": comparisons, "rust_workers_match": rust_workers_match,
            "matches_cpp": name not in RANDOM,
            "passed": case_passed(name, repeatable, comparisons, rust_workers_match)}


def case_passed(name, repeatable, comparisons, rust_workers_match):
    """Rust must be deterministic across worker counts; it must match C++ where both
    draw the same random numbers. C++'s multithreaded mode is reported only."""
    required = [repeatable["cpp-1"], repeatable["rust-1"], repeatable["rust-8"], rust_workers_match]
    if name not in RANDOM:
        required += [comparisons["rust-1"]["passed"], comparisons["rust-8"]["passed"]]
    return all(required)


def inside(args):
    output = Path("/run")
    result = {"platform": platform.platform(), "machine": platform.machine(),
              "cpu_count": os.cpu_count(), "rust_compiler": Path("/rust-compiler.txt").read_text(),
              "cpp_compiler": capture(["g++", "--version"]), "repetitions": args.repetitions,
              "timing": "whole process minus the median one-step run; container startup excluded",
              "cases": [], "passed": False}
    for name, counts in COUNTS.items():
        for agents in counts[:1] if args.quick else counts:
            case = output / f"{name}-{agents}"
            case.mkdir()
            result["cases"].append(measure(name, agents, case, args.repetitions))
            write_json(output / "timings.json", result)
    result["passed"] = all(case["passed"] for case in result["cases"])
    write_json(output / "timings.json", result)
    return 0 if result["passed"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--quick", action="store_true", help="smallest agent count only")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--source", type=Path, default=ROOT.parent / "scrimmage")
    args = parser.parse_args()
    if args.inside:
        return inside(args)
    if args.output is None:
        parser.error("--output must name a new directory")
    output = args.output.resolve()
    output.mkdir(parents=True)  # Never overwrite an earlier benchmark.
    target = "linux/arm64" if platform.machine() in ("arm64", "aarch64") else "linux/amd64"
    build = SimpleNamespace(source=args.source.resolve(), jobs=os.cpu_count(),
                            build_timeout=3600, timeout=600)
    cpp = build_reference(build, output, BASELINE_BRANCH, target)
    run_logged(["docker", "build", "--platform", target, "--progress", "plain",
                "-f", ROOT / "reference/rust.Dockerfile", "--target", "benchmark",
                "--build-arg", f"REFERENCE_IMAGE={cpp['image_tag']}",
                "--tag", f"scrimmage-rs-benchmark:{target.split('/')[1]}",
                "--iidfile", output / "image-id.txt", ROOT], output / "build.log", timeout=3600)
    # The new build took the tag; delete earlier benchmark images (labelled, now untagged).
    run_logged(["docker", "image", "prune", "--force", "--filter", "label=scrimmage-rs-benchmark"],
               output / "prune.log", timeout=600)
    image = (output / "image-id.txt").read_text().strip()
    files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock"] + list((ROOT / "crates").rglob("*.rs"))
    files += list((ROOT / "crates").rglob("*.xml")) + list((ROOT / "crates").rglob("Cargo.toml"))
    files += list((ROOT / "reference").glob("benchmark*")) + [ROOT / "reference/rust.Dockerfile",
                                                              ROOT / "reference/perf.py"]
    write_json(output / "provenance.json", {
        "cpp": cpp, "benchmark_image": image, "platform": target,
        "host": platform.platform(), "host_machine": platform.machine(),
        "docker_resources": capture(["docker", "info", "--format", "{{.NCPU}} CPUs; {{.MemTotal}} bytes RAM"]),
        "rust_head": capture(["git", "rev-parse", "HEAD"]),
        "rust_worktree": capture(["git", "status", "--short"]),
        "source_hashes": {str(p.relative_to(ROOT)): sha256(p) for p in sorted(files) if p.is_file()},
    })
    command = ["docker", "run", "--rm", "--network", "none", "--platform", target,
               "--env", "PYTHONDONTWRITEBYTECODE=1",
               "--mount", f"type=bind,source={ROOT},target=/rust,readonly",
               "--mount", f"type=bind,source={output},target=/run", image,
               "--repetitions", args.repetitions] + (["--quick"] if args.quick else [])
    return run_logged(command, output / "benchmark.log", timeout=4 * 3600, allow_failure=True)


if __name__ == "__main__":
    raise SystemExit(main())
