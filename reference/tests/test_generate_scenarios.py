from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET

import generate_scenarios


class GenerateScenariosTests(unittest.TestCase):
    def test_same_seed_writes_same_valid_missions(self):
        with tempfile.TemporaryDirectory() as directory:
            first, second = Path(directory) / "a", Path(directory) / "b"
            generate_scenarios.main(["--count", "5", "--seed", "3", "--output", str(first)])
            generate_scenarios.main(["--count", "5", "--seed", "3", "--output", str(second)])
            for path in sorted(first.glob("*.xml")):
                self.assertEqual(path.read_text(), (second / path.name).read_text())
                root = ET.parse(path).getroot()
                self.assertEqual(root.tag, "runscript")
                self.assertTrue(root.findall("entity"))


if __name__ == "__main__":
    unittest.main()
