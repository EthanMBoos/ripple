# Run records

Each `ripple run` creates a new directory containing `run.mcap`. The MCAP file
holds Ripple's protobuf schema, frames, generated entity descriptions, events,
the metric summary, and the resolved scenario. The final metadata gives the
termination reason, step count, and any run error. A run that fails while
stepping still closes a readable record of the work completed so far.

The main channels are:

| Channel | Contents |
| --- | --- |
| `/entities` | An entity's ID, team, group index and label, and visual model, once at its first frame |
| `/frames` | Simulation time and the entities' current truth and active flags |
| `/events` | Generation, removal, and collision events |
| `/summary` | Each metric plugin's score and columns per team, at full precision; a column with no value for a team is left out |

`--trace` adds `/beliefs`, `/readings`, `/deliveries`, and `/outputs` for
analysis and learning data. Belief records include the contacts a policy can see;
delivery records include message payloads. Controller outputs are recorded
after the last motion substep.

With perfect tracking, every entity can see every other entity. A traced run
then stores roughly one contact per entity pair per frame, so large population
traces grow quickly. Use `--trace` for runs whose observations you need.

The message timestamp is nanoseconds since the simulator's initial time
(`start_s - dt_s`); that origin is recorded in MCAP metadata. Each frame,
event, and trace record also carries its simulation time as a double.
Trace observations use MCAP's JSON message encoding and carry their exact
simulation time in `time_s`. `reference/run.py` reads them for analysis.

Terrain runs record a fingerprint of the terrain header, height grid, and any
aerial image. Replay loads those same files from the path in the recorded
scenario and checks the fingerprint before drawing. Keep the files at that
path; replay cannot draw changed or missing terrain.

## View a run

```sh
ripple replay runs/straight-no-gui/run000
ripple replay runs/straight-no-gui/run000 --draw-trails
```

Replay reads `run.mcap` and draws a temporary Rerun recording. It never steps
the simulator. `ripple run --viewer` streams the same drawing live. To keep an
RRD export beside the MCAP, pass `--export-rrd` to `run`; ordinary runs save
only MCAP. `--draw-trails` adds each agent's recent path to a live or replayed
view.

## Read it from Python

`run.mcap` is compressed with zstd. `reference/run.py` reads it with Python's
standard library from Python 3.14; on older versions, `pip install
backports.zstd` first. From the repository root:

```python
import sys
sys.path.insert(0, "reference")
from run import read_run

saved = read_run("runs/straight-no-gui/run000")
for frame in saved.frames:
    for entity in frame.entities.values():
        print(frame.time_s, entity.id, entity.position)

# Each metric plugin's results by team, as in a sweep's results.jsonl.
team = saved.metrics["metrics/SimpleCollisionMetrics:0"][1]
print(team["score"], team["non_team_collisions"])
```

For pandas, build rows from the same loop and pass them to
`pandas.DataFrame`. MCAP tools can decode the embedded protobuf schema too;
the schema source is `crates/core/proto/record.proto`.
