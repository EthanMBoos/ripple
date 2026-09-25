# Writing plugins

This guide covers what is common to all seven plugin types. It assumes you have
done [Your first autonomy plugin](../tutorial/first-autonomy.md). The complete
reference, with every rule and edge case, is
[`docs/RUST_PLUGINS.md`](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/RUST_PLUGINS.md)
in the repository.

## The seven plugin types

| Type | Trait | Runs for | Gets | Built-in to copy |
| --- | --- | --- | --- | --- |
| Autonomy | `Autonomy` | Each entity | Own belief, contacts, messages, a random stream; writes ports | [Straight](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/autonomy/straight/straight.rs) |
| Controller | `Controller` | Each entity | Same as autonomy; reads and writes ports | [SimpleAircraftControllerPID](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/controller/simple_aircraft_pid/simple_aircraft_pid.rs) |
| Motion model | `MotionModel` | Each entity | Mutable truth; reads ports | [SimpleAircraft](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/motion/simple_aircraft/simple_aircraft.rs) |
| Sensor | `Sensor` | Each entity | Read-only truth, contacts, a random stream; sets belief | [NoisyPosition](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/sensor/noisy_position/noisy_position.rs) |
| Interaction | `Interaction` | Whole world | All entities, mutable; emits events | [GroundCollision](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/interaction/ground_collision/ground_collision.rs) |
| Network | `Network` | Whole world | Every message, a random stream; decides delivery | [LocalNetwork](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/network/local_network/local_network.rs) |
| Metrics | `Metrics` | Whole world | Events and messages; reports team scores | [SimpleCollisionMetrics](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/metrics/simple_collision_metrics/simple_collision_metrics.rs) |

Start by copying the built-in for your type. They all follow the same layout.

## Anatomy of a plugin

```rust,ignore
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MyConfig { /* mission parameters */ }
impl Default for MyConfig { /* the value of each parameter a mission omits */ }
pub struct MyPlugin { /* per-entity state that persists between ticks */ }

impl Plugin for MyPlugin {
    type Config = MyConfig;
    fn configure(params: &PluginParams<'_>) -> Result<MyConfig> { /* params.parse() + validate */ }
    fn new(config: &MyConfig) -> Self { /* fresh state for one instance */ }
    fn ports(config: &MyConfig) -> Ports { /* inputs/outputs, if any */ }
}

impl Autonomy for MyPlugin {           // or Controller, MotionModel, Sensor, ...
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> { /* optional */ }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> { /* each tick */ }
}
```

- **Configuration is separate from state.** `configure` runs once per mission
  and should reject bad values with a clear error. `new` runs once per plugin
  instance: each vehicle gets its own state.
- **The context is borrowed, not stored.** The simulator hands you a short-lived
  context each call. Keep only your own configuration and state in the struct.
- **No threads, locks, or shared state.** The simulator runs entities in
  parallel for you. Your plugin is ordinary single-threaded Rust.
- **Write `step` in three stages**: read inputs, calculate, write outputs. Name
  every physical quantity with its unit and frame (`distance_m`,
  `heading_world_rad`). See [Coordinate frames](../concepts/coordinate-frames.md).

## Files and registration

Write your plugins in a user crate such as `crates/starter` (see [Your first
autonomy plugin](../tutorial/first-autonomy.md) and
[Writing your own plugins](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/USER_PROJECTS.md)):
one file per plugin under `src/`, a `mod` line in `src/lib.rs`, and one line
in its `register()`:

```rust,ignore
registry.register_motion::<MyPlugin>("MyPlugin")?;
```

Use `register_autonomy`, `register_controller`, `register_motion`,
`register_sensor`, `register_interaction`, `register_network`, or
`register_metrics` to match the type. Missions refer to the registered name.

### Adding a stock plugin to the simulator

Only when a plugin should ship with scrimmage-rs itself does it go in the
simulator's source. Each stock plugin gets its own folder:

```text
crates/core/src/plugin/<type>/<my_plugin>/
    my_plugin.rs
```

Then make two edits:

1. **Load the module** in the type's file, such as `crates/core/src/plugin/motion.rs`.
   Entries are alphabetical:

   ```rust,ignore
   #[path = "motion/my_plugin/my_plugin.rs"]
   mod my_plugin;
   pub use my_plugin::MyPlugin;
   ```

2. **Register a name** in `crates/core/src/plugin_manager/builtins.rs`:

   ```rust,ignore
   registry.register_motion::<motion::MyPlugin>("MyPlugin")?;
   ```

   with the `register_*` call that matches the type.

## Parameters and defaults

Declare the parameters as a struct, and let serde fill it:

```rust,ignore
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StraightConfig {
    #[serde(rename = "speed")]   // what the mission calls it
    speed_mps: f64,              // the Rust name, with its unit
}

impl Default for StraightConfig {
    fn default() -> Self {
        Self { speed_mps: 21.0 }
    }
}

fn configure(params: &PluginParams<'_>) -> Result<StraightConfig> {
    let config: StraightConfig = params.parse()?;
    ensure!(config.speed_mps >= 0.0, "speed must be nonnegative");
    Ok(config)
}
```

A parameter's value comes from the first of these that sets it:

1. an attribute on the mission tag: `<autonomy speed="30">Straight</autonomy>`,
2. a `param_common` group the tag references,
3. an optional `Straight.xml` overlay file on `SCRIMMAGE_PLUGIN_PATH`,
4. the struct's `Default`.

In a YAML mission the values are ordinary YAML: `speed: 25`, `gains: [1, 0, 2, 9]`.
In XML every value is text: numbers must be finite, booleans are `true`/`false`
(or `1`/`0`), and lists such as `"1 2 3"` or `"1, 2, 3"` fill a `Vec` or a fixed
array like `[f64; 3]`. The same struct reads either. A bad value is an error that names the key, and so is a
key the struct doesn't have: a typo never silently gives you the default.

## Ports: the control chain

Autonomy, controllers, and motion models pass commands to each other through
named ports:

```text
autonomy outputs -> controller inputs, controller outputs -> motion model inputs
```

Declare them in `ports` with a unit and a frame:

```rust,ignore
Ports::default()
    .input(Port::new("desired_speed", Unit::MetersPerSecond, Frame::None))
    .output(Port::new("throttle", Unit::Dimensionless, Frame::None))
```

Read and write them in `step` with `io.read("desired_speed")?` and
`io.write("throttle", value)?`. The simulator checks that every input is
connected to an output with the same name, unit, and frame *before* the
mission runs.

Port names can come from configuration. The Multirotor, for example, declares
one input per rotor: `Port::new(format!("motor_{index}"), ...)`.

## Messages

For anything that isn't a direct command down the control chain, plugins publish
and subscribe to typed messages on a named network:

```rust,ignore
// In initialize: subscribe once.
context.messages.subscribe::<BoundaryRegion>("GlobalNetwork", "Boundary")?;

// In step: publish ...
context.messages.publish("GlobalNetwork", "Boundary", region)?;

// ... and receive everything delivered since your last step.
for message in context.messages.receive::<BoundaryRegion>("GlobalNetwork", "Boundary")? {
    self.boundary = Some(*message.value);
}
```

- **GlobalNetwork** reaches every plugin in the simulation. The simulator also
  publishes events such as `EntityGenerated` and `NonTeamCollision` on it.
- **LocalNetwork** only reaches plugins on the *same* entity. Use it for a
  vehicle's own sensor data.
- Messages are delivered in the network phase, after the publisher runs.
  An autonomy sees a sensor's message on its next tick.
- The mission must list every network you use: `<network>LocalNetwork</network>`.
  An unlisted network is an error.

## The order of one tick

```text
generate new entities
-> all autonomies
-> all controller substeps
-> all motion substeps
-> sensors
-> interactions
-> networks deliver messages
-> metrics
-> remove dead entities, record the frame, advance time
```

Each phase finishes for **every** entity before the next one starts. So
everything in one phase sees the same consistent state of the world, whatever
order the entities happen to run in. Results are identical whether you run with
1 worker or 8.

## Returning from `step`

- `Ok(Update::Applied)`: normal.
- `Ok(Update::Stop)`: ask to end the simulation. The current tick still finishes.
- `Err(...)`: something went wrong, such as an invalid state or a nonfinite
  number. The simulation stops and the error names your plugin. Use
  `ensure!(condition, "message")` for these checks.

## Testing

Two kinds of tests work well:

- **Unit tests** in a `#[cfg(test)] mod tests` block at the bottom of your
  plugin file. Call `configure`, `new`, and your equations directly with
  hand-picked inputs. For example: "at zero noise, the measurement equals truth
  plus bias".
- **Mission tests** in your crate's `tests/` folder, such as
  `crates/starter/tests/`. Build the registry `scrimmage` uses (stock plugins
  plus your crate's `register`), load a mission, run it to the end, and check
  the events or scores. The [tutorial](../tutorial/first-autonomy.md#6-test-it)
  shows one. Stock plugins keep theirs in `crates/core/tests/`.

Before submitting a change, run the same checks as the project:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
