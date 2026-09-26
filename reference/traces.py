#!/usr/bin/env python3
"""Compare C++ and Rust comparison traces (trace.jsonl), without a build.

Both simulators write the same JSON-lines records; see crates/core/src/simcontrol/trace.rs
and the C++ benchmarking-edits branch (src/common/Trace.cpp):

  delivery            a message reached a subscriber this tick
  scheduled_delivery  a message was queued with a positive delay
  belief              an entity's belief state when its frame is logged (the
                      final tick twice, as frames repeat the terminal timestamp)
  output              an autonomy/controller output after all controller substeps
  publication         a published sensor payload: states with covariance

Known C++ defect: an entity destroyed in the pre-start interaction pass stays in
C++'s entity list, inactive, until the end of the first tick. It still runs its
plugins that tick, and the first tick's interaction phase publishes a second
EntityRemoved for it. Rust removes it once and does not step it. For exactly
those entities (from Rust's events) and only at the first tick, their outputs,
publications, and deliveries to or from them are excluded and the repeated
EntityRemoved deliveries are collapsed; the count is reported. Any effect on
other entities still appears in their records.

Deliveries are grouped by (tick, receiver, network, topic) and must contain the
same senders and event entity IDs in the same order. Publications must list the
same contact IDs in order. Belief, payload, and output values use the tolerances
below. A record present on only one side is a mismatch.
Python 3.10+, standard library only.
"""

import argparse
from collections import defaultdict
from dataclasses import asdict, dataclass, field
import json
import math
from pathlib import Path
import sys

from compare import (ANGULAR_VELOCITY_TOLERANCE_RADPS, ORIENTATION_TOLERANCE_RAD,
                     POSITION_TOLERANCE_M, VELOCITY_TOLERANCE_MPS, distance,
                     orientation_distance_rad)

OUTPUT_TOLERANCE = 1e-8  # Absolute; largest observed 2.2e-11 with commands up to ~1000.
COVARIANCE_TOLERANCE = 1e-9
TIME_DIGITS = 9  # Ticks are keyed by time rounded to nanoseconds.


@dataclass
class TraceComparison:
    passed: bool = False
    deliveries: int = 0
    beliefs: int = 0
    outputs: int = 0
    published_states: int = 0
    max_belief_position_error_m: float = 0.0
    max_belief_velocity_error_mps: float = 0.0
    max_belief_orientation_error_rad: float = 0.0
    max_belief_angular_velocity_error_radps: float = 0.0
    max_payload_position_error_m: float = 0.0
    max_payload_velocity_error_mps: float = 0.0
    max_payload_orientation_error_rad: float = 0.0
    max_payload_covariance_error: float = 0.0
    max_output_error: float = 0.0
    max_output_error_port: str = ""
    excluded_pre_start_records: int = 0
    # Delivery groups (tick, receiver, network, topic) that differ or exist on one side.
    delivery_mismatches: int = 0
    mismatch_count: int = 0
    first_mismatches: list = field(default_factory=list)

    def mismatch(self, message):
        self.mismatch_count += 1
        if len(self.first_mismatches) < 20:
            self.first_mismatches.append(message)


NONFINITE = {"NaN": math.nan, "Infinity": math.inf, "-Infinity": -math.inf}


def decode_numbers(value):
    """Both traces write non-finite numbers as the strings in NONFINITE."""
    if isinstance(value, str):
        return NONFINITE.get(value, value)
    if isinstance(value, list):
        return [decode_numbers(item) for item in value]
    if isinstance(value, dict):
        return {key: decode_numbers(item) for key, item in value.items()}
    return value


def read_trace(path):
    records = []
    with Path(path).open(encoding="utf-8") as stream:
        for number, line in enumerate(stream, start=1):
            if line.strip():
                try:
                    records.append(decode_numbers(json.loads(line)))
                except json.JSONDecodeError as error:
                    raise ValueError(f"{path}:{number}: {error}") from error
    return records


def tick(record):
    return round(record["t"], TIME_DIGITS)


def normalize_endpoint(endpoint, entity_ids):
    # C++ gives world plugins a placeholder parent entity; Rust has none.
    entity_id, plugin = endpoint
    return (entity_id if entity_id in entity_ids else None, plugin)


def index(records):
    entity_ids = {record["id"] for record in records if record["kind"] == "belief"}
    deliveries = defaultdict(list)
    beliefs, outputs, publications = {}, {}, {}
    belief_counts = defaultdict(int)
    output_counts = defaultdict(int)
    for record in records:
        kind = record["kind"]
        if kind in ("delivery", "scheduled_delivery"):
            key = (kind, tick(record), normalize_endpoint(record["to"], entity_ids),
                   record["network"], record["topic"])
            deliveries[key].append((normalize_endpoint(record["from"], entity_ids),
                                    tuple(record.get("ids", ())),
                                    record.get("deliver_at")))
        elif kind == "belief":
            # The final tick is logged twice: before its step and as the terminal frame.
            key = (tick(record), record["id"])
            beliefs[key + (belief_counts[key],)] = record
            belief_counts[key] += 1
        elif kind == "publication":
            key = (tick(record), normalize_endpoint(record["from"], entity_ids), record["topic"])
            if key in publications:
                raise ValueError(f"duplicate publication record {key}")
            publications[key] = record["states"]
        elif kind == "output":
            # Two plugins with one name (e.g. two autonomies) are told apart by chain order.
            key = (tick(record), record["id"], record["plugin"], record["port"])
            outputs[key + (output_counts[key],)] = record["value"]
            output_counts[key] += 1
        else:
            raise ValueError(f"unknown trace record kind {kind!r}")
    return deliveries, beliefs, outputs, publications


def state_errors(expected, actual):
    return (distance(expected["p"], actual["p"]), distance(expected["v"], actual["v"]),
            orientation_distance_rad(expected["q"], actual["q"]),
            distance(expected["w"], actual["w"]))


STATE_LIMITS = (POSITION_TOLERANCE_M, VELOCITY_TOLERANCE_MPS, ORIENTATION_TOLERANCE_RAD,
                ANGULAR_VELOCITY_TOLERANCE_RADPS)


def compare_keys(name, reference, candidate, result):
    for key in sorted(reference.keys() - candidate.keys(), key=repr):
        result.mismatch(f"{name} only in reference: {key}")
    for key in sorted(candidate.keys() - reference.keys(), key=repr):
        result.mismatch(f"{name} only in candidate: {key}")


def exclude_pre_start(records, removed, first_tick):
    """Apply the pre-start removal rule above; returns (kept records, excluded count)."""
    kept, seen, excluded = [], set(), 0
    for record in records:
        if not removed or tick(record) != first_tick:
            kept.append(record)
            continue
        kind = record["kind"]
        own = record.get("id") if kind == "output" else None
        ends = [record[key][0] for key in ("from", "to") if key in record]
        if own in removed or any(entity in removed for entity in ends):
            excluded += 1
            continue
        if kind == "delivery" and record["topic"] == "EntityRemoved" \
                and set(record.get("ids", ())) <= removed:
            key = (tuple(record["to"]), tuple(record["ids"]))
            if key in seen:
                excluded += 1
                continue
            seen.add(key)
        kept.append(record)
    return kept, excluded


def compare_traces(reference_records, candidate_records, pre_start_removed=(), first_tick=0.0):
    result = TraceComparison()
    removed = set(pre_start_removed)
    first_tick = round(first_tick, TIME_DIGITS)
    reference_records, excluded = exclude_pre_start(reference_records, removed, first_tick)
    candidate_records, candidate_excluded = exclude_pre_start(candidate_records, removed, first_tick)
    result.excluded_pre_start_records = excluded + candidate_excluded
    ref_deliveries, ref_beliefs, ref_outputs, ref_publications = index(reference_records)
    deliveries, beliefs, outputs, publications = index(candidate_records)
    result.deliveries = sum(len(v) for v in ref_deliveries.values())
    result.beliefs = len(ref_beliefs)
    result.outputs = len(ref_outputs)

    before = result.mismatch_count
    compare_keys("delivery group", ref_deliveries, deliveries, result)
    result.delivery_mismatches += result.mismatch_count - before
    for key in sorted(ref_deliveries.keys() & deliveries.keys(), key=repr):
        expected, actual = ref_deliveries[key], deliveries[key]
        if expected != actual:
            result.delivery_mismatches += 1
            order = "order only" if sorted(expected, key=repr) == sorted(actual, key=repr) else "content"
            result.mismatch(f"deliveries {key} differ ({order}): "
                            f"reference {expected[:3]}, candidate {actual[:3]}")

    compare_keys("belief", ref_beliefs, beliefs, result)
    for key in sorted(ref_beliefs.keys() & beliefs.keys()):
        expected, actual = ref_beliefs[key], beliefs[key]
        errors = state_errors(expected, actual)
        result.max_belief_position_error_m = max(result.max_belief_position_error_m, errors[0])
        result.max_belief_velocity_error_mps = max(result.max_belief_velocity_error_mps, errors[1])
        result.max_belief_orientation_error_rad = max(
            result.max_belief_orientation_error_rad, errors[2])
        result.max_belief_angular_velocity_error_radps = max(
            result.max_belief_angular_velocity_error_radps, errors[3])
        if not all(error <= limit for error, limit in zip(errors, STATE_LIMITS)):
            result.mismatch(f"belief {key} differs: {errors}")

    compare_keys("publication", ref_publications, publications, result)
    for key in sorted(ref_publications.keys() & publications.keys(), key=repr):
        expected, actual = ref_publications[key], publications[key]
        if [state["id"] for state in expected] != [state["id"] for state in actual]:
            result.mismatch(f"publication {key} contact IDs differ: "
                            f"{[s['id'] for s in expected]} vs {[s['id'] for s in actual]}")
            continue
        for expected_state, actual_state in zip(expected, actual):
            result.published_states += 1
            errors = state_errors(expected_state, actual_state)
            covariance = max((abs(a - b) for a, b in zip(expected_state["cov"], actual_state["cov"])),
                             default=0.0)
            if len(expected_state["cov"]) != len(actual_state["cov"]):
                covariance = math.inf
            result.max_payload_position_error_m = max(result.max_payload_position_error_m, errors[0])
            result.max_payload_velocity_error_mps = max(
                result.max_payload_velocity_error_mps, errors[1])
            result.max_payload_orientation_error_rad = max(
                result.max_payload_orientation_error_rad, errors[2])
            result.max_payload_covariance_error = max(result.max_payload_covariance_error, covariance)
            if not (all(error <= limit for error, limit in zip(errors, STATE_LIMITS))
                    and covariance <= COVARIANCE_TOLERANCE):
                result.mismatch(f"publication {key} contact {expected_state['id']} differs: "
                                f"{errors}, covariance {covariance}")

    compare_keys("output", ref_outputs, outputs, result)
    for key in sorted(ref_outputs.keys() & outputs.keys()):
        expected, actual = ref_outputs[key], outputs[key]
        if math.isfinite(expected) and math.isfinite(actual):
            error = abs(expected - actual)
        else:  # Equal infinities, or NaN on both sides, match.
            same_nan = math.isnan(expected) and math.isnan(actual)
            error = 0.0 if expected == actual or same_nan else math.inf
        if error > result.max_output_error:
            result.max_output_error = error
            result.max_output_error_port = f"{key[2]}.{key[3]}"
        if not error <= OUTPUT_TOLERANCE:
            result.mismatch(f"output {key}: reference {expected}, candidate {actual}")

    result.passed = result.mismatch_count == 0
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("reference", type=Path, help="C++ trace.jsonl")
    parser.add_argument("candidate", type=Path, help="Rust trace.jsonl")
    parser.add_argument("--report", type=Path, help="save a new JSON report")
    parser.add_argument("--pre-start-removed", default="",
                        help="comma-separated entity IDs destroyed before the first tick")
    parser.add_argument("--first-tick", type=float, default=0.0)
    args = parser.parse_args(argv)
    removed = [int(value) for value in args.pre_start_removed.split(",") if value]
    try:
        result = compare_traces(read_trace(args.reference), read_trace(args.candidate),
                                removed, args.first_tick)
        text = json.dumps(asdict(result), indent=2) + "\n"
        if args.report is not None:
            with args.report.open("x", encoding="utf-8") as stream:
                stream.write(text)
        print(text, end="")
        return 0 if result.passed else 1
    except (OSError, ValueError, KeyError) as error:
        print(f"trace comparison error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
