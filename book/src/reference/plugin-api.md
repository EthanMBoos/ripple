# Plugin API

The traits, contexts, ports, and messages a plugin uses. [Writing
plugins](../guides/writing-plugins.md) shows how to write and register one;
[How a run works](../concepts/how-a-run-works.md) gives the phase order. Import
everything from `ripple_core::plugin::*`. Each category's trait and context
live in `crates/core/src/plugin/<category>.rs`.

## Traits

Every plugin implements one trait, its category's. Its first half says how
the plugin is built and is read from the concrete type while the mission
loads; its second half is what the plugin does each tick, which is all the
engine keeps (`Box<dyn Autonomy>`). The first half is marked `where Self:
Sized` for that reason; `crates/core/src/plugin.rs` explains.

```rust,ignore
pub trait Autonomy: Send {
    // How it is built; each is `where Self: Sized`.
    type Config: DeserializeOwned;
    fn configure(params: &PluginParams) -> Result<Self::Config> { params.parse() }
    fn new(config: &Self::Config) -> Self;
    fn ports(config: &Self::Config) -> Ports { Ports::default() }

    // What it does.
    fn initialize(&mut self, context: &mut AgentContext) -> Result<()> { Ok(()) }
    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()>;
    fn close(&mut self, time: StepTime) -> Result<()> { Ok(()) }
}
```

The other traits have the same shape with their own context. Their build
halves differ in one method: `Controller` and `MotionModel` also have
`ports`; `Sensor` has `reading_kind` instead, the kind of reading it
publishes; `Localization` and `Tracking` have `inputs`, the readings they
read; `Interaction`, `Network`, and `Metrics` have only `Config`,
`configure`, and `new`.

`Config` must deserialize from the mission's parameters. A config that is
built from them rather than read as is, such as an airframe table filled in
from a name, derives `Deserialize` with `#[serde(try_from = "MyParams")]`
and converts in `TryFrom`; the built-in aircraft and multirotor models show
it.

Every category trait has an `initialize`, a required `step`, and an
optional `close`. `initialize` is optional everywhere except `Localization`.
The own-state belief starts at the origin, at rest, and decisions read
belief, never truth, so if nothing sets it the first decision is made for a
vehicle at the origin. Localization must set it, from the startup readings or
a known prior. Tracking's empty contact list is a true belief, "no one known
yet", so it needs no setting.

| Trait | `initialize` | `step` | Also |
| --- | --- | --- | --- |
| `Autonomy` | `AgentContext` | `AgentContext`, `PluginIo` | |
| `Controller` | `AgentContext` | `AgentContext`, `PluginIo` | |
| `MotionModel` | `MotionContext` | `MotionContext`, `PluginIo` | |
| `Sensor` | `SensorContext` | `SensorContext` | |
| `Localization` | `LocalizationContext` (required) | `LocalizationContext` | |
| `Tracking` | `TrackingContext` | `TrackingContext` | |
| `Interaction` | `WorldContext` | `InteractionContext` | `collision_exists` (startup placement) |
| `Network` | `WorldContext` | `NetworkContext` | |
| `Metrics` | `WorldContext` | `WorldContext` | `report` |

`step` returns `Ok(())` or an error, which stops the run and names the
plugin. An interaction's or a metrics plugin's `step` returns
`Ok(Update::Applied)`, or `Ok(Update::Stop)` to end the run after this tick.

`close` runs exactly once: when the entity is removed, when the run ends, or
when it fails. It must tolerate a plugin whose `initialize` never finished,
because a failed initialization closes every plugin already built for that
entity.

## Contexts

Every context has `time: StepTime` (`time_s`, `dt_s`, and `tick`). Entity
contexts also have `entity: EntityInfo` (`id`, `team_id`, `group_index`) and
`messages`. Contexts are borrowed for one call; never store them.

| Context | Notable fields |
| --- | --- |
| `AgentContext` | `state`: own-state belief; `contacts`: known contacts, without itself; `random` |
| `MotionContext` | `truth`, mutable; `terrain`; `applied_forces_world_n` from interactions |
| `SensorContext` | `truth`, read-only; `contacts_truth`; `random`; `publish_reading` |
| `LocalizationContext` | `inputs`; `commands`; `state`, mutable belief; `random` |
| `TrackingContext` | `inputs`; `pose` from localization; `contacts`, mutable; `random` |
| `InteractionContext` | `entities`, mutable; `nearby`, the spatial index |
| `NetworkContext` | `route` or `route_within_entity`; the network's own `messages()` |
| `WorldContext` | `contacts_truth`; `entity_teams`; `terrain` |

Decision plugins (autonomy and controllers) and estimators have no handle on
truth. Motion writes truth; sensors, interactions, networks, and metrics read
it.

## Ports and state

`ports` on `Autonomy`, `Controller`, and `MotionModel` declares named
inputs and outputs, each with a `Unit` and a `Frame`. Autonomy outputs feed
controller inputs, and controller outputs feed motion inputs; the engine
checks every connection (name, unit, frame) before the run starts and hands
each step a `PluginIo` for `read` and `write`. Names are owned strings, so
they can come from configuration (`format!("motor_{index}")`). `Port` is
`Clone` but not `Copy`, so build declarations inside `ports`.

`KinematicState` holds `position_world_m`, `velocity_world_mps`, and
`angular_velocity_world_radps` in the local East-North-Up world frame, and
`orientation_world_from_body`, a quaternion that rotates forward-left-up body
axes into world axes. Motion models keep their own body-frame rates privately
and convert when they read or write truth; `rotate_world_to_body` gives body
rates to a sensor or controller that needs them. `Pid`, `PidGains`,
`EulerAngles`, `Quaternion`, `Vec3`, and `rk4` are exported from `ripple_core`.

## Estimators

Each entity has one localization estimator, which writes its own-state
belief, and one tracking estimator, which writes its known contacts. Both
default to `PassThrough`. They run every tick after delivery, localization
first; autonomy and controllers see the result on their next step.

```rust,ignore
impl Localization for DeadReckoning {
    // ...Config, new...
    // The readings this estimator reads; the mission's `inputs` picks each sensor.
    fn inputs(_: &Config) -> Vec<Input> {
        vec![Input::new("fix", ReadingKind::Other)]
    }

    fn initialize(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // Set the first belief from startup readings or a known prior.
        Ok(())
    }
    fn step(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // Predict from what the motion model read on each substep...
        for substep in 0..context.commands.substeps() {
            let speed_mps = context.commands.get(substep, "speed")?;
            let dt_s = context.commands.time(substep).dt_s;
            context.state.position_world_m.x += self.assumed_gain * speed_mps * dt_s;
        }
        // ...then correct with a reading that arrived this tick.
        if let Some(fix) = context.inputs.fresh::<PositionFix>("fix")? {
            context.state.position_world_m = fix.value.position_world_m;
        }
        Ok(())
    }
}
```

- **Readings.** A sensor hands each sample to its entity's estimators with
  `context.publish_reading(value)`, keyed by the sensor's name in the mission.
  The sensor's `reading_kind` says what it holds: `State` (a `KinematicState`),
  `Contacts` (a `ContactList`), or `Other` (any type, checked when read).
  `inputs.latest` returns the newest reading; `inputs.fresh` only one that
  arrived since the last estimation step.
- **Choosing sources.** A mission's `inputs` names the sensor for each input,
  or `perfect` for the engine's perfect measurement. An input left out takes
  the one sensor of its kind, or the perfect measurement if there is none;
  several candidates, an unknown name, or a kind mismatch is a load error.
  `Other` inputs must be named.
- **Commands.** `LocalizationContext::commands` holds what the motion model
  actually read on each substep of the tick just finished: the last
  controller's outputs, or the autonomy's if there is no controller.
- **Tracking** gets localization's estimate as `context.pose` and writes
  `context.contacts`, a `ContactList` (`Arc<[Contact]>`). Each `Contact` has
  an identity, an estimated state, and `sampled_at_s`.
- **Startup.** Estimators initialize from one perfect measurement of the
  whole starting population, before any controller or autonomy, so decisions
  see belief from their first call. An entity that spawns later starts from
  the latest perfect measurement.
- Estimators reject `loop_rate_hz`: a skipped tick would lose a reading or a
  command. Do expensive work less often inside `step` instead.

In YAML the estimators are entity slots, like `motion_model`:

```yaml
sensors:
  gps: {plugin: NoisyState, position_stddev_m: [5, 5, 1]}
localization:
  PassThrough:
    inputs: {state: gps}
tracking:
  PassThrough:          # inputs left out: the one contact sensor, or perfect
```

From Rust, set `EntityGroupConfig::localization` to
`PluginConfig::new("DeadReckoning").input("fix", "gps")`.

## Finding nearby agents

Ask the engine which agents are within a distance, rather than looping over
every agent:

```rust,ignore
// Sensors, networks, and metrics: true contacts, in ID order.
for contact in context.contacts_truth.within(position_world_m, range_m) { /* ... */ }

// Interactions: indices into `context.entities`, ascending.
for index in context.nearby.within(position_world_m, range_m) { /* ... */ }

// Interactions: each close pair (first < second). SimpleCollision uses this.
for (first, second) in context.nearby.pairs_within(range_m) { /* ... */ }
```

The answer is every agent closer than `range_m`, including the asking agent
and agents already disabled, so apply your own rules. Sensors see positions
after motion; an interaction sees them as they are when it starts, including
moves by earlier interactions; networks and metrics see them after all
interactions. Autonomy and controllers can't query truth: `context.contacts`
is their own belief, short enough to scan.

How the engine answers is a mission setting, never plugin code:

```yaml
spatial_lookup:
  method: grid       # default: scan
  cell_width_m: 5
```

`scan` tests every agent. `grid` tests only the agents in cells near the
query and returns exactly the scan's answer in the scan's order, so output is
byte-identical either way. The grid costs a little to build each tick: in
the paper's benchmark it made a step up to 9% slower at 256 agents, and about
twice as fast as the scan at 4,096 spread-out agents. Start with a cell width
at or a little above your most common query range. The grid works on vertical
columns, so it suits agents spread out horizontally.

## Messages

```rust,ignore
// initialize:
context.messages.subscribe::<Population>("ExampleNetwork", "population")?;

// publisher step:
context.messages.publish("ExampleNetwork", "population", Population { entity_count: 8 })?;

// subscriber step:
for message in context.messages.receive::<Population>("ExampleNetwork", "population")? {
    self.entity_samples += message.value.entity_count;
}
```

- Payload types derive `serde::Serialize`, so `--trace` can record them.
- `receive` drains this subscriber's own queue and nobody else's. Each
  message has `sender`, `sent_at_s`, `delivered_at_s`, and `value`.
- A wrong payload type, or a network the mission doesn't list, is an error.
  GlobalNetwork is always present.
- Drain every subscription you create: a full queue stops the run (capacities
  are in [How a run works](../concepts/how-a-run-works.md#messages)).

A network implements `step` and calls `context.route` once with a decision
function, `|link, random|`. The `link` is a `Transmission` (sender, receiver,
topic, sequence, time, and `contacts_truth`); the function returns
`Delivery::Drop` or `Delivery::After { delay_s }`. Zero delay delivers in the
current network phase; a positive delay delivers at the first tick at or after
that time, even if the publisher has gone quiet. A network that only delivers
within the sender's entity calls `route_within_entity` instead, which sees
only those links and keeps routing cost linear in the number of agents.

A sensor's `publish_reading` is not a message: it goes only to its own
entity's estimators, and no network sees it. Use a message when
communication itself is part of the experiment.

## Metrics

`Metrics::report` returns a `MetricReport`: ordered column names and, for each
team, a score and values. Each metrics plugin reports its own score; scores
are not added across plugins. A column with no value for a team is left out
of the run record and the sweep results, not written as zero.
`Simulation::metric_reports()` returns every report.
