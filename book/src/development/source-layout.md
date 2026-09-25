# Source map for SCRIMMAGE developers

The core follows the C++ responsibility names, not its separate header/source
build layout. Named snake_case module files make editor tabs and file searches
useful: `simcontrol.rs`, `entity.rs`, `sensor.rs`, and so on. A subsystem's
supporting files stay in its matching directory; there are no `mod.rs` files.

This is a structural reorganization, not a change to physics, scheduling,
mission syntax, or the public plugin API. The copied C++ guides remain unchanged.

## Crates

```text
crates/core      scrimmage-core: the simulator and its stock plugins
crates/cli       scrimmage-rs: the command line as a library (scrimmage_cli),
                 plus the stock `scrimmage` binary (src/main.rs)
crates/starter   user plugins, missions, and tests; compiled into `scrimmage`
```

## File layout

```text
crates/core/src/
  lib.rs
  simcontrol.rs
  simcontrol/
    generation.rs
    scheduler.rs
    summary.rs
    world_plugins.rs
  sensor.rs
  sensor/
    observations.rs
  plugin.rs
  plugin/
    motion.rs
    motion/
      simple_aircraft/
        simple_aircraft.rs
```

Core files use ordinary Rust module discovery. For example, `mod generation;`
in `simcontrol.rs` loads `simcontrol/generation.rs`. The file and directory
serve one subsystem, not duplicate implementations. A one-file interface such
as `autonomy.rs` does not need an empty `autonomy/` directory.

Each plugin folder holds its implementation, including its parameter struct
and defaults. Each named category file uses a small `#[path]` declaration to
load its plugins; see [plugin authoring](../reference/plugin-api.md#add-a-plugin). The category files
contain module wiring and re-exports, while model code lives in named files.
Test models use `tests/support/plugin_fixtures.rs` and adjacent category files,
loaded by `tests/plugin_contracts.rs`; they are not extra Cargo test targets.

## C++ responsibilities to Rust files

Paths in the middle column are relative to `crates/core/src/`.

| C++ location / responsibility | Rust location | What belongs here |
| --- | --- | --- |
| `simcontrol/SimControl` | [simcontrol.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/simcontrol.rs) | Simulation ownership, phase order, termination and cleanup |
| SimControl entity generation | [simcontrol/generation.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/simcontrol/generation.rs) | Spawn timing, IDs and coordinator-owned random draws |
| SimControl threaded execution | [simcontrol/scheduler.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/simcontrol/scheduler.rs) | Joined parallel entity phases, including failures/panics |
| SimControl global plugins | [simcontrol/world_plugins.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/simcontrol/world_plugins.rs) | Interaction/network/metrics ownership and dispatch, not model equations |
| `entity/Entity` | [entity.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/entity.rs), [entity/plugin_stack.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/entity/plugin_stack.rs) | Entity state/lifecycle and its configured plugin instances |
| `parse/MissionParse` | [parse/mission.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/parse/mission.rs) (typed mission, validation), `parse/xml_mission.rs` + `parse/xml.rs` + `parse/params.rs` (XML), `parse/yaml_mission.rs` (YAML) | Mission loading and validated configuration |
| `math/State`, `Quaternion`, `Angles` | [math/state.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/math/state.rs), `math/quaternion.rs`, `math/angles.rs` | Physical state and coordinate conventions |
| Numerical integration | [math/integration.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/math/integration.rs) | RK4 with the reference arithmetic order |
| `common/PID`, `VariableIO`, `Random` | [common/pid.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/common/pid.rs), `common/variable_io.rs`, `common/random.rs` | Control helpers, named ports and legacy spawn randomness |
| Plugin timing and new plugin randomness | `common/loop_rate.rs`, `common/plugin_random.rs` | Simulation-time rates and stable per-plugin streams |
| `plugin_manager/EntityPlugin`, `PluginManager` | [plugin_manager/entity_plugin.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin_manager/entity_plugin.rs), `plugin_manager/registry.rs`, `plugin_manager/builtins.rs` | Shared lifecycle/context types, typed factories and bundled registrations |
| `pubsub/` | [pubsub/messages.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/pubsub/messages.rs), `pubsub/network.rs` | Publications, subscriber queues, delivery and the network interface |
| Legacy frame/log protocol | [protocol/frame.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/protocol/frame.rs), `protocol/wire.rs` | Validated legacy protobuf frame I/O |
| `src/plugins/<category>/` | [plugin/](https://github.com/EthanMBoos/scrimmage-rs/tree/main/crates/core/src/plugin) | The single concrete model/algorithm implementation tree |
| `include/scrimmage/plugins/<category>/<Plugin>/<Plugin>.xml` | The parameter struct's `Default` in `plugin/<category>/<plugin>/<plugin>.rs` | Mission defaults, beside the parameters they fill |

The C++ source is the sibling `../scrimmage`, branch `Ubuntu-24.04`; exact
reference provenance and compatibility gaps are in [Reference notes](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/REFERENCE_NOTES.md).
These locations map responsibilities, not claims of complete C++ class parity.

## Interfaces are not implementations

| Type | Framework interface | Concrete implementations |
| --- | --- | --- |
| Autonomy | `autonomy.rs` | `plugin/autonomy/` |
| Controller | `controller.rs` | `plugin/controller/` |
| MotionModel | `motion.rs` | `plugin/motion/` |
| Sensor | `sensor.rs` | `plugin/sensor/` |
| Interaction | `entity_interaction.rs` | `plugin/interaction/` |
| Metrics | `metrics.rs` | `plugin/metrics/` |
| Network | `pubsub/network.rs` | `plugin/network/` |

For example, `sensor.rs` defines the contract and `sensor/observations.rs`
implements observation delivery;
`plugin/sensor/noisy_position/noisy_position.rs` is an actual sensor model, with its
parameters and defaults at the top of the same file. Those are different
responsibilities, not competing implementation folders. There is no `plugins/`
tree beside `plugin/`.

[plugin.rs](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin.rs) is a small public-import facade.
It exposes the category implementations and re-exports the framework API.
Authors continue to use `scrimmage_core::plugin::*`; internal module moves do
not leak into their model code. [Plugin contract tests](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/tests/plugin_contracts.rs)
exercise all seven categories through the public API, with test-only models
under `crates/core/tests/support/`. There is no separate example application.
The [library-first refactor](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/LIBRARY_FIRST_REFACTOR.md) is planned for after parity.

## Follow a mission through the source

1. `parse/mission.rs`: load a `ScenarioConfig`, then validate and resolve it.
2. `plugin_manager/registry.rs`: compile registered types from mission parameters.
3. `entity/plugin_stack.rs` and `simcontrol/world_plugins.rs`: create and own
   independent entity-local and world-level plugin instances.
4. `simcontrol.rs`: generate entities; run autonomy; all controller
   substeps; all motion substeps; sensors; interactions; networks; metrics;
   then remove inactive entities and advance the simulation.
5. `simcontrol/scheduler.rs`: join each entity phase before the next starts.
   `pubsub/messages.rs` commits communication at the defined delivery phase;
   `sensor/observations.rs` delivers entity-local latest-value observations.
6. `protocol/frame.rs` and `simcontrol/summary.rs`: provide frame and summary
   output. The CLI owns files and Rerun through `crates/cli/src/viewer/` and
   `viewer.rs`; the viewer does not own simulation time or physics.

See [Plugin API reference](../reference/plugin-api.md) for authoring and phase semantics,
and [Roadmap](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/TODO.md) for remaining implementation work and intentional breaks.
