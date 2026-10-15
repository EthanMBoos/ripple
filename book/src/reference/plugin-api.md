# Plugin API and lifecycle

For engine navigation, see [Source map](../development/source-layout.md).
Each category's interface and its built-ins live in
`crates/core/src/plugin/<category>.rs`, with each built-in in its own folder
under `plugin/<category>/`. Authors import everything from
`ripple_core::plugin::*`.

## Extension points

| Type | Owned by | Built-in implementation |
| --- | --- | --- |
| Autonomy | Entity | [Straight](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/autonomy/straight/straight.rs); [AuctionAssign](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/autonomy/auction_assign/auction_assign.rs) |
| Controller | Entity | [SimpleAircraftControllerPID](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/controller/simple_aircraft_pid/simple_aircraft_pid.rs) |
| MotionModel | Entity | [SimpleAircraft](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/motion/simple_aircraft/simple_aircraft.rs) |
| Sensor | Entity | [NoisyState](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/sensor/noisy_state/noisy_state.rs); [NoisyContacts](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/sensor/noisy_contacts/noisy_contacts.rs); [NoisyPosition](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/sensor/noisy_position/noisy_position.rs) (Rust-only illustration) |
| Localization | Entity | [PassThrough](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/estimator/pass_through/pass_through.rs) |
| Tracking | Entity | [PassThrough](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/estimator/pass_through/pass_through.rs) |
| Interaction | Simulation | [SimpleCollision](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/interaction/simple_collision/simple_collision.rs) |
| Network | Simulation | [LocalNetwork](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/network/local_network/local_network.rs), [GlobalNetwork](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/network/global_network/global_network.rs), [SphereNetwork](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/network/sphere_network/sphere_network.rs) |
| Metrics | Simulation | [SimpleCollisionMetrics](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/metrics/simple_collision_metrics/simple_collision_metrics.rs) |

Each implementation is an ordinary struct containing its own state and model
code. No plugin author needs to implement the engine's erased adapters, manage
threads, take locks, or edit the simulator.

Inside the engine, each category has its own constructor catalog and typed
slots. Updates call the stored plugin directly; there is no all-category
instance enum to check each tick. The internal forwarding traits remain, and the
public plugin traits and registration methods are unchanged.

## Where to put your model code

For growing autonomy implementations, see [Fighting autonomy plugin
bloat](../guides/autonomy-composition.md). It describes ordinary component and
state composition without adding another plugin framework.

Every built-in follows the same layout: imports and configuration, owned state,
`impl Plugin`, the category implementation, private helpers, then tests. Read
the category's `step` first to understand the algorithm; `configure` explains
how mission parameters reach it. See [Rust
style](../development/rust-style.md#familiar-plugin-authoring) for the layout
and ownership rules.

These are the working patterns; the linked built-ins above contain the
actual code rather than a second set of templates to keep synchronized:

| Category | What belongs in its update | Regression coverage |
| --- | --- | --- |
| Autonomy | Read belief (own state and known contacts), calculate desired commands, write ports. | Straight configuration test; aircraft missions; plugin contract test. |
| Controller | Read desired/current state, run owned control loops, write actuator commands. | Named gain mapping and PID equation tests; aircraft missions. |
| MotionModel | Read actuator commands, integrate owned model state, update mutable truth. | Level-flight equation test; aircraft and substep missions. |
| Sensor | Read committed truth, construct a measurement, apply effects, publish a reading. | NoisyPosition zero-noise/bias and fixed-seed draw-order tests; sensor visibility contract. |
| Localization | Predict own state (optionally from commands), correct with readings, write belief. | PassThrough; estimation contract tests; plugin contract test (a custom localizer). |
| Tracking | Update known contacts from readings and localization's pose. | PassThrough; estimation contract tests (per-entity trackers). |
| Interaction | Inspect world entities, apply physical effects, emit events. | Straight collision/removal mission; same-tick interaction/metrics contract. |
| Network | Decide reachability and delivery for each link through `context.route` (or `route_within_entity`). | Built-in delay validation; local/global routing and custom delay/loss tests. |
| Metrics | Subscribe during initialization, consume events during step, report team totals. | Team aggregation unit test; mission summaries; independent subscriber contract. |

The engine supplies short-lived contexts; do not store them in the plugin.
Store only the plugin's own persistent model state and configuration. A motion
model is allowed to update truth, while a sensor borrows truth read-only.
Messages and readings must follow their documented delivery paths.

For C++ developers: the plugin struct holds what used to be class members; `impl
Plugin` supplies common lifecycle operations; `impl Sensor` (or another
category) supplies the specialized update. This is composition through traits,
not a duplicated inheritance hierarchy. `Result<Update>` distinguishes an
operation failure (including numerical failure) from an applied or
stop-requesting update.

Straight uses a named configuration struct. LocalNetwork and GlobalNetwork are
stateless and use `Config = ()`; configuration still rejects unsupported delay
modes. Call `configure` and pass its result to `new`. The controllers read
`{kp, ki, kd, integrate_within_*}` gains (`AngleGains`, `LengthGains`,
`SpeedGains`) into `PidGains`; angular PID updates use radians and convert the
degree-valued band once at construction.

Downstream plugins can import `Pid`, `PidGains`, `EulerAngles`, `Quaternion`,
`Vec3`, and `rk4` directly from `ripple_core`. Use `Quaternion::from_euler`
for attitude construction and `rotate_body_to_world` to rotate vectors.

## Add a plugin

A research plugin belongs in a user crate such as `crates/starter`, not among
the stock plugins: see [User plugins and the starter
crate](../guides/user-plugins.md). It implements the same traits below and
registers itself in the crate's `register()`, which the `ripple` command calls.
The steps here are for adding a stock plugin.

1. Use the built-in for your category as a reference. Add the plugin's named
   `<plugin>.rs` implementation under `plugin/<category>/<plugin>/`. Declare and
   re-export it in the named category module, following the example below.
2. Declare the mission parameters as a serde struct with a `Default`, and
   implement `Plugin::configure` to parse and validate it (see
   [Parameters and defaults](#parameters-and-defaults)). Implement `Plugin::new`
   to construct independent mutable state for each instance.
3. Implement your category's `step`. Use optional `initialize` for initial
   state and subscriptions, and `Plugin::close` for shutdown.
4. Register the type with `register_autonomy`, `register_controller`,
   `register_motion`, `register_sensor`, `register_localization`,
   `register_tracking`, `register_interaction`, `register_network`, or
   `register_metrics` in `register_builtins` (`plugin/registry.rs`) for a
   bundled plugin.
5. Reference that registration name in the mission.

[registry.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/registry.rs)
shows every registration category. Missions select the registered names:

```yaml
interactions: {SimpleCollision: {}}
networks: {GlobalNetwork: {}}
metrics: {SimpleCollisionMetrics: {}}
# Autonomy, controller, motion_model, and sensors belong in an entity group.
```

Start with `PluginRegistry::with_builtins()` and add your project types.
A bare `PluginRegistry::default()` is deliberately empty; it must also register
the `GlobalNetwork` used by simulator events. As in C++, the mission gets an
implicit GlobalNetwork when it does not explicitly list one.

The public registry is still covered by
[plugin_contracts.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/tests/plugin_contracts.rs).
Its supporting models are test fixtures, not a separate application or a
prescribed project layout. Public scenario construction is described in
[Building a scenario in Rust](../guides/rust-scenarios.md). File and Rust inputs
use the same registry and plugin configuration checks.

## Plugin files

Each built-in has one directory containing its implementation:

```text
crates/core/src/plugin/motion/simple_aircraft/
    simple_aircraft.rs   # parameters and defaults, state, and motion equations
```

This folds C++'s separate header, implementation, and XML defaults into one
Rust file. Every category follows this layout. The small category file
`plugin/motion.rs` loads it explicitly:

```rust,ignore
#[path = "motion/simple_aircraft/simple_aircraft.rs"]
mod simple_aircraft;
pub use simple_aircraft::SimpleAircraft;
```

The path is relative to the category file. The logical module remains
`plugin::motion::simple_aircraft`, and the public type remains
`ripple_core::plugin::motion::SimpleAircraft`. No extra wrapper module or
duplicate implementation is needed. Keep category interfaces in the category
files such as `plugin/motion.rs`, and registrations in `plugin/registry.rs`;
neither belongs in the model equations.

Reference headers, build files, schemas, and unported plugins remain in sibling
`../scrimmage`, branch `Ubuntu-24.04`; do not recreate a parallel reference tree
in this repo.

## Parameters and defaults

Each plugin declares its mission parameters as a struct. Serde fills it from the
mission's values, and `Default` supplies every key the mission leaves out:

```rust,ignore
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SphereNetworkConfig {
    range_m: f64,              // the mission key and the Rust name, with its unit
    transmit_probability: f64,
}

impl Default for SphereNetworkConfig {
    fn default() -> Self {
        Self { range_m: 100.0, transmit_probability: 1.0 }
    }
}

fn configure(params: &PluginParams<'_>) -> Result<SphereNetworkConfig> {
    let config: SphereNetworkConfig = params.parse()?;
    ensure!(config.range_m >= 0.0, "SphereNetwork range_m must be nonnegative");
    Ok(config)
}
```

- Values are YAML, and numbers must be finite: `[0, 0, 200]` fills `[f64; 3]`,
  and `{kp: 1, ki: 0.01, kd: 2, integrate_within_deg: 9}` fills `AngleGains`.
- The field name is the mission key, so a plugin has no `#[serde(rename)]`:
  one name, with its unit, as [Rust style](../development/rust-style.md#parameter-names)
  describes.
- `deny_unknown_fields` makes a misspelled or unsupported key an error that
  names the key, instead of silently using the default.
- An option a plugin does not implement is not a field at all;
  `deny_unknown_fields` rejects it by name.
- When the runtime needs derived values (radians, an inverse inertia, rotor
  geometry), parse a private `...Params` struct and build the `Config` from it,
  as SimpleAircraft and Multirotor do.
- A plugin with no parameters uses `Config = ()`; `params.parse()` still
  rejects any key the mission gives it. `loop_rate_hz` is read by the framework.

A value in the mission wins; `Default` supplies the rest. A value shared by
several groups goes in a [template](../guides/yaml-missions.md#templates).

The C++ plugins' XML defaults became these `Default` impls. Plugins are linked
as Rust code; runtime shared-library loading is intentionally out of scope.
NoisyPosition has its own Rust-example defaults and is not a C++ sensor port.

## Control signals and state

`Plugin::ports` declares named input/output channels with units and frames.
The engine validates connections before execution and supplies `PluginIo`
to autonomy, controller, and motion updates.

Port names are owned strings, so configuration can determine channel
names/count: `Port::new(format!("motor_{index}"), Unit::RadiansPerSecond,
Frame::None)`. String literals work too. `Port` is `Clone` but not `Copy`, so it
cannot be a `const`: construct declarations in `ports`, and clone only when the
same declaration is needed for both an input and an output.

```text
Autonomy outputs -> Controller chain -> MotionModel inputs
```

See `SimpleAircraftControllerPID` for desired heading/altitude/speed inputs
and throttle, roll-rate, and pitch-rate outputs.
Autonomies/controllers receive read-only state; motion receives mutable truth.
Interactions receive the entities after motion and sensors, and can update
physical state or health. `context.nearby` finds entities near a point (see
[Finding nearby agents](#finding-nearby-agents)), using positions as they are
when the interaction starts, including moves made by earlier interactions.
Structural creation/removal remains engine-owned.

`KinematicState` stores position, linear velocity, and angular velocity in the
local ENU world frame: `position_world_m`, `velocity_world_mps`, and
`angular_velocity_world_radps`. Its quaternion rotates forward/left/up body axes
into world axes. Motion models keep their own body-frame rates private and
convert when reading/writing truth, as C++ FixedWing6DOF does. Frame recording
copies world velocities directly; it performs no body/world rotation. A sensor
or controller needing body rates can use
`orientation_world_from_body.rotate_world_to_body(...)`.

`AgentContext` gives autonomy and controllers the entity's belief and nothing
else: `context.state` is its estimated own state and `context.contacts` the
contacts it knows about (without itself). Motion writes truth; frames,
physical interactions, sensors, networks, and metrics can read truth. Decision
plugins cannot.

## Estimators

Each entity has one **localization** estimator, which writes its own-state
belief, and one **tracking** estimator, which writes its known contacts. Both
default to `PassThrough`. They run once per tick in the estimation step, after
networks and delivery and before metrics, localization first; autonomy and
controllers see the result on their next update.

```rust,ignore
impl Localization for DeadReckoning {
    // The readings this estimator reads; the mission's `inputs` picks each sensor.
    fn inputs(_: &Config) -> Vec<Input> {
        vec![Input::new("fix", ReadingKind::Other)]
    }
    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        // Set the first belief from startup readings or a known prior.
        Ok(())
    }
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update> {
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
        Ok(Update::Applied)
    }
}
```

- **Readings.** A sensor hands one reading per sample to its entity's
  estimators with `context.publish_reading(value)`, keyed by the sensor's
  instance name. A sensor declares what its readings contain with
  `Sensor::reading_kind`: `State` (a `KinematicState`), `Contacts` (a
  `ContactList`), or `Other` (the default: any type, checked when read).
  `inputs.latest` returns the newest reading; `inputs.fresh` only one delivered
  since the last estimation step, so a slow sensor's old reading isn't
  mistaken for a new one.
- **Choosing sources.** A mission's `inputs` map names the sensor for each
  input, or `perfect` for the engine's perfect measurement (state or
  contacts, taken after interactions, leaving out entities marked for
  removal). An input left out takes the one sensor of its kind, or the perfect
  measurement if there is none; several candidates, an unknown name, or a kind
  mismatch is a load error. `Other` inputs must be named. The resolved
  scenario in `run.mcap` records every choice.
- **Commands.** `LocalizationContext::commands` holds what the motion model
  actually read on each substep of the tick just finished: the final
  controller's outputs, or the autonomy's if there is no controller.
- **Tracking** receives localization's estimate for this tick as
  `context.pose` and writes `context.contacts`, a `ContactList`
  (`Arc<[Contact]>`) that is cheap to copy from a reading. Each `Contact` has
  its identity, estimated state, and the time its reading was taken.
- **Startup.** The whole initial population is built (sensors, then motion),
  the startup interactions run, and one perfect measurement of that population
  is taken. Then every entity's estimators initialize from their startup
  readings, and only then its controllers and autonomy, which therefore see
  belief from their first call. An entity that spawns later starts from the
  latest perfect measurement; others learn of it at their next measurement.
- **What estimators can't see.** Their contexts have no truth handle: only
  readings, messages, commands, time, and (tracking) the pose.

In YAML, the estimators are entity slots like `motion_model`:

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

### NoisyState

`NoisyState` measures the entity's own state with Gaussian errors. It is a
`State` sensor: its startup reading is a sample of the initial state
(after the motion model has set it up), and each sample after that is the
measured post-motion state. With it installed, localization's `PassThrough`
copies those readings, so autonomy and controllers act on them. It publishes no
message.

- Parameters are a bias (the error's mean) and a standard deviation per axis:
  `position_bias_m` and `position_stddev_m` (x, y, z), `velocity_bias_mps` and
  `velocity_stddev_mps`, and `attitude_bias_rad` and `attitude_stddev_rad`
  (roll, pitch, yaw). Every one defaults to zero, so a sensor with no
  parameters measures exactly; set only the noise you want.
- Attitude noise right-multiplies body-axis roll, pitch, and yaw rotations.
- Angular velocity is reported exactly; there is no rate noise.
- Each error is the keyed normal number for `[reading, quantity, axis]`
  (quantity 0 position, 1 velocity, 2 attitude; reading 1 for the first
  reading, taken when the entity is created, 0 for every later one). See
  [Randomness and seeds](../concepts/randomness.md).

Try `missions/noisy-state.yaml`. Its two aircraft get independent noise (each
error's label holds its entity). The separate
`verification/noisy-state-bias.yaml` fixes standard deviations at zero to
check the equations and feedback without random numbers. NoisyPosition remains a distinct Rust illustration.

### NoisyContacts

`NoisyContacts` measures every other active contact after motion, in entity-ID
order, with the same parameters and defaults as NoisyState. It is a `Contacts`
sensor: tracking's `PassThrough` copies each sample into known contacts. It has
no range/FOV filtering, detection probability, or persistent tracks.

Its reading is a `ContactList`: each `Contact` keeps the target's identity
with a measured kinematic state, and target angular velocity passes through
exactly. Neither sensor reports a covariance.

An autonomy uses the measured contacts through belief:

```rust
// step:
for contact in context.contacts.iter() {
    let measured_position_m = contact.state.position_world_m;
    // Use this measured position in your behavior.
}
```

Removal clears a target on the next sensor sample after removal, not
retroactively. Each error is keyed by the contact it measures (see [Randomness
and seeds](../concepts/randomness.md)).

Try `missions/noisy-contacts.yaml`. Its route follower does not consume contacts;
the public-plugin contract tests include a consumer that drives from measured
positions and checks next-tick feedback, spawn/removal, and worker determinism.
`--trace` records sensor readings, believed contacts, and delivered message
payloads in `run.mcap`. It also records each autonomy and controller output
after the last motion substep.

### SphereNetwork and AuctionAssign

`SphereNetwork` routes between entity endpoints with strict 3D `distance <
range_m` (default 100), using current post-motion truth. Same-entity
endpoints bypass geometry. World plugins have no antenna position and are
unreachable on this network; use GlobalNetwork for their traffic.
`enable_altitude_split: true` permits two endpoints only when both are at/above
`split_altitude_m - split_margin_m`, or both at/below `split_altitude_m +
split_margin_m`. Both default to zero; the margin is nonnegative.

Every reachable delivery, including same-entity traffic, independently succeeds
with `transmit_probability` (0..1, default 1). Delivery is immediate in the network
phase; there are no retries, persistence, or legacy communication-delay modes.
Each subscriber has its own loss decision and queue. Adding subscribers changes
RNG consumption. The stream is deterministic for a fixed configuration at 1/2/8
workers.

`AuctionAssign` is the C++ single-round random-bid demonstration. Entity 1
starts once if `auctioneer=true` (default). All recipients, including itself,
bid in (0,10). On its first update strictly after `auction_duration_s` (default
5), it publishes the highest bid it has received; ties retain the first
delivery. No received bid produces `winner: None`, and late bids never revise
the result. Use one AuctionAssign instance per entity. There is no retry,
consensus guarantee under loss, task execution, or support for concurrent
auction rounds. Zero world-velocity outputs hold the agent still; a later
autonomy may overwrite them through the normal port merge.

Typed payloads and topic constants are exported from `plugin::autonomy`:
`AuctionStart { auctioneer_id }` / `START_AUCTION_TOPIC`, `AuctionBid {
bidder_id, bid }` / `BID_AUCTION_TOPIC`, and `AuctionResult { winner:
Option<AuctionBid> }` / `RESULT_AUCTION_TOPIC`. The topic strings are
`StartAuction`, `BidAuction`, and `ResultAuction`. Subscribe to results in an
entity plugin. `network` defaults to SphereNetwork; unlike C++'s hard-coded
CommsNetwork alias, it selects a registered network name.

Loss and bids are keyed random numbers ([Randomness and
seeds](../concepts/randomness.md)): labels hold the mission `<seed>`, the
entity ID (0 for world plugins), the plugin identity (such as
`autonomy/AuctionAssign:0`, or a network's name), the tick, and the plugin's
key. A bid is keyed by its auction's auctioneer; a message's loss by the
sending and receiving entities, the topic, and its sequence. Changing the
mission seed changes them. `AgentContext::random` gives autonomies and
controllers their numbers, `SensorContext::random` gives sensors theirs, and
`NetworkContext::route` passes the network's to its decision function as
`|link, random|`.

## Finding nearby agents

Many models ask which agents are within some distance of a point: collision
checks within a few metres, a radio within a kilometre, a sensor within its
range. Ask the engine instead of looping over every agent yourself:

```rust
// Sensors, networks, and metrics: true contacts, in id order.
for contact in context.contacts_truth.within(position_world_m, range_m) {
    // contact is closer than range_m
}

// Interactions: indices into `context.entities`, ascending.
for index in context.nearby.within(position_world_m, range_m) { /* ... */ }

// Interactions: each close pair (first < second), in the order a loop over
// every pair visits them. SimpleCollision uses this.
for (first, second) in context.nearby.pairs_within(range_m) { /* ... */ }
```

The answer includes every agent at a distance less than `range_m` (a
non-finite distance counts as within, as in the checks this replaced). Nothing
is filtered out: the asking agent and agents already disabled or removed are
included, so apply your own rules. Queries see current positions: sensors
see them after motion; an interaction sees them as they are when it starts,
including moves by earlier interactions; networks and metrics see them after
all interactions.

How the engine answers is a mission setting, never plugin code:

```yaml
spatial_lookup:
  method: grid       # default: scan
  cell_width_m: 5
```

- **Scan**, the default, tests every agent. It is the reference answer.
- **Grid** gives each agent an integer cell, `floor(position / cell_width_m)`
  on each axis, and keeps a map from each vertical (x, y) column to the agents
  in it. Only occupied columns are stored, so memory grows with the number of
  agents, not the size of the map. A query collects the agents in every cell
  that overlaps a
  slightly padded box around its sphere, gives each the scan's exact distance
  test, and returns the survivors in the scan's order.

The grid only decides which agents to test, so it never changes an answer or
its order: output is byte-identical to the scan. It falls back to the scan
when a position is not finite or too far out for a cell, or when looking up a
query's cells would cost more than testing every agent (one lookup costs about
30 distance tests). That keeps a poor cell width from costing much, but
building the grid still takes time: in the paper's benchmark the grid made a
step 9% slower with 256 agents and 2-4% slower when agents crowded together or
the range spanned many cells. With 4,096 spread-out agents it made a step 3.8
times faster than before (19.1 to 5.0 ms), against 1.9 times for the scan.
Start with a cell width at or somewhat above your most common query range:
50 m cells were slightly faster than 5 m cells for a 5 m collision range.
The grid suits agents spread out horizontally: a query tests every agent in
its (x, y) columns at any height, so many agents stacked over one spot are
tested almost as if scanned.

The engine builds the index after motion and before the parallel sensor
phase, so sensors share it without locks. Before each interaction, and before
networks and metrics, it checks the index against current positions (one pass
over the agents) and rebuilds it if anything moved. SCRIMMAGE's C++ R-tree is
built when entities are generated, before motion.

Decisions cannot query truth. Autonomy and controllers see only
`context.contacts`, the agent's own belief, which is short enough to scan.

## Messages

Each plugin instance owns independent subscriptions, an incoming queue, and an
outgoing queue. Message data is shared immutably for multicast, not plugin
state. The following excerpt uses the `Population` message and `ExampleNetwork`
from the plugin contract test fixtures, not names registered by the stock
command:

Message types derive `serde::Serialize` so trace recordings can include their
payloads.

```rust
// initialize:
context.messages.subscribe::<Population>("ExampleNetwork", "population")?;

// publisher step:
context.messages.publish("ExampleNetwork", "population", Population { entity_count: 8 })?;

// subscriber step:
for message in context.messages.receive::<Population>("ExampleNetwork", "population")? {
    self.entity_samples += message.value.entity_count;
}
```

Receive explicitly drains this subscriber's delivered queue at its update; it
does not drain anyone else's queue. This is the Rust equivalent of processing
messages in the step body, not a claim that C++ callback registration is ported.
Each message includes sender identity, publication time, and actual delivery
time. Wrong message types and unconfigured network names are errors.

Queues have fixed message-count limits in `pubsub/messages.rs`: 1,024 outgoing
publications per entity plugin (65,536 for world plugins), 65,536 unread
messages per subscriber/channel, and 65,536 pending deliveries per network
(including fan-out staged in the current phase). Overflow returns an error and
terminates the run with normal cleanup; it is not silent loss or backpressure. A
failing phase is not rolled back. These are simple research defaults, not byte
limits or legacy queue-policy compatibility. Drain subscriptions you create; a
deliberately slow subscriber can fill its own queue.

A network implements `step` and calls `context.route` once. Its decision
function, `|link, random|`, receives a `Transmission` (sender/receiver
identities, topic, the message's sequence, time, and world contacts) and the
network's random numbers, then returns `Delivery::Drop` or
`Delivery::After { delay_s }`. Zero delay delivers during the current network
phase; positive delay waits until a later phase, even if the publisher goes
silent. A network can use its
own mailbox through `context.messages()`.

A network that can only deliver between plugins of the same entity should call
`context.route_within_entity` instead: its decision function then sees only
links to subscribers in the sender's own entity, and none from world plugins.
That keeps routing linear in the team size; with `route`, a topic every agent
publishes and every agent subscribes to costs one decision per pair.
LocalNetwork uses it.

LocalNetwork connects plugins on the same entity, at once; GlobalNetwork
connects all endpoints. GlobalNetwork and SphereNetwork take `delay_s`
(default 0): a message arrives that long after it was sent, at the first tick
at or after that time, and its receiver sees both times (`sent_at_s`,
`delivered_at_s`). The older `comm_delay` and `is_stochastic_delay` keys are
errors; use `delay_s`.

`SensorContext::publish_reading` is not a message: a reading goes only to its
own entity's estimators, bypassing network policies. Use the message API when
communication behavior matters.

## Execution and lifecycle

```text
Generate entities
-> all autonomies
-> for each substep: controllers, then motion
-> sensors
-> interactions
-> networks / delivery
-> estimation: localization, then tracking
-> metrics
-> remove inactive entities
-> output / advance time
```

Entity phases join all worker tasks before the next phase. Cross-entity reads
use committed snapshots. Interactions, networks, and metrics run on the
coordinator; plugin state is never shared mutably between workers.

Interactions also run once before the first tick, matching the reference.
Networks run in name order. Metrics can consume interaction messages within the
same tick; autonomy cannot see post-motion sensor readings until its next step.
Estimation runs inside each entity's delivery task, so it adds no worker phase.
Autonomy/controller/sensor loop rates retain outputs when updates are skipped.
Estimators run every tick and reject `loop_rate_hz`: a reading or command from a
skipped tick would otherwise be lost. An estimator can do expensive work less
often inside its own `step`.
World-level plugins run each phase as in the C++ reference source.

Return `Update::Applied` or `Update::Stop`; use `Result::Err` with an
explanation when the operation failed, including nonfinite model state. A normal
plugin stop finishes the current tick, including networks and metrics, then
emits terminal state. This is an intentional Rust contract, not C++ early-exit
equivalence. Removal occurs after that tick's delivery/metrics; removed entities
receive no later messages, including already scheduled deliveries. Pending
future messages are not flushed at termination. `EntityPresentAtEnd` is written
to the event log during finalization, not delivered as an extra metrics/network
phase. Close is called once on removal, terminal completion, failure, or
dropping the simulation. Close errors/panics do not prevent other plugins from
closing. Initialization failure also closes every constructed plugin in that
entity; `close` must tolerate incomplete initialization. Ordinary `Result`
failures name the responsible plugin; caught panics may have only phase/entity
context. Do not rely on recovery from an aborted process or a panicking
constructor.

Metrics return a `MetricReport`: ordered column names and, per team, a score
and values. Multiple metrics instances are supported; each reports its own
score, and Ripple does not add scores across plugins. A column with no value
for a team is left out of the run record and sweep rows, not written as zero.
`Simulation::metric_reports()` exposes each report.

## What remains

See [Built-in models](models.md) for the selected aircraft, world, and waypoint
models, their options, and deliberate limits. The [paper's
validation](https://github.com/EthanMBoos/ripple/blob/main/paper/README.md)
records checks and results.
[Roadmap](https://github.com/EthanMBoos/ripple/blob/main/docs/TODO.md) lists
current priorities. Legacy delay behavior, additional sensor models, and plugin
debug geometry need further work. Services, runtime parameters, and callbacks
should be implemented where selected simulation models need them, not copied as
an unconditional framework backlog.

Compiled downstream Rust plugins remain supported. Runtime shared-library
discovery/loading and legacy protobuf/string-map spawning are explicitly
excluded. Learned policies are planned as an optional training bridge and
frozen-policy plugin
([Roadmap](https://github.com/EthanMBoos/ripple/blob/main/docs/TODO.md)). ROS
and JSBSim integrations are not planned; ArduPlane SITL is optional ([Flying
ArduPlane](../guides/ardupilot.md)). YAML missions, templates, and sweeps work;
typed runtime spawn requests are not implemented.
