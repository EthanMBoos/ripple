"""Benchmark missions set C++ threading per variant, and Rust must agree with itself."""
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET

from benchmark import case_passed, mission


def root_for(name, variant, one_step=False):
    with tempfile.TemporaryDirectory() as directory:
        return ET.parse(mission(name, 64, Path(directory), variant, one_step)).getroot()


class BenchmarkMissionTests(unittest.TestCase):
    def test_single_threaded_cpp_is_headless_serial_and_sized(self):
        root = root_for("motion", "cpp-1")
        self.assertEqual(root.findtext("multi_threaded"), "false")
        self.assertEqual(root.find("run").get("enable_gui"), "false")
        self.assertEqual(root.find("run").get("time_warp"), "0")
        self.assertEqual(root.find("entity").findtext("count"), "64")

    def test_threaded_cpp_asks_for_eight_threads(self):
        node = root_for("sensing", "cpp-8").find("multi_threaded")
        self.assertEqual((node.text, node.get("num_threads")), ("true", "8"))

    def test_one_step_run_keeps_the_mission(self):
        full, short = root_for("churn", "rust-1"), root_for("churn", "rust-1", one_step=True)
        self.assertEqual(short.find("run").get("end"), "0.1")
        self.assertEqual(ET.tostring(full.find("entity")), ET.tostring(short.find("entity")))


class BenchmarkPassTests(unittest.TestCase):
    REPEATABLE = {"cpp-1": True, "cpp-8": False, "rust-1": True, "rust-8": True}
    MATCH = {"rust-1": {"passed": True}, "rust-8": {"passed": True}}
    DIFFER = {"rust-1": {"passed": False}, "rust-8": {"passed": False}}

    def test_rust_worker_counts_must_agree_even_without_cpp_parity(self):
        self.assertTrue(case_passed("churn", self.REPEATABLE, self.DIFFER, True))
        self.assertFalse(case_passed("churn", self.REPEATABLE, self.DIFFER, False))

    def test_deterministic_workloads_must_match_cpp(self):
        self.assertTrue(case_passed("motion", self.REPEATABLE, self.MATCH, True))
        self.assertFalse(case_passed("motion", self.REPEATABLE, self.DIFFER, True))


if __name__ == "__main__":
    unittest.main()
