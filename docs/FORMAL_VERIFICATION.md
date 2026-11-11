# Checking Ripple's engine

This is an experiment with [Creusot](https://guide.creusot.rs/)'s formal verification.
The question is whether we can prove useful behavior of the Rust engine
without making it hard to read. The first trial proves the mailbox
`receive` operation under explicit library assumptions; routing and the
simulation loop remain unverified. See the trial result below.

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


## Receive trial result (2026-11-11)

**Proved, with the assumptions below:** the production
`Messages::receive` method in `crates/core/src/pubsub/messages.rs`.
This is a narrow result within item 3, not a proof of message routing,
delivery timing, or the engine as a whole.

### Contract

For any mailbox, requested network/topic, and payload type:

- A missing subscription, or one whose `TypeId` differs from the requested
  type, returns an error without changing the mailbox.
- With a matching subscription, receiving removes exactly the selected
  channel's queue. Other channel queues remain unchanged.
- A successful result contains every queued message in order, with the
  same sender, send time, delivery time, and downcast payload.
- An error with a matching subscription requires a payload that cannot be
  downcast. The selected queue is still removed, including messages before
  and after the bad payload. This is the existing behavior.
- Subscriptions, outgoing publications, and the mailbox time do not change.

These are postconditions on the method's return, not a liveness guarantee.
They do not establish that routing populated the queue correctly, that
payloads lack interior mutation, or that Rust allocation and destructors
have no effects outside this model. Error text is outside the proof; the
bad-payload error text is checked by a unit test.

### Production code and proof boundary

The production change replaces iterator `map`/`collect` with an explicit
loop and moves the two error constructors into private functions. It keeps
error messages and queue behavior. No Creusot dependency or annotations are
added to the normal workspace.

`reference/creusot/build.rs` parses the current production file and extracts
the actual method body, the five supporting structs, and three type aliases.
It adds six postconditions, one loop invariant, a ghost queue snapshot, and
one checked assertion identifying the bad payload on the error path.
The assertion is proved, not assumed. Derived implementations and unrelated
methods are omitted. Inspect the generated annotated source at
`reference/creusot/target/receive.generated.rs`.

The proof does **not** mark `receive` as trusted. Its trusted boundary is:

| Assumption | Location |
| --- | --- |
| Map lookup and removal obey their finite-map specifications. | `reference/creusot/receive.rs` |
| `TypeId::of` identifies the requested type; `Arc::downcast` follows the specified abstract cast result. | `reference/creusot/receive.rs` |
| Standard string, vector, iterator, and equality models describe their Rust operations. | Creusot's support library, plus `reference/creusot/std_models.rs` |
| The two error constructors return errors without changing a mailbox. | Trusted annotations on their extracted production bodies |
| The compiler, Creusot translation, Why3, and successful SMT prover answers are sound for this model. | Verification toolchain |

Creusot 0.14 lacks the required map and type-ID models. The runner copies
its pinned support library into ignored `target/creusot-std-0.14.0`, adds
those models, and exposes the existing string deep-model definitions.
It changes neither the Cargo registry copy nor the simulator's dependencies.
The cast model describes successful downcasts but does not independently
prove Rust's runtime type system or the implementation of `Arc`.

### Repeating the check

Install [Creusot](https://guide.creusot.rs/) 0.14.0 with its supported Rust
nightly (this trial used `nightly-2026-08-03`), Why3, why3find, and provers.
Then, from the repository root:

```sh
python3 reference/creusot/check.py --mutations
```

The runner prepares the local support library, verifies production, tests
faults in temporary source copies, and verifies production again. It writes
logs, generated source, proof records, timings, and source hashes under a
new `target/creusot-checks/receive-*` directory. It never edits production
for a mutation. Why3 needs permission to open local sockets.

Both deliberate faults reached verification and were rejected: removing
another channel's queue, and reversing the returned messages. They did not
fail merely because of compilation errors or missing library contracts.
The restored production method passed. Retained results are in
`paper/data/verification/receive-proof.json`; the earlier unsuccessful
capability probes remain in `creusot-trial.txt` as historical evidence.

This machine's trial installation uses tool sources under `/tmp`; install
those tools in persistent locations before deleting that directory or
relying on the setup long term.

### Runtime validation

Workspace build, tests, formatting, and Clippy passed, as did the standalone
proof crate's Clippy check and 81 reference-script tests. Two focused mailbox
tests exercise wrong-subscription, ordered delivery, other-queue isolation,
and a deliberately corrupted payload.

Before/after equivalence passed on all 40 cases at 1, 2, and 8 workers,
with zero differing outputs. Of those cases, 28 include observation traces.
The reader initially rejected recipe-spawned aircraft because it tried to
interpret their group
IDs using the static scenario's entity list. The checker now compares the
encoded channels directly, preserving its byte comparisons, scenario and
result-metadata checks, and worker counts. A regression fixture covers this
reader case. This changes the comparison reader, not simulation output.

The full 16-hub regional day now uses the checker's untraced mode, as large
generated workloads already do. Its full contact trace produced a 1.3 GB
partial recording before the comparison was stopped. All five normal output
channels remain compared for that mission at 1, 2, and 8 workers. Full trace
comparison remains enabled for the checked one-hub day and smaller shipped
missions, plus a generated contact-sensing case. This trial does not claim
full-day trace equivalence for the 16-hub mission. The stopped recording was
preserved, and the completed first 25 comparisons were reused when resuming
with the remaining cases in a fresh output directory.

### Cost and remaining work

The proof support is 340 lines across source extraction, contracts/models,
and the runner, excluding Cargo metadata and generated files. The contract
itself is small; most effort went into unsupported library operations and
connecting their models. Engineering time was not separately recorded;
individual verification timings are retained with the results.

The runtime method remains readable, but the proof harness and local support
library extension are a maintenance cost. This is a useful feasibility
result for a focused function. It is not yet evidence that annotating the
whole engine will be economical. Before expanding, consider upstreaming
these library models and using this example to review the contract with
another reader.

**Planned:** routing eligibility, queue population, delayed delivery, and
state-access contracts. Phase-order verification also remains Planned.
Do not describe any of those as proved by this trial.
