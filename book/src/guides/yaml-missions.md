# YAML missions

Missions are YAML files. Each loads into the typed `ScenarioConfig`, the same
data a [Rust-built scenario](rust-scenarios.md) fills in.

| | Status |
| --- | --- |
| Single missions, plugin parameters, command-line overrides | Implemented |
| Templates | Implemented |
| Sweeps (`*.sweep.yaml`), shards, and the Slurm launcher | Implemented (minimal) |

Every mission in
[missions/](https://github.com/EthanMBoos/ripple/blob/main/missions/README.md)
is an example; start with
[straight-no-gui](https://github.com/EthanMBoos/ripple/blob/main/missions/straight-no-gui.yaml)
and
[multirotor](https://github.com/EthanMBoos/ripple/blob/main/missions/multirotor.yaml).

## Rules

- Unknown keys are errors everywhere, including plugin parameters.
- A plugin with `{}` (or nothing) after it means "this plugin, all
  defaults". A slot whose plugins all take defaults fits on one line:
  `networks: {LocalNetwork: {}, GlobalNetwork: {}}`. Give a plugin with values
  its own block.
- Values are real YAML types: `25`, `true`, `[1, 0.01, 2, 9]`. No comma strings.
- Non-finite numbers (`.inf`, `.nan`) are rejected, with the path to the value.
- Field names carry units (`end_s`, `heading_deg`). Plugin parameters are the
  names their config structs declare (`speed`, `position_stddev_m`).
- Entities and plugins are map keys (labels), so a path such as
  `entities.red.autonomy.Straight.speed` names one value. Labels must be
  non-empty and contain no dots. Errors name the label: `entity red: ...`.
- **Key order matters.** Entities spawn and get IDs in file order, and
  plugins in a slot run in file order. YAML itself calls maps unordered, so do
  not let a tool sort the keys.
- No anchors or interpolation. Custom tags (`!name value`) are errors.
- A mission's values are all in the file, so results do not depend on an
  environment variable.

## A mission

```yaml
format_version: 1
name: straight-no-gui

run:
  end_s: 100
  dt_s: 0.1
  seed: 2147483648
end_conditions: [time, all_dead]

interactions: {SimpleCollision: {}}
networks: {LocalNetwork: {}, GlobalNetwork: {}}
metrics: {SimpleCollisionMetrics: {}}

entities:
  blue:
    team: 1
    color: [77, 77, 255]
    visual_model: zephyr-blue
    position_m: [-1000, 0, 200]
    position_variance_m2: [20, 20, 10]
    heading_deg: 0
    spawn:
      rate_hz: 0.5
      batch_size: 2
    autonomy:
      Straight:
        speed: 25
    controller: {SimpleAircraftControllerPID: {}}
    motion_model: {SimpleAircraft: {}}
```

### Top level

| Key | Default | Meaning |
| --- | --- | --- |
| `format_version` | required, `1` | the file format |
| `name` | required | a name for people; runs are named by the file |
| `run.start_s`, `run.end_s`, `run.dt_s` | 0, 100, 0.1 | time span and step |
| `run.seed` | 2147483648 | keys every random number |
| `run.motion_multiplier` | 1 | controller and motion substeps per step |
| `run.viewer` | false | open the viewer by default |
| `end_conditions` | `[time]` | any of `time`, `all_dead`, `one_team`, or `none` |
| `spatial_lookup` | `{method: scan}` | how range queries are answered |
| `terrain` | none | a terrain header ([Terrain](terrain.md)) |
| `interactions`, `networks`, `metrics` | empty | world plugins |
| `entities` | required | entity groups |

`spatial_lookup: {method: grid, cell_width_m: 5}` answers "who is within range?"
with a grid instead of testing every agent. Results are identical; only speed
changes. See [Finding nearby
agents](../reference/plugin-api.md#finding-nearby-agents).

Worker count and the viewer are command-line choices (`--workers`,
`--headless`); runs are not paced to wall-clock time.

### Entity

| Key | Default | Meaning |
| --- | --- | --- |
| `team` | -1 | team number |
| `color` | `[255, 255, 255]` | display color |
| `visual_model` | `aircraft` | display model |
| `health` | 1 | starting health |
| `id` | assigned | a fixed entity ID |
| `count` | 1 | entities in total |
| `position_m` | `[0, 0, 0]` | start position |
| `position_variance_m2` | `[0, 0, 0]` | spread of start positions (variances, m²) |
| `heading_deg`, `roll_deg`, `pitch_deg` | 0 | start attitude |
| `heading_variance_deg2` | 0 | spread of start headings |
| `velocity_mps` | `[0, 0, 0]` | start velocity |
| `randomize_every_spawn` | false | draw every start position from the spread; otherwise only when `position_m` is already taken |
| `spawn.rate_hz` | required in `spawn` | batches per second |
| `spawn.batch_size` | required in `spawn` | entities per batch |
| `spawn.start_s` | 0 | first batch time |
| `spawn.time_stddev_s` | 0 | spread of batch times |
| `autonomy`, `controller`, `sensors` | empty | entity plugins |
| `motion_model` | exactly one | the vehicle model |
| `localization`, `tracking` | `PassThrough` | estimators ([Belief vs truth](../concepts/belief-vs-truth.md)) |

Positions and velocities use the local ENU world frame.

Without `spawn`, all `count` entities spawn at the start. With it,
`batch_size` spawn every `1 / rate_hz` seconds until `count` have spawned.

### Plugins

Each key in a plugin slot is a label and, by default, the plugin name. Its map
holds the plugin's parameters, plus two framework keys: `loop_rate` (Hz; 0 runs
every step) and `plugin`. When one entity needs two of the same plugin, give
each a label and name the plugin; a sensor's label also keys its random numbers
([Randomness and seeds](../concepts/randomness.md)):

```yaml
    sensors:
      gps_coarse:
        plugin: NoisyState
        position_stddev_m: [5, 5, 10]
      gps_fine:
        plugin: NoisyState
        position_stddev_m: [0.5, 0.5, 1]
```

With two state sensors, localization can't guess which one feeds belief, so
the mission must name it. `inputs` maps each estimator input to a sensor label
(or `perfect`):

```yaml
    localization:
      PassThrough:
        inputs: {state: gps_fine}
```

An input left out takes the entity's one sensor of that kind, or the perfect
measurement when it has none. See [Belief vs
truth](../concepts/belief-vs-truth.md).

Waypoints are a list of points: `waypoints: [[10, 0, 0], [10, 10, 0]]`.

## Command-line overrides

Trailing `path:=value` arguments change a value for one run. The path is
dotted, as in a sweep, and the value is YAML:

```sh
ripple run missions/straight-no-gui.yaml entities.red.heading_deg:=170 run.seed:=7
```

Every part of the path must already exist (after template expansion), so a
typo is an error rather than a new key. To override a plugin parameter, write
it in the file or template first; a bare `Straight:` has no `speed` to replace.
Quote a value containing brackets or spaces for the shell:
`'entities.red.color:=[255, 0, 0]'`. A path can also name a whole plugin slot
the group has, replacing its plugins: `'entities.red.autonomy:={Straight: {}}'`.

## Sweeps

A sweep is a separate file ending in `.sweep.yaml`. It names a base mission:

```yaml
name: waypoints-point-agents
base_scenario: waypoints-point-agents.yaml   # relative to the sweep file
seeds: [1, 2]                                # sets run.seed; optional
parameter_combinations: cartesian            # or zip: lists must be the same length
parameters:
  entities.blue.autonomy.WaypointFollower.speed: [3, 5]
  interactions.WaypointBroadcast.update_at_s: [3, 100]
```

This is 2 seeds × 2 speeds × 2 update times = 8 cases, numbered so that seeds
change fastest, then the last path. Each case sets its paths
exactly as a command-line override does (after template expansion), so a path
must already exist in the base mission. Case 0 is loaded before any output is
created, so a misspelled path fails immediately.

```sh
ripple sweep missions/waypoints-point-agents.sweep.yaml                        # all cases
ripple sweep missions/waypoints-point-agents.sweep.yaml --shard-index 1 --shard-count 3
python3 scripts/bulk_run.py local  missions/waypoints-point-agents.sweep.yaml --jobs 4
python3 scripts/bulk_run.py submit missions/waypoints-point-agents.sweep.yaml --jobs 4 \
    -- --account=<account> --partition=<partition>
python3 scripts/bulk_run.py collect sweeps/waypoints-point-agents/run000         # after Slurm
```

Output goes to `sweeps/<sweep name>/runNNN` under the repository root; running
the same sweep again makes the next `runNNN`. Each case is one line of
`results.jsonl`: case ID, seed, parameter values, error, steps, termination,
and each metrics plugin's per-team values. A shard runs cases `i`, `i + n`,
`i + 2n`, ...; `bulk_run.py` puts shards in `shard-NNNN/` folders and `collect`
merges them in case order after checking every case appears exactly once.

Cases keep no frames or recordings. To look at one, rerun it with its values
as overrides; runs are deterministic, so it is the same run:

```sh
ripple run missions/waypoints-point-agents.yaml run.seed:=2 \
    entities.blue.autonomy.WaypointFollower.speed:=5
```

Only what sharding needs is implemented. Per-case output folders,
`--case-id` replay, digests, input hashing, summaries, and Slurm job watching
are not. Cluster setup: `conda env create -f environment.yaml`, activate it,
and `cargo build --release -p ripple` once on shared storage.

## Templates

A template is a named set of entity fields. A group names one with
`template:`, and its own keys replace the template's. The replacement is
whole: a group's `controller` replaces the template's entire `controller`
slot, and a group's `spawn` replaces the template's whole `spawn`, so a group
can replace an inherited motion model.

```yaml
templates:
  hovering_quad:
    visual_model: sphere
    autonomy:
      Straight:
        speed: 0
    controller:
      MotorSpeeds:
        speeds: [821.58, 821.58, 821.58, 821.58]
    motion_model: {Multirotor: {}}

entities:
  level_hover:
    template: hovering_quad
    team: 1
    position_m: [-10, -10, 30]
  unequal_motors:
    template: hovering_quad
    team: 3
    position_m: [-10, 10, 30]
    controller:              # replaces the template's controller slot
      MotorSpeeds:
        speeds: [820, 821, 823, 822]
```

Loading happens in this order: read the file, expand templates, apply
command-line overrides or sweep values, then validate. So an override
can change a field a group inherited:

```sh
ripple run missions/multirotor.yaml 'entities.level_hover.controller.MotorSpeeds.speeds:=[800, 800, 800, 800]'
```

Overrides cannot address `templates` itself; change the groups instead. One
template per group, and a template cannot use another template.
[multirotor.yaml](https://github.com/EthanMBoos/ripple/blob/main/missions/multirotor.yaml)
is the full example.
