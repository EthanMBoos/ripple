import tempfile
import unittest
from pathlib import Path

import demos


class DemoFiles(unittest.TestCase):
    def test_the_top_level_keys_are_read_without_a_yaml_library(self):
        keys = demos.top_level(
            "# a demo\nname: x\nsource: missions/m.yaml\noverrides: {a: 1}\n"
            "speed: 2\ncamera: {follow: {group: g}}\ncaptions:\n  - {at_s: 0, text: 't'}\n")
        self.assertEqual(keys["name"], "x")
        self.assertEqual(keys["source"], "missions/m.yaml")
        self.assertEqual(keys["speed"], 2.0)
        self.assertNotIn("overrides", keys)
        self.assertNotIn("camera", keys)

    def test_every_demo_file_names_a_source_in_the_repository(self):
        for path, keys in demos.demos([]):
            self.assertEqual(keys["name"], path.stem, path)
            self.assertTrue((demos.ROOT / keys["source"]).exists(), keys["source"])


class TerrainDemo(unittest.TestCase):
    """The gallery's fixture: the terrain demo exports within the size budget."""

    def test_the_terrain_demo_exports_within_the_budget(self):
        results = demos.build(["terrain-flight"], check=True)
        [(name, size, frames)] = results
        self.assertEqual(name, "terrain-flight")
        self.assertLessEqual(size, demos.RECORDING_BUDGET_BYTES)
        self.assertIsNone(frames)


if __name__ == "__main__":
    unittest.main()
