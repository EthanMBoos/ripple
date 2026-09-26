"""Benchmark input changes must not enable C++ threading or erase flight cases."""
import unittest

from benchmark import mission_for_benchmark
from reference_check import ROOT


class BenchmarkMissionTests(unittest.TestCase):
    def test_cpu_workload_is_headless_serial_and_sized(self):
        root = mission_for_benchmark(ROOT / "missions/test_missions/straight_cpu.xml", 128, 1000)
        self.assertEqual(root.findtext("multi_threaded"), "false")
        self.assertEqual(root.find("run").get("enable_gui"), "false")
        self.assertEqual(root.find("run").get("time_warp"), "0")
        self.assertEqual(sum(int(e.findtext("count", "1")) for e in root.findall("entity")), 128)
        self.assertAlmostEqual(float(root.find("run").get("end")),
                               1000 * float(root.find("run").get("dt")))

    def test_fixed_wing_preserves_both_initial_conditions(self):
        root = mission_for_benchmark(ROOT / "missions/fixed-wing-6dof.xml", 129, 1000)
        entities = root.findall("entity")
        self.assertEqual([e.findtext("count") for e in entities], ["65", "64"])
        self.assertNotEqual(entities[0].findtext("heading"), entities[1].findtext("heading"))
        self.assertEqual(entities[1].find("motion_model").get("wind_E"), "3")


if __name__ == "__main__":
    unittest.main()
