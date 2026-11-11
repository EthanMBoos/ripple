#!/usr/bin/env python3
"""Check production receive; optionally reject two faults in temporary copies."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
SOURCE = ROOT / "crates/core/src/pubsub/messages.rs"


def prepare():
    """Extend the pinned proof dependency, never the normal workspace."""
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    cached = list((cargo_home / "registry/src").glob("*/creusot-std-0.14.0"))
    if not cached:
        bootstrap = ROOT / "target/creusot-bootstrap"
        bootstrap.mkdir(parents=True, exist_ok=True)
        (bootstrap / "Cargo.toml").write_text(
            '[workspace]\n[package]\nname="creusot-bootstrap"\n'
            'version="0.0.0"\nedition="2024"\n[lib]\npath="lib.rs"\n'
            '[dependencies]\ncreusot-std="=0.14.0"\n')
        (bootstrap / "lib.rs").write_text("")
        subprocess.run(["cargo", "fetch", "--manifest-path",
                        str(bootstrap / "Cargo.toml")], check=True)
        cached = list((cargo_home / "registry/src").glob("*/creusot-std-0.14.0"))
    if len(cached) != 1:
        raise SystemExit("Expected one cached creusot-std 0.14.0 source directory")
    original = cached[0]
    local = ROOT / "target/creusot-std-0.14.0"
    library = (original / "src/lib.rs").read_text() + "\n" + (HERE / "std_models.rs").read_text()
    strings = (original / "src/std/string.rs").read_text().replace(
        "#[logic]\n    fn deep_model", "#[logic(open, inline)]\n    fn deep_model")
    if (not (local / "src/lib.rs").exists()
            or (local / "src/lib.rs").read_text() != library
            or (local / "src/std/string.rs").read_text() != strings):
        shutil.copytree(original, local, dirs_exist_ok=True)
        (local / "src/lib.rs").write_text(library)
        (local / "src/std/string.rs").write_text(strings)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mutations", action="store_true")
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    prepare()
    if args.prepare_only:
        return
    folder = ROOT / "target/creusot-checks"
    folder.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix="receive-", dir=folder))
    results = []

    def check(name, source, expected):
        command = ["cargo", "creusot", "--no-cache", "--span-mode", "relative"]
        start = time.monotonic()
        env = {**os.environ, "RIPPLE_VERIFY_SOURCE": str(source), "RUST_LOG": "error"}
        proc = subprocess.run(command, cwd=HERE, env=env, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (output / f"{name}.log").write_text(proc.stdout)
        generated = HERE / "target/receive.generated.rs"
        if generated.exists():
            shutil.copyfile(generated, output / f"{name}.rs")
        proof = HERE / "verif/ripple_receive_proof_rlib/impl_Messages/receive/proof.json"
        if proof.exists():
            shutil.copyfile(proof, output / f"{name}.proof.json")
        record = dict(name=name, command=command, exit_code=proc.returncode,
                      expected=expected, seconds=round(time.monotonic() - start, 3),
                      source_sha256=hashlib.sha256(source.read_bytes()).hexdigest())
        results.append(record)
        (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(f"{name}: exit {proc.returncode} (expected {expected}); {record['seconds']} s", flush=True)
        if expected == 0 and proc.returncode == 0:
            proofs = json.loads(proof.read_text())["proofs"]["Coma"]
            if "vc_receive_T" not in proofs or "null" in json.dumps(proofs):
                raise RuntimeError("Missing or incomplete production receive proof")
        if proc.returncode != expected:
            raise RuntimeError(f"Unexpected proof result; see {output / (name + '.log')}")
        if expected != 0 and ("unproved file" not in proc.stdout
                              or "could not compile" in proc.stdout
                              or "no contract" in proc.stdout
                              or "vc_receive_T" not in proc.stdout):
            raise RuntimeError(f"Fault did not reach the expected proof failure: {name}")

    print(f"Evidence: {output}", flush=True)
    check("production", SOURCE, 0)
    if args.mutations:
        original = SOURCE.read_text()
        faults = {
            "wrong-channel": ("self.inbox.remove(&channel)",
                              "self.inbox.remove(&(topic.to_owned(), network.to_owned()))"),
            "reversed-order": ("received.push(Message {", "received.insert(0, Message {"),
        }
        try:
            for name, (before, after) in faults.items():
                if original.count(before) != 1:
                    raise RuntimeError(f"Update mutation location for {name}")
                source = output / f"{name}-source.rs"
                source.write_text(original.replace(before, after))
                check(name, source, 1)
        finally:
            # Leave generated artifacts referring to production, even after a failed check.
            check("production-restored", SOURCE, 0)


if __name__ == "__main__":
    main()
