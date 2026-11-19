# Sweeps and results

An experiment is rarely one run. A *sweep* runs one mission across seeds and
parameter values and writes one row of results per run, so you can plot how a
score changes with, say, sensor noise.

## A sweep file

A sweep is a file ending in `.sweep.yaml` that names a base mission:

```yaml
name: waypoints-point-agents
base_scenario: waypoints-point-agents.yaml   # relative to the sweep file
seeds: [1, 2]                                # sets run.seed; optional
parameter_combinations: cartesian            # or zip: lists of equal length
parameters:
  entities.blue.autonomy.WaypointFollower.speed_mps: [3, 5]
  interactions.WaypointBroadcast.update_at_s: [3, 100]
  run.end_s: 120                             # one value: the same in every case
```

That is 2 seeds × 2 speeds × 2 update times = 8 cases. With `cartesian`,
every combination runs; with `zip`, the lists go in step (the first value of
each, then the second), which suits two settings that move together, such as
the noise on two axes. A path given one value instead of a list takes it in
every case under either rule, so a zipped sweep can hold a constant without
repeating it down a list. Cases are numbered with the seed changing fastest,
then the last path.

A study with many hand-picked conditions reads better as a table, one row
per case:

```yaml
name: waypoint-conditions
base_scenario: waypoints-point-agents.yaml
seeds: [1, 2, 3]
parameters: # constants only
  run.end_s: 12
prefix: entities.blue.autonomy.WaypointFollower # before each column
columns: [speed_mps, arrival_radius_m]
cases:
  - [5, 0.01]
  - [3, 0.01]
  - [3, 1.0]
```

Each row is one combination, numbered in order, so the cases run and are
recorded exactly as a zipped sweep of the same values would be, and
`results.jsonl` holds the full dotted paths. `parameter_combinations` is left
out. `repeat` runs every condition once per value, like `seeds`, for a
setting that repeats the study rather than changing it, such as a language
model's reply sample when a planner asks one. Seeds cycle fastest, then any
repeat values, then the rows: here 3 rows × 3 seeds = 9 cases.

Each path is a dotted path into the mission, set exactly as a [command-line
override](yaml-missions.md#command-line-overrides) sets it, after templates
are expanded. A path must already exist in the base mission, so give the
mission every setting the sweep varies. Case 0 is loaded before any output is
written, so a misspelled path fails at once.

## Running it

```sh
ripple sweep missions/waypoints-point-agents.sweep.yaml
```

Output goes to `sweeps/<name>/run000`, then `run001` the next time, with
`results.jsonl` holding one line per case (the folder's other files are
described below).

A sweep keeps results, not recordings. To look at one case closely, rerun the
base mission with that case's seed and values:

```sh
ripple run missions/waypoints-point-agents.yaml run.seed:=1 \
  entities.blue.autonomy.WaypointFollower.speed_mps:=3 \
  interactions.WaypointBroadcast.update_at_s:=3
```

Runs are deterministic, so this reproduces the case exactly, and saves a
`run.mcap` you can replay or read (see [Run records](run-records.md)).

## Running on all your cores

One process runs the cases one after another. `--jobs` runs them as that
many shards at once, one process and one CPU each:

```sh
ripple sweep missions/waypoints-point-agents.sweep.yaml --jobs 4
```

```text
waypoints-point-agents: 8 cases in 4 jobs -> sweeps/waypoints-point-agents/run000
waypoints-point-agents: 8 of 8 done, 0 errors, 12 s -> sweeps/waypoints-point-agents/run000/results.jsonl
```

`ripple sweep --help` says how many CPUs this machine has. Fewer than that
is often right: a laptop's slow cores add little, a large mission takes
memory per case (one 1,024-hub delivery day takes 1.5 GB), and a mission
that calls a language model is rate-limited by the provider, not the CPU.
Four jobs for model calls and six for plain simulation have served on a
ten-core laptop.

The results file is the same, byte for byte, at any job count. Every case
is deterministic, and the rows are merged in case order, so the analysis
never has to ask how the sweep ran.

## Watching it

While the sweep runs, one line keeps up with it:

```text
waypoints-point-agents: 12 of 20 done, 4 running, 0 errors, 9 min elapsed
```

The count is read from the rows the shards have written, so the same line
is available from another terminal, or the next day:

```sh
ripple status missions/waypoints-point-agents.sweep.yaml
```

```text
waypoints-point-agents: run000 running, 12 of 20 done, 0 errors, 9 min since it started
```

It says `complete` once the rows are merged and `stopped` when nothing is
running and they are not. There is no time-left estimate: cases of
different sizes interleave, and a guess would mislead. Each shard's log in
`logs/` records how long each case took.

## Stopping, and finishing later

Press Ctrl-C once and the sweep stops cleanly: no new case starts, the
running ones finish, and it says where things stand.

```text
waypoints-point-agents: stopping; 4 running cases will finish, press Ctrl-C again to kill them
waypoints-point-agents: stopped with 16 of 20 done -> sweeps/waypoints-point-agents/run000
waypoints-point-agents: finish it later with: ripple sweep missions/waypoints-point-agents.sweep.yaml
```

A case in flight cannot be paused, so a clean stop takes as long as the
longest running case. Press Ctrl-C again to kill the running cases now.
Every case already finished is kept either way; the second press costs
exactly the cases that were running.

To finish the run, type the same command again. It finds the newest run of
that file unfinished and resumes it, running only the missing cases:

```text
waypoints-point-agents: run000 is unfinished, 16 of 20 done; resuming with its 4 jobs (--new starts another)
waypoints-point-agents: 4 cases to run
waypoints-point-agents: 20 of 20 done, 0 errors, 3 min -> sweeps/waypoints-point-agents/run000/results.jsonl
```

A resume uses the job count the run started with; a `--jobs` given now is
ignored, and the line says so, because the split of cases into shards was
fixed when the run began. "Unfinished" means some case has no row yet. A
run whose cases all ran, some with an error, is finished: an error is a
result. To rerun errored cases, start a new run; for a sweep that calls a
model that costs little, since every saved reply replays without a call.

## A fresh run instead

`--new` leaves the unfinished run as it is and starts the next number:

```text
waypoints-point-agents: run000 is unfinished and left as is
waypoints-point-agents: 20 cases in 4 jobs -> sweeps/waypoints-point-agents/run001
```

A finished run never prompts anything: running the command again after a
complete run starts the next number, as it always did. Three things refuse
to resume, each naming `--new` as the way around:

```text
waypoints-point-agents: run000 is unfinished, but missions/waypoints-point-agents.sweep.yaml changed since it started; use --new
waypoints-point-agents: run000 is unfinished, but the ripple binary changed since it started; use --new
waypoints-point-agents: run000 is already running (pid 48213); wait for it or stop it
```

The first two keep rows from two versions of a file, or two builds, out of
one results file, which is the one mistake nobody could see afterward. The
third keeps two terminals from writing into one run.

## What is in the run folder

```text
sweeps/waypoints-point-agents/run000/
  campaign.json    the sweep file and binary (with their hashes), the job count, the start time
  logs/            one log per shard: a line per case with its time, kept across resumes as .1.log, .2.log
  shard-0000/      one shard's own sweep.json and results.jsonl
  shard-0001/
  ...
  results.jsonl    every case in case order, written when the last shard finishes
```

Shard `i` of `n` runs cases `i`, `i + n`, `i + 2n`, ... one after another
and appends a row to its own `results.jsonl` as each finishes. So case `k`
is the next row of shard `k mod n`, and the merge walks the case numbers,
takes the next row of the right shard, and checks its case ID. A gap, an
extra row, or a wrong ID stops the merge with the case named; nothing is
ever misordered silently. The merged file is renamed into place only when
it is complete, so a half-merged file can never pass for a whole one.

A shard needs nothing but the sweep file and its two numbers, which is why
one can be run by hand under any scheduler:

```sh
ripple sweep missions/waypoints-point-agents.sweep.yaml --shard-index 1 --shard-count 3 --output <folder>/shard-0001
```

A folder of such shards with a `campaign.json` merges with `ripple collect
<folder>`.

## On a cluster

With `--slurm`, the shards become one Slurm array job. Run it from a login
node, with the repository, the release binary, and your environment on
shared storage; arguments after `--` go to `sbatch` untouched:

```sh
ripple sweep missions/waypoints-point-agents.sweep.yaml --jobs 40 --slurm -- --account=<account> --partition=<partition>
```

```text
waypoints-point-agents: 1240 cases in 40 jobs -> sweeps/waypoints-point-agents/run000
waypoints-point-agents: submitted Slurm array 7731204, 40 tasks
waypoints-point-agents: when it finishes: ripple collect sweeps/waypoints-point-agents/run000
```

`--jobs` is the array size, so it is required here. Each task runs
`scripts/ripple-shard.slurm`, a short shell file that turns its task ID
into the shard command above. That file is yours to edit: module loads,
`conda activate` or `source .venv/bin/activate`, an `srun` prefix, a time
or memory line. Keep one copy per cluster and name it with `--script`. The
environment in `environment.yaml` builds the binary on shared storage.

The command returns at submission, so nothing has merged the shards when
the array finishes. `ripple collect <folder>` does that step; run it when
`squeue` no longer lists the job. Too early, it says what is missing and
writes nothing:

```text
waypoints-point-agents: expected 40 shards, found 12; the array is still running or a task never started
```

A task killed by a time limit leaves its shard short. Run the same `ripple
sweep --slurm` command again: it resumes the run by submitting an array of
only the short shards, and `--exclude=<node>` after `--` avoids a node
known to be bad. Then `collect` as before.

## Reading the results

Each line of `results.jsonl` is one case:

```json
{"case_id": "case-0000", "seed": 1,
 "parameters": {"entities.blue.autonomy.WaypointFollower.speed_mps": 3, "...": 3},
 "error": null, "steps": 120, "termination": "TimeLimit",
 "metrics": {"metrics/SimpleCollisionMetrics:0": {
     "1": {"score": 0.0, "non_team_collisions": 0.0, "flight_time_s": 12.0, "...": 0.0},
     "2": {"...": 0.0}}}}
```

`metrics` holds each metrics plugin's results by team. A value a plugin didn't
set for a team is left out rather than written as zero. A case that failed
has its message in `error` and no results; the other cases still run, and the
sweep exits with an error at the end.

To get a table, one row per case and team:

```python
import json
import pandas as pd

rows = []
for line in open("sweeps/waypoints-point-agents/run000/results.jsonl"):
    case = json.loads(line)
    for plugin, teams in case["metrics"].items():
        for team, values in teams.items():
            rows.append({"seed": case["seed"], **case["parameters"],
                         "plugin": plugin, "team": int(team), **values})
table = pd.DataFrame(rows)
print(table.groupby("entities.blue.autonomy.WaypointFollower.speed_mps")["score"].mean())
```

The starter crate's `follow-nearest.sweep.yaml` is a second example, and the
[first tutorial](../tutorial/first-autonomy.md#7-sweep-the-speed) sweeps a
plugin's speed.
