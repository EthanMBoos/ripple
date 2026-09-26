#!/usr/bin/env python3
"""Seeded-defect sensitivity study: can the C++ comparison catch plausible porting bugs?

    python3 reference/sensitivity.py --check runs/reference-check003 --output runs/sensitivity000

Reuses the saved C++ runs from a completed reference_check.py directory. Copies
the Rust workspace to the output directory (the working tree is never edited),
first runs it unmodified as a control, which must pass every case, and then
applies one defect at a time. Each defect is a one-line change of the kind a
port can plausibly get wrong. For each defect it reports which layer detected it
in which missions, by the checker's own criteria: frames (compare.py state and
entity IDs), summary (not explained by metrics_model.py), or trace
(traces.py deliveries, payloads, beliefs, outputs). An undetected defect is a
gap in the comparison, not a pass. Standard library only; needs Cargo.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

from reference_check import ROOT, compare_case, write_json

CORE = "crates/core/src/"
# (name, file, original text, defective text). Each original must occur exactly once.
DEFECTS = [
    ("controller substeps receive the full step dt", CORE + "simcontrol.rs",
     "            self.run_phase(|entity| entity.step_controller(time, &frame.entities))?;",
     "            let time = StepTime { time_s: time.time_s, dt_s };\n"
     "            self.run_phase(|entity| entity.step_controller(time, &frame.entities))?;"),
    ("frame timestamp labels the step start, not t + dt", CORE + "simcontrol.rs",
     "            time_s: self.time_s + self.config.run.dt_s,",
     "            time_s: self.time_s,"),
    ("spawn coordinates skip the C++ six-decimal round trip",
     CORE + "simcontrol/generation.rs",
     "    format!(\"{value:.6}\")\n        .parse()\n        .expect(\"formatted finite number must parse\")",
     "    value"),
    ("network delivers one tick late", CORE + "pubsub/messages.rs",
     "if message.envelope.delivered_at_s <= self.time.time_s + 1e-12 {",
     "if message.envelope.delivered_at_s < self.time.time_s - 1e-12 {"),
    ("SphereNetwork range becomes inclusive",
     CORE + "plugin/network/sphere_network/sphere_network.rs",
     "(sender_position_m - receiver_position_m).norm() >= self.range_m",
     "(sender_position_m - receiver_position_m).norm() > self.range_m"),
    ("SimpleCollision range becomes inclusive",
     CORE + "plugin/interaction/simple_collision/simple_collision.rs",
     "separation_m >= self.config.collision_range_m",
     "separation_m > self.config.collision_range_m"),
    ("sensor attitude errors applied in yaw-pitch-roll order",
     CORE + "plugin/sensor/noisy_state/noisy_state.rs",
     "truth_orientation * roll_error * pitch_error * yaw_error",
     "truth_orientation * yaw_error * pitch_error * roll_error"),
    ("NoisyContacts covariance is identity, not 5I",
     CORE + "plugin/sensor/noisy_contacts/noisy_contacts.rs",
     "covariance: Matrix3::identity() * 5.0,",
     "covariance: Matrix3::identity(),"),
    # "Straight ignores the NoisyState estimate" was an equivalent mutant: Rust
    # already passes the NoisyState belief as the plugin's state. Replaced by:
    ("LocalNetwork delivers across entities",
     CORE + "plugin/network/local_network/local_network.rs",
     "            if link.receiver.entity_id != Some(sender_entity_id) {",
     "            if link.receiver.entity_id.is_none() && sender_entity_id >= 0 {"),
    ("SingleIntegrator yaw swaps atan2 arguments",
     CORE + "plugin/motion/single_integrator/single_integrator.rs",
     "yaw_world_from_body_rad: velocity_mps.y.atan2(velocity_mps.x),",
     "yaw_world_from_body_rad: velocity_mps.x.atan2(velocity_mps.y),"),
    ("heading PID proportional gain 1% high",
     CORE + "plugin/controller/simple_aircraft_pid/simple_aircraft_pid.rs",
     "heading_gains: PidGains::from([1.0, 0.01, 2.0, 9.0]),",
     "heading_gains: PidGains::from([1.01, 0.01, 2.0, 9.0]),"),
    ("SimpleAircraft yaw rate 0.1% high", CORE + "plugin/motion/simple_aircraft/simple_aircraft.rs",
     "state.speed_mps / effective_turning_radius_m * state.roll_model_rad.tan();",
     "state.speed_mps / effective_turning_radius_m * state.roll_model_rad.tan() * 1.001;"),
]


def copy_workspace(destination):
    destination.mkdir(parents=True)
    for name in ("Cargo.toml", "Cargo.lock"):
        shutil.copy2(ROOT / name, destination / name)
    shutil.copytree(ROOT / "crates", destination / "crates",
                    ignore=shutil.ignore_patterns("target"))


def build(source, target, log):
    with log.open("w") as stream:
        subprocess.run(["cargo", "build", "--locked", "--package", "scrimmage-rs", "--bin",
                        "scrimmage"], cwd=source, env={**os.environ, "CARGO_TARGET_DIR": str(target)},
                       stdout=stream, stderr=subprocess.STDOUT, check=True)
    return target / "debug" / "scrimmage"


def evaluate(binary, cases, output):
    """Run every case and compare with its saved C++ run; returns per-mission findings."""
    findings = {}
    for case in cases:
        result = json.loads((case / "result.json").read_text())
        mission = Path(result["mission"]).name
        run = output / case.name
        completed = subprocess.run([
            binary, "run", case / "input/input.xml", "--root", ROOT, "--output", run,
            "--workers", "1", "--no-rerun", "--trace",
        ], capture_output=True, text=True)
        if completed.returncode:
            findings[mission] = ["run failed: " + completed.stderr.strip()[-200:]]
            continue
        comparison = compare_case(case / result["cpp_log_directory"], case / "cpp/trace.jsonl",
                                  run, run, case / "input/input.xml")
        layers = []
        if not comparison["comparison"]["passed"]:
            layers.append("frames")
        if comparison["summary_agreement"] == "unexplained":
            layers.append("summary")
        if not comparison["trace_comparison"]["passed"]:
            layers.append("trace")
        findings[mission] = layers
    return findings


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", type=Path, required=True, help="completed reference check")
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    args = parser.parse_args(argv)
    args.output = args.output.resolve()  # Cargo runs in the copied source folder.
    cases = sorted(path.parent for path in args.check.glob("*/result.json"))
    args.output.mkdir(parents=True)
    source, target = args.output / "source", args.output / "target"
    copy_workspace(source)
    report = {"check": str(args.check), "defects": []}

    binary = build(source, target, args.output / "control-build.log")
    control = evaluate(binary, cases, args.output / "control")
    report["control"] = control
    write_json(args.output / "report.json", report)
    if any(control.values()):
        raise SystemExit(f"control run is not clean: {control}")

    for index, (name, relative, original, defective) in enumerate(DEFECTS):
        path = source / relative
        text = path.read_text()
        if text.count(original) != 1:
            raise SystemExit(f"defect {name!r}: original text not found exactly once in {relative}")
        path.write_text(text.replace(original, defective))
        try:
            binary = build(source, target, args.output / f"defect-{index:02}-build.log")
            findings = evaluate(binary, cases, args.output / f"defect-{index:02}")
        finally:
            path.write_text(text)
        detected = {mission: layers for mission, layers in findings.items() if layers}
        report["defects"].append({"name": name, "file": relative, "detected": bool(detected),
                                  "missions": detected})
        write_json(args.output / "report.json", report)
        print(f"{'DETECTED' if detected else 'MISSED  '} {name}: "
              f"{sorted({layer for layers in detected.values() for layer in layers})}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
