import contextlib
import io
import unittest

import perf


def record(median, low, high):
    return {"median_s": median, "min_s": low, "max_s": high}


class PerfTests(unittest.TestCase):
    def compare(self, before, after):
        baseline = {"label": "base", "date": "2026-01-01", "commit": "abc1234567", "dirty": False,
                    "results": {"motion-64-w1": before}}
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            perf.compare({"motion-64-w1": after, "new-64-w1": record(1, 1, 1)}, baseline)
        return output.getvalue()

    def test_only_changes_beyond_the_old_range_and_noise_are_flagged(self):
        self.assertIn("SLOWER", self.compare(record(1.0, 0.98, 1.02), record(1.2, 1.2, 1.2)))
        self.assertIn("faster", self.compare(record(1.0, 0.98, 1.02), record(0.8, 0.8, 0.8)))
        # Inside the old spread, or within 5%: noise.
        self.assertIn("0 workload(s)", self.compare(record(1.0, 0.9, 1.1), record(1.08, 1.08, 1.08)))
        self.assertIn("0 workload(s)", self.compare(record(1.0, 0.99, 1.01), record(1.03, 1.03, 1.03)))
        self.assertIn("new", self.compare(record(1.0, 1.0, 1.0), record(1.0, 1.0, 1.0)))

    def test_workload_missions_are_valid_xml(self):
        import xml.etree.ElementTree as ET
        for name, (_, sizes, fields) in perf.WORKLOADS.items():
            ET.fromstring(perf.HEADER.format(name=name, agents=sizes[0], **fields))


if __name__ == "__main__":
    unittest.main()
