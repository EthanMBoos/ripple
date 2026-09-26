#!/usr/bin/env python3
"""Same-container Release benchmark. Run reference_check.py first to build C++.

Measures whole headless processes including startup and logging, NOT pure physics.
C++ is always single-threaded. Rust uses a core-only harness (no Rerun/CLI parsing).

Run from the repository root: python3 reference/benchmark.py --output runs/new-benchmark
Defaults: 128 aircraft, 1,000 steps, one warmup and three measured runs per variant.
On Apple Silicon both Release executables run under amd64 emulation in the same
container. Startup, parsing and logging differ; this is not native CPU throughput
or a general language-speed comparison. Builds/container startup are not timed.

The paper's retained results are in paper/data/benchmark/.
"""
import argparse
from dataclasses import asdict
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import sys
import time
import xml.etree.ElementTree as ET

from compare import compare_runs
from reference_check import ROOT, PLATFORM, capture, positive_int, run_logged, sha256, write_json


def set_text(root, name, value):
    node = root.find(name)
    if node is None:
        node = ET.SubElement(root, name)
    node.text = str(value)


def mission_for_benchmark(source, entities, steps):
    text = re.sub(r"\$\{\w+=(.*?)\}", r"\1", source.read_text())
    if "${" in text:
        raise ValueError("benchmark requires mission variables with defaults")
    root = ET.fromstring(text)
    run = root.find("run")
    run.attrib.update(start="0", end=str(steps * float(run.get("dt"))),
                      enable_gui="false", network_gui="false", start_paused="false", time_warp="0")
    set_text(root, "multi_threaded", "false")
    set_text(root, "display_progress", "false")
    set_text(root, "create_latest_dir", "false")
    set_text(root, "end_condition", "time")
    blocks = [node for node in root.findall("entity") if int(node.findtext("count", "1")) > 0]
    if entities < len(blocks):
        raise ValueError("entity count must cover every active source block")
    for index, node in enumerate(blocks):
        set_text(node, "count", entities // len(blocks) + (index < entities % len(blocks)))
    return root


def inside(args):
    output = Path("/run")
    environment = os.environ.copy()
    environment["SCRIMMAGE_PLUGIN_PATH"] = ""  # Rust uses its own bundled defaults.
    result = {"platform": platform.platform(), "rust_compiler": Path("/rust-compiler.txt").read_text(),
              "cpp_compiler": capture(["g++", "--version"]), "cases": [], "passed": False,
              "timing": "process startup, mission parsing, simulation and output; container startup excluded",
              "entities": args.entities, "steps": args.steps, "repetitions": args.repetitions}
    write_json(output / "timings.json", result)
    for source in [ROOT / "missions/test_missions/straight_cpu.xml", ROOT / "missions/fixed-wing-6dof.xml"]:
        case = output / source.stem
        case.mkdir()
        mission = mission_for_benchmark(source, args.entities, args.steps)
        timings = {"cpp-1": [], "rust-1": [], "rust-8": []}
        warmups = {}
        variants = list(timings)
        for repetition in range(args.repetitions + 1):
            # One untimed warmup each. Rotate measured order to reduce ordering bias.
            order = variants[repetition % 3:] + variants[:repetition % 3]
            for variant in order:
                directory = case / f"{repetition}-{variant}"
                directory.mkdir()
                set_text(mission, "log_dir", str(directory / "logs"))
                mission_path = directory / "mission.xml"
                ET.ElementTree(mission).write(mission_path, encoding="utf-8", xml_declaration=True)
                if variant == "cpp-1":
                    command = ["/opt/build/bin/scrimmage", str(mission_path)]
                    env = os.environ.copy()
                else:
                    command = ["/usr/local/bin/scrimmage-benchmark", str(mission_path),
                               str(ROOT), str(directory / "rust"), variant.split("-")[1]]
                    env = environment
                with (directory / "console.log").open("w") as console:
                    started = time.perf_counter()
                    subprocess.run(command, stdout=console, stderr=subprocess.STDOUT,
                                   env=env, check=True, timeout=600)
                    elapsed = time.perf_counter() - started
                frames = list(directory.rglob("frames.bin"))
                if len(frames) != 1:
                    raise RuntimeError(f"expected one frames.bin in {directory}")
                if repetition == 0:
                    warmups[variant] = frames[0].parent
                else:
                    timings[variant].append(elapsed)
                    # Repeated output must be deterministic, not a faster aborted run.
                    for name in ("frames.bin", "summary.csv"):
                        if sha256(frames[0].parent / name) != sha256(warmups[variant] / name):
                            raise RuntimeError(f"non-repeatable {variant} {name}")
                print(f"{source.stem} {variant} repetition={repetition}: {elapsed:.6f}s", flush=True)
        comparisons = {variant: asdict(compare_runs(warmups["cpp-1"], warmups[variant]))
                       for variant in ("rust-1", "rust-8")}
        stats = {variant: {"samples_s": samples, "median_s": statistics.median(samples),
                           "min_s": min(samples), "max_s": max(samples)}
                 for variant, samples in timings.items()}
        for variant in ("rust-1", "rust-8"):
            stats[variant]["cpp_over_rust"] = stats["cpp-1"]["median_s"] / stats[variant]["median_s"]
        result["cases"].append({"mission": source.name, "statistics": stats, "comparisons": comparisons})
        write_json(output / "timings.json", result)
    result["passed"] = all(c["passed"] for case in result["cases"] for c in case["comparisons"].values())
    write_json(output / "timings.json", result)
    return 0 if result["passed"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--entities", type=positive_int, default=128)
    parser.add_argument("--steps", type=positive_int, default=1000)
    parser.add_argument("--repetitions", type=positive_int, default=3)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.inside:
        return inside(args)
    if args.output is None:
        parser.error("--output must name a new directory")
    output = args.output.resolve()
    output.mkdir(parents=True)  # Never overwrite an earlier benchmark.
    commit = capture(["git", "-C", str(ROOT.parent / "scrimmage"), "rev-parse", "Ubuntu-24.04"])
    reference_tag = f"scrimmage-rs-reference:{commit[:12]}"
    reference_image = capture(["docker", "image", "inspect", reference_tag,
                               "--format", "{{.Id}}"])
    run_logged(["docker", "build", "--platform", PLATFORM, "--progress", "plain",
                "-f", ROOT / "reference/rust.Dockerfile", "--target", "benchmark", "--build-arg", f"REFERENCE_IMAGE={reference_tag}",
                "--iidfile", output / "image-id.txt", ROOT], output / "build.log", timeout=3600)
    image = (output / "image-id.txt").read_text().strip()
    files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock"] + list((ROOT / "crates").rglob("*.rs"))
    files += list((ROOT / "crates").rglob("*.xml")) + list((ROOT / "crates").rglob("Cargo.toml"))
    files += list((ROOT / "reference").glob("benchmark*")) + [ROOT / "reference/rust.Dockerfile"]
    write_json(output / "provenance.json", {
        "cpp_commit": commit, "reference_image": reference_image, "benchmark_image": image,
        "host": platform.platform(), "host_machine": platform.machine(), "platform": PLATFORM,
        "docker_resources": capture(["docker", "info", "--format", "{{.NCPU}} CPUs; {{.MemTotal}} bytes RAM"]),
        "rust_head": capture(["git", "rev-parse", "HEAD"]),
        "rust_worktree": capture(["git", "status", "--short"]),
        "source_hashes": {str(p.relative_to(ROOT)): sha256(p) for p in sorted(files) if p.is_file()},
    })
    return run_logged(["docker", "run", "--rm", "--network", "none", "--platform", PLATFORM,
                       "--env", "PYTHONDONTWRITEBYTECODE=1",
                       "--mount", f"type=bind,source={ROOT},target=/rust,readonly",
                       "--mount", f"type=bind,source={output},target=/run", image,
                       "--entities", args.entities, "--steps", args.steps, "--repetitions", args.repetitions],
                      output / "benchmark.log", timeout=3600, allow_failure=True)


if __name__ == "__main__":
    raise SystemExit(main())
