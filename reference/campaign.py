#!/usr/bin/env python3
"""Rerun every C++ comparison and refresh the paper's data and tables.

    python3 reference/campaign.py            # everything (about two hours)
    python3 reference/campaign.py --quick    # agreement matrix only (about 20 minutes)
    python3 reference/campaign.py --bench    # also run reference/perf.py
    python3 reference/campaign.py --no-retain

Needs Docker, Cargo, and a C++ SCRIMMAGE checkout with local Ubuntu-24.04 and
benchmarking-edits branches: ../scrimmage by default, or --source PATH (see
README.md). Each step writes to runs/campaign-NNN/<step>/ with a log beside it:

  matrix        reference_check.py on missions.txt
  sensitivity   sensitivity.py against the matrix's saved C++ runs
  noise         the noise mission, then noise_statistics.py
  generated     100 generated scenarios (seed 1)
  long          10 long scenarios (seed 7, 1,000 s), then divergence.py
  platform      platform_check.py on the curated and generated missions
  perf          perf.py, compared with its history (--bench only)

Each step is judged from its report, not only its exit code, because some
mismatches are results rather than failures: randomized spawning against the
unmodified C++ build differs by design, the noise mission's random payloads
differ, and long runs may diverge late. A step FAILS if it did not run, if the
matrix, generated scenarios, or sensitivity study did not pass, or if the noise
or long checks fail beyond those expected differences. The platform step reports
Linux-versus-macOS rounding differences; it fails only if it cannot run.

The paper's data is replaced only if no step failed: new data is prepared in a
staging folder first, then swapped in, and the tables are regenerated. The
campaign prints the headline numbers before and after, writes summary.json last,
and exits non-zero if any step failed. Standard library only.
"""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys

from compare import POSITION_TOLERANCE_M, VELOCITY_TOLERANCE_MPS
from traces import COVARIANCE_TOLERANCE, OUTPUT_TOLERANCE

ROOT = Path(__file__).resolve().parents[1]
REFERENCE = ROOT / "reference"
DATA = ROOT / "paper/data"


def allocate(runs):
    number = 0
    while True:
        path = runs / f"campaign-{number:03}"
        try:
            path.mkdir(parents=True)
            return path
        except FileExistsError:
            number += 1


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


def headline(folders):
    """The numbers to watch between campaigns, from a set of result folders."""
    numbers = {}
    for name in ("matrix", "generated", "long"):
        cases = [case for case in results(folders[name]) if case]
        if cases:
            numbers[name] = f"{sum(case['passed'] for case in cases)} of {len(cases)} pass"
            passing = [case["comparison"]["max_position_error_m"]
                       for case in cases if case["passed"]]
            if passing:
                numbers[f"{name} worst position (passing)"] = f"{max(passing):.2e} m"
    sensitivity = read(Path(folders["sensitivity"]) / "report.json")
    if sensitivity:
        defects = sensitivity["defects"]
        numbers["planted bugs"] = f"{sum(d['detected'] for d in defects)} of {len(defects)} caught"
    noise = read(Path(folders["noise"]) / "noise-statistics.json")
    if noise:
        numbers["noise smallest p"] = f"{min(q['p_value'] for q in noise['quantities'].values()):.3f}"
    platform = read(Path(folders["platform"]) / "report.json")
    if platform:
        numbers["platform worst position"] = f"{platform['max_position_difference_m']:.2e} m"
    return numbers


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--quick", action="store_true", help="agreement matrix only")
    parser.add_argument("--bench", action="store_true", help="also run reference/perf.py")
    parser.add_argument("--no-retain", action="store_true",
                        help="leave paper/data and the tables unchanged")
    parser.add_argument("--source", type=Path, default=ROOT.parent / "scrimmage",
                        help="C++ SCRIMMAGE checkout (default: ../scrimmage)")
    args = parser.parse_args(argv)
    out = allocate(ROOT / "runs")
    print(f"Campaign: {out}", flush=True)
    codes, outcomes = {}, {}

    def step(name, *arguments):
        command = [sys.executable, *[str(argument) for argument in arguments]]
        print(f"  {name} ...", flush=True)
        with (out / f"{name}.log").open("w") as log:
            codes[name] = subprocess.run(command, cwd=ROOT, stdout=log,
                                         stderr=subprocess.STDOUT).returncode
        return codes[name]

    def judge(name, ok, note=""):
        outcomes[name] = "ok" if ok else "FAILED"
        print(f"  {name}: {outcomes[name]}{' - ' + note if note else ''}", flush=True)

    check = [REFERENCE / "reference_check.py", "--source", args.source.resolve()]
    step("matrix", *check, "--output", out / "matrix")
    judge("matrix", (read(out / "matrix/report.json") or {}).get("passed", False))
    spawning = next((out / "matrix").glob("*-aircraft-substeps-spawning"), None)
    if spawning is not None and (spawning / "cpp-baseline/logs").is_dir():
        # Quoted in the paper: the unmodified build's random engine differs, so
        # compare.py is expected to report a mismatch (exit 1), not an error.
        code = step("unmodified-spawning", REFERENCE / "compare.py",
                    next((spawning / "cpp-baseline/logs").iterdir()), spawning / "rust-1",
                    "--report", out / "matrix/unmodified-linux-randomized-spawning.json")
        judge("unmodified-spawning", code == 1, "differs, as expected")

    if not args.quick:
        code = step("sensitivity", REFERENCE / "sensitivity.py", "--check", out / "matrix",
                    "--output", out / "sensitivity")
        report = read(out / "sensitivity/report.json") or {}
        judge("sensitivity", code == 0 and bool(report.get("defects"))
              and not any(report["control"].values())
              and all(defect["detected"] for defect in report["defects"]))

        step("noise", *check, "--mission", ROOT / "missions/verification/noisy-contacts-noise.xml",
             "--output", out / "noise")
        case = read(out / "noise/00-noisy-contacts-noise/result.json")
        judge("noise", ran_cleanly(case) and case["comparison"]["passed"] and only_noise_differs(case)
              and case["summary_agreement"] != "unexplained", "random payloads differ, as expected")
        judge("noise-statistics", step("noise-statistics", REFERENCE / "noise_statistics.py",
                                       out / "noise/00-noisy-contacts-noise", "--report",
                                       out / "noise/noise-statistics.json") == 0)

        judge("generate", step("generate", REFERENCE / "generate_scenarios.py", "--count", 100,
                               "--seed", 1, "--output", out / "generated-missions") == 0)
        step("generated", *check, "--mission-dir", out / "generated-missions",
             "--output", out / "generated")
        judge("generated", (read(out / "generated/report.json") or {}).get("passed", False)
              and len(results(out / "generated")) == 100)

        judge("generate-long", step("generate-long", REFERENCE / "generate_scenarios.py", "--count",
                                    10, "--seed", 7, "--horizon", 1000,
                                    "--output", out / "long-missions") == 0)
        step("long", *check, "--mission-dir", out / "long-missions", "--timeout", 3600,
             "--output", out / "generated-long")
        cases = results(out / "generated-long")
        # Late numerical divergence is a result, but every run must finish cleanly and
        # any summary difference must be explained by the scoring rules.
        judge("long", len(cases) == 10 and all(ran_cleanly(case) for case in cases)
              and all(case["summary_agreement"] != "unexplained" for case in cases),
              f"{sum(case['passed'] for case in cases if case)} of {len(cases)} agree throughout")
        judge("divergence", step("divergence", REFERENCE / "divergence.py", out / "generated-long",
                                 "--output", out / "divergence") == 0)
        judge("platform", step("platform", REFERENCE / "platform_check.py", "--output",
                               out / "platform", "--mission-dir", out / "generated-missions") == 0)
    if args.bench:
        judge("perf", step("perf", REFERENCE / "perf.py") == 0, f"see {out / 'perf.log'}")

    failed = [name for name, outcome in outcomes.items() if outcome != "ok"]
    summary = {"exit_codes": codes, "outcomes": outcomes, "retained": False}
    new = {"matrix": out / "matrix", "generated": out / "generated",
           "long": out / "generated-long", "sensitivity": out / "sensitivity",
           "noise": out / "noise", "platform": out / "platform"}
    old = {"matrix": DATA / "cpp-comparison", "generated": DATA / "generated",
           "long": DATA / "generated-long", "sensitivity": DATA / "sensitivity",
           "noise": DATA / "noise", "platform": DATA / "platform"}
    before, after = headline(old), headline(new)
    summary["headline"] = {"before": before, "after": after}
    print("\nWhat moved (paper data before -> this campaign):", flush=True)
    for key in sorted(before.keys() | after.keys()):
        print(f"  {key:<34} {before.get(key, '-'):>16} -> {after.get(key, '-')}", flush=True)

    if failed:
        print(f"\nFAILED: {', '.join(failed)}. Paper data left unchanged; see the logs in {out}.")
    elif not args.no_retain:
        summary["retained"] = retain(out, args.quick, step)
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    if failed or (not args.no_retain and not summary["retained"]):
        return 1
    print(f"\nDone: {out}.{' Rebuild the paper and check the prose.' if summary['retained'] else ''}")
    return 0


def retain(out, quick, step):
    """Stage the new data, then swap it in and regenerate the tables."""
    staging = DATA / ".campaign-staging"
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir()
    kept = [("matrix", "cpp-comparison",
             ["--extra", out / "matrix/unmodified-linux-randomized-spawning.json"])]
    if not quick:
        shutil.copyfile(out / "divergence/report.json", out / "divergence.json")
        kept += [
            ("sensitivity", "sensitivity", []),
            ("noise", "noise", ["--extra", out / "noise/noise-statistics.json"]),
            ("generated", "generated", ["--missions", out / "generated-missions"]),
            ("generated-long", "generated-long",
             ["--extra", out / "divergence.json", "--missions", out / "long-missions"]),
            ("platform", "platform", []),
        ]
    for source, destination, extra in kept:
        if step(f"retain-{destination}", DATA / "retain_check.py", out / source,
                staging / destination, *extra) != 0:
            print(f"Retaining {destination} failed; paper data left unchanged.")
            shutil.rmtree(staging)
            return False
    for _, destination, _ in kept:
        shutil.rmtree(DATA / destination, ignore_errors=True)
        (staging / destination).rename(DATA / destination)
    staging.rmdir()
    if step("tables", DATA / "make_tables.py") != 0:
        print("Regenerating the tables failed; see tables.log.")
        return False
    return True


if __name__ == "__main__":
    raise SystemExit(main())
