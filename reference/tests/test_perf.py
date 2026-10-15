import contextlib
import io
import unittest

import perf

ENVIRONMENT = {"workloads": "w", "machine": "m", "rustc": "r"}


def record(median, low, high):
    return {"median_s": median, "min_s": low, "max_s": high}


def baseline(result, **environment):
    return {"date": "2026-01-01", "environment": {**ENVIRONMENT, **environment},
            "results": {"motion-64-w1": result}}


class PerfTests(unittest.TestCase):
    def slower(self, before, after):
        with contextlib.redirect_stdout(io.StringIO()):
            return perf.compare({"motion-64-w1": after}, baseline(before))

    def test_only_slowdowns_beyond_the_old_range_and_noise_count(self):
        self.assertEqual(self.slower(record(1.0, 0.98, 1.02), record(1.2, 1.2, 1.2)), 1)
        self.assertEqual(self.slower(record(1.0, 0.98, 1.02), record(0.8, 0.8, 0.8)), 0)
        # Inside the old spread, or within 5%: noise.
        self.assertEqual(self.slower(record(1.0, 0.9, 1.1), record(1.08, 1.08, 1.08)), 0)
        self.assertEqual(self.slower(record(1.0, 0.99, 1.01), record(1.03, 1.03, 1.03)), 0)

    def test_an_incompatible_baseline_is_not_a_pass(self):
        results = {"motion-64-w1": record(1, 1, 1)}
        self.assertIsNone(perf.incompatibility(baseline(record(1, 1, 1)), ENVIRONMENT, results))
        self.assertIn("no accepted baseline", perf.incompatibility(None, ENVIRONMENT, results))
        for key in ENVIRONMENT:
            reason = perf.incompatibility(baseline(record(1, 1, 1), **{key: "other"}),
                                          ENVIRONMENT, results)
            self.assertIn(f"different {key}", reason)
        reason = perf.incompatibility(baseline(record(1, 1, 1)), ENVIRONMENT,
                                      {**results, "new-64-w1": record(1, 1, 1)})
        self.assertIn("new-64-w1", reason)

    def test_changing_a_workload_changes_its_hash(self):
        before = perf.workloads_hash()
        original = perf.WORKLOADS["motion"]
        try:
            perf.WORKLOADS["motion"] = (original[0], (64, 513), original[2])
            self.assertNotEqual(perf.workloads_hash(), before)
        finally:
            perf.WORKLOADS["motion"] = original
        self.assertEqual(perf.workloads_hash(), before)

    def test_workloads_are_missions_that_change_only_what_is_asked(self):
        for name, (_, sizes, _) in perf.WORKLOADS.items():
            mission = perf.workload(name, sizes[0])
            self.assertEqual(mission["entities"]["aircraft"]["count"], sizes[0])
            self.assertEqual(perf.workload(name, sizes[0], end=0.1),
                             {**mission, "run": {**mission["run"], "end_s": 0.1}})


if __name__ == "__main__":
    unittest.main()
