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
halves differ in one member: `Controller` and `MotionModel` also have
`ports`; `Sensor` has a `Reading` type instead, the type it publishes;
`Localization` and `Tracking` have `inputs`, the readings they read;
`Interaction`, `Network`, and `Metrics` have only `Config`, `configure`,
and `new`.

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
a known prior. Tracking's empty track list is a true belief, "no one known
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
| `AgentContext` | `state`: own-state belief, a `GaussianState`; `tracks`: known tracks, without itself; `random` |
| `MotionContext` | `truth`, mutable; `terrain`; `applied_forces_world_n` from interactions |
| `SensorContext` | `truth`, read-only; `contacts_truth`; `random`; `publish_reading` |
| `LocalizationContext` | `inputs`; `commands`; `state`, mutable belief; `random` |
| `TrackingContext` | `inputs`; `pose` from localization; `tracks`, mutable; `random` |
| `InteractionContext` | `entities`, mutable; `nearby`, the spatial index; `terrain` |
| `NetworkContext` | `route` or `route_within_entity`; the network's own `messages()` |
| `WorldContext` | `contacts_truth`; `entity_teams`; `terrain` |

Decision plugins (autonomy and controllers) and estimators have no handle on
truth. Motion writes truth; sensors, interactions, networks, and metrics read
it.

## Ports and state

`ports` on `Autonomy`, `Controller`, and `MotionModel` declares named
inputs and outputs, each with a `Unit` and a `Frame`. Autonomy outputs feed
controller inputs, and controller outputs feed motion inputs; with no
controller, motion reads the autonomies directly ([What a mission can leave
out](../concepts/how-a-run-works.md#what-a-mission-can-leave-out)). Before
the run starts the engine checks every connection (name, unit, frame) and
that each port name has one writing plugin per entity. An entity keeps one
map of port values, and each step hands the plugin a `PluginIo`, its view of
that map limited to its declared ports, for `read` and `write`; a value
persists on the steps a rate-limited plugin skips. Names are owned strings,
so they can come from configuration (`format!("motor_{index}")`). `Port` is
`Clone` but not `Copy`, so build declarations inside `ports`.

`KinematicState` holds `position_world_m`, `velocity_world_mps`, and
`angular_velocity_world_radps` in the local East-North-Up world frame, and
`orientation_world_from_body`, a quaternion that rotates forward-left-up body
axes into world axes. Motion models keep their own body-frame rates privately
and convert when they read or write truth; `rotate_world_to_body` gives body
rates to a sensor or controller that needs them. `Pid`, `PidGains`,
`EulerAngles`, `Quaternion`, `Vec3`, and `rk4` are exported from `ripple_core`.

## Stepping a motion model yourself

A plugin that predicts, such as a model predictive controller trying
candidate commands or an estimator's prediction step, can step a built-in
motion model outside the engine. Each model's config type has an `advance`
method, the very code the motion phase runs: it takes a `KinematicState` you
supply, that model's controls, and a step in seconds, and leaves the state
one step later. All are exported from `ripple_core::plugin::motion`.

The point is to have one set of equations, not two. A predictive controller
written against its own copy of the vehicle's physics drifts from the
simulated vehicle as either is edited, and a retyped model is where sign and
unit mistakes live. Calling `advance` makes the prediction exactly as
faithful as the motion being simulated, and leaves nothing to keep in step.
Ripple ships no predictive controller; this is the piece one needs.

| Model | Config type | Controls |
| --- | --- | --- |
| SingleIntegrator | `SingleIntegratorConfig` | the commanded velocity, a `Vec3` |
| SimpleAircraft | `SimpleAircraftConfig` | `SimpleAircraftControl { throttle, roll_rate_radps, pitch_rate_radps }` |
| Aerosonde | `AerosondeConfig` | `AerosondeControl { throttle, elevator_rad, aileron_rad, rudder_rad }` |
| Multirotor | `MultirotorConfig` | one shaft speed per rotor, rad/s, and the list of shaft speeds it carries between steps |

There is no trait over the five: the controls differ per vehicle, and a
shared form would flatten them into a list of numbers. FixedWing6DOF has no
`advance` yet. When it gets one it will predict an aircraft in flight over
flat ground: the catapult and the terrain the wheels roll on belong to the
engine's step, and a controller has no terrain to pass in any case.

A config deserializes from the model's mission keys, so give your plugin's
parameter struct a field of that type. The model it predicts with is then
set in the mission, apart from the true vehicle's:

```rust,ignore
use ripple_core::plugin::motion::MultirotorConfig;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MyControllerConfig {
    /// Mission: `model: {airframe: crazyflie2, mass_kg: 0.035}`.
    model: MultirotorConfig,
}

// In `step`: start from belief, never from truth.
let mut predicted = *context.state;
let mut rotor_speeds_radps = Vec::new(); // empty: each rotor starts at its command
for _ in 0..horizon_steps {
    config
        .model
        .advance(&mut predicted, &mut rotor_speeds_radps, &commands_radps, dt_s)?;
}
```

`advance` limits the controls exactly as the engine does, and returns an
error when the state stops being finite. `MultirotorConfig::airframe()` gives
the vehicle's mass, inertia, rotor layout, and speed limits. SimpleAircraft
first brings the state within its attitude and speed limits, as it does for
an entity at the start of a run. Match `dt_s` to the motion step you want to
predict: the mission's `dt_s` over its `motion_multiplier`. The multirotor's
shaft speeds are the one piece of state outside `KinematicState`: keep the
list it hands back and pass it to the next step, or start it empty to take
each rotor at its command, which is what the engine does on its first step.

`advance` steps; it does not hand out the accelerations or the per-stage
derivatives. Those can follow when a filter or a fitting script first needs
them. `crates/core/tests/model_step.rs` checks each `advance` against a run
of the engine, bit for bit ([How models are checked](checks.md)).

## Estimators

Each entity has one localization estimator, which writes its own-state
belief, a `GaussianState` (a mean and the covariance of its error), and one
tracking estimator, which writes its tracks, a `Track` per object it
believes in. Both default to `PassThrough`. They run every tick after
delivery, localization first; autonomy and controllers see the result on
their next step. [Uncertainty](../concepts/uncertainty.md) describes the
types and their conventions.

```rust,ignore
impl Localization for PositionKalman {
    // ...Config, new...
    // The readings this estimator reads; the mission's `inputs` picks each sensor.
    fn inputs(_: &Config) -> Vec<Input> {
        vec![Input::new("fix", ReadingKind::State)]
    }

    fn initialize(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // Set the first belief, mean and covariance, from startup readings
        // or a known prior.
        Ok(())
    }
    fn step(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // Predict from what the motion model read on each substep, growing
        // the covariance by the process noise...
        for substep in 0..context.commands.substeps() {
            let speed_mps = context.commands.get(substep, "speed")?;
            let dt_s = context.commands.time(substep).dt_s;
            context.state.mean.position_world_m.x += self.assumed_gain * speed_mps * dt_s;
            // ...context.state.covariance grows here...
        }
        // ...then correct with a reading that arrived this tick, weighing
        // its covariance against the belief's.
        if let Some(fix) = context.inputs.fresh::<GaussianState>("fix")? {
            let fix_covariance_m2 = fix.value.covariance.position();
            // ...the Kalman gain moves the mean and shrinks the covariance...
        }
        Ok(())
    }
}
```

- **Readings.** A sensor hands each sample to its entity's estimators with
  `context.publish_reading(value)`, keyed by the sensor's name in the mission.
  The value is the sensor's declared `Reading` type, which also gives its
  kind: a `GaussianState` is a `State` reading, a `DetectionList` a
  `Detections` reading, and any other type (`PositionMeasurement`, say) an
  `Other` reading that estimators name explicitly and that is checked when
  read. A reading carries the covariance of the sensor's noise. A sensor
  that publishes nothing declares `type Reading = ();`. `inputs.latest`
  returns the newest reading; `inputs.fresh` only one that arrived since the
  last estimation step; `inputs.perfect_tracks` the engine's shared track
  list when the input is the perfect measurement, for a tracker that
  believes it outright.
- **Choosing sources.** A mission's `inputs` names the sensor for each input,
  or `perfect` for the engine's perfect measurement. An input left out takes
  the one sensor of its kind, or the perfect measurement if there is none;
  several candidates, an unknown name, or a kind mismatch is a load error.
  `Other` inputs must be named.
- **Commands.** `LocalizationContext::commands` holds what the motion model
  actually read on each substep of the tick just finished: the last
  controller's outputs, or the autonomy's if there is no controller.
- **Tracking** gets localization's estimate as `context.pose` and writes
  `context.tracks`, a `TrackList` (`Arc<[Track]>`). Each `Track` has the
  tracker's own `track_id`, an `identity` when a detection gave one, a
  `GaussianState`, `updated_at_s`, and an `existence_probability`. Autonomy
  reads them through `KnownTracks`, which hides the entity's own track:
  `iter()`, `get(id)` by identity, and `within(position, range)`.
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

Tracking's input is named `detections`, so with two contact sensors the
mission writes `inputs: {detections: radar}`. From Rust, set
`EntityGroupConfig::localization` to
`PluginConfig::new("PositionKalman").input("fix", "gps")`.

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
interactions. Autonomy and controllers can't query truth: `context.tracks`
is their own belief, and its `within` scans it.

How the engine answers is a mission setting, never plugin code:

```yaml
spatial_lookup:
  method: grid       # default: scan
  cell_width_m: 5
```

`scan` tests every agent. `grid` tests only the agents in cells near the
query and returns exactly the scan's answer in the scan's order, so output is
byte-identical either way. The grid costs a little to build each tick: in
the spatial benchmark it made a step up to 9% slower at 256 agents, and about
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

### Spawn requests

A plugin asks the engine to build entities by publishing a `SpawnRequest`
on `GlobalNetwork` under `SpawnRequest::TOPIC`. It names one of the
mission's [recipes](../guides/yaml-missions.md#recipes) and says where, how
many, and which plugin values differ:

```rust,ignore
let request = SpawnRequest::recipe("fixed_wing")
    .count(3)
    .at(hub_world_m)
    .autonomy("Courier", CourierHome { hub, region })?;
context.messages.publish("GlobalNetwork", SpawnRequest::TOPIC, request)?;
```

`at` replaces the recipe's `position_m` (its spread still applies),
`heading` its `heading_deg`, and `autonomy` or `overlay(slot, plugin,
values)` lays a struct or map's keys over one plugin's values, by the
plugin's label in the mission. The engine builds the entities at the start
of the next tick, in delivery order, so a request published in `initialize`
gives entities present from tick 1. An unknown recipe or plugin, or a value
the plugin rejects, stops the run with an error naming the recipe and the
requesting plugin.

A sensor's `publish_reading` is not a message: it goes only to its own
entity's estimators, and no network sees it. Use a message when
communication itself is part of the experiment.

## Drawings

Every category trait has two methods for what the plugin draws in the
viewer; both return a `Vec<Drawing>` and draw nothing unless overridden.
A `Drawing` is one shape in world meters: a `Point`, a `Circle`, an open
`Line`, a flat convex `Polygon`, or an axis-aligned `Box` with open floor
and roof. Each carries a label and a color the plugin chooses. A prominent
point is larger and always labeled, for the few places a reader must find
at a glance; a plain point is small and labeled on hover, for the many. The
shapes are display only: the engine never reads them.

- `fixed_drawings` is asked once, when the plugin starts, for shapes that
  hold for the whole run: a boundary, the hubs and sites of a region, a
  goal. The engine keeps them by reference; they cost nothing after that.
- `drawings(time_s)` is asked every tick for shapes that come and go: an
  area closed for an hour, a planned route. Return the same shapes each
  tick while nothing has changed, and nothing when there is nothing; an
  empty list costs nothing to build or compare.

```rust,ignore
fn fixed_drawings(&self) -> Vec<Drawing> {
    vec![Drawing::Point {
        position_world_m: self.goal_world_m,
        label: "goal".into(),
        color: [255, 255, 255],
        prominent: true,
    }]
}
```

The engine asks every plugin after the metrics phase, in entity order then
world-plugin order. The frame carries both parts under the plugin's name,
with the entity's ID for an entity's plugin (`Dispatcher/7`), and the
record writes them to its `/drawings` channel only when a frame's drawings
differ from the previous frame's, whether or not a viewer is open. The
viewer redraws a plugin's fixed part once and its changing part on change.
`Boundary` draws its box this way and the delivery crate's Dispatcher its
hubs and sites, with the areas closed now as its changing part.

What the viewer can derive from the record over time is not a plugin
drawing. An agent's trail is its recent truth positions, already in the
frames; the viewer rebuilds it from them with `--draw-trails`, and the
speed and altitude plots the same way. Recording them again through this
API would double the frames and redraw every agent every frame, the cost
the trails module measured. Plugins see belief, not truth, so they could
not draw that trail anyway. A plugin draws what only it knows: its region,
its rules, its plan.

## Metrics

`Metrics::report` returns a `MetricReport`: ordered column names and, for each
team, a score and values. Each metrics plugin reports its own score; scores
are not added across plugins. A column with no value for a team is left out
of the run record and the sweep results, not written as zero.
`Simulation::metric_reports()` returns every report.
