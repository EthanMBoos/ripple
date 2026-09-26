import contextlib
from copy import deepcopy
from dataclasses import asdict
import io
import json
import math
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

import compare
import frames


def varint(value):
    value &= (1 << 64) - 1
    data = bytearray()
    while value >= 128:
        data.append((value & 127) | 128)
        value >>= 7
    data.append(value)
    return bytes(data)


def integer(field, value):
    return varint(field << 3) + varint(value)


def double(field, value):
    return varint(field << 3 | 1) + struct.pack("<d", value)


def message(field, payload):
    return varint(field << 3 | 2) + varint(len(payload)) + payload


def contact(entity_id=1, position=0.0, orientation=1.0):
    identity = integer(1, entity_id) + integer(3, -2)
    state = (message(1, double(1, position)) + message(2, double(1, orientation))
             + message(3, b"") + message(4, b""))
    return message(1, identity) + message(2, state) + integer(4, 1)


def frame(position=0.0):
    return frames.decode_frame(double(1, 0.1) + message(2, contact(position=position)))


def write_run(path, position=0.0):
    path.mkdir()
    payload = double(1, 0.1) + message(2, contact(position=position))
    (path / "frames.bin").write_bytes(varint(len(payload)) + payload)
    (path / "summary.csv").write_text("team_id,score\n-2,0.000000\n")


class ComparisonTests(unittest.TestCase):
    def test_quaternion_sign_and_rotation_angle(self):
        identity = (1.0, 0.0, 0.0, 0.0)
        self.assertEqual(compare.orientation_distance_rad(identity, (-1, 0, 0, 0)), 0.0)
        rotated = (math.cos(0.1), 0, 0, math.sin(0.1))
        self.assertAlmostEqual(compare.orientation_distance_rad(identity, rotated), 0.2)
        actual = frame()
        actual.entities[1].orientation = (-1, 0, 0, 0)
        self.assertTrue(compare.compare_frames([frame()], [actual]).passed)

    def test_physical_tolerances(self):
        for field, tolerance in (("position", compare.POSITION_TOLERANCE_M),
                                 ("velocity", compare.VELOCITY_TOLERANCE_MPS),
                                 ("angular_velocity", compare.ANGULAR_VELOCITY_TOLERANCE_RADPS)):
            for factor, passed in ((0.99, True), (1.01, False)):
                with self.subTest(field=field, factor=factor):
                    actual = frame()
                    setattr(actual.entities[1], field, (factor * tolerance, 0, 0))
                    self.assertEqual(compare.compare_frames([frame()], [actual]).passed, passed)
        limit = compare.ORIENTATION_TOLERANCE_RAD
        for angle, passed in ((0.99 * limit, True), (1.01 * limit, False)):
            actual = frame()
            actual.entities[1].orientation = (math.cos(angle / 2), 0, 0, math.sin(angle / 2))
            self.assertEqual(compare.compare_frames([frame()], [actual]).passed, passed)

    def test_velocity_direction_not_just_speed(self):
        expected, actual = frame(), frame()
        expected.entities[1].velocity = (1, 0, 0)
        actual.entities[1].velocity = (-1, 0, 0)
        self.assertFalse(compare.compare_frames([expected], [actual]).passed)

    def test_missing_entities_frames_and_empty_logs_fail(self):
        actual = frame()
        actual.entities.clear()
        for left, right in (([frame()], [actual]), ([frame()], []), ([], [])):
            self.assertFalse(compare.compare_frames(left, right).passed)

    def test_equal_timestamps_are_not_collapsed(self):
        result = compare.compare_frames([frame(), frame(1.0)], [frame(), frame()])
        self.assertFalse(result.passed)
        self.assertEqual(result.compared_entity_states, 2)

    def test_timestamp_and_metadata_differences(self):
        actual = frame()
        actual.time_s += 2e-9
        self.assertFalse(compare.compare_frames([frame()], [actual]).passed)
        for name, value in (("team_id", 3), ("sub_swarm_id", 1), ("kind", 2), ("active", False)):
            actual = frame()
            setattr(actual.entities[1], name, value)
            self.assertFalse(compare.compare_frames([frame()], [actual]).passed)

    def test_entity_order_and_summary_row_order_are_irrelevant(self):
        expected = frame()
        second = deepcopy(expected.entities[1])
        second.id = 2
        expected.entities[2] = second
        actual = deepcopy(expected)
        actual.entities = dict(reversed(list(actual.entities.items())))
        self.assertTrue(compare.compare_frames([expected], [actual]).passed)
        with tempfile.TemporaryDirectory() as directory:
            left, right = Path(directory) / "left.csv", Path(directory) / "right.csv"
            left.write_text("team_id,score\n1,0\n2,1\n")
            right.write_text("team_id,score\n2,1.0000005\n1,0\n")
            result = compare.Comparison()
            compare.compare_summaries(left, right, result)
            self.assertEqual(result.mismatch_count, 0)

    def test_summary_mismatch_not_hidden_by_matching_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            left, right = Path(directory) / "left", Path(directory) / "right"
            write_run(left)
            write_run(right)
            (right / "summary.csv").write_text("team_id,score\n-2,0.000002\n")
            result = compare.compare_runs(left, right)
            self.assertFalse(result.passed)
            self.assertEqual(result.mismatch_count, 1)
            for text in ("", "score,team_id\n0,1\n",
                         "team_id,score\n1,0\n1,0\n", "team_id,score\n1\n"):
                (right / "summary.csv").write_text(text)
                with self.assertRaises(ValueError):
                    compare.compare_runs(left, right)

    def test_nonfinite_summary_values(self):
        with tempfile.TemporaryDirectory() as directory:
            left, right = Path(directory) / "left.csv", Path(directory) / "right.csv"
            left.write_text("team_id,score,flight_time\n1,nan,inf\n")
            right.write_text("team_id,score,flight_time\n1,0.5,inf\n")
            result = compare.Comparison()
            compare.compare_summaries(left, right, result)
            self.assertEqual(result.mismatch_count, 1)  # NaN never matches; inf matches inf.

    def test_mismatch_examples_are_capped_not_the_count(self):
        result = compare.compare_frames([frame()] * 25, [frame(1)] * 25)
        self.assertEqual(result.mismatch_count, 25)
        self.assertEqual(len(result.first_mismatches), 20)
        self.assertFalse(asdict(result)["direct_event_stream_compared"])


class FrameReaderTests(unittest.TestCase):
    def test_defaults_negative_ids_unknown_fields_and_field_order(self):
        payload = (message(2, contact(-1)) + integer(100, 42)
                   + message(101, b"unknown") + double(102, 123)
                   + varint(103 << 3 | 5) + b"abcd" + double(1, 0.1))
        actual = frames.decode_frame(payload)
        self.assertEqual(actual.time_s, 0.1)
        self.assertEqual(actual.entities[-1].team_id, -2)
        self.assertEqual(actual.entities[-1].velocity, (0.0, 0.0, 0.0))
        self.assertEqual(actual.entities[-1].kind, 0)

    def test_invalid_state_is_rejected(self):
        for payload in (message(2, b""), message(2, message(1, b"")),
                        message(2, contact()) * 2,
                        message(2, contact(position=float("nan"))),
                        message(2, contact(orientation=0)),
                        message(2, contact() + integer(3, 5)),
                        double(1, float("inf")), integer(1, 3)):
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                frames.decode_frame(payload)

    def test_truncated_and_invalid_wire_data_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "frames.bin"
            for data in (b"\x80", b"\x80" * 10, b"\x05\x09", b"\x01\x00",
                         b"\x01\x09", b"\x02\x12\x05", b"\x01\x0f"):
                path.write_bytes(data)
                with self.subTest(data=data), self.assertRaises(ValueError):
                    frames.read_frames(path)


class CommandTests(unittest.TestCase):
    def test_standalone_cli_exit_codes_json_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            left, right = directory / "left run", directory / "right run"
            write_run(left)
            write_run(right)
            report = directory / "reports/result.json"
            command = [sys.executable, str(Path(compare.__file__).resolve()),
                       str(left), str(right), "--report", str(report)]
            process = subprocess.run(command, cwd=directory, capture_output=True, text=True)
            self.assertEqual(process.returncode, 0, process.stderr)
            self.assertTrue(json.loads(process.stdout)["passed"])
            original = report.read_bytes()
            self.assertEqual(json.loads(original), json.loads(process.stdout))
            process = subprocess.run(command, cwd=directory, capture_output=True, text=True)
            self.assertEqual(process.returncode, 2)
            self.assertEqual(report.read_bytes(), original)
            (right / "summary.csv").write_text("team_id,score\n-2,1\n")
            command[-1] = str(directory / "failed.json")
            process = subprocess.run(command, cwd=directory, capture_output=True, text=True)
            self.assertEqual(process.returncode, 1, process.stderr)
            self.assertFalse(json.loads(Path(command[-1]).read_text())["passed"])

    def test_invalid_input_creates_no_report(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "new/report.json"
            with contextlib.redirect_stderr(io.StringIO()):
                code = compare.main([directory, directory, "--report", str(report)])
            self.assertEqual(code, 2)
            self.assertFalse(report.parent.exists())


if __name__ == "__main__":
    unittest.main()
