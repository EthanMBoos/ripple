#!/usr/bin/env python3
"""Prove the engine's Verus contracts and check that planted faults fail.

    python3 reference/verus.py [--output DIR] [--no-faults] [--retain]

Runs `cargo verus verify -p ripple-core`, which proves every contract in the
core crate (docs/FORMAL_VERIFICATION.md is the catalog). Then, unless
--no-faults, each fault in FAULTS is written into the production source, its
check is rerun and must fail, and the file is restored. A proof fault reruns
the verification; a compile fault, one the types alone must catch, reruns
`cargo check`; a test fault, one in a trusted plugin call that no proof
covers, reruns the core crate's tests. A contract, type, or test that survives
its fault is too weak, so a surviving fault fails this check. The record names
the Verus version, how many items it verified, the time each step took, and
the hash of each source file that carries a contract; --retain copies it to
paper/data/verification/. Needs `cargo-verus` and `verus` on the PATH (the
Rust book's verification chapter says where they come from); standard library
only.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

from project import CORE_PACKAGE, ROOT, RUNS, allocate

CORE = ROOT / "crates/core/src"
RETAINED = ROOT / "paper/data/verification/record.json"
CHECKS = ("proof", "compile", "test")


class Fault:
    """A planted defect: `old` becomes `new` in `path` under crates/core/src,
    and `check` ("proof", "compile", or "test") must then fail."""

    def __init__(self, name, path, old, new, check):
        if check not in CHECKS:
            raise ValueError(f"{name}: check must be one of {CHECKS}")
        self.name, self.path, self.old, self.new, self.check = name, path, old, new, check


FAULTS = [
    Fault("rate reload deleted", "common/loop_rate.rs",
          "        if due {\n            self.remaining_s += self.period_s;\n        }\n",
          "",
          "proof"),
    Fault("elapsed time not reset when the plugin runs", "common/loop_rate.rs",
          "            let elapsed_s = self.elapsed_s;\n            self.elapsed_s = 0.0;\n",
          "            let elapsed_s = self.elapsed_s;\n",
          "proof"),
    Fault("countdown zeroed for every rate", "common/loop_rate.rs",
          "        if self.period_s == 0.0 {\n",
          "        if self.period_s >= 0.0 {\n",
          "proof"),
    Fault("due compared with a whole step instead of half", "common/loop_rate.rs",
          "        let due = self.remaining_s < dt_s / 2.0;\n",
          "        let due = self.remaining_s < dt_s;\n",
          "proof"),
]


def prove(env):
    """(exit code, seconds, output) of `cargo verus verify` on the core crate."""
    started = time.perf_counter()
    result = subprocess.run(["cargo", "verus", "verify", "-p", CORE_PACKAGE], cwd=ROOT, env=env,
                            capture_output=True, text=True)
    return result.returncode, round(time.perf_counter() - started, 1), result.stdout + result.stderr


def verified_count(output):
    """How many items Verus reports verified, or None when it did not get that far."""
    found = re.search(r"verification results:: (\d+) verified", output)
    return int(found.group(1)) if found else None


def compile_check(env):
    """(exit code, seconds, output) of `cargo check` on the core crate."""
    return cargo(["check", "-p", CORE_PACKAGE], env)


def test_check(env):
    """(exit code, seconds, output) of the core crate's tests."""
    return cargo(["test", "-p", CORE_PACKAGE], env)


def cargo(arguments, env):
    started = time.perf_counter()
    result = subprocess.run(["cargo"] + arguments, cwd=ROOT, env=env, capture_output=True, text=True)
    return result.returncode, round(time.perf_counter() - started, 1), result.stdout + result.stderr


def check_fault(fault, env, log):
    """True when the fault leaves a contract unproved, or fails to compile for a
    compile fault, or fails a test for a test fault. The source is restored even
    if the run is interrupted."""
    path = CORE / fault.path
    original = path.read_text()
    if original.count(fault.old) != 1:
        raise RuntimeError(f"{fault.name}: its text is not found once in {path}")
    try:
        path.write_text(original.replace(fault.old, fault.new))
        if fault.check == "proof":
            code, seconds, output = prove(env)
        elif fault.check == "compile":
            code, seconds, output = compile_check(env)
        else:
            code, seconds, output = test_check(env)
    finally:
        path.write_text(original)
    log.write_text(output)
    return {"fault": fault.name, "check": fault.check, "file": fault.path,
            "fails_as_required": code != 0, "seconds": seconds}


def source_hashes():
    """Every core source file carrying a contract or proof text, with its hash."""
    hashes = {}
    for path in sorted(CORE.rglob("*.rs")):
        text = path.read_text()
        if "#[verus_spec(" in text or "verus! {" in text:
            hashes[str(path.relative_to(ROOT))] = hashlib.sha256(text.encode()).hexdigest()
    return hashes


def versions(env):
    """The lines `verus --version` prints: version, platform, toolchain."""
    result = subprocess.run(["verus", "--version"], cwd=ROOT, env=env, capture_output=True, text=True)
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, help="new directory (default runs/verusNNN)")
    parser.add_argument("--no-faults", action="store_true", help="prove only; plant no faults")
    parser.add_argument("--retain", action="store_true",
                        help="copy the record to paper/data/verification/ when everything passes")
    args = parser.parse_args(argv)
    env = os.environ.copy()
    if shutil.which("cargo-verus", path=env.get("PATH")) is None:
        print("cargo-verus is not on the PATH; see the Rust book's verification chapter",
              file=sys.stderr)
        return 2
    output = args.output.resolve() if args.output else allocate(RUNS, "verus")
    output.mkdir(parents=True, exist_ok=True)

    code, seconds, log = prove(env)
    (output / "prove.log").write_text(log)
    proved = code == 0
    verified = verified_count(log)
    print(f"{'proved' if proved else 'UNPROVED'} in {seconds} s: {verified} items verified",
          flush=True)
    faults = []
    if proved and not args.no_faults:
        for fault in FAULTS:
            result = check_fault(fault, env, output / (fault.name.replace(" ", "-") + ".log"))
            faults.append(result)
            print(f"{'fails as required' if result['fails_as_required'] else 'SURVIVES'}: "
                  f"{fault.name} ({result['seconds']} s)", flush=True)
    passed = proved and all(result["fails_as_required"] for result in faults)
    record = {"tools": versions(env), "passed": passed, "prove_seconds": seconds,
              "verified_items": verified, "sources": source_hashes(), "faults": faults}
    (output / "record.json").write_text(json.dumps(record, indent=1) + "\n")
    if args.retain and passed:
        RETAINED.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(output / "record.json", RETAINED)
        print(f"retained {RETAINED.relative_to(ROOT)}")
    print(f"{'passed' if passed else 'FAILED'}: {output / 'record.json'}")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
