#!/usr/bin/env python3
"""Time SCRIMMAGE and Ripple in one Linux container.

    python3 reference/benchmark.py --output DIR [--quick]

The C++ comparison branch and Rust's core-only runner use the host's
architecture inside Docker, so neither is emulated. Both run the same
workloads at three fleet sizes: C++ at 1 and 8 threads, Rust at 1 and 8
workers. Timed process durations subtract a one-step startup measurement;
peak memory is reported separately.

Rust output must be identical across worker counts and match serial C++ on
workloads with shared inputs. C++ multithreaded output is reported but not
required to repeat: its worker can deadlock, so that variant is timed out,
counted, and retried. Writes timing and provenance reports; retained results
are in paper/data/speed-cpp-container/.
"""
import argparse
from dataclasses import asdict
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
from perf import WORKLOADS, workload, write_mission
from project import IMAGE_PREFIX, ROOT, sha256, source_identity
from reference_check import INSTRUMENTED_BRANCH, build_reference, capture, run_logged, write_json
import scenario as missions
import scrimmage_xml

COUNTS = {"motion": (64, 256, 1024), "substeps": (64, 256, 1024),
          "sensing": (32, 64, 128), "churn": (64, 256, 1024), "collision": (256, 1024, 2048)}
VARIANTS = ("cpp-1", "cpp-8", "rust-1", "rust-8")
HANG_S = 120  # far longer than any run here takes
HANG_RETRIES = 5
RUST_FILES = ("run.mcap",)
RANDOM = {"churn", "sensing", "collision"}  # random spawns; noisy state that Straight steers by


CLI = "/usr/local/bin/ripple"


def mission(name, agents, directory, variant, one_step=False):
    """The perf.py workload: YAML for Rust, or XML with C++ logging and threading set."""
    changes = {"end": 0.1} if one_step else {}
    path = write_mission(directory / "mission.yaml", workload(name, agents, **changes))
    if variant.startswith("rust"):
        return path
    root = scrimmage_xml.mission(missions.loaded(CLI, path, directory / "loaded"))
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
        command = ["/usr/local/bin/ripple-benchmark", mission_path, ROOT,
                   directory / "rust", variant.split("-")[1]]
        environment = os.environ.copy()
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
    outputs = list(directory.rglob("run.mcap")) + list(directory.rglob("frames.bin"))
    if len(outputs) != 1:
        raise RuntimeError(f"expected one output in {directory}")
    return outputs[0].parent


def same_output(left, right, names=("frames.bin", "summary.csv")):
    if (left / "run.mcap").exists():
        names = RUST_FILES
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
    cpp = build_reference(build, output, INSTRUMENTED_BRANCH, target)
    run_logged(["docker", "build", "--platform", target, "--progress", "plain",
                "-f", ROOT / "reference/rust.Dockerfile", "--target", "benchmark",
                "--build-arg", f"REFERENCE_IMAGE={cpp['image_tag']}",
                "--tag", f"{IMAGE_PREFIX}-benchmark:{target.split('/')[1]}",
                "--iidfile", output / "image-id.txt", ROOT], output / "build.log", timeout=3600)
    # The new build took the tag; delete earlier benchmark images (labelled, now untagged).
    run_logged(["docker", "image", "prune", "--force", "--filter", f"label={IMAGE_PREFIX}-benchmark"],
               output / "prune.log", timeout=600)
    image = (output / "image-id.txt").read_text().strip()
    identity = source_identity()
    write_json(output / "provenance.json", {
        "cpp": cpp, "benchmark_image": image, "platform": target,
        "host": platform.platform(), "host_machine": platform.machine(),
        "docker_resources": capture(["docker", "info", "--format", "{{.NCPU}} CPUs; {{.MemTotal}} bytes RAM"]),
        "source_sha256": identity["source_sha256"], "source_hashes": identity["files"],
    })
    command = ["docker", "run", "--rm", "--network", "none", "--platform", target,
               "--env", "PYTHONDONTWRITEBYTECODE=1",
               "--mount", f"type=bind,source={ROOT},target=/rust,readonly",
               "--mount", f"type=bind,source={output},target=/run", image,
               "--repetitions", args.repetitions] + (["--quick"] if args.quick else [])
    return run_logged(command, output / "benchmark.log", timeout=4 * 3600, allow_failure=True)


if __name__ == "__main__":
    raise SystemExit(main())
