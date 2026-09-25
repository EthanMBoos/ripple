#!/usr/bin/env python3
"""Run a scrimmage sweep in shards, locally or on Slurm, then merge the results.

A trimmed copy of Ripple's scripts/bulk_run.py. From the repository root:

    conda env create -f environment.yaml      # once, on shared storage
    conda activate scrimmage-rs
    cargo build --release -p scrimmage-rs

    # Local: 4 shards as parallel processes, then collect.
    python3 scripts/bulk_run.py local missions/waypoints-point-agents.sweep.yaml --jobs 4

    # Slurm: one array job of 4 shards. Arguments after -- go to sbatch.
    python3 scripts/bulk_run.py submit missions/waypoints-point-agents.sweep.yaml --jobs 4 \\
        -- --account=<account> --partition=<partition>
    # When the array finishes:
    python3 scripts/bulk_run.py collect sweeps/waypoints-point-agents/run000

Implemented: local shards, one Slurm array submission, the per-task worker, and
collect, which checks that the shards cover every case exactly once and merges
their rows in case order into <campaign>/results.jsonl.

Not implemented (Ripple has them): watching the Slurm job, a `status` progress
bar, `replay` of one case, refusing a dirty checkout, input hashes, and
CPU-hour accounting.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[1]
SWEEP_SUFFIX = ".sweep.yaml"


def numbered_output(sweep: Path) -> Path:
    """<repo>/sweeps/<sweep name>/runNNN, like `scrimmage sweep` without --output."""
    family = REPOSITORY / "sweeps" / sweep.name.removesuffix(SWEEP_SUFFIX)
    index = 0
    while (family / f"run{index:03}").exists():
        index += 1
    return family / f"run{index:03}"


def prepare(arguments: argparse.Namespace) -> tuple[Path, dict]:
    """Create the campaign folder and record what every shard needs to run."""
    sweep = arguments.sweep.resolve()
    binary = arguments.binary.resolve()
    if not sweep.name.endswith(SWEEP_SUFFIX) or not sweep.is_file():
        raise SystemExit(f"sweep input must be an existing file ending in {SWEEP_SUFFIX}")
    if not binary.is_file():
        raise SystemExit(f"binary not found: {binary}; run cargo build --release -p scrimmage-rs")
    output = arguments.output.resolve() if arguments.output else numbered_output(sweep)
    if output.exists():
        raise SystemExit(f"output already exists: {output}")
    record = {"sweep": str(sweep), "binary": str(binary), "jobs": arguments.jobs}
    (output / "logs").mkdir(parents=True)
    # The Slurm worker reads this back; it is the only state shared with shards.
    (output / "campaign.json").write_text(json.dumps(record, indent=2) + "\n")
    print(f"campaign: {output}")
    return output, record


def worker_command(record: dict, output: Path, index: int) -> list[str]:
    """One shard: `scrimmage sweep` writing to <campaign>/shard-NNNN."""
    return [
        record["binary"], "sweep", record["sweep"],
        "--output", str(output / f"shard-{index:04}"),
        "--shard-index", str(index),
        "--shard-count", str(record["jobs"]),
    ]


def run_local_shard(record: dict, output: Path, index: int) -> None:
    completed = subprocess.run(
        worker_command(record, output, index), cwd=REPOSITORY, text=True, capture_output=True
    )
    log = output / "logs" / f"shard-{index:04}.log"
    log.write_text(completed.stdout + completed.stderr)
    if completed.returncode:
        raise RuntimeError(f"shard {index} exited {completed.returncode}; see {log}")


def local(arguments: argparse.Namespace) -> None:
    output, record = prepare(arguments)
    with concurrent.futures.ThreadPoolExecutor(max_workers=record["jobs"]) as pool:
        futures = [pool.submit(run_local_shard, record, output, index)
                   for index in range(record["jobs"])]
        for future in futures:
            future.result()
    collect(output)


def submit(arguments: argparse.Namespace) -> None:
    if shutil.which("sbatch") is None:
        raise SystemExit("sbatch is not available; use `local` on this machine")
    output, record = prepare(arguments)
    # One array of independent shards; Slurm runs bulk_run.slurm once per shard.
    command = [
        "sbatch", "--parsable", *arguments.slurm_args,
        f"--array=0-{record['jobs'] - 1}",
        f"--output={output / 'logs' / 'slurm-%A_%a.out'}",
        # Compute nodes reuse the active environment and the build on shared storage.
        "--export=ALL",
        str(REPOSITORY / "scripts/bulk_run.slurm"),
    ]
    environment = os.environ.copy()
    environment["SCRIMMAGE_SWEEP_OUTPUT"] = str(output)
    submitted = subprocess.run(command, cwd=REPOSITORY, env=environment, check=True,
                               text=True, stdout=subprocess.PIPE)
    job_id = submitted.stdout.strip().split(";")[0]
    print(f"submitted Slurm array {job_id}")
    print(f"when it finishes: python3 scripts/bulk_run.py collect {output}")


def worker_from_slurm() -> None:
    """Runs inside one Slurm array task: the shard number is the task ID."""
    output = Path(os.environ["SCRIMMAGE_SWEEP_OUTPUT"])
    record = json.loads((output / "campaign.json").read_text())
    index = int(os.environ["SLURM_ARRAY_TASK_ID"])
    subprocess.run(worker_command(record, output, index), cwd=REPOSITORY, check=True)


def collect(output: Path) -> None:
    """Checks the shards cover every case once, then merges rows in case order."""
    record = json.loads((output / "campaign.json").read_text())
    shards = sorted(output.glob("shard-*"))
    if len(shards) != record["jobs"]:
        raise SystemExit(f"expected {record['jobs']} shards, found {len(shards)}")
    sweeps = [json.loads((shard / "sweep.json").read_text()) for shard in shards]
    total = sweeps[0]["total_case_count"]
    for index, (shard, sweep) in enumerate(zip(shards, sweeps)):
        if (shard.name != f"shard-{index:04}" or sweep["shard_index"] != index
                or sweep["shard_count"] != record["jobs"]
                or sweep["total_case_count"] != total):
            raise SystemExit(f"{shard} does not belong to this campaign")
    # Shard i holds cases i, i+n, i+2n, ... in order, so case k is the next row
    # of shard k % n. Any gap, extra row, or wrong ID stops the merge.
    rows = [(shard / "results.jsonl").open() for shard in shards]
    errors = 0
    partial = output / "results.jsonl.partial"
    try:
        with partial.open("w") as merged:
            for case in range(total):
                line = rows[case % len(shards)].readline()
                if not line or json.loads(line)["case_id"] != f"case-{case:04}":
                    raise SystemExit(f"case-{case:04} is missing; a shard did not finish")
                errors += json.loads(line)["error"] is not None
                merged.write(line)
            if any(source.readline() for source in rows):
                raise SystemExit("a shard has extra rows")
    finally:
        for source in rows:
            source.close()
    # Only a complete merge replaces results.jsonl.
    partial.replace(output / "results.jsonl")
    print(f"collected {total} cases from {len(shards)} shards into {output / 'results.jsonl'}"
          f" ({errors} with errors)")


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("local", "submit"):
        command = commands.add_parser(name)
        command.add_argument("sweep", type=Path)
        command.add_argument("--jobs", type=int, default=1, help="number of shards")
        command.add_argument("--binary", type=Path,
                             default=REPOSITORY / "target/release/scrimmage")
        command.add_argument("--output", type=Path,
                             help="campaign folder (default: sweeps/<sweep name>/runNNN)")
    commands.add_parser("worker", help="run by bulk_run.slurm inside a Slurm task")
    collected = commands.add_parser("collect")
    collected.add_argument("output", type=Path)
    raw_arguments = sys.argv[1:]
    slurm_args = []
    if "--" in raw_arguments:
        separator = raw_arguments.index("--")
        raw_arguments, slurm_args = raw_arguments[:separator], raw_arguments[separator + 1:]
    arguments = parser.parse_args(raw_arguments)
    if slurm_args and arguments.command != "submit":
        parser.error("arguments after -- are only accepted by submit")
    arguments.slurm_args = slurm_args
    if arguments.command in ("local", "submit") and arguments.jobs < 1:
        parser.error("--jobs must be positive")
    return arguments


if __name__ == "__main__":
    args = parse_arguments()
    if args.command == "local":
        local(args)
    elif args.command == "submit":
        submit(args)
    elif args.command == "worker":
        worker_from_slurm()
    else:
        collect(args.output.resolve())
