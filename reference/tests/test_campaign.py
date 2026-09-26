import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import campaign

CLEAN_CASE = {"passed": True, "comparison": {"passed": True, "max_position_error_m": 1e-12},
              "summary_agreement": "agree", "cpp_non_interference": {}, "worker_equality": {},
              "trace_equality": {}, "recording_equality": {},
              "trace_comparison": {"delivery_mismatches": 0, "max_payload_covariance_error": 0,
                                   "max_output_error": 0, "max_belief_position_error_m": 0,
                                   "max_belief_velocity_error_mps": 0}}


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


class CampaignTests(unittest.TestCase):
    def run_campaign(self, arguments, fail=None):
        """Run the campaign with every subprocess faked; `fail` maps a script to its failure."""
        commands = []

        def fake_run(command, **kwargs):
            script = Path(command[1]).name
            commands.append(script)
            args = [str(argument) for argument in command[2:]]
            output = Path(args[args.index("--output") + 1]) if "--output" in args else None
            if fail and fail.get(script) == "crash":
                return SimpleNamespace(returncode=2)
            if script == "reference_check.py":
                write(output / "report.json", {"passed": True})
                if "--mission" in args:
                    noise = CLEAN_CASE
                    if fail and fail.get("noise") == "deliveries":
                        noise = {**CLEAN_CASE, "trace_comparison": {
                            **CLEAN_CASE["trace_comparison"], "delivery_mismatches": 500}}
                    write(output / "00-noisy-contacts-noise/result.json", noise)
                elif "--timeout" in args:
                    for index in range(10):
                        write(output / f"{index:02}-case/result.json", CLEAN_CASE)
                elif "--mission-dir" in args:
                    for index in range(100):
                        write(output / f"{index:02}-case/result.json", CLEAN_CASE)
            elif script == "compare.py":
                return SimpleNamespace(returncode=1)
            elif script == "sensitivity.py":
                write(output / "report.json", {"control": {}, "defects": [{"detected": True}]})
                if fail and fail.get(script) == "interrupted":
                    return SimpleNamespace(returncode=1)
            elif script == "divergence.py":
                write(output / "report.json", {"cases": []})
            elif script == "retain_check.py":
                if fail and fail.get(script) == "fail":
                    return SimpleNamespace(returncode=1)
                Path(args[1]).mkdir(parents=True)
            return SimpleNamespace(returncode=0)

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "runs").mkdir()
            data = root / "data"
            (data / "cpp-comparison").mkdir(parents=True)
            (data / "cpp-comparison/old.txt").write_text("previous evidence")
            with patch.object(campaign, "ROOT", root), patch.object(campaign, "DATA", data), \
                    patch.object(campaign.subprocess, "run", side_effect=fake_run):
                code = campaign.main(arguments)
            kept_old = (data / "cpp-comparison/old.txt").is_file()
            summary = json.loads((root / "runs/campaign-000/summary.json").read_text())
        return code, commands, summary, kept_old

    def test_quick_success_refreshes_data_and_tables(self):
        code, commands, summary, kept_old = self.run_campaign(["--quick"])
        self.assertEqual(code, 0)
        self.assertEqual(commands, ["reference_check.py", "retain_check.py", "make_tables.py"])
        self.assertTrue(summary["retained"])
        self.assertFalse(kept_old)

    def test_full_campaign_retains_every_folder(self):
        code, commands, summary, _ = self.run_campaign([])
        self.assertEqual(code, 0, summary["outcomes"])
        self.assertEqual(commands.count("retain_check.py"), 6)
        self.assertEqual(commands[-1], "make_tables.py")

    def test_crashed_matrix_fails_and_keeps_existing_evidence(self):
        code, commands, summary, kept_old = self.run_campaign(["--quick"],
                                                              fail={"reference_check.py": "crash"})
        self.assertEqual(code, 1)
        self.assertEqual(summary["outcomes"]["matrix"], "FAILED")
        self.assertNotIn("retain_check.py", commands)
        self.assertTrue(kept_old)

    def test_failed_retain_keeps_existing_evidence(self):
        code, _, summary, kept_old = self.run_campaign(["--quick"], fail={"retain_check.py": "fail"})
        self.assertEqual(code, 1)
        self.assertFalse(summary["retained"])
        self.assertTrue(kept_old)

    def test_interrupted_sensitivity_study_fails(self):
        code, _, summary, kept_old = self.run_campaign([], fail={"sensitivity.py": "interrupted"})
        self.assertEqual((code, summary["outcomes"]["sensitivity"], kept_old), (1, "FAILED", True))

    def test_noise_delivery_mismatches_fail(self):
        code, _, summary, _ = self.run_campaign([], fail={"noise": "deliveries"})
        self.assertEqual((code, summary["outcomes"]["noise"]), (1, "FAILED"))

    def test_missing_checks_are_not_clean(self):
        self.assertFalse(campaign.ran_cleanly({}))
        self.assertTrue(campaign.ran_cleanly(CLEAN_CASE))

    def test_no_retain_runs_checks_only(self):
        code, commands, _, kept_old = self.run_campaign(["--quick", "--no-retain"])
        self.assertEqual((code, commands, kept_old), (0, ["reference_check.py"], True))


if __name__ == "__main__":
    unittest.main()
