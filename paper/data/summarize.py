#!/usr/bin/env python3
"""Write paper/data/SUMMARY.md, the readable detail behind the retained evidence.

    python3 paper/data/summarize.py

reference/suite.py --retain runs it after refreshing the evidence. The paper
keeps one summary table; this file holds the detail it points to. Edit this
script, not its output. Standard library only.
"""
import json
import math
import sys
from pathlib import Path

DATA = Path(__file__).resolve().parent
OUTPUT = DATA / "SUMMARY.md"
# Each folder is one check's evidence, written by reference/suite.py --retain;
# dates, sources, and references are in release.json and inside each report.
COMPARISON = DATA / "cpp-agreement"
GENERATED = DATA / "cpp-generated"
LONG = DATA / "cpp-long"
SENSITIVITY = DATA / "checker-sensitivity" / "report.json"
NOISE = DATA / "cpp-noise" / "noise-statistics.json"
SPATIAL = DATA / "speed-spatial" / "timings.json"
MESSAGES = DATA / "speed-messages" / "timings.json"
STALL = DATA / "stall-landing" / "results.json"
RELEASE = DATA / "release.json"

sys.path.insert(0, str(DATA.parents[1] / "reference"))
from reference_check import comparison_missions  # noqa: E402

# Labels for the agreement table come from reference/missions.txt, by case ID.
MODELS = {case: label for case, _, label in comparison_missions()}


def cases(folder):
    """The case results of a retained reference check, in the order they ran."""
    report = folder / "report.json"
    return json.loads(report.read_text())["cases"] if report.is_file() else []


def read(path):
    return json.loads(path.read_text()) if path.is_file() else None


def number(value):
    """Three significant figures; scientific notation outside 0.01-1000."""
    if value == 0:
        return "0"
    if 1e-2 <= abs(value) < 1e3:
        return f"{value:.3g}"
    exponent = math.floor(math.log10(abs(value)))
    return f"{value / 10**exponent:.1f}e{exponent}"


def table(header, rows):
    lines = ["| " + " | ".join(header) + " |", "|" + " --- |" * len(header)]
    lines += ["| " + " | ".join(str(cell) for cell in row) + " |" for row in rows]
    return "\n".join(lines)


def section(title, text, body):
    return f"## {title}\n\n{text}\n\n{body}\n"


def checks():
    release = read(RELEASE)
    if release is None:
        return None
    rows = [(f"`{name}`", check["purpose"], check["reference"],
             ("passed" if check["passed"] else "**failed**")
             + (f"; {check['note']}" if check["note"] else ""))
            for name, check in release["checks"].items()]
    sources = {check["source_sha256"] for check in release["checks"].values()}
    ran_on = (f"all on source `{next(iter(sources))[:12]}`" if len(sources) == 1
              else f"on {len(sources)} different sources")
    return section("Checks", f"Every check of `reference/suite.py` in this release, {ran_on} "
                   "(`release.json`).",
                   table(("Check", "What it checks", "Against", "Outcome"), rows))


def agreement():
    results = cases(COMPARISON)
    if not results:
        return None
    rows = []
    for result in results:
        frames, trace = result["comparison"], result["trace_comparison"]
        outcome = "match" if result["passed"] else "**differs**"
        if result["passed"] and result["summary_agreement"] == "explained_by_scoring_rules":
            outcome += " (summary explained by the scoring rule)"
        rows.append((f"`{result['case']}`", MODELS.get(result["case"], ""),
                     frames["compared_entity_states"], number(frames["max_position_error_m"]),
                     number(frames["max_velocity_error_mps"]),
                     number(frames["max_orientation_error_rad"]), trace["deliveries"],
                     trace.get("sensor_states", "-"), trace["outputs"], number(trace["max_output_error"]),
                     outcome))
    return section(
        "Agreement with SCRIMMAGE",
        "Each curated mission (`reference/missions.txt`) against the comparison branch. "
        "Largest errors over all compared states: position (m), velocity (m/s), attitude "
        "(rad). Deliveries, sensor states, and plugin outputs are compared in the trace; "
        "every delivery matched in sender, receiver, tick, and order.",
        table(("Case", "Exercises", "States", "Position", "Velocity", "Attitude",
               "Deliveries", "Sensor states", "Outputs", "Output error", "Result"), rows))


def generated():
    results = cases(GENERATED)
    if not results:
        return None
    frames = [result["comparison"] for result in results]
    traces = [result["trace_comparison"] for result in results]
    agreement_kinds = [result["summary_agreement"] for result in results]
    features = [("collisions", ">SimpleCollision<"), ("ground", ">GroundCollision<"),
                ("boundary", ">Boundary<"), ("scheduled spawns", "<generate_rate>"),
                ("NoisyState", ">NoisyState<"), ("NoisyContacts", ">NoisyContacts<"),
                ("fixed-wing", ">FixedWing6DOF<"), ("point-mass", ">SingleIntegrator<")]
    texts = [(GENERATED / r["case"] / "input.xml").read_text() for r in results]
    rows = [
        ("Scenarios passing", f"{sum(r['passed'] for r in results)} of {len(results)}"),
        ("Compared entity states", f"{sum(f['compared_entity_states'] for f in frames):,}"),
        ("Compared deliveries / outputs",
         f"{sum(t['deliveries'] for t in traces):,} / {sum(t['outputs'] for t in traces):,}"),
        ("Largest position / velocity error",
         f"{number(max(f['max_position_error_m'] for f in frames))} m / "
         f"{number(max(f['max_velocity_error_mps'] for f in frames))} m/s"),
        ("Largest attitude / output error",
         f"{number(max(f['max_orientation_error_rad'] for f in frames))} rad / "
         f"{number(max(t['max_output_error'] for t in traces))}"),
        ("Summaries identical / explained",
         f"{agreement_kinds.count('agree')} / "
         f"{agreement_kinds.count('explained_by_scoring_rules')}"),
        *[(f"Scenarios with {label}", sum(marker in text for text in texts))
          for label, marker in features],
    ]
    return section("Generated scenarios",
                   f"{len(results)} scenarios from `generate_scenarios.py` (seed 1), checked "
                   "as the curated missions are.", table(("Measure", "Value"), rows))


def long_runs():
    divergence = read(LONG / "divergence.json")
    if divergence is None:
        return None

    def when(seconds):
        return "never" if seconds is None else f"{seconds:.1f} s"

    rows = [(f"`{case['case']}`", f"{case['end_s']:.0f} s",
             when(case["cpp_vs_rust_exceeds_tolerance_at_s"]),
             when(case["rust_vs_nudged_rust_exceeds_tolerance_at_s"]),
             number(case["cpp_vs_rust_final_m"])) for case in divergence["cases"]]
    return section(
        "Long runs",
        "When SCRIMMAGE and Ripple first differ by more than 1e-8 m and, as a control, when "
        "Ripple differs from itself after one-ulp changes to initial roll, commanded speed, "
        "and bank limit, with each controller loop's proportional gain scaled by 1 + 1e-12. "
        "Final: the largest position difference at the end (m).",
        table(("Scenario", "Length", "SCRIMMAGE vs Ripple", "Ripple vs nudged", "Final"),
              rows))


def noise():
    report = read(NOISE)
    if report is None:
        return None
    rows = [(name, f"{q['cpp_mean']:.3f} / {q['rust_mean']:.3f}",
             f"{q['cpp_std']:.3f} / {q['rust_std']:.3f}", f"{q['ks_statistic']:.4f}",
             f"{q['p_value']:.3f}") for name, q in report["quantities"].items()]
    samples = next(iter(report["quantities"].values()))["cpp_samples"]
    noise_cases = cases(NOISE.parent)
    one_for_one = ""
    if noise_cases and {"sensor_states", "max_payload_position_error_m"} <= set(
            noise_cases[0]["trace_comparison"]):
        trace = noise_cases[0]["trace_comparison"]
        one_for_one = (f"The {trace['sensor_states']:,} measured states agree one for one, to "
                       f"{number(trace['max_payload_position_error_m'])} m. ")
    return section(
        "Sensor noise",
        f"{one_for_one}NoisyContacts error distributions, SCRIMMAGE / Ripple, from "
        f"{samples:,} samples per quantity: two-sample Kolmogorov-Smirnov test, Bonferroni "
        f"threshold {report['per_test_threshold']:.4f}. Position and velocity errors in m "
        "and m/s; attitude as the error rotation angle.",
        table(("Quantity", "Mean", "Std. dev.", "D", "p"), rows))


def sensitivity():
    report = read(SENSITIVITY)
    if report is None:
        return None
    rows = []
    for defect in report["defects"]:
        layers = sorted({layer for found in defect["missions"].values() for layer in found})
        rows.append((defect["name"], len(defect["missions"]),
                     ", ".join(layers) if layers else "**missed**"))
    detected = sum(defect["detected"] for defect in report["defects"])
    return section(
        "Planted bugs",
        f"{detected} of {len(report['defects'])} detected. Each is a one-line change to a "
        "copy of the source, compared with the saved SCRIMMAGE runs of the curated missions; "
        "missions counts those that failed, and the layers are those that reported it.",
        table(("Defect", "Missions", "Detected by"), rows))


def spatial():
    timings = read(SPATIAL)
    if timings is None:
        return None
    rows = []
    for name, case in timings["cases"].items():
        ms = {variant: value["median_ms_per_step"] for variant, value in case["variants"].items()}
        others = {width: value for width, value in ms.items() if width not in ("none", "scan", "5")}
        best = min(others, key=others.get) if others else None
        rows.append((name.rsplit("-", 1)[0].replace("-", " "), f"{case['agents']:,}",
                     f"{case['collision_range_m']:g}", case["collisions"],
                     *[f"{ms[v]:.2f}" if v in ms else "-" for v in ("none", "scan", "5")],
                     f"{others[best]:.2f} ({best} m)" if best else "-"))
    return section(
        "Collision checking cost",
        f"Milliseconds per step with SimpleCollision, median of {timings['repeats']} runs, "
        f"one worker, {timings['machine']}. Every variant except the one without a collision "
        "check wrote identical frames and events. Sparse: aircraft over about a kilometre; "
        "clustered: a 20 m spread.",
        table(("Case", "Agents", "Range (m)", "Collisions", "No check", "Scan", "Grid 5 m",
               "Best other grid"), rows))


def messages():
    timings = read(MESSAGES)
    if timings is None:
        return None
    by_agents = {}
    for case in timings["cases"]:
        by_agents.setdefault(case["agents"], {})[case["case"]] = case["variants"]
    rows = [(f"{agents:,}", *[f"{runs[label][variant]['ms_per_step']:.2f}"
                              for label in ("without messages", "NoisyState")
                              for variant in ("cpp", "rust")])
            for agents, runs in by_agents.items()]
    return section(
        "Message routing cost",
        f"Milliseconds per step for perf.py's aircraft, without and with a message from every "
        f"aircraft to itself, in native SCRIMMAGE and Ripple: median of {timings['repeats']} "
        f"runs, one thread, {timings['provenance']['platform']}.",
        table(("Agents", "SCRIMMAGE, none", "Ripple, none", "SCRIMMAGE, messages",
               "Ripple, messages"), rows))


def stall():
    report = read(STALL)
    if report is None:
        return None
    labels = {"linear": "Linear", "lift": "Lift table", "lift-drag": "Both tables"}
    rows = [(label, *[f"{report['results'][variant][key]:.2f}" for key in (
        "touchdown_ground_speed_mps", "touchdown_sink_mps", "max_bounce_height_m",
        "max_rollout_offset_m")], f"{report['results'][variant]['max_alpha_airborne_deg']:.1f}")
        for variant, label in labels.items()]
    return section(
        "Stall tables and the Rascal's landing",
        "ArduPlane's runway landing with and without stall tables: speed and sink rate at "
        "first touchdown, highest bounce, widest offset from the centreline, and largest "
        "airborne angle of attack in the last 15 s.",
        table(("Lift and drag", "Speed (m/s)", "Sink (m/s)", "Bounce (m)", "Offset (m)",
               "Alpha (deg)"), rows))


def main():
    parts = [part for part in (checks(), agreement(), generated(), long_runs(), noise(),
                               sensitivity(), spatial(), messages(), stall()) if part]
    OUTPUT.write_text("# Evidence summary\n\nGenerated by `paper/data/summarize.py` from the "
                      "retained evidence in this folder; do not edit.\n\n"
                      + "\n".join(parts))


if __name__ == "__main__":
    main()
