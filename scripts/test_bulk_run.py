"""Checks for bulk_run.py's collect and Slurm submission (a trimmed copy of Ripple's)."""

from __future__ import annotations

import argparse
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import bulk_run


class CollectTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="scrimmage-bulk-")
        self.root = Path(self.temporary.name)
        (self.root / "campaign.json").write_text(json.dumps(
            {"sweep": "test.sweep.yaml", "binary": "scrimmage", "jobs": 2}))
        # Two shards of four cases: shard 0 has cases 0 and 2, shard 1 has 1 and 3.
        for index in range(2):
            shard = self.root / f"shard-{index:04}"
            shard.mkdir()
            (shard / "sweep.json").write_text(json.dumps({
                "name": "test", "total_case_count": 4,
                "shard_index": index, "shard_count": 2, "case_count": 2,
            }))
            (shard / "results.jsonl").write_text("".join(
                json.dumps({"case_id": f"case-{case:04}", "error": None}) + "\n"
                for case in range(index, 4, 2)))

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_complete_shards_merge_in_case_order(self) -> None:
        bulk_run.collect(self.root)
        rows = [json.loads(line) for line in (self.root / "results.jsonl").read_text().splitlines()]
        self.assertEqual([row["case_id"] for row in rows],
                         ["case-0000", "case-0001", "case-0002", "case-0003"])

    def test_missing_case_is_rejected_and_leaves_no_results(self) -> None:
        (self.root / "shard-0001/results.jsonl").write_text(
            json.dumps({"case_id": "case-0001", "error": None}) + "\n")
        with self.assertRaisesRegex(SystemExit, "case-0003 is missing"):
            bulk_run.collect(self.root)
        self.assertFalse((self.root / "results.jsonl").exists())

    def test_shard_from_another_campaign_is_rejected(self) -> None:
        sweep = self.root / "shard-0001/sweep.json"
        record = json.loads(sweep.read_text())
        record["shard_count"] = 3
        sweep.write_text(json.dumps(record))
        with self.assertRaisesRegex(SystemExit, "does not belong"):
            bulk_run.collect(self.root)

    def test_worker_runs_one_shard_into_its_own_folder(self) -> None:
        record = {"sweep": "s.sweep.yaml", "binary": "scrimmage", "jobs": 4}
        command = bulk_run.worker_command(record, self.root, 2)
        self.assertEqual(command[:3], ["scrimmage", "sweep", "s.sweep.yaml"])
        self.assertIn(str(self.root / "shard-0002"), command)
        self.assertEqual(command[-4:], ["--shard-index", "2", "--shard-count", "4"])

    def test_submit_starts_one_array_for_the_campaign(self) -> None:
        sweep = self.root / "test.sweep.yaml"
        sweep.write_text("")
        binary = self.root / "scrimmage"
        binary.write_text("")
        arguments = argparse.Namespace(sweep=sweep, binary=binary, output=self.root / "new",
                                       jobs=3, slurm_args=["--partition=cpu"])
        submitted = bulk_run.subprocess.CompletedProcess([], 0, stdout="12345;cluster\n")
        with patch.object(bulk_run.shutil, "which", return_value="sbatch"), \
                patch.object(bulk_run.subprocess, "run", return_value=submitted) as run:
            bulk_run.submit(arguments)
        command = run.call_args.args[0]
        self.assertIn("--array=0-2", command)
        self.assertIn("--partition=cpu", command)
        self.assertIn("--export=ALL", command)
        self.assertEqual(run.call_args.kwargs["env"]["SCRIMMAGE_SWEEP_OUTPUT"],
                         str((self.root / "new").resolve()))


if __name__ == "__main__":
    unittest.main()
