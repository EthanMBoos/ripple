#!/bin/sh
# One Slurm array task runs one shard of a ripple sweep campaign.
# `ripple sweep --slurm` submits the array and sets the RIPPLE_* variables;
# --export=ALL carries your activated environment (and any API keys) here.
#
# Edit this file for your cluster: module loads, environment activation, an
# srun prefix, a time or memory line. Point `ripple sweep --slurm --script`
# at a copy if you keep one per cluster. What ripple reads back (the shard
# folder names, the case numbering, sweep.json) is fixed; everything else
# here is yours.
#SBATCH --job-name=ripple-sweep
#SBATCH --cpus-per-task=1

set -eu
shard=$(printf '%04d' "$SLURM_ARRAY_TASK_ID")
# RIPPLE_SHARD_ARGS holds --root, --max-steps and, on a resume, --resume;
# it is split on spaces on purpose.
# shellcheck disable=SC2086
"$RIPPLE_BIN" sweep "$RIPPLE_SWEEP" \
    --output "$RIPPLE_CAMPAIGN/shard-$shard" \
    --shard-index "$SLURM_ARRAY_TASK_ID" --shard-count "$RIPPLE_SHARD_COUNT" \
    $RIPPLE_SHARD_ARGS
