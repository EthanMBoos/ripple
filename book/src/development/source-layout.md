# Source map

The core is organized by subsystem, one named snake_case module file each:
`simulation.rs`, `entity.rs`, `mission.rs`, and so on, which keeps editor tabs
and file searches useful. A subsystem's supporting files stay in its matching
directory; there are no `mod.rs` files. The public scenario describes an
experiment; the engine owns its running state.

## Crates

```text
crates/core      ripple-core: the simulator and its stock plugins
crates/cli       ripple: the command line as a library (ripple_cli),
                 plus the stock `ripple` binary (src/main.rs)
crates/starter   user plugins, missions, and tests; compiled into `ripple`
crates/delivery  the paper's medical-delivery pilot: model dispatcher, couriers, score
crates/ardupilot optional: ArduPlane SITL bridge (`--features ardupilot`)
crates/learning  optional: RL demos, policies run from ONNX (`--features learning`);
                 python/ holds the Python module and trainers
```

## File layout

```text
crates/core/src/
  lib.rs
  mission.rs
  scenario.rs
  simulation.rs
  simulation/
    generation.rs
    scheduler.rs
    trace.rs
    world_plugins.rs
  plugin.rs
  plugin/
    registry.rs
    sensor.rs
    sensor/
      readings.rs
      noisy_state/
        noisy_state.rs
    motion.rs
    motion/
      simple_aircraft/
        simple_aircraft.rs
```

Core files use ordinary Rust module discovery. For example, `mod generation;`
in `simulation.rs` loads `simulation/generation.rs`. The file and directory
serve one subsystem, not duplicate implementations. Only keep a directory
that holds files.

Each plugin folder holds its implementation, including its parameter struct and
defaults. Each named category file uses a small `#[path]` declaration to load
its plugins; see [Writing plugins](../guides/writing-plugins.md#a-plugin-that-ships-with-ripple).
The category files contain module wiring and re-exports, while model code lives
in named files. Test models use `tests/support/plugin_fixtures.rs` and adjacent
category files, loaded by `tests/plugin_contracts.rs`; they are not extra Cargo
test targets.

## What lives where

Paths are relative to `crates/core/src/`.

| File | What belongs here |
| --- | --- |
| [simulation.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/simulation.rs) | Simulation ownership, the tick's phases in order, termination and cleanup |
| [simulation/generation.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/simulation/generation.rs) | Spawn timing, IDs and coordinator-owned random draws |
| [simulation/scheduler.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/simulation/scheduler.rs) | Joined parallel entity phases, including failures/panics |
| [simulation/world_plugins.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/simulation/world_plugins.rs) | Interaction/network/metrics ownership and dispatch, not model equations |
| `simulation/trace.rs` | The optional observation trace (`--trace`) |
| [entity.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/entity.rs), [entity/plugin_stack.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/entity/plugin_stack.rs) | Entity state/lifecycle and its configured plugin instances |
| [mission.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/mission.rs) | Finding a YAML mission, expanding templates and overrides |
| [scenario.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/scenario.rs) | Public experiment data and its validation |
| [math/state.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/math/state.rs), `math/quaternion.rs`, `math/angles.rs` | Physical state and coordinate conventions |
| [math/integration.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/math/integration.rs) | RK4 |
| [ports.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/ports.rs) | Named, unit-checked ports between autonomy, controller, and motion |
| [common/pid.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/common/pid.rs), `common/random.rs` | Control helpers and keyed random numbers (labels, hashing, uniform and normal) |
| `common/loop_rate.rs`, `common/plugin_random.rs` | Simulation-time rates and each plugin's keyed random numbers |
| `common/spatial.rs` | Finding nearby agents: `within` and `pairs_within`, answered by scan or grid; built once per tick after motion |
| `common/terrain.rs`, `common/waypoints.rs` | Terrain heights and imagery; waypoint routes |
| [pubsub/messages.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/pubsub/messages.rs) | Publications, subscriber queues, and delivery |
| [record.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/record.rs), `proto/record.proto` | MCAP channels and embedded protobuf schema |
| [plugin.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin.rs) | The shared plugin API: `PluginParams`, `StepTime`, `EntityInfo`, `Update`, `AgentContext`, `WorldContext`, and why each category trait is shaped as it is |
| `plugin/<category>.rs` | One category's interface, its context, and its built-ins |
| [plugin/registry.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/registry.rs) | `PluginRegistry`, typed factories, and the list of built-in names |
| `plugin/<category>/<plugin>/<plugin>.rs` | One built-in model; its parameter struct's `Default` holds the mission defaults |

## Interfaces and implementations

Each category file holds the interface and lists the built-ins:
`plugin/sensor.rs` defines `Sensor` and `SensorContext`,
`plugin/sensor/readings.rs` delivers readings to the estimators, and
`plugin/sensor/noisy_position/noisy_position.rs` is an actual sensor model, with
its parameters and defaults at the top of the same file. There is no `plugins/`
tree beside `plugin/`. `NetworkContext` lives with the message engine in
`pubsub/messages.rs`; `plugin.rs` re-exports it with the rest of the plugin
API.

[plugin.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin.rs)
holds the shared API and re-exports every category's interface, so authors
import from one place. Authors continue to use `ripple_core::plugin::*`;
internal module moves do not leak into their model code. [Plugin contract
tests](https://github.com/EthanMBoos/ripple/blob/main/crates/core/tests/plugin_contracts.rs)
exercise the categories through the public API, with test-only models under
`crates/core/tests/support/`. There is no separate example application. The same
registry supports [Rust-built scenarios](../guides/rust-scenarios.md).

## Follow a mission through the source

1. `mission.rs`: load a `Mission` containing scenario data and file
   defaults. `scenario.rs`: define the shared data and validate it inside
   `Simulation::new`.
2. `plugin/registry.rs`: compile registered types from mission
   parameters.
3. `entity/plugin_stack.rs` and `simulation/world_plugins.rs`: create and own
   independent entity-local and world-level plugin instances.
4. `simulation.rs`: `step_inner` shows the phases in order: generate
   entities; run autonomy; for each substep, controllers then motion; sensors;
   interactions; networks; estimation; metrics; then remove inactive entities
   and advance the simulation.
5. `simulation/scheduler.rs`: join each entity phase before the next starts.
   `pubsub/messages.rs` commits communication at the defined delivery phase;
   `plugin/sensor/readings.rs` holds each sensor's latest reading for the
   estimators in `plugin/estimator.rs`, which write belief.
6. `record.rs`: writes frames, events, and each metric plugin's results. The
   CLI owns run directories and Rerun through `crates/cli/src/viewer/` and
   `viewer.rs`; the viewer does not own simulation time or physics.

See [Writing plugins](../guides/writing-plugins.md) for authoring, [How a run
works](../concepts/how-a-run-works.md) for the phases, and
[Roadmap](https://github.com/EthanMBoos/ripple/blob/main/docs/TODO.md) for
remaining implementation work and intentional breaks.
