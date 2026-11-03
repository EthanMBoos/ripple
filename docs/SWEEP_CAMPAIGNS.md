# Sweep campaigns in `ripple`, replacing `scripts/bulk_run.py`

A plan, not scheduled. `scripts/bulk_run.py` runs a sweep in shards, on this
machine or on a Slurm cluster, and merges the shards. This moves that into
the `ripple` command, so a sweep is one tool with one install, and keeps the
part a cluster user edits as a shell script they own.

## What exists today

`ripple sweep` (Rust, `crates/cli/src/sweep.rs`) does the work: it reads the
sweep file, numbers the cases (seeds cycle fastest, then repeats, then value
combinations), runs its shard's cases one after another with one worker
each, and writes a row per case to `results.jsonl` and the counts to
`sweep.json`. `--shard-index i --shard-count n` runs cases `i`, `i + n`,
`i + 2n`, and so on.

`scripts/bulk_run.py` (Python, 200 lines) is glue around that binary:

- `local`: start `n` `ripple sweep` child processes, one per shard, each
  writing `<campaign>/shard-NNNN/`, wait, then collect.
- `submit`: write `campaign.json`, run `sbatch --array=0-(n-1)` on
  `scripts/bulk_run.slurm`, which calls the script back as `worker`, which
  runs the same `ripple sweep` command for its array task.
- `collect`: check that the shards cover every case exactly once, then merge
  the rows in case order into `<campaign>/results.jsonl`.

`environment.yaml` is a conda environment with Rust in it, so the release
binary can be built on the cluster's shared storage.

## Why move it

- The case-numbering rule, "case `k` is the next row of shard `k mod n`",
  is defined in `sweep.rs` and copied into `collect`, with
  `test_bulk_run.py` testing the copy. One language should hold it.
- Slurm's interface here is one command and two environment variables:
  `sbatch`, `SLURM_ARRAY_TASK_ID`, and the submit directory. Rust spawns
  `sbatch` the way Python does; no Slurm library is involved.
- The wheel install (the book's "The Python bridge" page, "Build") puts
  `ripple` on a cluster with `uv add ripple`; a sweep should not also need a
  checkout for a script. With it, the conda environment goes too.
- Shards as separate processes is the right model once plugins can be
  written in Python: each process has its own interpreter and lock, so `n`
  shards give `n`-way parallelism where threads in one process would take
  turns.

## The design

### What a person types

```sh
ripple sweep missions/x.sweep.yaml                 # this machine, one shard per core
ripple sweep missions/x.sweep.yaml --jobs 4        # this machine, four shards
ripple sweep missions/x.sweep.yaml --jobs 40 --slurm -- --account=a --partition=p
ripple collect sweeps/x/run000                     # when the Slurm array has finished
ripple retry sweeps/x/run000 [--slurm -- ...]      # rerun what did not finish, then collect
```

On this machine, `ripple sweep` returns when the sweep is done, with every
case merged into `results.jsonl`. With `--slurm`, run from a shell that can
submit (a login node), it submits one array job of `n` tasks, prints the job
ID and the `collect` line, and returns; `--jobs` is required there, since
it is the array size and the core count says nothing about it. The result
is the same whatever `n` is and wherever it ran: cases are deterministic
and merged in case order, so the shard count never changes a row.

### What a campaign is on disk

```text
sweeps/x/run000/
  campaign.json    the sweep file, the executable, the shard count
  results.jsonl    every case in case order, written by collect
  logs/            one log per shard
  shard-0000/      one shard's own sweep.json and results.jsonl
  shard-0001/
  ...
```

A sweep run whole today leaves one `results.jsonl` and `sweep.json` in the
run folder; a campaign leaves the merged `results.jsonl` in the same place,
so the analysis scripts read it as before.

### How the shards work

Take a sweep of 1,240 cases and `ripple sweep x.sweep.yaml --jobs 40`.

1. **The parent plans.** It reads the sweep file, counts the cases, creates
   the campaign folder, writes `campaign.json` (the sweep path, the
   executable, the number 40), and makes `logs/`.
2. **The parent starts 40 copies of itself.** Copy 7 is started as
   `ripple sweep x.sweep.yaml --shard-index 7 --shard-count 40 --output
   <campaign>/shard-0007`, its terminal output in `logs/shard-0007.log`.
   The two shard flags exist today; nobody types them. The parent waits for
   all 40 to exit.
3. **Each copy picks its cases on its own.** Every copy reads the same file
   and numbers the same 1,240 cases in the same order. Copy 7 keeps the
   cases whose number mod 40 is 7: cases 7, 47, 87, ... (31 of them), runs
   them one at a time, appends a row per case to its own `results.jsonl`,
   flushing after each, and writes `sweep.json` saying "shard 7 of 40,
   1,240 cases in all, 31 here". No copy talks to another: there is no
   queue, no coordinator, and no shared state beyond the sweep file.
4. **The parent merges.** When all 40 have exited it walks the case numbers
   0 to 1,239: case `k` is the next unread line of shard `k mod 40`. Each
   line must carry the expected case ID, or the merge stops and says which
   case is missing. The rows go to `results.jsonl.partial`, renamed to
   `results.jsonl` when complete.

With `--slurm`, only step 2 changes: instead of starting 40 processes the
parent runs `sbatch --array=0-39` on the batch script and exits. Slurm
later starts 40 tasks, each with its task ID in an environment variable,
and the script turns that ID into the command of step 2. Step 4 is `ripple
collect`, run when the array is done, because the parent is long gone.

Step 3 is the whole trick. A shard needs only the file and two numbers, so
a local process and a Slurm task are the same thing, and the merge needs no
bookkeeping beyond "case `k` is the next line of shard `k mod n`". The batch
script is the one place a person reads the shard flags, because that file
is the one a cluster user edits.

### On Slurm, start to finish

Log in to the cluster, where the checkout, the binary, and the environment
are on shared storage, and run `ripple sweep x.sweep.yaml --jobs 40 --slurm
-- --account=a --partition=p`. `ripple` creates the campaign folder there,
writes `campaign.json`, submits one array job of 40 tasks, prints the job
ID and the `collect` command, and exits. Nothing has run yet.

A task is a shard. Slurm starts tasks as the cluster has room, maybe all 40
at once, maybe ten at a time for hours, in any order; each gets one CPU,
runs the batch script with its task ID in an environment variable, and the
script runs that shard into `shard-00<id>/`. With 1,240 cases a task runs
31 of them one after another. Tasks are independent, so one still queued
holds up none of the others.

`ripple sweep --slurm` does not collect, because it exited at submission.
Run `ripple collect <campaign>` when the array is done, which is when
`squeue` no longer shows the job ID. Run too early, it refuses and says
what is missing ("expected 40 shards, found 12" while tasks are starting,
"case-0407 is missing; a shard did not finish" while one runs), leaves no
partial file, and can be run again. It assumes nothing about filesystem
timing beyond what Slurm gives: a shard flushes a row as each case
finishes and closes its files when it exits, and a shared filesystem reads
a closed file complete from any node, so once Slurm reports the job
finished every shard file is final. During the job, rows can lag seconds
on NFS, and the worst case is a "missing" message for a case that is in
fact done; a wrong row can never be merged, since every line is checked
against its expected case ID.

A task killed by a time limit or a node failure leaves its shard partial,
and `collect` names the first missing case. `ripple retry` reruns what is
missing (below).

### What `--slurm` runs

One `sbatch` call, with the arguments after `--` passed through untouched:

```sh
sbatch --parsable <arguments after --> --array=0-(n-1) \
    --output=<campaign>/logs/slurm-%A_%a.out --export=ALL scripts/ripple-shard.slurm
```

and these in the environment, which `--export=ALL` carries to each task:

| Variable | Value |
| --- | --- |
| `RIPPLE_BIN` | the executable that ran `ripple sweep`, so the task runs the same build |
| `RIPPLE_SWEEP` | the sweep file, absolute |
| `RIPPLE_CAMPAIGN` | the campaign folder, absolute |
| `RIPPLE_SHARD_COUNT` | `n` |

The batch script is the whole worker, in plain sight:

```sh
#!/bin/sh
# One Slurm array task runs one shard of a ripple sweep campaign.
# `ripple sweep --slurm` submits the array and sets the RIPPLE_* variables.
# Edit this file for your cluster: module loads, environment activation, an
# srun prefix. Point `ripple sweep --slurm --script` at a copy if you keep one
# per cluster. What ripple reads back (shard folder names, case numbering,
# sweep.json) is fixed; everything else here is yours.
#SBATCH --job-name=ripple-sweep
#SBATCH --cpus-per-task=1
set -eu
shard=$(printf '%04d' "$SLURM_ARRAY_TASK_ID")
"$RIPPLE_BIN" sweep "$RIPPLE_SWEEP" \
    --output "$RIPPLE_CAMPAIGN/shard-$shard" \
    --shard-index "$SLURM_ARRAY_TASK_ID" --shard-count "$RIPPLE_SHARD_COUNT"
```

### `ripple collect <campaign>`

The merge, as the script does it: `n` shard folders, each `sweep.json`
belonging to this campaign (index, count, total), rows read in case order
from shard `k mod n`, any gap, extra row, or wrong ID an error, written to
`results.jsonl.partial` and renamed only when complete. The error count is
reported. It needs no plugin registry, so it is its own command, not a flag
on `sweep`.

### `ripple retry <campaign> [--jobs n] [--slurm -- <sbatch arguments>]`

Reruns the parts of a campaign that did not finish, then collects. It
reads `campaign.json` for the sweep file and the shard count, looks at
every shard, and reruns a shard that is

- **missing**: its folder does not exist, because its task never started
  (a cancelled array, a node that never came up);
- **partial**: its `results.jsonl` has fewer rows than its `sweep.json`
  promised, because the task died (time limit, node failure, a kill);
- **errored**: one or more of its rows has an `error`. A case error is a
  recorded result, and a deterministic one comes back identical, but the
  common error in a model-calling sweep is transient (a rate limit, an
  outage after the planner's three tries), and a rerun asks again.

Before running anything it prints what it found: "3 shards incomplete, 7
cases with errors in 5 shards". Then each such shard is run again by the
shard command with one more plumbing flag, `--resume`, which lets the shard
write into its existing folder: it reads the rows already there, keeps
every complete row without an error, and runs only its other cases, in
case order as always. The previous `results.jsonl` and log are kept as
`results.jsonl.1` and `shard-0007.1.log` (then `.2`, ...), so a retry
never destroys an attempt. Shards that finished cleanly are not touched.

Locally the reruns are child processes, as for `sweep`, and `retry`
collects at the end. With `--slurm` it submits one array job of only the
shard indices it is rerunning (`--array=3,17,22`); Slurm places them on
whatever nodes have room, so a shard that died with a node lands elsewhere,
and `--exclude=<node>` after `--` pins that if a node is known bad. Then
`ripple collect` as usual. `retry` is its own command rather than a flag on
`sweep` because it takes a campaign, not a sweep file, and everything else
comes from `campaign.json`.

### Where it lives

`crates/cli/src/sweep/campaign.rs`, a submodule of `sweep.rs`: the campaign
record, the worker command, local jobs, the `sbatch` call, collect, and
retry, with the three collect tests and the two command tests from
`test_bulk_run.py` as Rust unit tests on a temporary folder, plus tests
that retry picks the right shards and that a resumed shard keeps its good
rows. `sweep.rs` gains the `--jobs`, `--slurm`, `--script`, and trailing
`-- <sbatch arguments>` options (clap's `last = true`) and the hidden
`--resume`. `lib.rs` gains `Collect` and `Retry`.

## The edits a cluster needs, and where they go

The worry: cluster glue always needs a small edit to run right, and a
compiled command takes that away. It does not, because the split is by who
reads what:

- **Slurm reads the batch script.** Module loads, `conda activate` or
  `source .venv/bin/activate`, an `srun` prefix, a different binary path, a
  time limit, a memory line: all in `scripts/ripple-shard.slurm`, a shell
  file in the repository that the user edits or copies (`--script` names the
  copy). The worker command is written out in full there, not hidden in
  Rust, so changing how a shard is run is a one-line edit.
- **Slurm reads the submit arguments.** Account, partition, QoS, time: after
  `--`, passed through untouched, as today.
- **`ripple` reads the campaign.** Folder names, `campaign.json`, case
  numbering, `sweep.json`, the merge check. These are the things a hand edit
  should never touch, because a wrong edit silently misorders results, and
  they are the only things that move into Rust.

Someone who needs more than that, for example a cluster without arrays or a
scheduler that is not Slurm, still has the primitives: `ripple sweep
--shard-index --shard-count` per task however they like to launch it, and
`ripple collect` on a folder of shards. Only `campaign.json` has to exist,
and the plan has `ripple collect` accept a folder without one by counting
the shards and trusting their `sweep.json`, so a hand-launched campaign
collects too.

## Decisions to make

Each of these changes something or was not in the script; the recommended
choice is first.

1. **The default job count.** The core count, so a plain `ripple sweep`
   uses the machine; `--jobs 1` for a sweep whose cases should not overlap,
   such as one that calls a rate-limited model. The result is the same
   either way.
2. **A failed case.** Today a case error makes `ripple sweep` exit nonzero,
   the script raises on the exit code, and the campaign is never collected,
   even though every row is there with its error recorded. Recommended:
   collect anyway, report the count, exit nonzero. The rows are the record;
   an uncollected folder of shards helps no one.
3. **Collecting twice.** The script replaces `results.jsonl` on a second
   collect. Recommended: refuse if it exists ("already collected"), in line
   with never overwriting a run. A rerun of a shard goes in a new campaign.
4. **The cluster binary.** `RIPPLE_BIN` is whatever ran the submit: the
   release build on shared storage today, the console script of a wheel in
   a shared venv later. Both need to be visible on the compute nodes, as the
   conda environment is today; nothing new, but the guide should say it.
5. **`environment.yaml`.** Keep until wheels exist (it is how the binary gets
   built on the cluster), with its comment pointing at the new command; drop
   it with the wheel. It is not part of this change.
6. **Watching the job.** `ripple collect --wait`, polling `squeue` for the
   array, would be a few lines. Not in this change; the script never had it
   and `collect` already says what is missing.
7. **What `retry` reruns.** Missing and partial shards, and shards with an
   errored case, as above. The alternative, incomplete shards only, leaves
   a model-calling sweep with transient errors in its rows and no way to
   clear them short of a new campaign. Rerunning a deterministic error
   costs one case and changes nothing.

Things checked that need nothing:

- **Parallel shards and the model-reply cache.** The delivery planner writes
  each reply to a per-process file and hard-links it into place, so shards
  that ask the same prompt at once agree on the first reply stored. Local
  `--jobs` on the delivery sweeps is safe today and stays so.
- **Python plugins on a cluster.** Each task's environment needs the
  plugin's libraries; a missing one fails at load with the package name and
  the `uv add` line ("Plugins in Python", "Three rules"), in that
  shard's log.
- **Inner parallelism.** A shard runs each case with one worker, matching
  one CPU per task. On a laptop `--jobs 8` is eight single-worker
  processes, which is what the script does now.

## What changes in the repository

- `crates/cli/src/sweep.rs`: options; module doc no longer names the script.
- `crates/cli/src/sweep/campaign.rs`: new, with the tests.
- `crates/cli/src/lib.rs`: the `Collect` and `Retry` commands.
- `scripts/ripple-shard.slurm`: the batch script above, replacing
  `scripts/bulk_run.slurm`.
- Deleted: `scripts/bulk_run.py`, `scripts/test_bulk_run.py`.
- `book/src/guides/sweeps.md`, "Shards and clusters": the three commands,
  the batch script as the place for cluster edits.
- `crates/delivery/README.md`, `missions/waypoints-point-agents.sweep.yaml`,
  `environment.yaml`: the command in place of the script.
- `book/src/development/python-bridge.md`: one line under "Build" that a
  sweep campaign is part of the same command.

## Steps

1. `campaign.rs` with collect and its tests, ported from `test_bulk_run.py`.
2. `--jobs`: the campaign folder, `campaign.json`, child processes, logs,
   collect at the end.
3. `--slurm`, `--script`, the pass-through arguments, the batch script.
4. `ripple collect`.
5. `--resume` on the shard, then `ripple retry`, local and Slurm.
6. Docs and the deletions.

Verification, before the script is deleted: run
`missions/waypoints-point-agents.sweep.yaml` with `bulk_run.py local
--jobs 3` and with `ripple sweep --jobs 3`, and `cmp` the two
`results.jsonl`; they must be byte-identical. Submit a one-case campaign to
Slurm when a cluster is at hand; until then the `sbatch` call is covered by
the unit test that checks the command line, as the script's test does.
