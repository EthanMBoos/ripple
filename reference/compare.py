#!/usr/bin/env python3
"""Compare saved SCRIMMAGE runs (frames.bin and summary.csv), without a build.

Python 3.10+, standard library only. Exit 0 for a match, 1 for a mismatch,
or 2 for invalid input/I/O errors. Existing reports are never overwritten.
"""

import argparse
import csv
from dataclasses import asdict, dataclass, field
import json
import math
from pathlib import Path
import sys

from frames import read_frames


# Set from measured agreement, not inherited. In the 2026-09-25 campaign the
# largest differences in passing runs were 3.7e-10 m (a 100 s fixed-wing run),
# 9.6e-11 m/s, 2.2e-12 rad, and 8.9e-12 rad/s: the position limit has 27x
# headroom and still catches sub-micrometre defects (the original comparator
# used 0.01 m and 1e-3, which let one through).
POSITION_TOLERANCE_M = 1e-8
VELOCITY_TOLERANCE_MPS = 1e-8
ORIENTATION_TOLERANCE_RAD = 1e-9
ANGULAR_VELOCITY_TOLERANCE_RADPS = 1e-9
TIMESTAMP_TOLERANCE_S = 1e-9
SUMMARY_TOLERANCE = 1e-6  # C++ summaries print six decimal places.


@dataclass
class Comparison:
    passed: bool = False
    reference_frames: int = 0
    candidate_frames: int = 0
    compared_entity_states: int = 0
    max_position_error_m: float = 0.0
    max_velocity_error_mps: float = 0.0
    max_orientation_error_rad: float = 0.0
    max_angular_velocity_error_radps: float = 0.0
    max_timestamp_error_s: float = 0.0
    mismatch_count: int = 0
    first_mismatches: list = field(default_factory=list)
    # Standard C++ logs do not contain the full pub/sub event stream.
    direct_event_stream_compared: bool = False

    def mismatch(self, message):
        self.mismatch_count += 1
        if len(self.first_mismatches) < 20:
            self.first_mismatches.append(message)


def distance(left, right):
    return math.sqrt(sum((a - b) * (a - b) for a, b in zip(left, right)))


def orientation_distance_rad(reference, candidate):
    difference_squared = sum((a - b) * (a - b) for a, b in zip(reference, candidate))
    sum_squared = sum((a + b) * (a + b) for a, b in zip(reference, candidate))
    # q and -q represent the same orientation. Chords avoid acos near 1.
    return 4.0 * math.atan2(math.sqrt(min(difference_squared, sum_squared)),
                            math.sqrt(max(difference_squared, sum_squared)))


def compare_frames(reference, candidate):
    result = Comparison(reference_frames=len(reference), candidate_frames=len(candidate))
    if not reference or len(reference) != len(candidate):
        result.mismatch(f"frame counts: reference {len(reference)}, candidate {len(candidate)}")

    # Pair by index: the reference has a repeated terminal timestamp.
    for index, (expected_frame, actual_frame) in enumerate(zip(reference, candidate)):
        timestamp_error = abs(expected_frame.time_s - actual_frame.time_s)
        result.max_timestamp_error_s = max(result.max_timestamp_error_s, timestamp_error)
        if timestamp_error > TIMESTAMP_TOLERANCE_S:
            result.mismatch(f"frame {index}: timestamp differs by {timestamp_error} s")
        if expected_frame.entities.keys() != actual_frame.entities.keys():
            result.mismatch(f"frame {index}: entity IDs differ")
        for entity_id, expected in sorted(expected_frame.entities.items()):
            actual = actual_frame.entities.get(entity_id)
            if actual is None:
                continue
            result.compared_entity_states += 1
            if (expected.team_id != actual.team_id
                    or expected.sub_swarm_id != actual.sub_swarm_id
                    or expected.active != actual.active or expected.kind != actual.kind):
                result.mismatch(f"frame {index}, entity {entity_id}: contact metadata differs")
            position = distance(expected.position, actual.position)
            velocity = distance(expected.velocity, actual.velocity)
            orientation = orientation_distance_rad(expected.orientation, actual.orientation)
            angular_velocity = distance(expected.angular_velocity, actual.angular_velocity)
            result.max_position_error_m = max(result.max_position_error_m, position)
            result.max_velocity_error_mps = max(result.max_velocity_error_mps, velocity)
            result.max_orientation_error_rad = max(result.max_orientation_error_rad, orientation)
            result.max_angular_velocity_error_radps = max(
                result.max_angular_velocity_error_radps, angular_velocity)
            if (position > POSITION_TOLERANCE_M or velocity > VELOCITY_TOLERANCE_MPS
                    or orientation > ORIENTATION_TOLERANCE_RAD
                    or angular_velocity > ANGULAR_VELOCITY_TOLERANCE_RADPS):
                result.mismatch(
                    f"frame {index}, t={expected_frame.time_s} s, entity {entity_id}: "
                    f"position={position} m, velocity={velocity} m/s, "
                    f"orientation={orientation} rad, angular velocity={angular_velocity} rad/s")
    result.passed = result.mismatch_count == 0
    return result


def read_summary(path):
    with Path(path).open(newline="", encoding="utf-8") as stream:
        rows = csv.reader(stream, strict=True)
        header = next(rows, [])
        if not header or header[0] != "team_id":
            raise ValueError(f"{path}: summary must begin with team_id")
        teams = {}
        for row in rows:
            if len(row) != len(header):
                raise ValueError(f"{path}: summary row has wrong field count")
            team_id = int(row[0])
            if not -(1 << 31) <= team_id < (1 << 31):
                raise ValueError(f"{path}: team ID exceeds int32")
            if team_id in teams:
                raise ValueError(f"{path}: duplicate summary team {team_id}")
            # C++ can print nan/inf. Infinity matches itself; NaN never matches.
            teams[team_id] = [float(value) for value in row[1:]]
    return header, teams


def compare_summaries(reference, candidate, result):
    expected_header, expected_teams = read_summary(reference)
    actual_header, actual_teams = read_summary(candidate)
    if expected_header != actual_header or expected_teams.keys() != actual_teams.keys():
        result.mismatch("summary columns or team IDs differ")
        return
    for team_id, expected in sorted(expected_teams.items()):
        for column, (left, right) in enumerate(zip(expected, actual_teams[team_id]), start=1):
            if left == right or abs(left - right) <= SUMMARY_TOLERANCE:
                continue
            result.mismatch(f"summary team {team_id}, {expected_header[column]}: "
                            f"reference {left}, candidate {right}")


def compare_runs(reference, candidate):
    reference, candidate = Path(reference), Path(candidate)
    result = compare_frames(read_frames(reference / "frames.bin"),
                            read_frames(candidate / "frames.bin"))
    compare_summaries(reference / "summary.csv", candidate / "summary.csv", result)
    result.passed = result.mismatch_count == 0
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--report", type=Path, help="save a new JSON report, including mismatches")
    args = parser.parse_args(argv)
    try:
        result = compare_runs(args.reference, args.candidate)
        text = json.dumps(asdict(result), indent=2, allow_nan=False) + "\n"
        if args.report is not None:
            args.report.parent.mkdir(parents=True, exist_ok=True)
            with args.report.open("x", encoding="utf-8") as stream:
                stream.write(text)
        print(text, end="")
        return 0 if result.passed else 1
    except (OSError, ValueError, csv.Error) as error:
        print(f"comparison error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
