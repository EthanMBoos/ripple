# Architecture overview

Ripple keeps SCRIMMAGE's structure (entities that carry a stack of replaceable
plugins, plus world-level plugins that act on every entity) and changes how an
experiment is described, run, and checked. The main structural addition is
the estimation step: sensors only measure, and each entity's estimators own its
belief (see [Truth, measurements, and belief](information-flow.md)).

## The big picture

```text
     mission.yaml / Rust scenario API
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
│  │  Sensors → Localization → Tracking → belief                 │   │
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
     run.mcap (frames, events, metrics, scenario, optional trace)
     optional Rerun export (output only)
```

## Crates

| Crate | Package | What it holds |
| --- | --- | --- |
| `crates/core` | `ripple-core` | The engine, the scenario types, and the stock plugins |
| `crates/cli` | `ripple` | The `ripple` command: run, sweep, replay, validation |
| `crates/starter` | user crate | Your plugins and missions, compiled into `ripple` |

Plugins are compiled in, not loaded at run time. A user crate registers its
plugins in `crates/cli/src/main.rs`, and the same executable runs stock and user
models. See [User plugins and the starter crate](../guides/user-plugins.md).

## Plugin types

| Type | Scope | Job | Reads | Writes |
| --- | --- | --- | --- | --- |
| Autonomy | Per entity | Decide what to do | Belief, messages | Commands on its ports |
| Controller | Per entity | Turn commands into motion inputs | Belief, commands | Motion inputs |
| MotionModel | Per entity | Advance the physics | Truth, motion inputs | Truth |
| Sensor | Per entity | Measure the world | Truth, other entities' truth | Readings, messages |
| Localization | Per entity | Estimate own state | Readings, messages, the commands motion read | Own-state belief |
| Tracking | Per entity | Estimate known contacts | Readings, messages, localization's pose | Known contacts |
| Interaction | World | Apply effects between entities | Every entity's truth | Truth, health, events |
| Network | World | Deliver messages | Senders, receivers, truth | Delivery decisions |
| Metrics | World | Score the run | Events, messages | Scores |

Every plugin is an ordinary struct with a typed parameter struct
(`#[serde(default, deny_unknown_fields)]`, so a misspelled setting is an error,
not a silent default). Plugins never see threads or locks. Entity plugins
(autonomy, controllers, motion, sensors) can change only their own entity;
they read other entities through read-only snapshots. Interactions are the
exception by design: they run on the coordinator with mutable access to every
entity, because collisions and boundaries change several entities at once. The
engine hands each plugin a context for its phase
(`AgentContext`, `MotionContext`, `SensorContext`, `LocalizationContext`,
`TrackingContext`, `InteractionContext`, `NetworkContext`, `WorldContext`).

## Key types

| Type | File (under `crates/core/src/`) | Role |
| --- | --- | --- |
| `ScenarioConfig` | `scenario.rs` | The typed experiment: run settings, entity groups, world plugins |
| `Simulation` | `simulation.rs` | Owns entities and world plugins; `step()` runs one tick |
| `Entity` | `entity.rs` | Identity, truth, health, and its `PluginStack` |
| `PluginStack` | `entity/plugin_stack.rs` | The entity's plugin instances, readings, and belief |
| `Contact`, `ContactList` | `plugin/estimator.rs` | A tracked contact, and the shared list trackers write |
| `Scheduler` | `simulation/scheduler.rs` | Runs an entity phase across workers and joins it |
| `KinematicState` | `math/state.rs` | Position, velocity, orientation, angular velocity (ENU world frame) |
| `EntitySnapshot` | `entity.rs` | A read-only copy of one entity for cross-entity reads |
| `Messages` | `pubsub/messages.rs` | A plugin's own outbox, subscriptions, and inbox |

A minimal run through the library:

```rust,ignore
let mission = Mission::load(path, root, &Params::new())?;
let mut simulation =
    Simulation::new(mission.scenario, &PluginRegistry::with_builtins(), workers)?;
while let Some(frame) = simulation.step()? {
    // Inspect the state or pass it to a recorder.
    inspect(frame);
}
```

## Guarantees the engine keeps

- **Deterministic at any worker count.** Output is byte-identical at 1, 2, and
  8 workers and with Rerun on or off. Workers own separate entities, every
  phase joins before the next, and cross-entity reads use snapshots.
- **Keyed randomness.** Every random number is a function of a label saying
  what it is for: seed, entity, plugin identity, tick, and the plugin's key
  (such as a contact's ID and an axis). A number doesn't depend on how many
  were drawn before it, so two runs on one seed give the same noise on the
  same measurement. See [Randomness and seeds](../concepts/randomness.md).
- **Every subscriber has its own queue**, and delivery order follows entity and
  plugin order, never worker timing.
- **Truth and belief stay separate.** See
  [Truth, measurements, and belief](information-flow.md).

## Where to go next

- [The tick](tick.md): the phase order, what runs in parallel, and when each
  plugin sees what.
- [Truth, measurements, and belief](information-flow.md): how information moves
  from the world to a decision.
- [Messages](messages.md): publishing, networks, delivery timing.
- [Plugin API and lifecycle](../reference/plugin-api.md): the exact contracts.
