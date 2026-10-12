import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import suite

CLEAN_CASE = {"passed": True, "comparison": {"passed": True, "max_position_error_m": 1e-12},
              "summary_agreement": "agree", "cpp_non_interference": {}, "worker_equality": {},
              "trace_equality": {}, "recording_equality": {},
              "trace_comparison": {"delivery_mismatches": 0, "max_payload_covariance_error": 0,
                                   "max_output_error": 0, "max_belief_position_error_m": 0,
                                   "max_belief_velocity_error_mps": 0}}


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


def write_case(folder, name, result):
    write(folder / name / "result.json", result)
    (folder / name / "input").mkdir()
    (folder / name / "input/input.xml").write_text("<runscript/>")


class SuiteTests(unittest.TestCase):
    def run_suite(self, arguments, fail=None):
        """Run the suite with every subprocess faked; `fail` maps a step to its failure."""
        fail = fail or {}
        steps = []

        def fake_run(command, **kwargs):
            if command[0] == "cargo":
                steps.append("cargo " + command[1])
                return SimpleNamespace(returncode=0)
            script = Path(command[1]).name if command[1] != "-m" else "unittest"
            steps.append(script)
            args = command[2:]
            output = Path(args[args.index("--output") + 1]) if "--output" in args else None
            if fail.get(script) == "crash":
                return SimpleNamespace(returncode=2)
            if script == "reference_check.py":
                write(output / "report.json", {"passed": True})
                if "--mission" in args:
                    noise = CLEAN_CASE
                    if fail.get("noise") == "deliveries":
                        noise = {**CLEAN_CASE, "trace_comparison": {
                            **CLEAN_CASE["trace_comparison"], "delivery_mismatches": 500}}
                    write_case(output, "noisy-contacts-noise", noise)
                elif "--timeout" in args:
                    for index in range(10):
                        write_case(output, f"generated-{index:03}", CLEAN_CASE)
                elif "--mission-dir" in args:
                    for index in range(100):
                        write_case(output, f"generated-{index:03}", CLEAN_CASE)
                else:
                    write_case(output, "aircraft-collision", CLEAN_CASE)
            elif script == "determinism.py":
                write(output / "report.json", {"passed": True, "missions": 3,
                                               "missions_differing": 0})
            elif script == "perf.py":
                code = fail.get("perf.py", 0)
                write(Path(args[args.index("--output") + 1]), {"slower": 0})
                return SimpleNamespace(returncode=code)
            elif script == "divergence.py":
                write(output / "report.json", {"cases": []})
            elif script == "platform_check.py":
                write(output / "report.json", {"cases": [], "max_position_difference_m": 0})
            elif script == "sensitivity.py":
                write(output / "report.json", {"control": {}, "defects": [{"detected": True}]})
            elif script == "make_tables.py" and fail.get("tables"):
                return SimpleNamespace(returncode=1)
            return SimpleNamespace(returncode=0)

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            data = root / "data"
            (data / "cpp-agreement").mkdir(parents=True)
            (data / "cpp-agreement/old.txt").write_text("previous evidence")
            with patch.object(suite, "RUNS", root / "runs"), patch.object(suite, "DATA", data), \
                    patch.object(suite, "references", return_value={}), \
                    patch.object(suite.subprocess, "run", side_effect=fake_run):
                code = suite.main(arguments)
            kept_old = (data / "cpp-agreement/old.txt").is_file()
            summary = json.loads((root / "runs/suite000/summary.json").read_text())
            release = json.loads((data / "release.json").read_text()) \
                if (data / "release.json").is_file() else None
            retained = sorted(path.name for path in data.iterdir() if path.is_dir())
        return SimpleNamespace(code=code, steps=steps, summary=summary, kept_old=kept_old,
                               release=release, retained=retained)

    def test_quick_set_runs_the_quick_checks_only(self):
        result = self.run_suite([])
        self.assertEqual(result.code, 0, result.summary)
        self.assertEqual(list(result.summary["checks"]),
                         ["speed-regression", "rust-tests", "rust-determinism", "cpp-agreement",
                          "multirotor-control", "rascal-control"])
        self.assertNotIn("make_tables.py", result.steps)
        self.assertTrue(result.kept_old)

    def test_retained_evidence_replaces_only_the_checks_that_ran(self):
        result = self.run_suite(["--retain"])
        self.assertEqual(result.code, 0, result.summary)
        self.assertFalse(result.kept_old)
        self.assertEqual(result.retained, ["cpp-agreement", "multirotor-control", "rascal-control",
                                           "rust-determinism", "speed-regression"])
        self.assertEqual(result.steps[-1], "make_tables.py")
        self.assertTrue(all(entry["passed"] for entry in result.release["checks"].values()))

    def test_full_release_runs_every_check(self):
        result = self.run_suite(["--release"])
        self.assertEqual(result.code, 0, result.summary)
        self.assertEqual(list(result.summary["checks"]), [check.id for check in suite.CHECKS])

    def test_a_check_brings_its_prerequisite(self):
        result = self.run_suite(["--check", "checker-sensitivity"])
        self.assertEqual(list(result.summary["checks"]), ["cpp-agreement", "checker-sensitivity"])

    def test_crashed_matrix_fails_and_keeps_existing_evidence(self):
        result = self.run_suite(["--retain"], fail={"reference_check.py": "crash"})
        self.assertEqual(result.code, 1)
        self.assertFalse(result.summary["checks"]["cpp-agreement"]["passed"])
        self.assertNotIn("make_tables.py", result.steps)
        self.assertTrue(result.kept_old)
        self.assertIsNone(result.release)

    def test_missing_performance_baseline_is_a_failure(self):
        result = self.run_suite(["--check", "speed-regression"], fail={"perf.py": 2})
        self.assertEqual(result.code, 1)
        self.assertIn("no compatible baseline", result.summary["checks"]["speed-regression"]["note"])

    def test_noise_delivery_mismatches_fail(self):
        result = self.run_suite(["--check", "cpp-noise"], fail={"noise": "deliveries"})
        self.assertEqual(result.code, 1)
        self.assertFalse(result.summary["checks"]["cpp-noise"]["passed"])

    def test_failed_tables_report_a_failed_retain(self):
        result = self.run_suite(["--retain"], fail={"tables": True})
        self.assertEqual(result.code, 1)
        self.assertFalse(result.summary["retained"])

    def test_missing_checks_are_not_clean(self):
        self.assertFalse(suite.ran_cleanly({}))
        self.assertTrue(suite.ran_cleanly(CLEAN_CASE))

    def test_unknown_check_is_rejected(self):
        with self.assertRaises(SystemExit):
            suite.selected(SimpleNamespace(check=["no-such-check"], release=False))


if __name__ == "__main__":
    unittest.main()
