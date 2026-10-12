#!/usr/bin/env python3
"""Run the checks: a quick set after a change, the release set before a release.

    python3 reference/suite.py --list              # every check: purpose, reference, criterion
    python3 reference/suite.py                     # the quick set (about 30 minutes)
    python3 reference/suite.py --release           # every check (several hours)
    python3 reference/suite.py --check cpp-agreement --check rust-determinism
    python3 reference/suite.py --release --retain  # also refresh the release evidence

Each check has a stable ID and runs one of the scripts in this folder. Output
goes to runs/suiteNNN/<check>/, with a log per step and summary.json last;
earlier runs are never reused, and a failing check keeps its diagnostics there.
The suite exits non-zero if any selected check fails. A check that needs a
prerequisite (the planted-bug study needs the agreement matrix) runs it too.

--retain refreshes the release evidence, paper/data/<check>/, but only when
every selected check passed: the new evidence is staged first, then swapped in
for the checks that ran, and paper/data/release.json records each check's
outcome, date, source hash, and references. The manuscript tables are then
regenerated. Review the result before committing it: retaining never accepts
a failure, and a new performance baseline is a separate, explicit step
(perf.py --accept).

Needs Python 3.10+ and Cargo; the C++ checks need Docker and a C++ SCRIMMAGE
checkout with local Ubuntu-24.04 and benchmarking-edits branches (../scrimmage,
or --source); the native timings need macOS; stall-landing needs Docker for
ArduPilot SITL. Standard library only.
"""

import argparse
from dataclasses import dataclass, field
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from compare import POSITION_TOLERANCE_M, VELOCITY_TOLERANCE_MPS
from project import DATA, ROOT, RUNS, allocate, source_identity
import retain
from traces import COVARIANCE_TOLERANCE, OUTPUT_TOLERANCE

REFERENCE = ROOT / "reference"
QUICK, RELEASE = "quick", "release"


@dataclass
class Check:
    id: str
    purpose: str
    reference: str
    criterion: str
    sets: tuple
    run: object = None        # function(context) -> (passed, note)
    keep: object = None       # function(run folder, evidence folder); None keeps nothing
    requires: tuple = field(default_factory=tuple)


class Context:
    """One suite run: its folder, the C++ checkout, and a logged step runner."""

    def __init__(self, out, source):
        self.out, self.source = out, source
        self.codes = {}

    def step(self, name, *command, python=True):
        """Run a command with its output in <name>.log; return its exit code."""
        command = [sys.executable, *command] if python else list(command)
        print(f"  {name} ...", flush=True)
        # native_build.sh reads the C++ checkout from CPP_SOURCE.
        environment = {**os.environ, "CPP_SOURCE": str(self.source)}
        with (self.out / f"{name}.log").open("w") as log:
            self.codes[name] = subprocess.run([str(part) for part in command], cwd=ROOT,
                                              env=environment, stdout=log,
                                              stderr=subprocess.STDOUT).returncode
        return self.codes[name]

    def check_cpp(self, name, *arguments):
        """reference_check.py into <name>/, against the C++ checkout."""
        return self.step(name, REFERENCE / "reference_check.py", "--source", self.source,
                         "--output", self.out / name, *arguments)


def read(path):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return None


def results(folder):
    return [read(path) for path in sorted(Path(folder).glob("*/result.json"))]


def ran_cleanly(result):
    """The case ran, instrumentation changed nothing, Rust was deterministic, and its
    output did not depend on tracing or recording. A missing check counts as a failure."""
    if result is None or "error" in result:
        return False
    try:
        return (all(all(files.values()) for files in result["cpp_non_interference"].values())
                and all(all(files.values()) for files in result["worker_equality"].values())
                and all(result["trace_equality"].values())
                and all(result["recording_equality"].values()))
    except KeyError:
        return False


def only_noise_differs(result):
    """The noise mission's random payload values differ by design; the deliveries,
    covariances, owner beliefs, and controller outputs must still agree."""
    trace = result["trace_comparison"]
    return (trace["delivery_mismatches"] == 0
            and trace["max_payload_covariance_error"] <= COVARIANCE_TOLERANCE
            and trace["max_output_error"] <= OUTPUT_TOLERANCE
            and trace["max_belief_position_error_m"] <= POSITION_TOLERANCE_M
            and trace["max_belief_velocity_error_mps"] <= VELOCITY_TOLERANCE_MPS)


# Each run_* function runs one check in context.out/<check id>/ and judges it
# from its report, not only the exit code: some mismatches are results.

def run_rust_tests(context):
    commands = [("fmt", ["cargo", "fmt", "--all", "--", "--check"]),
                ("clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--locked",
                            "--", "-D", "warnings"]),
                ("test", ["cargo", "test", "--workspace", "--locked"]),
                ("reference-tools", [sys.executable, "-m", "unittest", "discover", "-s",
                                     "reference", "-p", "test_*.py"]),
                ("scripts", [sys.executable, "-m", "unittest", "discover", "-s", "scripts",
                             "-p", "test_*.py"])]
    failed = [name for name, command in commands
              if context.step(f"rust-tests-{name}", *command, python=False) != 0]
    return not failed, f"failed: {', '.join(failed)}" if failed else ""


def run_determinism(context):
    code = context.step("rust-determinism", REFERENCE / "determinism.py",
                        "--output", context.out / "rust-determinism")
    report = read(context.out / "rust-determinism/report.json") or {}
    return code == 0 and report.get("passed", False), \
        f"{report.get('missions', 0)} missions, {report.get('missions_differing', '?')} differing"


def run_agreement(context):
    context.check_cpp("cpp-agreement")
    folder = context.out / "cpp-agreement"
    passed = (read(folder / "report.json") or {}).get("passed", False)
    spawning = folder / "substeps-random-spawn"
    if (spawning / "cpp-baseline/logs").is_dir():
        # Quoted in the report: the unmodified build's random engine differs, so
        # compare.py is expected to report a mismatch (exit 1), not an error.
        code = context.step("cpp-agreement-unmodified-spawning", REFERENCE / "compare.py",
                            next((spawning / "cpp-baseline/logs").iterdir()), spawning / "rust-1",
                            "--report", folder / "unmodified-linux-randomized-spawning.json")
        passed = passed and code == 1
    cases = [case for case in results(folder) if case]
    return passed, f"{sum(case['passed'] for case in cases)} of {len(cases)} pass"


def run_generated(context):
    missions = context.out / "cpp-generated-missions"
    if context.step("cpp-generated-generate", REFERENCE / "generate_scenarios.py", "--count", 100,
                    "--seed", 1, "--output", missions) != 0:
        return False, "generation failed"
    context.check_cpp("cpp-generated", "--mission-dir", missions)
    cases = [case for case in results(context.out / "cpp-generated") if case]
    passed = (read(context.out / "cpp-generated/report.json") or {}).get("passed", False)
    return passed and len(cases) == 100, f"{sum(case['passed'] for case in cases)} of {len(cases)} pass"


def run_long(context):
    missions = context.out / "cpp-long-missions"
    if context.step("cpp-long-generate", REFERENCE / "generate_scenarios.py", "--count", 10,
                    "--seed", 7, "--horizon", 1000, "--output", missions) != 0:
        return False, "generation failed"
    context.check_cpp("cpp-long", "--mission-dir", missions, "--timeout", 3600)
    folder = context.out / "cpp-long"
    cases = results(folder)
    # Late numerical divergence is a result, but every run must finish cleanly and
    # any summary difference must be explained by the scoring rules.
    clean = len(cases) == 10 and all(ran_cleanly(case) for case in cases) \
        and all(case["summary_agreement"] != "unexplained" for case in cases)
    divergence = context.step("cpp-long-divergence", REFERENCE / "divergence.py", folder,
                              "--output", context.out / "cpp-long-divergence")
    if divergence == 0:
        shutil.copyfile(context.out / "cpp-long-divergence/report.json", folder / "divergence.json")
    return clean and divergence == 0, \
        f"{sum(case['passed'] for case in cases if case)} of {len(cases)} agree throughout"


def run_noise(context):
    context.check_cpp("cpp-noise", "--mission",
                      ROOT / "missions/verification/noisy-contacts-noise.xml")
    case_folder = context.out / "cpp-noise/noisy-contacts-noise"
    case = read(case_folder / "result.json")
    clean = (ran_cleanly(case) and case["comparison"]["passed"] and only_noise_differs(case)
             and case["summary_agreement"] != "unexplained")
    statistics = context.step("cpp-noise-statistics", REFERENCE / "noise_statistics.py",
                              case_folder, "--report", context.out / "cpp-noise/noise-statistics.json")
    return clean and statistics == 0, "random payloads differ, as expected"


def run_sensitivity(context):
    code = context.step("checker-sensitivity", REFERENCE / "sensitivity.py", "--check",
                        context.out / "cpp-agreement", "--output", context.out / "checker-sensitivity")
    report = read(context.out / "checker-sensitivity/report.json") or {}
    defects = report.get("defects", [])
    passed = code == 0 and bool(defects) and not any(report["control"].values()) \
        and all(defect["detected"] for defect in defects)
    return passed, f"{sum(d['detected'] for d in defects)} of {len(defects)} planted bugs caught"


def run_platform(context):
    # Differences between platforms are results; the check fails only if it cannot run.
    code = context.step("rust-platform", REFERENCE / "platform_check.py",
                        "--output", context.out / "rust-platform")
    report = read(context.out / "rust-platform/report.json") or {}
    return code == 0 and "cases" in report, \
        f"largest position difference {report.get('max_position_difference_m', '?')} m"


def run_perf(context):
    (context.out / "speed-regression").mkdir()
    code = context.step("speed-regression", REFERENCE / "perf.py", "--output",
                        context.out / "speed-regression/perf.json")
    notes = {0: "no workload slower than the baseline", 1: "slower than the baseline",
             2: "no compatible baseline; review, then perf.py --accept"}
    return code == 0, notes.get(code, "failed to run")


def script_check(script, name, with_source=False):
    """A check that runs one script into context.out/<name> and passes on exit code 0."""
    def run(context):
        source = ["--source", context.source] if with_source else []
        code = context.step(name, REFERENCE / script, "--output", context.out / name, *source)
        return code == 0, ""
    return run


def keep_reference_check(run, evidence):
    retain.copy_reference_check(run, evidence)


def keep_named(*names):
    def keep(run, evidence):
        retain.copy_files(run, evidence, [name for name in names if (Path(run) / name).exists()])
    return keep


CHECKS = [
    # First, so it times an idle machine: Docker builds and runs slow it measurably.
    Check("speed-regression", "Rust speed and memory have not regressed",
          "reference/perf_baseline.json, accepted on the same machine and toolchain",
          "no workload's median outside the baseline's range and over 10% slower",
          (QUICK, RELEASE), run_perf, keep_named("perf.json")),
    Check("rust-tests", "Rust unit, integration, and mission regression tests; formatting; "
          "lints; tests of these checking tools",
          "expected values in the tests", "every test passes, no lint or format finding",
          (QUICK, RELEASE), run_rust_tests),
    Check("rust-determinism", "worker counts, recording, tracing, and spatial lookup method "
          "change no output", "the one-worker run",
          "byte-identical frames, events, and summaries on every mission and workload",
          (QUICK, RELEASE), run_determinism, keep_named("report.json")),
    Check("cpp-agreement", "behaviour matches C++ SCRIMMAGE on the curated missions "
          "(reference/missions.txt)", "C++ SCRIMMAGE in Docker: the unmodified and comparison branches",
          "states within tolerance, identical deliveries, summaries equal or explained, "
          "C++ instrumentation and Rust variants byte-identical",
          (QUICK, RELEASE), run_agreement,
          lambda run, evidence: retain.copy_reference_check(
              run, evidence, [path for path in [Path(run) / "unmodified-linux-randomized-spawning.json"]
                              if path.is_file()])),
    Check("cpp-generated", "agreement over 100 random scenarios (seed 1)", "C++ SCRIMMAGE",
          "as cpp-agreement, all 100 pass", (RELEASE,), run_generated, keep_reference_check),
    Check("cpp-long", "agreement over ten 1,000 s scenarios (seed 7), and whether late "
          "divergence is the port or the dynamics", "C++ SCRIMMAGE; Rust after one-ulp nudges",
          "every run finishes cleanly with explained summaries; divergence is reported",
          (RELEASE,), run_long,
          lambda run, evidence: retain.copy_reference_check(run, evidence,
                                                            [Path(run) / "divergence.json"])),
    Check("cpp-noise", "sensor noise distributions match where samples cannot",
          "C++ SCRIMMAGE NoisyContacts samples",
          "deliveries, covariances, beliefs, outputs agree; no KS test below the Bonferroni threshold",
          (RELEASE,), run_noise,
          lambda run, evidence: retain.copy_reference_check(run, evidence,
                                                            [Path(run) / "noise-statistics.json"])),
    Check("checker-sensitivity", "the C++ comparison catches plausible porting bugs",
          "cpp-agreement's saved C++ runs", "an unmodified control passes; every planted bug is caught",
          (RELEASE,), run_sensitivity, keep_named("report.json"), requires=("cpp-agreement",)),
    Check("rust-platform", "Linux/amd64 and native builds agree", "the native build",
          "runs complete; differences are reported", (RELEASE,), run_platform, keep_named("report.json")),
    Check("speed-cpp-container", "C++ and Rust speed and memory in one Linux container",
          "C++ SCRIMMAGE comparison branch, same container",
          "Rust identical at 1 and 8 workers and to C++ where both draw the same numbers",
          (RELEASE,), script_check("benchmark.py", "speed-cpp-container", with_source=True),
          keep_named("timings.json", "provenance.json")),
    Check("speed-cpp-native", "C++ and Rust speed and memory on bare-metal macOS",
          "native C++ SCRIMMAGE comparison branch", "as speed-cpp-container",
          (RELEASE,), script_check("native_benchmark.py", "speed-cpp-native"),
          keep_named("timings.json"), requires=("speed-cpp-container",)),
    Check("speed-spatial", "collision checking cost with the scan and with grids",
          "the scan, which tests every contact", "every variant writes identical output",
          (RELEASE,), script_check("spatial_benchmark.py", "speed-spatial"), keep_named("timings.json")),
    Check("speed-messages", "message routing cost with all-agent traffic",
          "native C++ SCRIMMAGE", "both run; timings reported",
          (RELEASE,), script_check("message_benchmark.py", "speed-messages"),
          keep_named("timings.json", "profiles"), requires=("speed-cpp-container",)),
    Check("rk4-loads", "what evaluating forces at every RK4 stage changes",
          "unmodified C++ SCRIMMAGE, which holds forces through the step",
          "runs complete; errors reported", (RELEASE,),
          script_check("rk4_loads.py", "rk4-loads", with_source=True),
          keep_named("results.json", "summary.md")),
    Check("multirotor-control", "MultirotorVelocityController's step responses on the "
          "Crazyflie, and convergence as the step shrinks",
          "the commands; Förster's rotor drag; the 20-substep run",
          "every velocity error within 5% over the last second, Crazyflie drag tilt within 2%, 5 substeps within 5 mm",
          (QUICK, RELEASE), script_check("multirotor_control.py", "multirotor-control"),
          keep_named("results.json", "summary.md")),
    Check("rascal-control", "AircraftPIDController's rascal settings on FixedWing6DOF's Rascal: "
          "altitude derivative gain after upsets, and steps under each attitude limit",
          "the commands; the a4d limits", "altitude D changes the error under 1 cm; under the "
          "rascal limits every step settles and pitch stays under 25 degrees",
          (QUICK, RELEASE), script_check("rascal_control.py", "rascal-control"),
          keep_named("results.json", "summary.md")),
    Check("rotors-motor", "Multirotor's motor lag and rotor loads against RotorS, sample by sample",
          "RotorS's motor-plugin source (ethz-asl/rotors_simulator), compiled in Docker",
          "every sample within 1e-12 relative; samples equal the committed fixture",
          (RELEASE,), script_check("rotors_motor.py", "rotors-motor"),
          keep_named("report.json")),
    Check("crazyflow-crazyflie", "Multirotor's Crazyflie against Crazyflow's first-principles "
          "dynamics, same constants and motor commands",
          "Crazyflow (learnsyslab/crazyflow, pinned), run with uv",
          "without rotor drag, within 1e-6 m and 1e-6 rad over 3 s; the drag difference is reported",
          (RELEASE,), script_check("crazyflow_check.py", "crazyflow-crazyflie"),
          keep_named("results.json")),
    Check("stall-landing", "whether stall tables change ArduPlane's Rascal landing",
          "the same mission without tables, and a repeat of it",
          "the repeat is byte-identical", (RELEASE,), script_check("stall_landing.py", "stall-landing"),
          keep_named("results.json", "summary.md", "missions")),
]

BY_ID = {check.id: check for check in CHECKS}


def selected(arguments):
    """The checks to run, with prerequisites, in CHECKS order."""
    if arguments.check:
        unknown = [name for name in arguments.check if name not in BY_ID]
        if unknown:
            raise SystemExit(f"unknown check(s): {', '.join(unknown)}; see --list")
        wanted = set(arguments.check)
    else:
        wanted = {check.id for check in CHECKS
                  if (RELEASE if arguments.release else QUICK) in check.sets}
    for check in CHECKS:
        if check.id in wanted:
            wanted.update(check.requires)
    return [check for check in CHECKS if check.id in wanted]


def headline(folder):
    """The C++ agreement numbers to watch, from a set of result folders."""
    numbers = {}
    for name in ("cpp-agreement", "cpp-generated", "cpp-long"):
        cases = [case for case in results(folder / name) if case]
        if cases:
            numbers[name] = f"{sum(case['passed'] for case in cases)} of {len(cases)} pass"
            passing = [case["comparison"]["max_position_error_m"] for case in cases if case["passed"]]
            if passing:
                numbers[f"{name} worst position"] = f"{max(passing):.2e} m"
    sensitivity = read(folder / "checker-sensitivity/report.json")
    if sensitivity:
        defects = sensitivity["defects"]
        numbers["planted bugs"] = f"{sum(d['detected'] for d in defects)} of {len(defects)} caught"
    noise = read(folder / "cpp-noise/noise-statistics.json")
    if noise:
        numbers["noise smallest p"] = f"{min(q['p_value'] for q in noise['quantities'].values()):.3f}"
    return numbers


def references(out):
    """External reference versions recorded by this run's reports."""
    found = {"rustc": subprocess.check_output(["rustc", "--version"], text=True).strip()}
    for name in ("cpp-agreement", "cpp-generated", "cpp-long", "cpp-noise"):
        report = read(out / name / "report.json")
        if report and "cpp" in report:
            found["cpp"] = {key: {field: report[key][field]
                                  for field in ("branch", "commit", "image", "platform")}
                            for key in ("cpp_baseline", "cpp")}
            break
    return found


def retain_evidence(out, checks, outcomes, context):
    """Stage the evidence of the checks that ran, swap it in, update release.json, and
    regenerate the manuscript tables."""
    staging = DATA / ".suite-staging"
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir(parents=True)
    try:
        for check in checks:
            if check.keep is not None:
                check.keep(out / check.id, staging / check.id)
    except (OSError, StopIteration) as error:
        print(f"Retaining evidence failed ({error}); paper/data left unchanged.")
        shutil.rmtree(staging)
        return False
    for check in checks:
        if check.keep is not None:
            shutil.rmtree(DATA / check.id, ignore_errors=True)
            (staging / check.id).rename(DATA / check.id)
    staging.rmdir()
    release = read(DATA / "release.json") or {"checks": {}}
    identity = source_identity()["source_sha256"]
    for check in checks:
        release["checks"][check.id] = {
            "purpose": check.purpose, "reference": check.reference, "criterion": check.criterion,
            "passed": outcomes[check.id][0], "note": outcomes[check.id][1],
            "date": datetime.date.today().isoformat(), "source_sha256": identity,
            "evidence": f"paper/data/{check.id}/" if check.keep else None}
    release["checks"] = {check.id: release["checks"][check.id] for check in CHECKS
                         if check.id in release["checks"]}
    release["references"] = {**release.get("references", {}), **references(out)}
    (DATA / "release.json").write_text(json.dumps(release, indent=2) + "\n")
    if context.step("tables", DATA / "make_tables.py") != 0:
        print("Regenerating the tables failed; see tables.log.")
        return False
    return True


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--list", action="store_true", help="describe every check and exit")
    parser.add_argument("--release", action="store_true", help="run every check")
    parser.add_argument("--check", action="append", help="run this check (repeatable)")
    parser.add_argument("--retain", action="store_true",
                        help="refresh paper/data for the checks that ran, if all passed")
    parser.add_argument("--source", type=Path, default=ROOT.parent / "scrimmage",
                        help="C++ SCRIMMAGE checkout (default: ../scrimmage)")
    arguments = parser.parse_args(argv)
    if arguments.list:
        for check in CHECKS:
            print(f"{check.id}  [{', '.join(check.sets)}]\n  purpose:   {check.purpose}\n"
                  f"  reference: {check.reference}\n  passes if: {check.criterion}\n")
        return 0
    checks = selected(arguments)
    out = allocate(RUNS, "suite")
    context = Context(out, arguments.source.resolve())
    print(f"Suite: {out} ({', '.join(check.id for check in checks)})", flush=True)
    outcomes = {}
    for check in checks:
        try:
            outcomes[check.id] = check.run(context)
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
            outcomes[check.id] = (False, f"could not judge: {error}")
        passed, note = outcomes[check.id]
        print(f"  {check.id}: {'ok' if passed else 'FAILED'}{' - ' + note if note else ''}",
              flush=True)

    failed = [name for name, (passed, _) in outcomes.items() if not passed]
    before, after = headline(DATA), headline(out)
    if after:
        print("\nWhat moved (release evidence -> this run):", flush=True)
        for key in sorted(before.keys() | after.keys()):
            print(f"  {key:<34} {before.get(key, '-'):>16} -> {after.get(key, '-')}", flush=True)
    summary = {"checks": {name: {"passed": passed, "note": note}
                          for name, (passed, note) in outcomes.items()},
               "exit_codes": context.codes, "headline": {"before": before, "after": after},
               "retained": False}
    if failed:
        print(f"\nFAILED: {', '.join(failed)}. See the logs in {out}."
              + (" Evidence left unchanged." if arguments.retain else ""))
    elif arguments.retain:
        summary["retained"] = retain_evidence(out, checks, outcomes, context)
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    if failed or (arguments.retain and not summary["retained"]):
        return 1
    print(f"\nPassed: {out}."
          + (" Evidence refreshed; rebuild the paper and check the prose." if summary["retained"] else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
