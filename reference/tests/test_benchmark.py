"""Benchmark missions set C++ threading per variant, and Rust must agree with itself."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

import benchmark
from benchmark import case_passed, mission
from tests.fixtures import scenario


def root_for(name, variant):
    """The C++ mission, with Ripple's loading (in the container) replaced by a fixture."""
    with tempfile.TemporaryDirectory() as directory, \
            patch.object(benchmark.missions, "loaded", return_value=scenario()):
        return ET.parse(mission(name, 64, Path(directory), variant)).getroot()


class BenchmarkMissionTests(unittest.TestCase):
    def test_single_threaded_cpp_is_headless_serial_and_sized(self):
        root = root_for("motion", "cpp-1")
        self.assertEqual(root.findtext("multi_threaded"), "false")
        self.assertEqual(root.find("run").get("enable_gui"), "false")
        self.assertEqual(root.find("run").get("time_warp"), "0")
        self.assertEqual(root.findtext("log_dir").rsplit("/", 1)[-1], "logs")

    def test_threaded_cpp_asks_for_eight_threads(self):
        node = root_for("sensing", "cpp-8").find("multi_threaded")
        self.assertEqual((node.text, node.get("num_threads")), ("true", "8"))

    def test_rust_reads_the_workload_and_one_step_changes_only_the_end(self):
        with tempfile.TemporaryDirectory() as directory:
            full = json.loads(mission("churn", 64, Path(directory), "rust").read_text())
        with tempfile.TemporaryDirectory() as directory:
            short = json.loads(mission("churn", 64, Path(directory), "rust", True).read_text())
        self.assertEqual(short["run"]["end_s"], 0.1)
        self.assertEqual(full, {**short, "run": {**short["run"], "end_s": 200}})


class BenchmarkPassTests(unittest.TestCase):
    REPEATABLE = {"cpp-1": True, "cpp-8": False, "rust": True}
    MATCH = {"rust": {"passed": True}}
    DIFFER = {"rust": {"passed": False}}

    def test_rust_must_repeat_even_without_cpp_parity(self):
        self.assertTrue(case_passed("churn", self.REPEATABLE, self.DIFFER))
        self.assertFalse(case_passed("churn", {**self.REPEATABLE, "rust": False}, self.DIFFER))

    def test_deterministic_workloads_must_match_cpp(self):
        self.assertTrue(case_passed("motion", self.REPEATABLE, self.MATCH))
        self.assertFalse(case_passed("motion", self.REPEATABLE, self.DIFFER))


if __name__ == "__main__":
    unittest.main()
