"""Bare-metal macOS run of benchmark.py's workloads; build C++ first with build.sh.

    python3 paper/data/benchmark-native/run.py runs/new-native-benchmark

Uses benchmark.py's missions, checks, and statistics, but runs the native C++
build and the Rust runner directly on the host instead of in a container.
Exits non-zero if any required check fails.
"""
import json
import os
import platform
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "reference"))
import benchmark  # noqa: E402
import perf  # noqa: E402
from reference_check import sha256  # noqa: E402

N = REPO / "runs/native-cpp"
CPP = N / "build/bin/scrimmage.app/Contents/MacOS/scrimmage"
CPP_ENV = {**os.environ,
           "SCRIMMAGE_PLUGIN_PATH": f"{N}/build/plugin_libs:{N}/source/include/scrimmage/plugins",
           "SCRIMMAGE_DATA_PATH": f"{N}/source/data", "SCRIMMAGE_CONFIG_PATH": f"{N}/source/config",
           "DYLD_LIBRARY_PATH": f"{N}/build/lib:{N}/build/plugin_libs"}


def run(variant, mission_path, directory, runner):
    if variant.startswith("cpp"):
        command, env = [CPP, mission_path], CPP_ENV
    else:
        command, env = [runner, mission_path, REPO, directory / "rust", variant.split("-")[1]], os.environ
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


def git(*arguments, cwd=REPO):
    return subprocess.check_output(["git", *arguments], cwd=cwd, text=True).strip()


def provenance():
    files = [REPO / "Cargo.toml", REPO / "Cargo.lock", REPO / "reference/benchmark_runner.rs"]
    files += list((REPO / "crates").rglob("*.rs")) + list((REPO / "crates").rglob("Cargo.toml"))
    return {"cpp_commit": git("rev-parse", "Ubuntu-24.04", cwd=REPO.parent / "scrimmage"),
            "rust_head": git("rev-parse", "HEAD"), "rust_worktree": git("status", "--short"),
            "source_hashes": {str(p.relative_to(REPO)): sha256(p) for p in sorted(files) if p.is_file()},
            "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
            "cpp_build": "paper/data/benchmark-native/build.sh; " + subprocess.check_output(
                ["/usr/bin/clang++", "--version"], text=True).splitlines()[0]}


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    output = Path(sys.argv[1]).resolve()
    output.mkdir(parents=True)  # Never overwrite an earlier benchmark.
    runner = perf.build_runner()
    benchmark.run = lambda variant, mission_path, directory: run(variant, mission_path, directory, runner)
    result = {"platform": platform.platform(), "machine": platform.machine(),
              "cpu_count": os.cpu_count(), "repetitions": 3, "provenance": provenance(),
              "timing": "bare-metal macOS; whole process minus the median one-step run", "cases": []}
    for name, counts in benchmark.COUNTS.items():
        for agents in counts:
            case = output / f"{name}-{agents}"
            case.mkdir()
            result["cases"].append(benchmark.measure(name, agents, case, 3))
            (output / "timings.json").write_text(json.dumps(result, indent=2))
    result["passed"] = all(case["passed"] for case in result["cases"])
    (output / "timings.json").write_text(json.dumps(result, indent=2))
    print("passed", result["passed"])
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
