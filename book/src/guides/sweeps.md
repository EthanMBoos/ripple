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
```

That is 2 seeds × 2 speeds × 2 update times = 8 cases. With `cartesian`,
every combination runs; with `zip`, the lists go in step (the first value of
each, then the second), which suits two settings that move together, such as
the noise on two axes. Cases are numbered with the seed changing fastest, then
the last path.

Each path is a dotted path into the mission, set exactly as a [command-line
override](yaml-missions.md#command-line-overrides) sets it, after templates
are expanded. A path must already exist in the base mission, so give the
mission every setting the sweep varies. Case 0 is loaded before any output is
written, so a misspelled path fails at once.

## Running it

```sh
ripple sweep missions/waypoints-point-agents.sweep.yaml
```

Output goes to `sweeps/<name>/run000`, then `run001` the next time:

```text
sweeps/waypoints-point-agents/run000/
  sweep.json       the sweep's name, case count, and shard
  results.jsonl    one line per case
```

A sweep keeps results, not recordings. To look at one case closely, rerun the
base mission with that case's seed and values:

```sh
ripple run missions/waypoints-point-agents.yaml run.seed:=1 \
  entities.blue.autonomy.WaypointFollower.speed_mps:=3 \
  interactions.WaypointBroadcast.update_at_s:=3
```

Runs are deterministic, so this reproduces the case exactly, and saves a
`run.mcap` you can replay or read (see [Run records](run-records.md)).

## Shards and clusters

A large sweep splits into shards. Shard `i` of `n` runs cases `i`, `i + n`,
`i + 2n`, and so on:

```sh
ripple sweep missions/waypoints-point-agents.sweep.yaml --shard-index 1 --shard-count 3
```

`scripts/bulk_run.py` runs the shards for you, on this machine or on a Slurm
cluster, and merges them:

```sh
cargo build --release
python3 scripts/bulk_run.py local  missions/waypoints-point-agents.sweep.yaml --jobs 4
python3 scripts/bulk_run.py submit missions/waypoints-point-agents.sweep.yaml --jobs 4 \
    -- --account=<account> --partition=<partition>
python3 scripts/bulk_run.py collect sweeps/waypoints-point-agents/run000   # after Slurm
```

Each shard writes to a `shard-NNNN/` folder; `collect` merges them in case
order after checking that every case appears exactly once. `bulk_run.py` uses
the existing release build, so rebuild after changing code. On a cluster,
create the environment once (`conda env create -f environment.yaml`) and
build on shared storage.

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
