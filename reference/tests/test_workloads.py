import unittest

import workloads


class WorkloadTests(unittest.TestCase):
    def test_workloads_are_missions_that_change_only_what_is_asked(self):
        for name, (_, sizes, _) in workloads.WORKLOADS.items():
            mission = workloads.workload(name, sizes[0])
            self.assertEqual(mission["entities"]["aircraft"]["count"], sizes[0])
            self.assertEqual(workloads.workload(name, sizes[0], end=0.1),
                             {**mission, "run": {**mission["run"], "end_s": 0.1}})


if __name__ == "__main__":
    unittest.main()
