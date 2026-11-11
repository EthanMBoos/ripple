# Checking Ripple's engine

This is an experiment with [Creusot](https://guide.creusot.rs/)'s formal verification.
The question is whether we can prove useful behavior of the Rust engine
without making it hard to read. No Ripple code is formally verified yet.

Prove the code that missions run. A small refactor is fine if the engine
calls the proved function and mission outputs stay byte-identical. A
separate model of the engine does not establish a claim about Ripple.

## Behaviors worth checking

The ranking is about value to someone using Ripple, not proof difficulty.
Paths are relative to `crates/core/src/`.

| Rank | Engine behavior | Code |
| --- | --- | --- |
| 1 | Autonomy and controllers see belief; sensors and motion get their declared truth access. Estimators see only their selected readings. | `plugin.rs`, `entity/plugin_stack.rs` |
| 2 | Each phase finishes before the next. Controllers precede motion; sensing follows motion; localization precedes tracking. An error stops later phases after launched work joins. | `simulation.rs`, `simulation/scheduler.rs` |
| 3 | Publications reach only subscribers of the right network, topic, and type. Each subscriber has its own queue. | `pubsub/messages.rs` |
| 4 | Delayed messages arrive at the first routing phase at or after their due time, even if the publisher is silent. Removed receivers get nothing. | `pubsub/messages.rs` |
| 5 | Spawns receive unique IDs, and removal never reuses an old ID. | `simulation/generation.rs` |
| 6 | Missing or mismatched ports and ambiguous estimator sources fail when the mission loads. | `ports.rs`, `entity/plugin_stack.rs` |
| 7 | Sensor readings reach only their bound estimator inputs; `fresh` and `latest` keep their stated meanings. | `plugin/sensor/readings.rs`, `entity/plugin_stack.rs` |
| 8 | Plugins initialize, step, and close in order. Rate-limited steps receive the right elapsed time. | `entity.rs`, `entity/plugin_stack.rs` |
| 9 | A keyed random draw depends on its seed, purpose, tick, and IDs, not unrelated draws. | `common/random.rs`, `common/plugin_random.rs` |
| 10 | Removed entities receive no further steps or messages; removal is recorded at the stated point in the tick. | `simulation.rs`, `entity.rs`, `pubsub/messages.rs` |

## The trial

Start with message delivery (3 and 4). State a precise contract for
`Messages::receive` and the routing path: who gets a publication, when,
and what receiving from one mailbox leaves in another. Try to prove it
in the running code. If type-erased payloads or collection operations
block Creusot, identify the exact boundary. An extracted function counts
only if routing calls it.

Next try state access (1 and 7): show how a mission's selected reading
reaches an estimator and how autonomy receives belief. Keep the explicit
perfect-measurement option in the contract. Phase order (2) is valuable,
but Rayon's join behavior may need an assumption that the Rust proof does
not establish. Record that boundary rather than claiming the whole loop
is proved. Use the remaining rows as follow-on targets if these work.

For each result, hand back the property, the production functions checked,
the assumptions, the proof command and result, and what remains unchecked.
Also report time spent, lines of annotations, and any engine changes.
That lets us decide whether the guarantee is worth its cost before the
paper release. A proof of a helper alone earns only a helper-level claim;
its caller must establish the preconditions.

Keep behavior changes separate from proof work. Run
`reference/equivalence.py OLD NEW` for a readability refactor and retain
focused tests for concrete examples. If the work is kept, proofs can run
in manually triggered CI, not on every push. A checked workspace plugin
would be a useful later demonstration, once the engine boundary it uses
is understood.
