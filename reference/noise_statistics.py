#!/usr/bin/env python3
"""Compare C++ and Rust NoisyContacts noise distributions from comparison traces.

    python3 reference/reference_check.py --mission missions/verification/noisy-contacts-noise.xml
    python3 reference/noise_statistics.py runs/reference-checkNNN/00-noisy-contacts-noise

Rust draws sensor noise from independent per-plugin streams, so samples cannot
match C++ one for one. This checks that the error distributions do. An error is
a published contact state minus that contact's true state when it was measured
(the next tick's logged belief; the mission has no own-state sensor, so belief is
truth). For position and velocity axes and the attitude error angle, a two-sample
Kolmogorov-Smirnov test compares C++ and Rust samples. The case passes when
every p-value exceeds 0.05 divided by the number of tests (Bonferroni).
Standard library only.
"""

import argparse
import json
import math
from pathlib import Path
import sys

from compare import orientation_distance_rad
from traces import read_trace, tick

ALPHA = 0.05


def errors(records):
    beliefs, publications = {}, []
    for record in records:
        if record["kind"] == "belief":
            beliefs.setdefault((tick(record), record["id"]), []).append(record)
        elif record["kind"] == "publication" and record["topic"] == "ContactsWithCovariances":
            publications.append(record)
    ticks = sorted({key[0] for key in beliefs})
    following = dict(zip(ticks, ticks[1:]))
    samples = {name: [] for name in ("pos_x", "pos_y", "pos_z", "vel_x", "vel_y", "vel_z",
                                     "attitude_rad")}
    for publication in publications:
        now = tick(publication)
        for state in publication["states"]:
            # Measured after motion: the next pre-step log, or at the end the
            # terminal frame (the second log of the final tick).
            if now in following:
                logged = beliefs.get((following[now], state["id"]), [])
                truth = logged[0] if logged else None
            else:
                logged = beliefs.get((now, state["id"]), [])
                truth = logged[1] if len(logged) > 1 else None
            if truth is None:
                continue
            for axis, name in enumerate("xyz"):
                samples[f"pos_{name}"].append(state["p"][axis] - truth["p"][axis])
                samples[f"vel_{name}"].append(state["v"][axis] - truth["v"][axis])
            samples["attitude_rad"].append(orientation_distance_rad(truth["q"], state["q"]))
    return samples


def ks_two_sample(first, second):
    """Kolmogorov-Smirnov statistic and asymptotic p-value (Numerical Recipes form)."""
    first, second = sorted(first), sorted(second)
    n, m = len(first), len(second)
    i = j = 0
    statistic = 0.0
    while i < n and j < m:
        value = min(first[i], second[j])
        while i < n and first[i] <= value:
            i += 1
        while j < m and second[j] <= value:
            j += 1
        statistic = max(statistic, abs(i / n - j / m))
    effective = math.sqrt(n * m / (n + m))
    lam = (effective + 0.12 + 0.11 / effective) * statistic
    if lam == 0:
        return statistic, 1.0
    if lam < 1.18:  # The alternating series converges slowly here; use its complement.
        y = math.exp(-math.pi ** 2 / (8 * lam * lam))
        p = 1 - math.sqrt(2 * math.pi) / lam * sum(y ** ((2 * k - 1) ** 2) for k in range(1, 20))
    else:
        p = 2 * sum((-1) ** (k - 1) * math.exp(-2 * k * k * lam * lam) for k in range(1, 101))
    return statistic, min(max(p, 0.0), 1.0)


def summary(values):
    mean = sum(values) / len(values)
    variance = sum((value - mean) ** 2 for value in values) / (len(values) - 1)
    return mean, math.sqrt(variance)


def compare_noise(cpp_records, rust_records):
    cpp, rust = errors(cpp_records), errors(rust_records)
    threshold = ALPHA / len(cpp)
    quantities = {}
    for name in cpp:
        if len(cpp[name]) < 30 or len(rust[name]) < 30:
            raise ValueError(f"too few samples for {name}: {len(cpp[name])}, {len(rust[name])}")
        statistic, p = ks_two_sample(cpp[name], rust[name])
        cpp_mean, cpp_std = summary(cpp[name])
        rust_mean, rust_std = summary(rust[name])
        quantities[name] = {"cpp_samples": len(cpp[name]), "rust_samples": len(rust[name]),
                            "cpp_mean": cpp_mean, "rust_mean": rust_mean,
                            "cpp_std": cpp_std, "rust_std": rust_std,
                            "ks_statistic": statistic, "p_value": p, "passed": p > threshold}
    return {"passed": all(q["passed"] for q in quantities.values()),
            "per_test_threshold": threshold, "quantities": quantities}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("case", type=Path, help="reference_check case directory")
    parser.add_argument("--report", type=Path, help="save a new JSON report")
    args = parser.parse_args(argv)
    try:
        result = compare_noise(read_trace(args.case / "cpp/trace.jsonl"),
                               read_trace(args.case / "rust-trace/trace.jsonl"))
    except (OSError, ValueError, KeyError) as error:
        print(f"noise comparison error: {error}", file=sys.stderr)
        return 2
    text = json.dumps(result, indent=2) + "\n"
    if args.report is not None:
        with args.report.open("x", encoding="utf-8") as stream:
            stream.write(text)
    print(text, end="")
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
