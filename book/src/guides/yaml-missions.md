# YAML missions and templates

Missions are YAML files. Each loads into the typed `ScenarioConfig`, the same
data a [Rust-built scenario](rust-scenarios.md) fills in. To run one mission
across seeds and settings, see [Sweeps and results](sweeps.md).

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
- Values are real YAML types: `25`, `true`, `[0, 0, 200]`. No comma strings.
- Non-finite numbers (`.inf`, `.nan`) are rejected, with the path to the value.
- Names carry units (`end_s`, `heading_deg`, `speed_mps`). Plugin parameters
  are the names their config structs declare, with one name each (see [Rust
  style](../development/rust-style.md#parameter-names)).
- Entities and plugins are map keys (labels), so a path such as
  `entities.red.autonomy.Straight.speed_mps` names one value. Labels must be
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
end_conditions: [time, no_entities]

interactions: {SimpleCollision: {}}
networks: {LocalNetwork: {}, GlobalNetwork: {}}
metrics: {SimpleCollisionMetrics: {}}

entities:
  blue:
    team: 1
    visual_model: zephyr-blue
    position_m: [-1000, 0, 200]
    heading_deg: 0
    spawn:
      rate_hz: 0.5
      batch_size: 2
    autonomy:
      Straight:
        speed_mps: 25
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
| `end_conditions` | `[time]` | any of `time`, `no_entities`, `one_team_left`, or `none` |
| `spatial_lookup` | `{method: scan}` | how range queries are answered |
| `terrain` | none | a terrain header ([Terrain](terrain.md)) |
| `interactions`, `networks`, `metrics` | empty | world plugins |
| `entities` | required | entity groups |
| `recipes` | empty | entity groups built only when a plugin asks ([Recipes](#recipes)) |

`spatial_lookup: {method: grid, cell_width_m: 5}` answers "who is within range?"
with a grid instead of testing every agent. Results are identical; only speed
changes. See [Finding nearby
agents](../reference/plugin-api.md#finding-nearby-agents).

Worker count and the viewer are command-line choices (`--workers`,
`--headless`); runs are not paced to wall-clock time.

### Entity

| Key | Default | Meaning |
| --- | --- | --- |
| `team` | -1 | team number; the viewer colors entities by team |
| `visual_model` | `aircraft` | display model |
| `health` | 1 | starting health |
| `id` | assigned | a fixed entity ID |
| `count` | 1 | entities in total |
| `position_m` | `[0, 0, 0]` | start position |
| `position_stddev_m` | `[0, 0, 0]` | spread of start positions (standard deviations, m); an entity with a spread is always sampled around `position_m`, one without is placed there exactly unless that spot is already taken |
| `heading_deg`, `roll_deg`, `pitch_deg` | 0 | start attitude |
| `heading_stddev_deg` | 0 | spread of start headings (standard deviation) |
| `velocity_mps` | `[0, 0, 0]` | start velocity |
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
holds the plugin's parameters, plus two framework keys: `loop_rate_hz` (0 runs
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

Waypoints are a list of points in metres: `waypoints_m: [[10, 0, 0], [10, 10, 0]]`.

## Command-line overrides

Trailing `path:=value` arguments change a value for one run. The path is
dotted, as in a sweep, and the value is YAML:

```sh
ripple run missions/straight-no-gui.yaml entities.red.heading_deg:=170 run.seed:=7
```

Every part of the path must already exist (after template expansion), so a
typo is an error rather than a new key. To override a plugin parameter, write
it in the file or template first; a bare `Straight:` has no `speed_mps` to replace.
Quote a value containing brackets or spaces for the shell:
`'entities.red.position_m:=[0, 0, 200]'`. A path can also name a whole
plugin slot
the group has, replacing its plugins: `'entities.red.autonomy:={Straight: {}}'`.

## Templates

A template is a named set of entity fields. A group names one with
`template:`, and its own keys replace the template's. The replacement is
whole, with one exception. When the template and the group both give a slot
exactly one plugin and it is the same plugin, the group's values lay over the
template's values for that plugin and the rest stay. A group that names a
different plugin, or several, replaces the template's whole slot, so a group
can still swap a motion model or a controller.

```yaml
templates:
  hovering_quad:
    visual_model: sphere
    autonomy:
      Straight:
        speed_mps: 0
    controller:
      MotorSpeeds:
        speeds_radps: [821.58, 821.58, 821.58, 821.58]
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
    controller:              # the same plugin: these speeds replace the
      MotorSpeeds:           # template's, and any other MotorSpeeds value
        speeds_radps: [820, 821, 823, 822]   # it had would stay
  velocity_held:
    template: hovering_quad
    team: 4
    position_m: [10, 10, 30]
    controller: {MultirotorVelocityController: {}}  # a different plugin:
                                                     # replaces the slot
```

## Recipes

The two sections differ in who decides when an entity appears. Under
`entities`, the mission decides: a group's `count` appear at the start, or
on its `spawn` timetable when the mission knows the schedule up front, as
in a stream of aircraft entering at a fixed rate. Under `recipes`, a plugin
decides during the run, because the number and the places depend on
something only the run knows, such as how many hubs a region has or where
a vehicle was lost. A recipe is written like an entity group but has no
`count` or `spawn`; the mission rejects them. The plugin asks by publishing
a `SpawnRequest` that names the recipe and says where, how many, and which
plugin values differ ([Plugin API](../reference/plugin-api.md#spawn-requests)).
Templates apply to recipes as they do to entities, and the entities built
from a recipe belong to it, so the record and the viewer label them by its
name.

Use `entities` whenever you can write the schedule into the file; it is
the simpler of the two and needs no plugin. Reach for a recipe when writing
the groups out would mean generating the mission.

```yaml
recipes:
  fixed_wing:
    team: 1
    position_stddev_m: [60, 60, 0]
    autonomy: {Courier: {class: fixed_wing}}
    controller: {SimpleAircraftControllerPID: {}}
    motion_model: {SimpleAircraft: {}}

entities:
  region:
    autonomy:
      Dispatcher:
        region: {hubs: 16, hub_spacing_m: 15000}
        fleet:
          - {recipe: fixed_wing, per_hub: 3, altitude_m: 120}
```

Here the Dispatcher asks for three `fixed_wing` couriers at every hub and
gives each its hub number, so the mission is the same length at 16 hubs and
at 1,024. A template saves lines inside a group: the group inherits the
template's fields and writes only its own. A recipe saves groups: the
mission writes one, and a plugin decides at run time how many to build and
where.

Why a message and a section of their own (decided 2026-11-07, when the
1,024-hub delivery mission had reached 278 KB of generated courier groups):
a grid or list construct in the mission format is machinery for one study;
a `spawn()` method on every plugin context is a new slot in every context
for one caller; a spawner interaction plugin would carry a copy of the
layout its dispatcher already owns. A message needs nothing the plugins do
not have. The recipes are their own section, not entities with `count: 0`,
so that a reader sees from the file which groups exist at the start and
which a plugin builds.

Why this rule and not another (decided 2026-11-07, when the 64-hub delivery
mission had reached 2,367 lines because every group repeated its template's
Courier values): replacing whole slots is simple but makes a group that
changes one value repeat them all; merging every nested map would make it
impossible to drop a template's value or swap a plugin without naming every
key; a grid or list construct that spawns groups at many positions is
machinery this project leaves out. Merging only when both sides name the
same single plugin keeps a swap a swap and a tweak a tweak.

Loading happens in this order: read the file, expand templates, apply
command-line overrides or sweep values, then validate. So an override
can change a field a group inherited:

```sh
ripple run missions/multirotor.yaml 'entities.level_hover.controller.MotorSpeeds.speeds_radps:=[800, 800, 800, 800]'
```

Overrides cannot address `templates` itself; change the groups instead. One
template per group, and a template cannot use another template.
[multirotor.yaml](https://github.com/EthanMBoos/ripple/blob/main/missions/multirotor.yaml)
is the full example.
