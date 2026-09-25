#!/bin/sh
# One Slurm array task = one shard of a sweep. Copied from Ripple.
# bulk_run.py submit starts the array; this only runs the shard for this task.
# The environment, checkout, and release binary are on shared storage.
# --export=ALL carries the activated environment here; nothing is reinstalled.
# Pass any extra sbatch arguments after `--` in the launcher command.
#SBATCH --job-name=scrimmage-sweep
#SBATCH --cpus-per-task=1

set -eu
cd "${SLURM_SUBMIT_DIR:?submit from the scrimmage-rs repository root}"
python3 scripts/bulk_run.py worker
