# Architecture overview

scrimmage-rs keeps SCRIMMAGE's structure (entities that carry a stack of
replaceable plugins, plus world-level plugins that act on every entity) and
changes how an experiment is described, run, and checked. This section is the
Rust counterpart of the [C++ architecture guide](../appendix/cpp/architecture.md).

Parts marked **planned** describe the estimation design in
[ESTIMATION.md](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/ESTIMATION.md).
They are not in the code yet; everything else is.

## The big picture

```text
  mission.xml / mission.yaml / Rust scenario API
                     │
                     ▼
            ┌──────────────────┐      plugin names and typed
            │  ScenarioConfig  │◀──── parameters checked against
            └────────┬─────────┘      the PluginRegistry
                     ▼
┌────────────────────────────────────────────────────────────────────┐
│                        Simulation (engine)                         │
│                                                                    │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐                            │
│  │  Entity  │ │  Entity  │ │  Entity  │  ...  each owns its truth, │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘       belief, and stack    │
│       ▼            ▼            ▼                                  │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ Plugin stack (per entity)                                   │   │
│  │  Autonomy → Controller(s) → MotionModel                     │   │
│  │  Sensors → Localization → Tracking (planned) → belief       │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                    │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ World plugins (run on the coordinator)                      │   │
│  │  Interactions: collisions, boundaries, ground contact       │   │
│  │  Networks:     decide who receives each message, and when   │   │
│  │  Metrics:      scores from events and messages              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                    │
│  Scheduler: entity phases on a worker pool, joined before the      │
│  next phase. Snapshots: read-only copies other entities read.      │
└───────────────────────────────┬────────────────────────────────────┘
                                ▼
     frames.bin, events.json, summary.csv, manifest, optional
     trace.jsonl, optional Rerun recording (output only)
```

## Crates

| Crate | Package | What it holds |
| --- | --- | --- |
| `crates/core` | `scrimmage-core` | The engine, the scenario types, and the stock plugins |
| `crates/cli` | `scrimmage-rs` | The `scrimmage` command: run, sweep, replay, validation |
| `crates/starter` | user crate | Your plugins and missions, compiled into `scrimmage` |

Plugins are compiled in, not loaded at run time. A user crate registers its
plugins in `crates/cli/src/main.rs`, and the same executable runs stock and
user models. See [User plugins and the starter crate](../guides/user-plugins.md).

## Plugin types

| Type | Scope | Job | Reads | Writes |
| --- | --- | --- | --- | --- |
| Autonomy | Per entity | Decide what to do | Belief, messages | Commands on its ports |
| Controller | Per entity | Turn commands into motion inputs | Belief, commands | Motion inputs |
| MotionModel | Per entity | Advance the physics | Truth, motion inputs | Truth |
| Sensor | Per entity | Measure the world | Truth, other entities' truth | Readings, messages |
| Estimator (planned) | Per entity | Localization (own state), then tracking (contacts) | Readings, messages, known commands | Belief |
| Interaction | World | Apply effects between entities | Every entity's truth | Truth, health, events |
| Network | World | Deliver messages | Senders, receivers, truth | Delivery decisions |
| Metrics | World | Score the run | Events, messages | Scores |

Today a sensor can also write belief, and decisions fall back to truth when no
sensor does; the planned estimators replace both (see
[Truth, measurements, and belief](information-flow.md)).

Every plugin is an ordinary struct with a typed parameter struct
(`#[serde(default, deny_unknown_fields)]`, so a misspelled setting is an error,
not a silent default). Plugins never see threads or locks. Entity plugins
(autonomy, controllers, motion, sensors) can change only their own entity;
they read other entities through read-only snapshots. Interactions are the
exception by design: they run on the coordinator with mutable access to every
entity, because collisions and boundaries change several entities at once. The
engine hands each plugin a context for its phase
(`AgentContext`, `MotionContext`, `SensorContext`, `InteractionContext`,
`NetworkContext`, `WorldContext`).

## Key types

| Type | File (under `crates/core/src/`) | Role |
| --- | --- | --- |
| `ScenarioConfig` | `scenario.rs` | The typed experiment: run settings, entity groups, world plugins |
| `Simulation` | `simcontrol.rs` | Owns entities and world plugins; `step()` runs one tick |
| `Entity` | `entity.rs` | Identity, truth, health, and its `PluginStack` |
| `PluginStack` | `entity/plugin_stack.rs` | The entity's plugin instances, belief, and observations |
| `Scheduler` | `simcontrol/scheduler.rs` | Runs an entity phase across workers and joins it |
| `KinematicState` | `math/state.rs` | Position, velocity, orientation, angular velocity (ENU world frame) |
| `EntitySnapshot` | `entity.rs` | A read-only copy of one entity for cross-entity reads |
| `Messages` | `pubsub/messages.rs` | A plugin's own outbox, subscriptions, and inbox |

A minimal run through the library:

```rust,ignore
let mission = Mission::load(path, root, &Params::new())?;
let mut simulation =
    Simulation::new(mission.scenario, &PluginRegistry::with_builtins(), workers)?;
while let Some(frame) = simulation.step()? {
    write_frame(&mut frames, &frame)?;
}
```

## Guarantees the engine keeps

- **Deterministic at any worker count.** Output is byte-identical at 1, 2, and
  8 workers and with Rerun on or off. Workers own separate entities, every
  phase joins before the next, and cross-entity reads use snapshots.
- **Stable randomness.** Each plugin instance draws from its own random stream,
  keyed by entity and plugin identity (such as `sensor/NoisyContacts:0`), so
  adding a plugin does not change another's numbers as long as the other's
  identity is unchanged. Inserting a plugin earlier in a list of the same kind
  shifts the index, and so the stream, of the ones after it.
- **Every subscriber has its own queue**, and delivery order follows entity and
  plugin order, never worker timing.
- **Truth and belief stay separate.** See
  [Truth, measurements, and belief](information-flow.md).

## Where to go next

- [The tick](tick.md): the phase order, what runs in parallel, and when each
  plugin sees what.
- [Truth, measurements, and belief](information-flow.md): how information moves
  from the world to a decision, today and with the planned Estimator.
- [Messages](messages.md): publishing, networks, delivery timing.
- [Plugin API and lifecycle](../reference/plugin-api.md): the exact contracts.
