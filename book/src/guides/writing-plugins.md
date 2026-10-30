# Writing plugins

This guide covers what every plugin type has in common. It assumes you have
done [Your first autonomy plugin](../tutorial/first-autonomy.md). The [plugin
API reference](../reference/plugin-api.md) lists each trait and context.

## Where your plugins live

Write plugins in a user crate in this repository, such as
[crates/starter](https://github.com/EthanMBoos/ripple/tree/main/crates/starter).
Plugins are compiled into the one `ripple` command rather than loaded at run
time, so your missions use `ripple run`, `sweep`, and `replay` exactly as the
stock missions do. The command builds a *registry*, the table from the names
missions use to Rust types, in
[crates/cli/src/main.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/cli/src/main.rs):

```rust,ignore
let mut registry = PluginRegistry::with_builtins();   // stock plugins
starter::register(&mut registry)?;                    // your plugins
ripple_cli::main(registry, repo_root, env::args_os())  // run, sweep, replay
```

Changing a plugin means rebuilding; `cargo run -- ...` does that for you.

The starter holds the two tutorials' plugins, `FollowNearest` and
`DeadReckoning`, with their missions, a sweep, and tests:

```sh
cargo run -- run crates/starter/missions/follow-nearest.yaml --headless
cargo run -- replay runs/follow-nearest/run000
cargo test -p starter
```

## The plugin types

| Type | Trait | Gets | Built-in to copy |
| --- | --- | --- | --- |
| Autonomy | `Autonomy` | Own belief, contacts, messages, random numbers; writes ports | [Straight](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/autonomy/straight/straight.rs) |
| Controller | `Controller` | The same; reads and writes ports | [SimpleAircraftControllerPID](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/controller/simple_aircraft_pid/simple_aircraft_pid.rs) |
| Motion model | `MotionModel` | Mutable truth; reads ports | [SimpleAircraft](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/motion/simple_aircraft/simple_aircraft.rs) |
| Sensor | `Sensor` | Read-only truth, contacts, random numbers; publishes readings | [NoisyPosition](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/sensor/noisy_position/noisy_position.rs) |
| Localization | `Localization` | Readings, messages, the commands motion read; writes own-state belief | [PassThrough](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/estimator/pass_through/pass_through.rs) |
| Tracking | `Tracking` | Readings, messages, localization's pose; writes known contacts | [PassThrough](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/estimator/pass_through/pass_through.rs) |
| Interaction | `Interaction` | Every entity, mutable; emits events | [GroundCollision](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/interaction/ground_collision/ground_collision.rs) |
| Network | `Network` | Every message, random numbers; decides delivery | [LocalNetwork](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/network/local_network/local_network.rs) |
| Metrics | `Metrics` | Events, messages, truth; reports team scores | [SimpleCollisionMetrics](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/metrics/simple_collision_metrics/simple_collision_metrics.rs) |

Start by copying the built-in for your type; they all follow the same layout.
A motion model also opens with a fidelity header saying where its equations
and numbers come from; see [Adding a measured-data vehicle
model](measured-vehicle-models.md).

## Anatomy of a plugin

```rust,ignore
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MyConfig { /* mission parameters */ }
impl Default for MyConfig { /* the value of each parameter a mission omits */ }
pub struct MyPlugin { /* per-entity state that persists between ticks */ }

impl Plugin for MyPlugin {
    type Config = MyConfig;
    fn configure(params: &PluginParams<'_>) -> Result<MyConfig> { /* parse, validate */ }
    fn new(config: &MyConfig) -> Self { /* fresh state for one instance */ }
    fn ports(config: &MyConfig) -> Ports { /* inputs and outputs, if any */ }
}

impl Autonomy for MyPlugin {           // or Controller, MotionModel, Sensor, ...
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> { /* optional */ }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> { /* each tick */ }
    fn close(&mut self, time: StepTime) -> Result<()> { /* optional: once, at the end */ }
}
```

- **Configuration is separate from state.** `configure` runs once per mission
  and rejects bad values with a clear error. `new` runs once per instance, so
  each vehicle gets its own state.
- **The context is borrowed, not stored.** Each call gets a short-lived
  context. Keep only your own configuration and state in the struct.
- **No threads, locks, or shared state.** The engine runs entities in
  parallel for you; your plugin is ordinary single-threaded Rust.
- **Write `step` in three stages:** read inputs, calculate, write outputs.
  Name every physical quantity with its unit and frame (`distance_m`,
  `heading_world_rad`); see [Coordinate
  frames](../concepts/coordinate-frames.md).

## Parameters and defaults

Serde fills the parameter struct from the mission, and `Default` supplies
every key the mission leaves out:

```rust,ignore
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StraightConfig {
    speed_mps: f64,   // the mission key and the Rust name, with its unit
}

impl Default for StraightConfig {
    fn default() -> Self {
        Self { speed_mps: 21.0 }
    }
}

fn configure(params: &PluginParams<'_>) -> Result<StraightConfig> {
    let config: StraightConfig = params.parse()?;
    ensure!(config.speed_mps >= 0.0, "speed_mps must be nonnegative");
    Ok(config)
}
```

- The field name is the mission key, with its unit, so each parameter has one
  name ([Parameter names](../development/rust-style.md#parameter-names)).
- Values are ordinary YAML: `speed_mps: 25`, `center_m: [0, 0, 200]`,
  `heading_pid: {kp: 1, ki: 0.01, kd: 2, integrate_within_deg: 9}`. Numbers
  must be finite.
- `deny_unknown_fields` makes a misspelled key an error that names it, so a
  typo never silently gives you the default. An option the plugin doesn't
  implement simply isn't a field.
- When the runtime needs derived values (radians, an inverse inertia), parse
  a private `...Params` struct and build the config from it, as SimpleAircraft
  and Multirotor do.
- A plugin with no parameters uses `Config = ()`; `params.parse()` still
  rejects any key the mission gives it. The engine reads `loop_rate_hz` itself.

## Register it

Put each plugin in its own file under `crates/starter/src/`, add a `mod`
line to `src/lib.rs`, and register it in `register()` under the name missions
will use:

```rust,ignore
registry.register_sensor::<MySensor>("MySensor")?;
```

Use the call for its type: `register_autonomy`, `register_controller`,
`register_motion`, `register_sensor`, `register_localization`,
`register_tracking`, `register_interaction`, `register_network`, or
`register_metrics`. Registering a name already used in the same category is an
error at startup, so nothing is silently replaced.

`register_autonomy_with` takes a closure that builds each instance in place of
`Plugin::new`, for a program that owns the simulation and must hand every
instance something of its own. The Python package's training simulation uses
it to hand each `Python` autonomy its trainer ([Training a single
agent](training-single-agent.md#the-pieces)).

## Ports: the control chain

Autonomy, controllers, and motion models pass commands down a chain of named
ports:

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
`io.write("throttle", value)?`. Before the mission runs, the engine checks that
every input is connected to an output with the same name, unit, and frame.
Port names can come from configuration: Multirotor declares one input per
rotor, `Port::new(format!("motor_{index}"), ...)`.

## Messages

For anything that isn't a command down the chain, publish and subscribe to
typed messages on a named network:

```rust,ignore
// In initialize: subscribe once.
context.messages.subscribe::<BoundaryRegion>("GlobalNetwork", "Boundary")?;

// In step: publish ...
context.messages.publish("GlobalNetwork", "Boundary", region)?;

// ... and receive everything delivered to you since your last step.
for message in context.messages.receive::<BoundaryRegion>("GlobalNetwork", "Boundary")? {
    self.boundary = Some(*message.value);
}
```

A message type derives `Serialize` and `Deserialize`, so it round-trips
through JSON: that is how a plugin written in Python sends and receives it
([Plugins in Python](python-plugins.md#messages)), and how `--trace`
records it. GlobalNetwork reaches every plugin, and the engine publishes
events such as `EntityGenerated` and `NonTeamCollision` on it. Any other
network must be
listed in the mission. An autonomy reads a message on the tick after it was
sent; [How a run works](../concepts/how-a-run-works.md#messages) explains the
timing.

## Returning from `step`

- `Ok(Update::Applied)`: normal.
- `Ok(Update::Stop)`: ask to end the run. The current tick still finishes.
- `Err(...)`: something went wrong, such as an invalid state or a nonfinite
  number. The run stops and the error names your plugin. Use
  `ensure!(condition, "message")` for these checks.

## When an autonomy grows

Most research happens in autonomy, and its `step` can turn into one long
mixture of mission logic, navigation, and special cases. The plugin is what
the mission selects, but it doesn't have to be the smallest unit of code:
give it ordinary fields for parts with their own job and state (a mission
phase, a navigator), and have `step` call them. Those parts need no
registration or messages between them. Extract one when it owns a distinct
responsibility, not for every short calculation. World rules, such as
boundaries and collisions, belong in interactions, not in every autonomy.

## Several behaviors on one vehicle

A vehicle that is told what to do next by a planner (wait, fly to a site,
land, come home) is best written as one autonomy plugin that hosts a library
of *skills*, each an ordinary struct in its own file. [Skills: many behaviors
in one autonomy](skills.md) describes the pattern, with the delivery crate's
`Courier` as the example.

Coordination above the vehicle, such as a fleet planner, is another entity's
autonomy that sends orders as messages; the delivery crate's `Dispatcher`
does this from the depot.

## Testing

- **Unit tests** go in a `#[cfg(test)] mod tests` block at the bottom of the
  plugin file. Call `configure`, `new`, and your equations directly with
  hand-picked inputs: "at zero noise, the measurement equals truth plus
  bias".
- **Mission tests** go in your crate's `tests/` folder. Build the registry
  `ripple` uses, load a mission, run it to the end, and check the events or
  scores; the [tutorial](../tutorial/first-autonomy.md#6-test-it) shows one.

Before submitting a change, run the project's checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Another user crate

Copy `crates/starter` to `crates/my-lab` and rename the package in its
`Cargo.toml`. Remove the copied `FollowNearest` and `DeadReckoning` (files,
`mod` lines, registrations, missions, and tests), since the starter already
registers those names. Then:

1. add it to the workspace `members` in the root `Cargo.toml`;
2. add `my-lab = { path = "../my-lab" }` to `crates/cli/Cargo.toml`, and call
   `my_lab::register(&mut registry)?;` in `crates/cli/src/main.rs`.

## Your own project

Your plugins can live in a repository of their own, with Ripple as a
dependency, when you don't want to work inside this one. The command crate
is a library, so your project's binary is the stock `main.rs` with your
plugins in place of the starter's and your repository as the project
folder:

```toml
# Cargo.toml
[package]
name = "my-lab"
version = "0.1.0"
edition = "2024"

[dependencies]
ripple-core = { git = "https://github.com/EthanMBoos/ripple" }
ripple = { git = "https://github.com/EthanMBoos/ripple" }
anyhow = "1"
serde = { version = "1", features = ["derive"] }
```

```rust,ignore
// src/main.rs
use ripple_core::plugin::PluginRegistry;

mod my_autonomy;

fn main() -> anyhow::Result<()> {
    let mut registry = PluginRegistry::with_builtins();
    registry.register_autonomy::<my_autonomy::MyAutonomy>("MyAutonomy")?;
    ripple_cli::main(registry, env!("CARGO_MANIFEST_DIR"), std::env::args_os())
}
```

`cargo run -- run my-mission.yaml --headless` then looks the mission up in
your `missions/`, writes runs to your `runs/` and sweeps to your `sweeps/`,
and `sweep` and `replay` work the same. Pin the dependency to a commit
(`rev = "..."`) for a study, so a later change here cannot alter your
results. `with_builtins` gives you the stock plugins; the starter's and the
delivery crate's come only if you depend on those crates and call their
`register`. In Python the same project is `uv add ripple` and a folder of
missions, pending the wheels ([Plugins in
Python](python-plugins.md#your-own-project)).

## A plugin that ships with Ripple

Only a plugin meant for everyone goes in the simulator's own source. It gets
its own folder, `crates/core/src/plugin/<type>/<my_plugin>/my_plugin.rs`,
and two edits:

1. **Load the module** in the type's file, such as
   `crates/core/src/plugin/motion.rs`, in alphabetical order:

   ```rust,ignore
   #[path = "motion/my_plugin/my_plugin.rs"]
   mod my_plugin;
   pub use my_plugin::MyPlugin;
   ```

2. **Register a name** in `register_builtins` in
   `crates/core/src/plugin/registry.rs`:

   ```rust,ignore
   registry.register_motion::<motion::MyPlugin>("MyPlugin")?;
   ```

Its tests go beside it and in `crates/core/tests/`, and it needs a section in
[Built-in models](../reference/models.md).
