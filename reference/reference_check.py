#!/usr/bin/env python3
"""Build the C++ Docker reference and compare the current Rust port.

Builds two C++ branches from the sibling checkout: the unmodified baseline
(Ubuntu-24.04) and the opt-in instrumentation branch (benchmarking-edits).
Per mission, the baseline, the instrumented build, and the instrumented build with
tracing must write byte-identical frames and summaries (non-interference). The
comparison run adds SCRIMMAGE_LIBCXX_SPAWN_RANDOM=1 and is compared with Rust
frames, summaries, and traces. Requires Python 3.10+, Docker, Cargo, and the
sibling SCRIMMAGE checkout. No host C++ toolchain is needed.
"""

import argparse
from dataclasses import asdict
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET

from compare import Comparison, compare_frames, compare_summaries
from metrics_model import counts_match, matches, predict, weights
from frames import read_frames
from traces import compare_traces, read_trace


ROOT = Path(__file__).resolve().parents[1]
BASELINE_BRANCH = "Ubuntu-24.04"
INSTRUMENTED_BRANCH = "benchmarking-edits"
# Instrumented-build environments. Non-interference runs must match the baseline.
UNTRACED_ENV = {}
TRACED_ENV = {"SCRIMMAGE_TRACE": "/run/trace.jsonl"}
COMPARISON_ENV = {**TRACED_ENV, "SCRIMMAGE_LIBCXX_SPAWN_RANDOM": "1"}
CPP_OUTPUT_FILES = ("frames.bin", "summary.csv")
PLATFORM = "linux/amd64"  # Upstream install-jsbsim.sh downloads amd64 .deb files.
DEPENDENCY_DOCKERFILE = "ci/dockerfiles/ubuntu-24.04-slim-dependency-only"


def comparison_missions():
    """(path under missions/, label) pairs from missions.txt."""
    missions = []
    for line in (ROOT / "reference/missions.txt").read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            path, label = (part.strip() for part in line.split("|", 1))
            missions.append((path, label))
    return missions


OUTPUT_FILES = ("frames.bin", "events.json", "summary.csv")



def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n")


def capture(command, cwd=ROOT):
    return subprocess.check_output(command, cwd=cwd, text=True).strip()


def run_logged(command, log, *, timeout, cwd=ROOT, allow_failure=False):
    """Retain full logs and print a useful tail on failure, including timeouts."""
    command = [str(argument) for argument in command]
    print(f"  {Path(command[0]).name}: {log}", flush=True)
    with Path(log).open("w") as stream:
        stream.write(json.dumps(command) + "\n")
        stream.flush()
        try:
            result = subprocess.run(
                command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT,
                timeout=timeout, check=False,
            )
        except subprocess.TimeoutExpired as error:
            raise RuntimeError(f"timed out after {timeout}s; inspect {log}") from error
    if result.returncode and not allow_failure:
        tail = "\n".join(Path(log).read_text(errors="replace").splitlines()[-25:])
        raise RuntimeError(f"command failed ({result.returncode}); {log}\n{tail}")
    return result.returncode


def allocate_output(explicit, runs):
    if explicit is not None:
        explicit = explicit.resolve()
        explicit.parent.mkdir(parents=True, exist_ok=True)
        explicit.mkdir()  # Never reuse an earlier check, even an empty one.
        return explicit
    runs.mkdir(parents=True, exist_ok=True)
    numbers = [int(p.name[15:]) for p in runs.iterdir()
               if p.name.startswith("reference-check") and p.name[15:].isdigit()]
    number = max(numbers, default=-1) + 1
    while True:
        output = runs / f"reference-check{number:03}"
        try:
            output.mkdir()
            return output.resolve()
        except FileExistsError:
            number += 1


def prepare_mission(mission, cpp):
    """Keep physical inputs unchanged; force the C++ reference headless/serial."""
    cpp.mkdir()
    shutil.copyfile(mission, cpp / "input.xml")
    tree = ET.parse(mission)
    root = tree.getroot()
    if root.tag != "runscript":
        raise ValueError("expected a runscript mission")
    if any(node.tag.rsplit("}", 1)[-1] == "include" for node in root.iter()):
        raise ValueError("reference_check requires an expanded mission (XIncludes unsupported)")
    run = root.find("run")
    if run is None:
        raise ValueError("mission has no run element")
    run_overrides = {
        "enable_gui": "false", "network_gui": "false",
        "start_paused": "false", "time_warp": "0",
    }
    root_overrides = {
        "log_dir": "/run/logs", "create_latest_dir": "false",
        "multi_threaded": "false", "display_progress": "false",
        # Frames are written only with full logging; this changes no physics.
        "output_type": "all", "no_bin_logging": "false",
    }
    run.attrib.update(run_overrides)
    for key, value in root_overrides.items():
        nodes = root.findall(key)
        if not nodes:
            nodes = [ET.SubElement(root, key)]
        for node in nodes:
            node.text = value
    effective = cpp / "effective.xml"
    tree.write(effective, encoding="utf-8", xml_declaration=True)
    return {
        "input_sha256": sha256(cpp / "input.xml"),
        "effective_sha256": sha256(effective),
        "run_overrides": run_overrides, "root_overrides": root_overrides,
    }


def same_outputs(left, right):
    return {name: sha256(left / name) == sha256(right / name) for name in OUTPUT_FILES}


def edit(text, old, new):
    if old not in text:
        raise RuntimeError(f"expected {old!r} in a Dockerfile; update build_reference")
    return text.replace(old, new)


def build_reference(args, output, branch, target_platform=PLATFORM):
    """Build one committed branch; outputs go in build-<branch>/ under the check.

    Another platform (benchmark.py on Apple Silicon) builds without JSBSim, whose
    installer is amd64-only; no compared or benchmarked mission uses it.
    """
    commit = capture(["git", "-C", str(args.source), "rev-parse", f"refs/heads/{branch}^{{commit}}"])
    native = target_platform != PLATFORM
    logs = output / f"build-{branch}"
    context = logs / "context"
    context.mkdir(parents=True)
    dependency_file = context / "Dependencies.Dockerfile"
    dependency_text = subprocess.check_output([
        "git", "-C", str(args.source), "show", f"{commit}:{DEPENDENCY_DOCKERFILE}",
    ], text=True)
    build_text = (ROOT / "reference/Dockerfile").read_text()
    if native:
        dependency_text = edit(dependency_text, " && ./setup/install-jsbsim.sh", "")
        build_text = edit(build_text, "-DENABLE_JSBSIM=1", "-DENABLE_JSBSIM=0")
    dependency_file.write_text(dependency_text)
    # Archive only committed branch files; the sibling checkout is never modified.
    with (context / "source.tar").open("wb") as archive:
        subprocess.run([
            "git", "-C", str(args.source), "archive", "--format=tar", "--prefix=source/", commit,
        ], stdout=archive, check=True)
    (context / "Dockerfile").write_text(build_text)
    # Only the constant-command drivers are ours; the models remain upstream code.
    shutil.copytree(ROOT / "reference/fixtures", context / "fixtures")
    fixture_hashes = {path.name: sha256(path) for path in sorted((context / "fixtures").iterdir())}
    suffix = "-" + target_platform.split("/")[1] if native else ""
    dependency_tag = f"scrimmage-rs-reference-deps:{sha256(dependency_file)[:12]}{suffix}"
    image_tag = f"scrimmage-rs-reference:{commit[:12]}{suffix}"
    dependency_id = logs / "dependencies-image-id.txt"
    image_id = logs / "reference-image-id.txt"
    common = ["docker", "build", "--platform", target_platform, "--progress", "plain"]
    run_logged(common + [
        "--file", dependency_file, "--tag", dependency_tag,
        "--iidfile", dependency_id, context,
    ], logs / "dependencies-build.log", timeout=args.build_timeout)
    run_logged(common + [
        "--file", context / "Dockerfile", "--tag", image_tag,
        "--build-arg", f"DEPENDENCY_IMAGE={dependency_tag}",
        "--build-arg", f"SOURCE_COMMIT={commit}",
        "--build-arg", f"BUILD_JOBS={args.jobs}", "--iidfile", image_id, context,
    ], logs / "reference-build.log", timeout=args.build_timeout)
    image = image_id.read_text().strip()
    inspect = json.loads(capture(["docker", "image", "inspect", image]))[0]
    run_logged([
        "docker", "run", "--rm", "--network", "none", "--platform", target_platform,
        "--entrypoint", "/bin/sh", image, "-c",
        "g++ --version && uname -m && cat /etc/os-release && dpkg-query -W",
    ], logs / "cpp-environment.log", timeout=args.timeout)
    info = {
        "branch": branch, "commit": commit, "source_checkout": str(args.source),
        "source_policy": "committed local branch snapshot; no worktree edits",
        "source_archive_sha256": sha256(context / "source.tar"),
        "dependency_dockerfile_sha256": sha256(dependency_file),
        "dependency_image": dependency_id.read_text().strip(),
        "build_dockerfile_sha256": sha256(context / "Dockerfile"),
        "image": image, "image_tag": image_tag, "platform": target_platform,
        "fixture_sha256": fixture_hashes,
        "image_architecture": inspect["Architecture"],
    }
    write_json(logs / "source.json", info)
    return info


def build_rust(args, output):
    run_logged([
        "cargo", "build", "--locked", "--package", "scrimmage-rs", "--bin", "scrimmage",
    ], output / "rust-build.log", timeout=args.build_timeout)
    metadata = json.loads(capture(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
    executable = "scrimmage.exe" if os.name == "nt" else "scrimmage"
    binary = Path(metadata["target_directory"]) / "debug" / executable
    files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock"]
    files += [path for path in (ROOT / "crates").rglob("*")
              if path.is_file() and path.suffix in {".rs", ".xml", ".toml"}]
    return binary, {
        "binary": str(binary), "binary_sha256": sha256(binary),
        "compiler": capture(["rustc", "--version", "--verbose"]),
        "host": platform.platform(),
        "source_hashes": {str(path.relative_to(ROOT)): sha256(path) for path in sorted(files)},
    }


def run_cpp(args, image, directory, effective, environment):
    """Run one headless C++ mission; only `directory` is writable inside the container."""
    directory.mkdir()
    shutil.copyfile(effective, directory / "effective.xml")
    # A unique name permits cleanup if the Docker client times out.
    container = f"scrimmage-check-{os.getpid()}-{time.time_ns()}"
    variables = [argument for name, value in environment.items()
                 for argument in ("--env", f"{name}={value}")]
    try:
        run_logged([
            "docker", "run", "--rm", "--network", "none", "--platform", PLATFORM,
            "--name", container, *variables,
            "--mount", f"type=bind,source={directory},target=/run",
            image, "/run/effective.xml",
        ], directory / "console.log", timeout=args.timeout)
    finally:
        subprocess.run(["docker", "rm", "--force", container],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
    frames = list((directory / "logs").rglob("frames.bin"))
    if len(frames) != 1 or not frames[0].stat().st_size:
        raise RuntimeError(f"expected one nonempty C++ frames.bin; inspect {directory}")
    return frames[0].parent


def compare_case(cpp_logs, cpp_trace, serial, traced, mission):
    """Frames, summary, and traces of a C++ comparison run versus Rust.

    `serial` supplies Rust frames and summary, `traced` its trace, events, and
    manifest (the same run may serve both). A summary that differs must be
    explained by the documented scoring differences: identical counts, and Rust's
    metrics messages reproduce the C++ summary under the C++ rule and the Rust
    summary under the Rust rule (metrics_model.py). Nothing is masked.
    """
    frames = compare_frames(read_frames(cpp_logs / "frames.bin"), read_frames(serial / "frames.bin"))
    frames.passed = frames.mismatch_count == 0
    run = json.loads((traced / "manifest.json").read_text())["scenario"]["run"]
    events = json.loads((traced / "events.json").read_text())
    removed = [event["entity_ids"][0] for event in events
               if event["kind"] == "EntityRemoved" and event["time_s"] < run["start_s"] - 1e-9]
    rust_trace = read_trace(traced / "trace.jsonl")
    trace = compare_traces(read_trace(cpp_trace), rust_trace, removed,
                           run["start_s"])
    summary = Comparison()
    compare_summaries(cpp_logs / "summary.csv", serial / "summary.csv", summary)
    agreement = "agree"
    if summary.mismatch_count:
        final_s = read_frames(serial / "frames.bin")[-1].time_s - run["dt_s"]
        rules = [(rule, path) for rule, path in (("cpp", cpp_logs / "summary.csv"),
                                                   ("rust", serial / "summary.csv"))]
        explained = counts_match(cpp_logs / "summary.csv", serial / "summary.csv") and all(
            matches(predict(rust_trace, run["start_s"], final_s, weights(mission), rule), path)
            for rule, path in rules)
        agreement = "explained_by_scoring_rules" if explained else "unexplained"
    return {"comparison": asdict(frames), "trace_comparison": asdict(trace),
            "summary_differences": summary.first_mismatches,
            "summary_agreement": agreement, "pre_start_removed": removed}


def check_mission(args, mission, directory, images, rust):
    directory.mkdir()
    result = {"mission": str(mission), "passed": False}
    try:
        inputs = directory / "input"
        result["input"] = prepare_mission(mission, inputs)
        effective = inputs / "effective.xml"
        # Non-interference: instrumentation must not change C++ outputs.
        baseline = run_cpp(args, images["baseline"], directory / "cpp-baseline", effective, {})
        untraced = run_cpp(args, images["instrumented"], directory / "cpp-untraced",
                           effective, UNTRACED_ENV)
        traced = run_cpp(args, images["instrumented"], directory / "cpp-traced",
                         effective, TRACED_ENV)
        result["cpp_non_interference"] = {
            variant: {name: sha256(logs / name) == sha256(baseline / name)
                      for name in CPP_OUTPUT_FILES}
            for variant, logs in (("untraced", untraced), ("traced", traced))
        }
        cpp_logs = run_cpp(args, images["instrumented"], directory / "cpp", effective,
                           COMPARISON_ENV)
        result["cpp_log_directory"] = str(cpp_logs.relative_to(directory))
        result["cpp_output_hashes"] = {
            name: sha256(cpp_logs / name) for name in CPP_OUTPUT_FILES
        }
        # Use the saved input for Rust too; neither simulator may rewrite it.
        for workers in (1, 2, 8):
            run_logged([
                rust, "run", inputs / "input.xml", "--root", ROOT,
                "--output", directory / f"rust-{workers}",
                "--workers", workers, "--no-rerun",
            ], directory / f"rust-{workers}.log", timeout=args.timeout)
        serial = directory / "rust-1"
        result["worker_equality"] = {
            str(workers): same_outputs(serial, directory / f"rust-{workers}") for workers in (2, 8)
        }
        run_logged([
            rust, "run", inputs / "input.xml", "--root", ROOT,
            "--output", directory / "rust-rerun", "--workers", "8", "--headless",
        ], directory / "rust-rerun.log", timeout=args.timeout)
        result["recording_equality"] = same_outputs(serial, directory / "rust-rerun")
        recording = directory / "rust-rerun/recording.rrd"
        if not recording.is_file() or not recording.stat().st_size:
            raise RuntimeError("headless Rerun run did not produce a nonempty recording")
        result["recording_sha256"] = sha256(recording)
        run_logged([
            rust, "run", inputs / "input.xml", "--root", ROOT,
            "--output", directory / "rust-trace", "--workers", "1", "--no-rerun", "--trace",
        ], directory / "rust-trace.log", timeout=args.timeout)
        result["trace_equality"] = same_outputs(serial, directory / "rust-trace")
        result["rust_output_hashes"] = {
            variant: {name: sha256(directory / variant / name) for name in OUTPUT_FILES}
            for variant in ("rust-1", "rust-2", "rust-8", "rust-rerun", "rust-trace")
        }
        comparison = compare_case(cpp_logs, directory / "cpp/trace.jsonl", serial,
                                  directory / "rust-trace", inputs / "input.xml")
        result.update(comparison)
        write_json(directory / "comparison.json", comparison["comparison"])
        write_json(directory / "trace-comparison.json", comparison["trace_comparison"])
        result["passed"] = (
            result["comparison"]["passed"] and result["trace_comparison"]["passed"]
            and result["summary_agreement"] != "unexplained"
            and all(all(files.values()) for files in result["cpp_non_interference"].values())
            and all(all(files.values()) for files in result["worker_equality"].values())
            and all(result["recording_equality"].values())
            and all(result["trace_equality"].values())
        )
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        result["error"] = str(error)
    write_json(directory / "result.json", result)
    return result


def positive_int(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=ROOT.parent / "scrimmage",
                        help="C++ checkout containing both local branches")
    parser.add_argument("--baseline-branch", default=BASELINE_BRANCH,
                        help="unmodified reference branch")
    parser.add_argument("--branch", default=INSTRUMENTED_BRANCH,
                        help="opt-in instrumentation branch used for comparison")
    parser.add_argument("--output", type=Path, help="new check directory (default: runs/reference-checkNNN)")
    parser.add_argument("--mission", type=Path, action="append",
                        help="repeat to replace the default mission matrix; requires expanded XML")
    parser.add_argument("--mission-dir", type=Path,
                        help="add every *.xml in a directory, e.g. from generate_scenarios.py")
    parser.add_argument("--jobs", type=positive_int, default=4, help="parallel C++ compilation jobs")
    parser.add_argument("--timeout", type=positive_int, default=120, help="per-run timeout in seconds")
    parser.add_argument("--build-timeout", type=positive_int, default=3600, help="per-build timeout in seconds")
    args = parser.parse_args(argv)
    args.source = args.source.resolve()
    missions = list(args.mission or [])
    if args.mission_dir:
        found = sorted(args.mission_dir.glob("*.xml"))
        if not found:
            parser.error(f"no *.xml missions in {args.mission_dir}")
        missions += found
    missions = missions or [ROOT / "missions" / path for path, _ in comparison_missions()]
    missions = [mission.resolve(strict=True) for mission in missions]
    capture(["docker", "version", "--format", "{{.Server.Version}}"])
    output = allocate_output(args.output, ROOT / "runs")
    print(f"Reference check: {output}", flush=True)
    report = {
        "passed": False, "cases": [], "platform": PLATFORM,
        "direct_cpp_event_stream_compared": True,
        "native_viewer_launched": False,
        "notes": [
            "C++ frames use reference/compare.py tolerances; differing summaries must be "
            "explained by reference/metrics_model.py.",
            "Rust worker/recording checks compare frames, events, and summaries byte-for-byte.",
            "Headless recording is tested; interactive viewer QA is a separate workflow.",
            "Default random engines/distributions differ between libc++ and libstdc++; the "
            "comparison run uses the branch's libc++-compatible spawn stream.",
            "Traces compare deliveries, beliefs, and plugin outputs (reference/traces.py).",
        ],
    }
    try:
        report["cpp_baseline"] = build_reference(args, output, args.baseline_branch)
        report["cpp"] = build_reference(args, output, args.branch)
        rust, report["rust"] = build_rust(args, output)
        images = {"baseline": report["cpp_baseline"]["image"], "instrumented": report["cpp"]["image"]}
        for index, mission in enumerate(missions):
            print(f"Checking {mission.name}", flush=True)
            result = check_mission(args, mission, output / f"{index:02}-{mission.stem}",
                                   images, rust)
            report["cases"].append(result)
            write_json(output / "report.json", report)
            print(f"  {'PASS' if result['passed'] else 'FAIL'}: {mission.name}", flush=True)
        report["passed"] = bool(report["cases"]) and all(case["passed"] for case in report["cases"])
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        report["error"] = str(error)
        print(error, file=sys.stderr)
    finally:
        write_json(output / "report.json", report)
    print(f"{'PASS' if report['passed'] else 'FAIL'}: {output / 'report.json'}", flush=True)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error
