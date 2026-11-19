import json
from pathlib import Path
import tempfile
import unittest

import generate_scenarios


class GenerateScenariosTests(unittest.TestCase):
    def test_same_seed_writes_same_valid_missions(self):
        with tempfile.TemporaryDirectory() as directory:
            first, second = Path(directory) / "a", Path(directory) / "b"
            generate_scenarios.main(["--count", "5", "--seed", "3", "--output", str(first)])
            generate_scenarios.main(["--count", "5", "--seed", "3", "--output", str(second)])
            paths = sorted(first.glob("*.yaml"))
            self.assertEqual(len(paths), 5)
            for path in paths:
                self.assertEqual(path.read_text(), (second / path.name).read_text())
                mission = json.loads(path.read_text())  # YAML written as JSON
                self.assertEqual(mission["format_version"], 1)
                self.assertTrue(mission["entities"])


if __name__ == "__main__":
    unittest.main()
