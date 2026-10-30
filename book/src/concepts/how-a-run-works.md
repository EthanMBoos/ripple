# How a run works

A run is a loop of ticks. Each tick runs the same phases in the same order,
and every phase finishes for every agent before the next one starts:

```text
          ┌──────────────┐
  ┌──────▶│ autonomy     │  decide, from belief only
  │       │ controllers ◀┼─┐  repeated for each substep:
  │       │ motion       ├─┘  controllers, then motion   ◀── dynamics, wind
  │       │ sensors      │  measure the world            ◀── noise
  │       │ interactions │  collisions, boundaries, ground
  │       │ networks     │  deliver messages             ◀── loss, latency
  │       │ estimation   │  readings and messages → belief
  │       │ metrics      │  score the tick
  │       └──────┬───────┘
  └──── next tick: time advances by dt
```

Spawning happens at the start of a tick, and removal of destroyed agents at
the end. The arrows on the right mark where each kind of disturbance enters
(see [Disturbances](../guides/disturbances.md)).

Because each phase is joined before the next, everything in a phase sees the
same state of the world, whatever order the agents run in. That is why output
is byte-identical at 1 worker or 8, and with recording on or off.

## What runs in each phase

Each agent, called an *entity*, carries a stack of plugins. World plugins act
on every entity at once.

| Plugin | Runs for | Reads | Writes |
| --- | --- | --- | --- |
| Autonomy | Each entity | Its belief, messages | Commands on its ports |
| Controller | Each entity | Its belief, commands | Motion inputs |
| Motion model | Each entity | Truth, motion inputs | Truth |
| Sensor | Each entity | Truth, other entities' truth | Readings, messages |
| Localization | Each entity | Readings, messages, commands | Own-state belief |
| Tracking | Each entity | Readings, messages, own pose | Known contacts |
| Interaction | The world | Every entity's truth | Truth, health, events |
| Network | The world | Senders, receivers, truth | Who gets each message, and when |
| Metrics | The world | Events, messages, truth | Scores |

Entity plugins run in parallel and can change only their own entity; they read
others through read-only snapshots. World plugins run on one thread with
access to every entity, because a collision changes two entities at once.
Plugins never see threads or locks. [Belief vs
truth](belief-vs-truth.md) explains why decisions read belief and never truth.

## Rules the order keeps

- **Each substep runs controllers, then motion.** With `motion_multiplier: 5`,
  every controller runs, then every motion model, five times per tick, so a
  controller sees the state the previous substep wrote. Autonomy and
  everything after motion run once per tick.
- **Sensors see the world after motion.** A reading in tick k describes the
  end of tick k. Estimation turns it into belief at the end of tick k, and
  autonomy uses that belief at the start of tick k+1: the same moment in
  simulated time, so a current reading adds no delay.
- **Spatial queries see current positions.** The index that answers "who is
  within range?" is built after motion and checked again before each
  interaction, networks, and metrics, rebuilt if anything moved.
- **Removal happens last.** An entity destroyed in tick k still has its
  messages delivered and counted in tick k's metrics.
- **Rates skip, they don't reorder.** A plugin with `loop_rate_hz` below the
  tick rate keeps its previous outputs on the ticks it skips. Estimators run
  every tick.

Each parallel phase costs a hand-off to the worker pool, about 35 µs with
eight workers on bare metal. Light missions run fastest on one worker; many
sensors or heavy models are where more workers pay off.

## Messages

Within an entity, autonomy passes commands to controllers and controllers to
the motion model through named ports, and sensors hand readings straight to
the entity's estimators. Everything else is a message: between entities,
between plugins that aren't wired together, and to and from world plugins.

```text
 plugin ── publish(network, topic, value) ──▶ its own outbox
 plugin ── subscribe::<T>(network, topic) ──▶ its own subscription
 plugin ── receive::<T>(network, topic)   ◀── its own inbox
```

Every subscriber has its own queue, so one plugin reading a message never
removes it for another. In the network phase, each network decides for every
message and subscriber whether to drop it or deliver it after a delay:

| Network | Delivers to |
| --- | --- |
| `GlobalNetwork` | Every subscriber; added to every mission that doesn't list it |
| `LocalNetwork` | Subscribers on the sender's own entity |
| `SphereNetwork` | Subscribers within range of the sender, with optional loss |

When a message is read depends on when the reader runs. Estimators and
metrics run after delivery, so they read it in the tick it arrives; autonomy
and controllers run before, so they read it next tick. A delayed message
arrives in a later tick, never an earlier one. Delivery order follows entity
and plugin order, never worker timing.

Queues have fixed capacities: 1,024 unsent messages per entity plugin, and
65,536 per world plugin, per subscriber's unread queue, and per network's
pending deliveries. Going over stops the run with an error naming the plugin.

## Start and end

```text
Simulation::new
  build the starting entities in a setup pass (tick -1)
  run the interactions once, so starting overlaps are settled
  take one perfect measurement of the whole population
  each entity: estimators initialize, then controllers and autonomy
step()   runs one tick and returns its frame
...
step()   after an end condition: closes every plugin once and returns
         the final frame
step()   returns nothing; the run is over
```

A run ends at its end time, when an interaction or metrics plugin returns `Update::Stop` (the tick
still finishes), when no entities remain, or when one team is left, as the
mission's end conditions choose. A plugin error stops the run, and the message
names the plugin. Every plugin is closed exactly once, whether its entity was
removed, the run ended, or it failed.

From Rust, the same loop is:

```rust,ignore
let mission = Mission::load(path, root, &Params::new())?;
let mut simulation =
    Simulation::new(mission.scenario, &PluginRegistry::with_builtins(), workers)?;
while let Some(frame) = simulation.step()? {
    inspect(frame);
}
```

## Randomness

Every random number is keyed by what it is for: the seed, the entity, the
plugin, the tick, and the plugin's own key (such as a contact and an axis).
A number doesn't depend on how many were drawn before it, so two runs on one
seed give the same noise on the same measurement, even when the two runs
measure different things around it. See [Randomness and seeds](randomness.md).
