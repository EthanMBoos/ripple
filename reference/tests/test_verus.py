from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import verus


class VerusTests(unittest.TestCase):
    def test_a_fault_is_planted_proved_and_restored_even_when_proving_raises(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "loop_rate.rs"
            source.write_text("fn due() {\n    reload();\n    count();\n}\n")
            fault = verus.Fault("reload deleted", "loop_rate.rs", "    reload();\n", "", "proof")
            seen = {}

            def fake_prove(env):
                seen["text"] = source.read_text()
                return 1, 0.5, "error: postcondition not satisfied"

            with patch.object(verus, "CORE", Path(temporary)), \
                 patch.object(verus, "prove", fake_prove):
                result = verus.check_fault(fault, {}, Path(temporary) / "log")
            self.assertEqual(seen["text"], "fn due() {\n    count();\n}\n")
            self.assertTrue(result["fails_as_required"])
            self.assertEqual(source.read_text(), "fn due() {\n    reload();\n    count();\n}\n")

            def failing_prove(env):
                raise RuntimeError("prover missing")

            with patch.object(verus, "CORE", Path(temporary)), \
                 patch.object(verus, "prove", failing_prove), \
                 self.assertRaises(RuntimeError):
                verus.check_fault(fault, {}, Path(temporary) / "log")
            self.assertEqual(source.read_text(), "fn due() {\n    reload();\n    count();\n}\n")

    def test_a_compile_fault_runs_cargo_check_instead_of_the_prover(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "stack.rs"
            source.write_text("call(&self.belief);\n")
            fault = verus.Fault("truth as belief", "stack.rs", "&self.belief", "truth", "compile")
            seen = {}

            def fake_compile_check(env):
                seen["text"] = source.read_text()
                return 1, 0.2, "error[E0308]: mismatched types"

            def never_prove(env):
                raise AssertionError("a compile fault must not run the prover")

            with patch.object(verus, "CORE", Path(temporary)), \
                 patch.object(verus, "compile_check", fake_compile_check), \
                 patch.object(verus, "prove", never_prove):
                result = verus.check_fault(fault, {}, Path(temporary) / "log")
            self.assertEqual(seen["text"], "call(truth);\n")
            self.assertEqual(result["check"], "compile")
            self.assertTrue(result["fails_as_required"])
            self.assertEqual(source.read_text(), "call(&self.belief);\n")

    def test_a_test_fault_runs_the_tests_instead_of_the_prover(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "stack.rs"
            source.write_text("plugin.step(context)\n")
            fault = verus.Fault("call deleted", "stack.rs", "plugin.step(context)", "Ok(())", "test")
            with patch.object(verus, "CORE", Path(temporary)), \
                 patch.object(verus, "test_check", lambda env: (101, 9.0, "test failed")), \
                 patch.object(verus, "prove", None), \
                 patch.object(verus, "compile_check", None):
                result = verus.check_fault(fault, {}, Path(temporary) / "log")
            self.assertEqual(result["check"], "test")
            self.assertTrue(result["fails_as_required"])
            self.assertEqual(source.read_text(), "plugin.step(context)\n")

    def test_a_fault_whose_text_is_not_found_once_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            (Path(temporary) / "loop_rate.rs").write_text("a();\na();\n")
            fault = verus.Fault("twice", "loop_rate.rs", "a();\n", "", "proof")
            with patch.object(verus, "CORE", Path(temporary)), \
                 self.assertRaises(RuntimeError):
                verus.check_fault(fault, {}, Path(temporary) / "log")

    def test_an_unknown_check_kind_is_refused(self):
        with self.assertRaises(ValueError):
            verus.Fault("odd", "loop_rate.rs", "a", "b", "lint")

    def test_the_verified_count_is_read_from_the_report_line(self):
        self.assertEqual(verus.verified_count("verification results:: 7 verified, 0 errors\n"), 7)
        self.assertIsNone(verus.verified_count("error: could not compile\n"))

    def test_a_surviving_fault_fails_the_check(self):
        with patch.object(verus, "prove", lambda env: (0, 1.0, "verification results:: 1 verified, 0 errors")), \
             patch.object(verus.shutil, "which", lambda name, path=None: "/usr/bin/cargo-verus"), \
             patch.object(verus, "versions", lambda env: ["Verus", "Version: 0.2026.11.02"]), \
             patch.object(verus, "source_hashes", dict), \
             patch.object(verus, "check_fault",
                          lambda fault, env, log: {"fault": fault.name, "fails_as_required": False,
                                                   "seconds": 1.0}), \
             tempfile.TemporaryDirectory() as temporary:
            with patch("sys.stdout"):
                code = verus.main(["--output", temporary])
            self.assertEqual(code, 1)


if __name__ == "__main__":
    unittest.main()
