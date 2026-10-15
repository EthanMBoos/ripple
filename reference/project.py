"""Names and paths of this project, in one place for every checking script.

Paths resolve from this checkout, so the suite runs the same from any folder
name. Rename the executable or a package here, not in each script.
"""

import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = "ripple"            # the command-line package (crates/cli)
BINARY = "ripple"             # its executable
CORE_PACKAGE = "ripple-core"  # the simulator (crates/core)
RUNNER = "reference"          # the core-only benchmark runner, a bench target of CORE_PACKAGE
IMAGE_PREFIX = "ripple"       # Docker image tags and labels
RUNS = ROOT / "runs"
DATA = ROOT / "paper/data"


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_cli(profile="debug", cwd=ROOT, env=None, log=None):
    """Build the executable with Cargo and return its path."""
    command = ["cargo", "build", "--locked", "--package", PACKAGE, "--bin", BINARY]
    if profile == "release":
        command.append("--release")
    output = subprocess.DEVNULL if log is None else log
    subprocess.run(command, cwd=cwd, env=env, stdout=output, stderr=subprocess.STDOUT, check=True)
    target = Path((env or os.environ).get("CARGO_TARGET_DIR") or target_directory(cwd))
    return target / profile / (BINARY + (".exe" if os.name == "nt" else ""))


def target_directory(cwd=ROOT):
    metadata = subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=cwd, text=True)
    return json.loads(metadata)["target_directory"]


def build_runner(cwd=ROOT):
    """Build the release core-only runner (reference/benchmark_runner.rs) and return its path."""
    output = subprocess.check_output(
        ["cargo", "bench", "--locked", "-p", CORE_PACKAGE, "--bench", RUNNER,
         "--no-run", "--message-format=json"], cwd=cwd, text=True, stderr=subprocess.DEVNULL)
    for line in output.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-artifact" and message.get("executable") \
                and message["target"]["name"] == RUNNER:
            return Path(message["executable"])
    raise RuntimeError("could not find the benchmark runner")


def source_files(root=ROOT):
    """The files that determine what the simulator computes."""
    files = [root / "Cargo.toml", root / "Cargo.lock", root / "reference/benchmark_runner.rs"]
    files += [path for path in (root / "crates").rglob("*")
              if path.is_file() and path.suffix in {".rs", ".xml", ".yaml", ".toml"}
              and "target" not in path.relative_to(root).parts]
    return sorted(files)


def source_identity(root=ROOT):
    """A hash of every source file, with the per-file hashes. Needs no Git history."""
    hashes = {str(path.relative_to(root)): sha256(path) for path in source_files(root)}
    combined = hashlib.sha256(json.dumps(hashes, sort_keys=True).encode()).hexdigest()
    return {"source_sha256": combined, "files": hashes}


def missions(root=ROOT):
    """Every runnable mission under missions/ and the user crates, not sweeps.

    ArduPilot missions need SITL in Docker and are run by their own study.
    """
    found = [path for path in sorted((root / "missions").rglob("*"))
             if path.suffix == ".yaml"
             and not path.name.endswith((".sweep.yaml", ".terrain.yaml"))]
    found += [path for path in sorted((root / "crates").glob("*/missions/*.yaml"))
              if not path.name.endswith(".sweep.yaml") and path.parts[-3] != "ardupilot"]
    return found


def allocate(parent, prefix):
    """A new numbered folder, parent/prefixNNN; earlier ones are never reused."""
    parent.mkdir(parents=True, exist_ok=True)
    number = 0
    while True:
        path = parent / f"{prefix}{number:03}"
        try:
            path.mkdir()
            return path.resolve()
        except FileExistsError:
            number += 1
