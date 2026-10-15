#!/usr/bin/env python3
"""Check that Rust outputs are byte-identical across platforms.

    python3 reference/platform_check.py --output runs/platform000 [--mission-dir DIR]

Builds the ripple CLI natively (here, macOS/arm64) and in a linux/amd64
container (the cli target of reference/rust.Dockerfile), runs every curated mission (and any
--mission-dir missions) at 1 and 8 workers with the comparison trace, and
compares MCAP channel payloads and result metadata byte for byte.
Different CPU architectures, operating systems, and math libraries can change
floating-point results; this shows whether they do for this code. Standard
library only; needs Docker and Cargo.
"""

import argparse
import json
import platform
import subprocess
from pathlib import Path

from compare import compare_frames
from frames import read_frames
import project
from project import IMAGE_PREFIX, ROOT, build_cli
from run import read_run

CHANNELS = ("/entities", "/frames", "/events", "/summary",
            "/beliefs", "/readings", "/deliveries", "/outputs")
IMAGE = f"{IMAGE_PREFIX}-platform"


def missions(extra):
    found = project.missions()
    if extra:
        found += sorted(extra.glob("*.yaml"))
    return found


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    parser.add_argument("--mission-dir", type=Path, help="additional *.yaml missions")
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True)

    native = build_cli("release")
    with (output / "linux-build.log").open("w") as log:
        subprocess.run(["docker", "build", "--platform", "linux/amd64", "--progress", "plain",
                        "-f", ROOT / "reference/rust.Dockerfile", "--target", "cli", "--tag", IMAGE, ROOT],
                       stdout=log, stderr=subprocess.STDOUT, check=True)
    linux_compiler = subprocess.check_output(
        ["docker", "run", "--rm", "--platform", "linux/amd64", "--entrypoint", "cat", IMAGE,
         "/rust-compiler.txt"], text=True)

    mount = ["-v", f"{ROOT}:/work:ro", "-v", f"{output}:/out"]
    if args.mission_dir:
        mount += ["-v", f"{args.mission_dir.resolve()}:/extra:ro"]
    cases = []
    for mission in missions(args.mission_dir):
        in_repo = mission.is_relative_to(ROOT)
        container_path = f"/work/{mission.relative_to(ROOT)}" if in_repo \
            else f"/extra/{mission.name}"
        name = str(mission.relative_to(ROOT) if in_repo else mission.name).replace("/", "_")
        for workers in (1, 8):
            label = f"{name}-w{workers}"
            common = ["--headless", "--trace", "--workers", str(workers)]
            subprocess.run([native, "run", mission, "--root", ROOT, "--output",
                            output / "native" / label, *common],
                           check=True, capture_output=True)
            subprocess.run(["docker", "run", "--rm", "--network", "none", "--platform",
                            "linux/amd64", *mount, IMAGE, "run", container_path, "--root",
                            "/work", "--output", f"/out/linux/{label}", *common],
                           check=True, capture_output=True)
            native_run = read_run(output / "native" / label)
            linux_run = read_run(output / "linux" / label)
            equal = {topic: native_run.channels.get(topic, []) == linux_run.channels.get(topic, [])
                     for topic in CHANNELS}
            equal["metadata"] = (native_run.scenario == linux_run.scenario
                                 and native_run.metadata == linux_run.metadata)
            difference = 0.0
            if not equal["/frames"]:
                difference = compare_frames(
                    read_frames(output / "native" / label / "run.mcap"),
                    read_frames(output / "linux" / label / "run.mcap")).max_position_error_m
            cases.append({"mission": name, "workers": workers, "identical": equal,
                          "max_position_difference_m": difference})
            print(f"{'SAME' if all(equal.values()) else 'DIFF'} {label}", flush=True)

    report = {
        "native": {"platform": platform.platform(), "compiler": subprocess.check_output(
            ["rustc", "--version", "--verbose"], text=True)},
        "linux": {"platform": "linux/amd64 (Docker)", "compiler": linux_compiler},
        "channels": list(CHANNELS),
        "identical": all(all(case["identical"].values()) for case in cases),
        "max_position_difference_m": max(case["max_position_difference_m"] for case in cases),
        "cases": cases,
    }
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("identical" if report["identical"] else "DIFFERENT", output / "report.json")
    return 0  # Differences are results in the report; failures to run raise instead.


if __name__ == "__main__":
    raise SystemExit(main())
