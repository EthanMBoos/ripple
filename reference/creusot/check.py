#!/usr/bin/env python3
"""Check production mailbox proofs; optionally reject faults in temporary copies."""
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
    parser.add_argument("--routing", action="store_true")
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    prepare()
    if args.prepare_only:
        return
    # The two feature modes share Creusot's output manifest. Re-extract this
    # small proof crate when switching modes; keep all dependency builds cached.
    subprocess.run(["cargo", "+nightly-2026-08-03", "clean", "-p", "ripple-receive-proof", "--target-dir",
                    str(HERE / "target/creusot")], cwd=HERE, check=True)
    folder = ROOT / "target/creusot-checks"
    folder.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix="routing-" if args.routing else "receive-", dir=folder))
    results = []

    targets = {
        "impl_Messages/receive": "vc_receive_T",
    }
    if args.routing:
        targets.update({
            "impl_NetworkContext/route_links": "vc_route_links_F",
            "impl_NetworkContext/deliver_scheduled": "vc_deliver_scheduled",
            "subscriber_index": "vc_subscriber_index",
            "receiver_index": "vc_receiver_index",
            "message_envelope": "vc_message_envelope_T",
            "delayed/pending_messages": "vc_pending_messages",
            "delayed/delivered_messages": "vc_delivered_messages",
            "delayed/endpoint_index": "vc_endpoint_index",
            "delayed/pending_step": "vc_pending_step",
            "delayed/delivered_step": "vc_delivered_step",
            "delayed/empty_delivery": "vc_empty_delivery",
            "delayed/endpoint_step": "vc_endpoint_step",
            "delayed/sequence_prefixes": "vc_sequence_prefixes_T",
            "delayed/append_suffix": "vc_append_suffix_T",
            "delayed/mailbox_update": "vc_mailbox_update",
            "delayed/quiet_suffix": "vc_quiet_suffix",
            "delayed/delivery_keep_step": "vc_delivery_keep_step",
            "delayed/delivery_append_step": "vc_delivery_append_step",
        })

    def check(name, source, expected, failed_goal="vc_receive_T"):
        start = time.monotonic()
        env = {**os.environ, "RIPPLE_VERIFY_SOURCE": str(source), "RUST_LOG": "error"}
        attempts, logs = [], []
        # Production checks every body. A fault changes one body, so rerun only
        # that target and require a proof failure (not a compilation failure).
        selected = targets if expected == 0 else {
            target: goal for target, goal in targets.items() if goal == failed_goal
        }
        if expected != 0 and len(selected) != 1:
            raise RuntimeError(f"Expected one mutated proof target: {failed_goal}")
        # One Why3 process per body: processing the original five together retained about
        # 10 GB on the trial machine. This also excludes stale feature outputs.
        for target in selected:
            command = ["cargo", "creusot", "--no-cache", "--span-mode", "relative",
                       "-j", "4", Path(target).name]
            if args.routing:
                command += ["--why3find-arg=--depth", "--why3find-arg=3",
                            "--why3find-arg=--time", "--why3find-arg=5",
                            "--", "--features", "routing"]
            proc = subprocess.run(command, cwd=HERE, env=env, text=True,
                                  stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            logs.append(proc.stdout)
            attempts.append(dict(target=target, command=command, exit_code=proc.returncode))
            (output / f"{name}.log").write_text("\n".join(logs))
            print(f"{name}: {target}: exit {proc.returncode}", flush=True)
        stdout = "\n".join(logs)
        exit_code = next((attempt["exit_code"] for attempt in attempts
                          if attempt["exit_code"] != 0), 0)
        generated = HERE / "target/receive.generated.rs"
        if generated.exists():
            shutil.copyfile(generated, output / f"{name}.rs")
        proofs_by_target = {}
        for target, goal in selected.items():
            proof = HERE / "verif/ripple_receive_proof_rlib" / target / "proof.json"
            if proof.exists():
                shutil.copyfile(proof, output / f"{name}-{Path(target).name}.proof.json")
                proofs_by_target[target] = json.loads(proof.read_text())["proofs"]["Coma"]
        record = dict(name=name, attempts=attempts, exit_code=exit_code,
                      expected=expected, seconds=round(time.monotonic() - start, 3),
                      source_sha256=hashlib.sha256(source.read_bytes()).hexdigest())
        results.append(record)
        (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(f"{name}: exit {exit_code} (expected {expected}); {record['seconds']} s", flush=True)
        if expected == 0 and exit_code == 0:
            for target, goal in targets.items():
                proofs = proofs_by_target.get(target, {})
                if goal not in proofs or "null" in json.dumps(proofs):
                    raise RuntimeError(f"Missing or incomplete production proof: {target}")
        if exit_code != expected:
            raise RuntimeError(f"Unexpected proof result; see {output / (name + '.log')}")
        if expected != 0 and ("unproved file" not in stdout
                              or "could not compile" in stdout
                              or "no contract" in stdout
                              or failed_goal not in stdout):
            raise RuntimeError(f"Fault did not reach the expected proof failure: {name}")
        if expected != 0:
            for attempt in attempts:
                wanted = 1 if targets[attempt["target"]] == failed_goal else 0
                if attempt["exit_code"] != wanted:
                    raise RuntimeError(f"Unexpected failure outside the mutated target: {attempt['target']}")

    print(f"Evidence: {output}", flush=True)
    check("production", SOURCE, 0)
    if args.mutations:
        original = SOURCE.read_text()
        faults = {
            "wrong-channel": ("self.inbox.remove(&channel)",
                              "self.inbox.remove(&(topic.to_owned(), network.to_owned()))"),
            "reversed-order": ("received.push(Message {", "received.insert(0, Message {"),
        }
        if args.routing:
            faults = {
                "wrong-recipient": (
                    "entry_or_insert(&mut receivers, mailbox.endpoint, index);",
                    "entry_or_insert(&mut receivers, mailbox.endpoint, 0);"),
                "drop-future-message": ("pending.push(message);", "drop(message);"),
                "deliver-before-due": (
                    "if message.envelope.delivered_at_s <= self.time.time_s + 1e-12 {",
                    "if true {"),
                "retain-removed-recipient": (
                    "// A removed entity cannot receive delayed messages.\n                continue;",
                    "pending.push(message);\n                continue;"),
                "skip-silent-phase": (
                    "let subscribers = if publications.is_empty() {",
                    "if publications.is_empty() { return Ok(()); }\n        let subscribers = if publications.is_empty() {"),
            }
        try:
            for name, (before, after) in faults.items():
                if original.count(before) != 1:
                    raise RuntimeError(f"Update mutation location for {name}")
                source = output / f"{name}-source.rs"
                source.write_text(original.replace(before, after))
                failed_goal = ("vc_receiver_index" if name == "wrong-recipient" else
                               "vc_route_links_F" if name == "skip-silent-phase" else
                               "vc_deliver_scheduled" if args.routing else "vc_receive_T")
                check(name, source, 1, failed_goal)
        finally:
            # Leave generated artifacts referring to production, even after a failed check.
            check("production-restored", SOURCE, 0)


if __name__ == "__main__":
    main()
