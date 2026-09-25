# YAML missions

YAML is the native mission format; XML stays supported for single runs. Both
load into the same typed `ScenarioConfig`, so the simulation cannot tell which
file it came from. See [TODO.md](TODO.md) section 2 for the plan.

| | Status |
| --- | --- |
| Single missions, plugin parameters, command-line overrides | Implemented |
| `scrimmage compare a.xml b.yaml` | Implemented |
| Templates | Implemented |
| Sweeps (`*.sweep.yaml`) and the Slurm launcher | Planned |

Paired examples, each checked against its XML by `crates/core/tests/yaml_missions.rs`:
[straight-no-gui](../missions/straight-no-gui.yaml),
[multirotor](../missions/multirotor.yaml),
[waypoints-point-agents](../missions/waypoints-point-agents.yaml), and
[verification/noisy-state-bias](../missions/verification/noisy-state-bias.yaml).

## Rules

- Unknown keys are errors everywhere, including plugin parameters.
- Block style: one key per line. A plugin key with nothing after it
  (`SimpleAircraft:`) means "this plugin, all defaults".
- Values are real YAML types: `25`, `true`, `[1, 0.01, 2, 9]`. No comma strings.
- Non-finite numbers (`.inf`, `.nan`) are rejected, with the path to the value.
- Field names carry units (`end_s`, `heading_deg`). Plugin parameters keep the
  C++ names their config structs declare (`speed`, `pos_noise_0`).
- Entities and plugins are map keys (labels), so a path such as
  `entities.red.autonomy.Straight.speed` names one value. Labels must be
  non-empty and contain no dots. Errors name the label: `entity red: ...`.
- **Key order matters.** Entities spawn, draw random numbers, and get IDs in
  file order, and plugins in a slot run in file order, as XML tag order does.
  YAML itself calls maps unordered, so do not let a tool sort the keys.
- Only fields Rust uses are accepted. XML's GUI and C++ logging tags
  (`stream_port`, `grid_size`, `terrain`, `log_dir`, and so on) have no YAML form.
- No anchors or interpolation. Custom tags (`!name value`) are errors.

## A mission

```yaml
format_version: 1
name: straight-no-gui

run:
  end_s: 100
  dt_s: 0.1
  seed: 2147483648
end_conditions: [time, all_dead]

interactions:
  SimpleCollision:
networks:
  LocalNetwork:
  GlobalNetwork:
metrics:
  SimpleCollisionMetrics:

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
    controller:
      SimpleAircraftControllerPID:
    motion_model:
      SimpleAircraft:
```

### Top level

| Key | Default | XML equivalent |
| --- | --- | --- |
| `format_version` | required, `1` | none |
| `name` | required | `<runscript name>` |
| `run.start_s`, `run.end_s`, `run.dt_s` | 0, 100, 0.1 | `<run start end dt>` |
| `run.seed` | 2147483648 | `<seed>` |
| `run.motion_multiplier` | 1 | `<run motion_multiplier>` |
| `run.viewer` | false | `<run enable_gui>` |
| `end_conditions` | `[time]` | `<end_condition>time, all_dead</end_condition>` |
| `interactions`, `networks`, `metrics` | empty | `<entity_interaction>`, `<network>`, `<metrics>` |
| `entities` | required | `<entity>` blocks |

Worker count, pacing, and the viewer are command-line choices (`--workers`,
`--headless`); XML's `multi_threaded`, `time_warp`, and `start_paused` have no
YAML form.

### Entity

| Key | Default | XML equivalent |
| --- | --- | --- |
| `team` | -1 | `team_id` |
| `color` | `[255, 255, 255]` | `color` |
| `visual_model` | `sphere` | `visual_model` |
| `health` | 1 | `health` |
| `id` | assigned | `id` |
| `count` | 1 | `count`: entities in total |
| `position_m` | `[0, 0, 0]` | `x`, `y`, `z` |
| `position_variance_m2` | `[100, 100, 0]` | `variance_x`, `variance_y`, `variance_z` (variances, m²) |
| `heading_deg`, `roll_deg`, `pitch_deg` | 0 | `heading`, `roll`, `pitch` |
| `heading_variance_deg2` | 0 | `variance_heading` |
| `velocity_mps` | `[0, 0, 0]` | `vx`, `vy`, `vz` |
| `speed_mps` | 0 | `speed` (legacy: applied along world x when velocity is zero) |
| `randomize_every_spawn` | false | `use_variance_all_ents` |
| `spawn.rate_hz` | required in `spawn` | `generate_rate` (`"1 / 2"` becomes `0.5`) |
| `spawn.batch_size` | required in `spawn` | `generate_count` |
| `spawn.start_s` | 0 | `generate_start_time` |
| `spawn.time_stddev_s` | 0 | `generate_time_variance` (C++ uses it as a standard deviation) |
| `autonomy`, `controller`, `sensors` | empty | `<autonomy>`, `<controller>`, `<sensor>` |
| `motion_model` | exactly one | `<motion_model>` |

Without `spawn`, all `count` entities spawn at the start. With it,
`batch_size` spawn every `1 / rate_hz` seconds until `count` have spawned.

### Plugins

Each key in a plugin slot is a label and, by default, the plugin name. Its map
holds the plugin's parameters, plus two framework keys: `loop_rate` (Hz; 0 runs
every step) and `plugin`. When one entity needs two of the same plugin, give
each a label and name the plugin; the label also names a sensor's random stream:

```yaml
    sensors:
      gps_coarse:
        plugin: NoisyState
        pos_noise_0: [0, 5]      # [mean, stddev]
      gps_fine:
        plugin: NoisyState
        pos_noise_0: [0, 0.5]
```

Waypoints are a list of points: `waypoints: [[10, 0, 0], [10, 10, 0]]`. The XML
string form `"10,0,0;10,10,0"` is also accepted.

## Command-line overrides

XML fills `${name=default}` placeholders from trailing `name:=value` arguments.
For YAML, the same arguments take a dotted path, as a sweep will:

```sh
scrimmage run missions/straight-no-gui.yaml entities.red.heading_deg:=170 run.seed:=7
```

Every part of the path must already exist (after template expansion), so a
typo is an error rather than a new key. To override a plugin parameter, write
it in the file or template first; a bare `Straight:` has no `speed` to replace.
Quote a value containing brackets or spaces for the shell:
`'entities.red.color:=[255, 0, 0]'`.

## Checking a YAML mission against its XML

```sh
scrimmage compare missions/straight-no-gui.xml missions/straight-no-gui.yaml
```

It first lists any setup differences by path (`entities.1.heading_deg: 180.0 vs
170.0`), matching entities and plugins by position and ignoring labels, the file
path, and pacing, viewer, and worker settings. Then it runs both, one step at a
time, and stops at the first frame that differs; events and the summary must
match too. It takes `--max-steps` like `run`.

Plugin parameter values are checked only through those outputs, since XML holds
them as text. A value that no recorded output reflects (such as a sensor's noise
when nothing uses its observations) can differ without `compare` noticing.

## Sweeps (planned)

A sweep is a separate file ending in `.sweep.yaml`, using
[Ripple's](../../ripple) syntax. It names a base mission, which must be YAML:

```yaml
name: straight_speed
base_scenario: straight.yaml
seeds: [1, 2, 3]                     # replaces run.seed; optional
parameter_combinations: cartesian    # or zip: lists must be the same length
parameters:
  entities.red.autonomy.Straight.speed: [15, 21, 30]
  entities.red.spawn.batch_size: [2, 4]
```

This is 3 seeds × 3 speeds × 2 batch sizes = 18 cases. Each case sets the paths
exactly as a command-line override does, then loads the result like any mission.

```sh
scrimmage sweep missions/straight.sweep.yaml
python3 scripts/bulk_run.py local  missions/straight.sweep.yaml --jobs 4
python3 scripts/bulk_run.py submit missions/straight.sweep.yaml --jobs 18 \
    -- --account=<account> --partition=<partition>
```

Each case gets a stable ID, its own run directory, and one result row, as in
Ripple. Sweep values are applied after template expansion, like overrides.

## Templates

A template is a named set of entity fields. A group names one with
`template:`, and its own keys replace the template's. The replacement is
whole: a group's `controller` replaces the template's entire `controller`
slot, and a group's `spawn` replaces the template's whole `spawn`. Templates
replace XML's `entity_common` and `param_common`, and unlike XML, a group can
replace an inherited motion model.

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
    motion_model:
      Multirotor:

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
command-line overrides (and, later, sweep values), then validate. So an override
can change a field a group inherited:

```sh
scrimmage run missions/multirotor.yaml 'entities.level_hover.controller.MotorSpeeds.speeds:=[800, 800, 800, 800]'
```

Overrides cannot address `templates` itself; change the groups instead. One
template per group, and a template cannot use another template.
[multirotor.yaml](../missions/multirotor.yaml) is the full example.
