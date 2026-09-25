# Plugin API and lifecycle

Use [C++ plugin guide](../appendix/cpp/plugin-development.md) for the SCRIMMAGE design.
This document maps that design to the current Rust API; it does not redefine
the original documentation or claim that every legacy service is ported.

For engine navigation, see [Source map](../development/source-layout.md). Category
interfaces live in named framework modules; concrete built-ins
live only under `crates/core/src/plugin/<category>/`. The public authoring
imports remain `scrimmage_core::plugin::*`; the source moves do not require
changes to an external plugin project.

## Seven extension points

| Type | Owned by | Built-in implementation |
| --- | --- | --- |
| Autonomy | Entity | [Straight](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/autonomy/straight/straight.rs); [AuctionAssign](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/autonomy/auction_assign/auction_assign.rs) |
| Controller | Entity | [SimpleAircraftControllerPID](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/controller/simple_aircraft_pid/simple_aircraft_pid.rs) |
| MotionModel | Entity | [SimpleAircraft](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/motion/simple_aircraft/simple_aircraft.rs) |
| Sensor | Entity | [NoisyState](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/sensor/noisy_state/noisy_state.rs); [NoisyContacts](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/sensor/noisy_contacts/noisy_contacts.rs); [NoisyPosition](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/sensor/noisy_position/noisy_position.rs) (Rust-only illustration) |
| Interaction | Simulation | [SimpleCollision](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/interaction/simple_collision/simple_collision.rs) |
| Network | Simulation | [LocalNetwork](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/network/local_network/local_network.rs), [GlobalNetwork](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/network/global_network/global_network.rs), [SphereNetwork](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/network/sphere_network/sphere_network.rs) |
| Metrics | Simulation | [SimpleCollisionMetrics](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/metrics/simple_collision_metrics/simple_collision_metrics.rs) |

Each implementation is an ordinary struct containing its own state and model code.
No plugin author needs to implement the engine's erased adapters, manage
threads, take locks, or edit the simulator.

Inside the engine, each category has its own constructor catalog and typed slots.
Updates call the stored plugin directly; there is no all-category instance enum
to check each tick. The internal forwarding traits remain, and the public
plugin traits and registration methods are unchanged.

## Where to put your model code

For growing autonomy implementations, see
[Fighting autonomy plugin bloat](../guides/autonomy-composition.md). It describes
ordinary component and state composition without adding another plugin framework.

Every built-in follows the same layout: imports and configuration, owned state,
`impl Plugin`, the category implementation, private helpers, then tests. Read
the category's `step` first to understand the algorithm; `configure` explains
how mission parameters reach it. See [Rust style](../development/rust-style.md#familiar-plugin-authoring)
for the layout and ownership rules.

These are the seven working patterns; the linked built-ins above contain the
actual code rather than a second set of templates to keep synchronized:

| Category | What belongs in its update | Regression coverage |
| --- | --- | --- |
| Autonomy | Read current state/observations, calculate desired commands, write ports. | Straight configuration test; aircraft missions; seven-category contract test. |
| Controller | Read desired/current state, run owned control loops, write actuator commands. | Named XML gain mapping and PID equation tests; aircraft missions. |
| MotionModel | Read actuator commands, integrate owned model state, update mutable truth. | Level-flight equation test; aircraft and substep missions. |
| Sensor | Read committed truth, construct a measurement, apply effects, queue output. | NoisyPosition zero-noise/bias and fixed-seed draw-order tests; sensor visibility contract. |
| Interaction | Inspect world entities, apply physical effects, emit events. | Straight collision/removal mission; same-tick interaction/metrics contract. |
| Network | Decide reachability and delivery for each link through `context.route`. | Built-in delay validation; local/global routing and custom delay/loss tests. |
| Metrics | Subscribe during initialization, consume events during step, report team totals. | Team aggregation unit test; mission summaries; independent subscriber contract. |

The engine supplies short-lived contexts; do not store them in the plugin.
Store only the plugin's own persistent model state and configuration. A motion
model is allowed to update truth, while a sensor borrows truth read-only.
Messages and observations must follow their documented delivery paths.

For C++ developers: the plugin struct holds what used to be class members;
`impl Plugin` supplies common lifecycle operations; `impl Sensor` (or another
category) supplies the specialized update. This is composition through traits,
not a duplicated inheritance hierarchy. `Result<Update>` distinguishes an
operation failure (including numerical failure) from an applied or stop-requesting update.

Straight uses a named configuration struct. LocalNetwork and GlobalNetwork are
stateless and use `Config = ()`; configuration still rejects unsupported delay modes.
Call `configure` and pass its result to `new`. The controller parses legacy
`[P, I, D, integral band]` XML values into named `PidGains`; angular PID updates
still use radians and convert the legacy degree-valued band once at construction.

Downstream plugins can import `Pid`, `PidGains`, `EulerAngles`, `Quaternion`,
`Vec3`, and `rk4` directly from `scrimmage_core`. Use `Quaternion::from_euler`
for attitude construction and `rotate_body_to_world` to rotate vectors.

## Add a plugin

A research plugin belongs in a user crate such as `crates/starter`, not among
the stock plugins: see [User plugins and the starter crate](../guides/user-plugins.md). It implements the
same traits below and registers itself in the crate's `register()`, which the
`scrimmage` command calls. The steps here are for adding a stock plugin.

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
   `register_motion`, `register_sensor`, `register_interaction`,
   `register_network`, or `register_metrics` in `plugin_manager/builtins.rs`
   for a bundled plugin.
5. Reference that registration name in the mission.

[builtins.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin_manager/builtins.rs) shows all seven
registration categories. Missions select the registered names:

```xml
<entity_interaction>SimpleCollision</entity_interaction>
<network>GlobalNetwork</network>
<metrics>SimpleCollisionMetrics</metrics>
<!-- Autonomy, controller, motion_model, and sensor belong inside entity. -->
```

Start with `PluginRegistry::with_builtins()` and add your project types.
A bare `PluginRegistry::default()` is deliberately empty; it must also register
the `GlobalNetwork` used by simulator events. As in C++, the mission gets an
implicit GlobalNetwork when it does not explicitly list one.

The public registry is still covered by
[plugin_contracts.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/tests/plugin_contracts.rs). Its supporting
models are test fixtures, not a separate application or a prescribed project
layout. The intended later direct Rust construction API is described in
[Direct Rust construction plan](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/LIBRARY_FIRST_REFACTOR.md) and remains deferred.

## Plugin files

Each built-in has one directory containing its implementation:

```text
crates/core/src/plugin/motion/simple_aircraft/
    simple_aircraft.rs   # parameters and defaults, state, and motion equations
```

This folds C++'s separate header, implementation, and XML defaults into one
Rust file. All seven categories follow this layout. The small category file
`plugin/motion.rs` loads it explicitly:

```rust,ignore
#[path = "motion/simple_aircraft/simple_aircraft.rs"]
mod simple_aircraft;
pub use simple_aircraft::SimpleAircraft;
```

The path is relative to the category file. The logical module remains
`plugin::motion::simple_aircraft`, and the public type remains
`scrimmage_core::plugin::motion::SimpleAircraft`. No extra wrapper module or
duplicate implementation is needed. Keep category interfaces in named framework
files such as `motion.rs`, and registrations in
`plugin_manager/builtins.rs`; neither belongs in the model equations.

Reference headers, build files, schemas, and unported plugins remain in sibling
`../scrimmage`, branch `Ubuntu-24.04`; do not recreate a parallel reference tree
in this repo.

## Parameters and defaults

Each plugin declares its mission parameters as a struct. Serde fills it from the
mission's values (XML text or YAML), and `Default` supplies every key the
mission leaves out:

```rust,ignore
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SphereNetworkConfig {
    #[serde(rename = "range")]   // the mission key
    range_m: f64,                // the Rust name, with its unit
    #[serde(rename = "prob_transmit")]
    probability_transmit: f64,
}

impl Default for SphereNetworkConfig {
    fn default() -> Self {
        Self { range_m: 100.0, probability_transmit: 1.0 }
    }
}

fn configure(params: &PluginParams<'_>) -> Result<SphereNetworkConfig> {
    let config: SphereNetworkConfig = params.parse()?;
    ensure!(config.range_m >= 0.0, "SphereNetwork range must be nonnegative");
    Ok(config)
}
```

- Numbers must be finite. In XML, booleans are `true`, `false`, `1`, or `0`, and
  lists (`Vec` or a fixed array) are separated by commas or whitespace, so
  `"1, 0.01, 2, 9"` fills `[f64; 4]` and `PidGains`. YAML uses its own
  booleans and lists (`[1, 0.01, 2, 9]`).
- `deny_unknown_fields` makes a misspelled or unsupported key an error that
  names the key, instead of silently using the default.
- A C++ option that Rust does not implement stays a named field with a comment,
  and `configure` rejects any value other than the one that means "off". An
  option that is safe to ignore (Straight's `show_text_label`) is a `_`-prefixed field.
- When the runtime needs derived values (radians, an inverse inertia, rotor
  geometry), parse a private `...Params` struct and build the `Config` from it,
  as SimpleAircraft and Multirotor do.
- A plugin with no parameters uses `Config = ()`; `params.parse()` still
  rejects any key the mission gives it. `loop_rate` is read by the framework.

Values are resolved in this order, first match wins: an attribute on the mission
tag, a `param_common` group the tag references, a `PluginName.xml` overlay found
on `SCRIMMAGE_PLUGIN_PATH`, then `Default`. Overlays are optional: a lab can use
one to change a default for every mission without editing code. Their `<library>`
element is ignored, and any other key must be one the plugin declares.

The C++ plugins' XML defaults became these `Default` impls. Plugins are linked as
Rust code; runtime shared-library loading is intentionally out of scope.
NoisyPosition has its own Rust-example defaults and is not a C++ sensor port.

## Control signals and state

`Plugin::ports` declares named input/output channels with units and frames.
The engine validates connections before execution and supplies `PluginIo`
to autonomy, controller, and motion updates.

Port names are owned strings, so configuration can determine channel names/count:
`Port::new(format!("motor_{index}"), Unit::RadiansPerSecond, Frame::None)`.
String literals work too. `Port` is `Clone` but not `Copy`, so it cannot be a
`const`: construct declarations in `ports`, and clone only when the same
declaration is needed for both an input and an output.

```text
Autonomy outputs -> Controller chain -> MotionModel inputs
```

See `SimpleAircraftControllerPID` for desired heading/altitude/speed inputs
and throttle, roll-rate, and pitch-rate outputs.
Autonomies/controllers receive read-only state; motion receives mutable truth.
Interactions receive the entities after motion and sensors, and can update
physical state or health. Structural creation/removal remains engine-owned.

`KinematicState` stores position, linear velocity, and angular velocity in the
local ENU world frame: `position_world_m`, `velocity_world_mps`, and
`angular_velocity_world_radps`. Its quaternion rotates forward/left/up body
axes into world axes. Motion models keep their own body-frame rates private
and convert when reading/writing truth, as C++ FixedWing6DOF does. Frame recording
copies world velocities directly; it performs no body/world rotation. A sensor
or controller needing body rates can use `orientation_world_from_body.rotate_world_to_body(...)`.

`AgentContext.state` reads the entity's owned belief, falling back to read-only
truth when no sensor has installed an estimate. Sensors call
`context.set_belief(state)` with an owned value or `context.clear_belief()` to
restore ideal feedback. Both autonomy and controllers use this same state.
Motion still writes truth; frames, physical interactions, and explicitly named
`contacts_truth` still expose truth. This does not hide the world from every
legacy autonomy: it removes mutable truth aliasing from the belief path.

Belief persists between sensor samples. Multiple sensor writers execute in XML
order; the last update wins. This is not sensor fusion. Sensor initialization
runs before motion, controllers, and autonomy, matching C++'s initialization
order. During simulation, sensors still sample after all motion substeps; the
next autonomy/controller phase sees the updated estimate.

### NoisyState

`NoisyState` ports the legacy own-state noise equations and XML parameters.
It detaches initial belief, then samples post-motion truth, updates belief, and
publishes `StateWithCovariance` on `LocalNetwork`. Straight prefers the newest
delivered message; when none arrives, it uses current belief. The PID reads
belief directly, not the network queue. Delaying/dropping LocalNetwork messages
therefore does not delay the direct belief update.

Include `<network>LocalNetwork</network>` when using Straight or NoisyState.
Straight now subscribes to local state messages even when no sensor publishes.
Missing configured networks remain errors; they are not silently created.

Legacy details retained deliberately:

- Each `pos_noise_0..2`, `vel_noise_0..2`, and `orient_noise_0..2` value is
  `mean standard_deviation`, in meters, meters/second, and radians respectively.
- Attitude noise right-multiplies body-axis roll, pitch, and yaw rotations.
- The C++ message leaves angular velocity zero and covariance equal to a 3x3
  identity matrix. That covariance is **not** a calibrated sensor uncertainty.
- Samples use the existing Rust per-sensor RNG, keyed by mission seed, entity
  ID, and sensor instance. Draws interleave position/velocity per axis, followed
  by roll/pitch/yaw, including zero-noise draws. This is not C++ RNG-sequence parity.

Try `missions/noisy-state.xml`. Its two aircraft have independent sensor streams.
The separate `verification/noisy-state-bias.xml` fixes standard deviations at
zero to compare the retained equations and feedback against C++ independently
of random-number generation. NoisyPosition remains a distinct Rust illustration.

### NoisyContacts

`NoisyContacts` measures every other active contact after motion, in entity-ID
order, using the same `pos_noise_0..2`, `vel_noise_0..2`, and `orient_noise_0..2`
mean/stddev parameters as NoisyState. It does not change own belief or truth and
has no range/FOV filtering, detection probability, or persistent tracks.

The public `plugin::sensor::ContactsWithCovariances` contains a `contacts` vector.
Each `ContactWithCovariance` contains `entity: EntityInfo` and
`measurement: StateWithCovariance`. Identity is retained; the kinematic state
is measured. Like C++, this sensor preserves target angular velocity and sets
covariance to **5I**, regardless of configured noise. NoisyState instead uses
zero angular velocity and identity covariance. Neither is calibrated uncertainty.

Include `<network>LocalNetwork</network>`. Every sample, including an empty one,
is published on `topic_name` (default `ContactsWithCovariances`, exported as
`CONTACTS_TOPIC`). Subscribe in autonomy initialization and consume it next tick:

```rust
use scrimmage_core::plugin::sensor::{CONTACTS_TOPIC, ContactsWithCovariances};
// initialize:
context.messages.subscribe::<ContactsWithCovariances>("LocalNetwork", CONTACTS_TOPIC)?;
// step:
for message in context.messages.receive::<ContactsWithCovariances>("LocalNetwork", CONTACTS_TOPIC)? {
    for contact in &message.value.contacts {
        let measured_position_m = contact.measurement.state.position_world_m;
        // Use this measured position in your behavior.
    }
}
```

Like C++, the message is the only output; there is no separate local observation
copy. A consumer that keeps the newest message sees it one tick after sampling.
Removal clears a target on the next sensor sample after removal, not
retroactively. Sensor streams use mission seed/entity/instance; C++ uses a
shared generator and different contact iteration order.

Try `missions/noisy-contacts.xml`. Its route follower does not consume contacts;
the public-plugin contract tests include a consumer that drives from measured
positions and checks next-tick feedback, spawn/removal, and worker determinism.
Neither measurements nor auction messages are automatically saved in stock
`events.json`, summaries, or the Rerun dashboard. Add an experiment-specific
consumer/output path when those records are needed.

### SphereNetwork and AuctionAssign

`SphereNetwork` routes between entity endpoints with strict 3D
`distance < range` (meters, default 100), using current post-motion truth.
Same-entity endpoints bypass geometry. World plugins have no antenna position
and are unreachable on this network; use GlobalNetwork for their traffic.
`filter_comms_plane=true` permits two endpoints only when both are at/above
`comms_boundary_altitude - comms_boundary_epsilon`, or both at/below altitude plus
epsilon. Altitude and epsilon are meters; defaults are zero, epsilon nonnegative.

Every reachable delivery, including same-entity traffic, independently succeeds
with `prob_transmit` (0..1, default 1). Delivery is immediate in the network phase;
there are no retries, persistence, or legacy communication-delay modes. Each
subscriber has its own loss decision and queue. Adding subscribers changes RNG
consumption. The stream is deterministic for a fixed configuration at 1/2/8 workers.

`AuctionAssign` is the C++ single-round random-bid demonstration. Entity 1 starts
once if `auctioneer=true` (default). All recipients, including itself, bid in
(0,10). On its first update strictly after `auction_duration_s` (default 5), it
publishes the highest bid it has received; ties retain the first delivery.
No received bid produces `winner: None`, and late bids never revise the result.
Use one AuctionAssign instance per entity. There is no retry, consensus guarantee
under loss, task execution, or support for concurrent auction rounds. Zero world-velocity outputs hold the agent still;
a later autonomy may overwrite them through the normal port merge.

Typed payloads and topic constants are exported from `plugin::autonomy`:
`AuctionStart { auctioneer_id }` / `START_AUCTION_TOPIC`,
`AuctionBid { bidder_id, bid }` / `BID_AUCTION_TOPIC`, and
`AuctionResult { winner: Option<AuctionBid> }` / `RESULT_AUCTION_TOPIC`.
The topic strings are `StartAuction`, `BidAuction`, and `ResultAuction`.
Subscribe to results in an entity plugin. `network_name` defaults to SphereNetwork;
unlike C++'s hard-coded CommsNetwork alias, it selects a registered network name.

Loss and bids draw from each plugin instance's own stream, derived from the
mission `<seed>`, the entity ID (0 for world plugins), and a stable plugin
identity such as `autonomy/AuctionAssign:0`. Changing the mission seed changes
them, as C++'s shared mission-seeded generator does, but the sequences are not
C++'s. `AgentContext::random` gives autonomies and controllers their stream,
`SensorContext::random` gives sensors theirs, and `NetworkContext::route` passes
the network's stream to its decision function as `|link, random|`.

## Messages

Each plugin instance owns independent subscriptions, an incoming queue, and an
outgoing queue. Message data is shared immutably for multicast, not plugin state.
The following excerpt uses the `Population` message and `ExampleNetwork` from
the plugin contract test fixtures, not names registered by the stock command:

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

Receive explicitly drains this subscriber's delivered queue at its update;
it does not drain anyone else's queue. This is the Rust equivalent of processing
messages in the step body, not a claim that C++ callback registration is ported.
Each message includes sender identity, publication time, and actual delivery time.
Wrong message types and unconfigured network names are errors.

Queues have fixed message-count limits in `pubsub/messages.rs`: 1,024 outgoing
publications per plugin, 1,024 unread messages per subscriber/channel, and 65,536
pending deliveries per network (including fan-out staged in the current phase).
Overflow returns an error and terminates the run with normal cleanup; it is not
silent loss or backpressure. A failing phase is not rolled back. These are simple
research defaults, not byte limits or legacy queue-policy compatibility. Drain
subscriptions you create; a deliberately slow subscriber can fill its own queue.

A network implements `step` and calls `context.route` once. Its decision
function, `|link, random|`, receives a `Transmission` (sender/receiver
identities, topic, time, and world contacts) and the network's own random
stream, then returns `Delivery::Drop` or `Delivery::After { delay_s }`.
Zero delay delivers during the current network phase; positive delay waits
until a later phase, even if the publisher goes silent. A network can use its
own mailbox through `context.messages()`.

LocalNetwork connects plugins on the same entity; GlobalNetwork connects all
endpoints. The source's nonnegative `comm_delay` queue semantics and stochastic
delay are not ported by these built-ins and are rejected explicitly.
Custom networks have the explicit delay/loss behavior above.

`SensorContext::publish_local` is a separate convenience for latest-value
entity-local observations. It bypasses simulated network policies; use the
message API when communication behavior matters.

## Execution and lifecycle

```text
Generate entities
-> all autonomies
-> all controller substeps
-> all motion substeps
-> sensors
-> interactions
-> networks / delivery
-> metrics
-> remove inactive entities
-> output / advance time
```

Entity phases join all worker tasks before the next phase. Cross-entity reads
use committed snapshots. Interactions, networks, and metrics run on the
coordinator; plugin state is never shared mutably between workers.

Interactions also run once before the first tick, matching the reference.
Networks run in name order. Metrics can consume interaction messages within the
same tick; autonomy cannot see post-motion sensor deliveries until its next step.
Autonomy/controller/sensor loop rates retain outputs when updates are skipped.
World-level plugins run each phase as in the C++ reference source.

Return `Update::Applied` or `Update::Stop`; use `Result::Err` with an explanation
when the operation failed, including nonfinite model state. A normal plugin stop
finishes the current tick, including networks and metrics, then emits terminal
state. This is an intentional Rust contract, not C++ early-exit equivalence.
Removal occurs after that tick's delivery/metrics; removed entities receive no
later messages, including already scheduled deliveries. Pending future messages
are not flushed at termination. `EntityPresentAtEnd` is written to the event log
during finalization, not delivered as an extra metrics/network phase.
Close is called once on removal, terminal completion, failure, or dropping the
simulation. Close errors/panics do not prevent other plugins from closing.
Initialization failure also closes every constructed plugin in that entity;
`close` must tolerate incomplete initialization. Ordinary `Result` failures name
the responsible plugin; caught panics may have only phase/entity context. Do not
rely on recovery from an aborted process or a panicking constructor.

Metrics return a `MetricReport` containing ordered headers and per-team values
and scores. Multiple metrics instances are supported. CSV output sums team
scores and qualifies duplicate column names with the plugin instance identity.
`Simulation::metric_reports()` exposes each report separately.

## What remains

See [Built-in models](models.md) for the selected aircraft, world, and waypoint
models, their options, and deliberate limits. [Verification evidence](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/EVIDENCE.md) provides
runnable checks. [Roadmap](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/TODO.md) lists current priorities. Legacy delay
behavior, additional sensor models, and plugin debug
geometry need further work. Services, runtime parameters, and
callbacks should be implemented where selected simulation models need them,
not copied as an unconditional framework backlog.

Compiled downstream Rust plugins remain supported. Runtime shared-library
discovery/loading and legacy protobuf/string-map spawning are explicitly excluded.
Burn and optional ROS 1/2 and ArduPilot adapters are future work, not current
capabilities. JSBSim is planned only as offline flight-model reference tooling,
not a production plugin or FFI integration. XML/template coverage is partial.
YAML missions, templates, and sweeps work; typed runtime spawn requests
are not implemented.
