# Estimation, measurements, and belief

Plan revised 2026-09-27. This is proposed work, not the current API.
Related: RQ1 in the paper, [TODO.md](TODO.md),
[reference findings](REFERENCE_NOTES.md), and the current
[plugin lifecycle](../book/src/reference/plugin-api.md#execution-and-lifecycle).

The objective is to give estimation one clear owner and a defined place in the
SCRIMMAGE tick. Sensors produce measurements; an estimator maintains belief;
autonomy and controllers use that belief. Developers should be able to replace
one of these pieces without rewriting the others.

## What C++ already provides, and what we are changing

C++ already distinguishes truth from belief and allows plugins to update belief.
`NoisyState` supplies noisy state feedback. `SimpleINS` accumulates synthetic
position error and responds to GPS-fix status. `ROSIMUSensor` simulates inertial
measurement errors. A sensor can reasonably represent a complete navigation
instrument, including its internal estimation.

The missing agreement is who owns the entity's canonical belief, which inputs
feed it, and when it becomes available. C++ leaves those choices to plugin
code. Its XML can share parameter blocks, but does not establish that estimation
contract. A developer can implement the desired behavior, at the cost of wiring
and coordinating the pieces for each experiment.

The Rust port currently retains several of those choices:

- `NoisyState` writes belief directly and publishes a state message. `Straight`
  can use that message instead of its context state.
- `PluginStack::belief` is optional. Without it, decisions read truth.
- Decision contexts expose truth contacts. Only `FollowNearest` in
  `crates/starter` actually reads them; stock autonomy and controller plugins
  pass an empty list only in their tests. Removing the field is a small
  migration.
- Sensors run in mission order; the last sensor that actually writes belief
  wins. Different update rates make selecting the last configured sensor an
  incorrect replacement for that behavior.
- YAML already names sensor instances, and Rust rejects unknown parameters.

We will replace the implicit choices with an explicit Estimator role. This is
an intentional API change. The other seven plugin responsibilities remain.

## The ownership rule

| Responsibility | Owner |
| --- | --- |
| Advance the physical world | Motion models and world interactions |
| Produce measurements and apply sensor errors | Sensors |
| Maintain the entity's own state (localization) | One localization estimator per entity |
| Maintain the contacts the entity knows about (tracking) | One tracking estimator per entity |
| Choose actions and predict candidate futures | Autonomy and controllers |
| Compare belief and predictions against truth | Evaluation and output code |

Autonomy and controllers receive belief only, including during initialization.
There is no optional observation-only mode, direct truth setting, or fallback
when an estimate is absent. Remove truth contacts from decision contexts too.
Sensors and simulation infrastructure still access truth for their jobs.
Estimator contexts receive declared measurements, delivered messages, known
control inputs where needed, and time; they do not receive a general truth handle.

Sensors lose `set_belief` and `clear_belief`. `Straight` reads the canonical
belief rather than letting a raw state message override it. A sensor may still
represent a complete navigation instrument; its state output then feeds a
pass-through estimator.

Belief has two parts, each with one owner. **Localization** answers "where am
I?": it turns navigation readings (GPS, IMU) and the issued commands into the
entity's own state. **Tracking** answers "what is around me, and where?": it
turns contact readings (radar, camera) into a map of known contacts, with
sample times and availability. They are separate because their inputs,
outputs, and research questions differ, and an experiment should be able to
swap a tracker without owning the localizer. Tracking runs after localization
and receives its result, because relative readings such as radar range and
bearing need the entity's own estimated pose to become world positions. The
first implementation accepts only readings taken this tick, which it combines
with the pose localization produced for this tick. An older relative reading
needs the observer's pose at the reading's sample time; supporting that is
deferred.
Sophisticated tracking is deferred, but removing truth-contact access is part
of the first migration. Known mission information, such as a destination,
remains distinct from hidden world state.

## Measurements, prediction, and correction

An estimator may simply copy a complete state measurement. A predictive
estimator instead advances its previous belief using known inputs and its own
model, then corrects that prediction using available measurements.

These are separate sources of error:

- **Physical response:** what the motion model actually does with a command.
- **Model mismatch:** how the estimator assumes that response works.
- **Measurement error:** what a sensor adds to its reading.

Configure each once at its owner. The actual and assumed response are purposely
independent experimental inputs. Do not inject accumulated estimation error into
physics as a second disturbance. The controller's response to an incorrect
belief can itself change the physical trajectory.

The estimator may use declared measurement uncertainty, but must not receive
hidden error realizations or unknown physical parameters automatically. A known
calibration or initial prior must be explicit. Prediction components used by a
planner can share equations and robot-side assumptions with the estimator;
their hypothetical states must remain separate from physical truth.

### The perfect default

Simple missions use perfect, current state and contact measurements through a
pass-through estimator. The loader may supply this default setup and show it in
the resolved configuration. Decision plugins use exactly the same belief API as
in sensing experiments; there is no separate `Truth` estimator with privileged
access.

Its timing, equality guarantee, and startup rules are part of the
[measurement and lifecycle contract](#measurement-and-lifecycle-contract).

Zero measurement noise alone does not imply exact belief for every estimator.
A wrong prediction model or an old measurement can still produce error. Do not
silently replace a predictive estimator with perfect state copying when its
noise settings reach zero.

## Fit into the existing tick

Preserve the order and global boundaries of the existing physical/control phases:

```text
generate
  -> autonomy
  -> all controller substeps
  -> all motion substeps
  -> sensors
  -> interactions
  -> networks and local observation delivery
  -> estimation: localization, then tracking   [new logical step]
  -> metrics, removal, output, time advancement
```

The estimator predicts over the interval just completed, using the known
control inputs issued for that interval, then processes available measurements
once. It publishes belief for the next decision. These inputs are the commands
the robot knows it issued, not privileged knowledge of an imperfect actuator's
actual response.

Concretely, the known control inputs are the values the motion model actually
read on each motion substep, with their substep times. Today all controller
substeps finish before motion begins, so every motion substep reads the final
controller output; when an entity has no controller, the motion model reads the
autonomy's outputs directly, and those are what is recorded. They are not the
autonomy's desired state (unless it feeds the motion model directly) and not
the motion model's internal response. The first experiment uses only the speed
command, but the interface carries every declared motion-model input from the
start.

A measurement delivered at the end of tick k can affect the next decision in
tick k+1. A delayed message cannot affect an earlier belief. Define measurement,
delivery, and belief times explicitly. Retained readings must not be mistaken
for new measurements every tick.

Investigate running estimation in the existing entity-local observation-delivery
task. A new plugin category does not necessarily require another worker barrier.
Choose execution mechanics after establishing the timing contract, and measure
the actual overhead. Do not interleave controller and motion substeps.

## Measurement and lifecycle contract

When readings are taken, what belief is guaranteed to contain, and how belief
starts and ends.

**When readings are taken.**

- Ordinary sensors sample in the sensor phase, after motion and before
  interactions, as real sensors would. Their readings do not reflect that
  tick's collisions or other interaction effects.
- The perfect default samples the post-interaction snapshot, the one networks
  and metrics already read. Its readings therefore include that tick's
  interaction effects.

**What the perfect default guarantees.** Belief equals truth, in value, at the
next decision for every entity present in the post-interaction snapshot and not
marked for removal there. Two consequences:

- An entity that spawns at the start of tick k+1 is unknown to others until
  their next measurement, one tick later. No sensor has seen it yet. In the
  current code only `FollowNearest` (in `crates/starter`) reads contacts; check
  the RQ2 workflow result before claiming no paper result changes.
- An entity marked for removal in tick k is left out of the perfect default's
  readings. Perfect knowledge includes knowing it is gone.

**Removal is not a tracking rule.** Ordinary trackers receive no pruning from
engine state. A tracker keeps or drops a contact that stops appearing in its
readings by its own logic, as a real one would, so belief can legitimately hold
a stale contact.

**Startup.**

1. Build the whole initial population and run the startup interactions (as the
   engine already does once before the first tick).
2. Take one snapshot of that population.
3. Initialize every entity's measurements and belief from it: own state and,
   for the perfect default, contacts. This is a batch step; initializing one
   entity at a time would give the first decisions empty or
   construction-order-dependent contacts (today each entity's sensors are
   initialized with an empty contact list while the population is still being
   built).
4. Only then initialize decision plugins, which see belief from their first
   call.

An entity that spawns mid-run initializes its own state and, for the perfect
default, its contacts from the snapshot at its spawn; other entities learn about
it at their next measurement. Missing inputs get an explicit waiting/invalid
policy or a configuration error, never a fallback to truth.

**Finalization.** The terminal frame records the last committed belief without
advancing the estimators again.

## Plugin and mission shape

Add an extensible `Estimator` category through the public registry, with two
roles: a localization estimator writes the entity's own state, and a tracking
estimator writes its known contacts. Each entity has one of each. Concrete
built-ins live under `plugin/estimator/<name>/<name>.rs`. Keep student
implementations ordinary structs with typed configuration and a small update
method.

Start with a pass-through for each role: localization copies a named
complete-state measurement, and tracking copies named contact measurements.
Require explicit source selection when several eligible sources exist. Check missing names and incompatible measurement types before
execution. Measurements carry their sensor identity and sample time.

Use the existing YAML plugin-map style and named sensor instances. Estimator
inputs should name their sources, for example
`localization: {plugin: PassThrough, inputs: {state: navigation}}` and
`tracking: {plugin: PassThrough, inputs: {contacts: radar}}`, rather than
relying on sensor order. This is proposed syntax; finalize it with the
first implementation. Preserve the existing templates and separate `.sweep.yaml`
files. A universal sensor-profile or error-model configuration language is not
part of this milestone.

XML remains supported for existing single-run missions through the same typed
scenario. New configuration goes into YAML and the Rust scenario API. Existing
missions must resolve to explicit internal measurement/estimator choices; do not
silently pick a source in ambiguous cases. Check paired XML/YAML missions together.

## First experiment: prediction error without measurement noise

Use a simple straight-line robot with a known initial position. Hold its speed
command constant so feedback does not obscure the first result.

| Setting | Value |
| --- | --- |
| Requested speed | 10 m/s |
| Actual motion response | 0.8 times the requested speed: 8 m/s |
| Estimator's assumed response | Compare gains 0.8 and 1.0 |
| Position measurements | Exact initial fix, then one every 5 seconds |
| Measurement noise | Zero initially |

The motion model advances position using the actual gain. The estimator advances
position from the command and assumed gain. On a position fix it replaces its
predicted position with the measured position. This deliberately simple
correction does not learn the response gain or claim to correct every state error.

With the wrong gain, predicted position is 10 metres ahead immediately before
the fix at 5 seconds. The fix removes that position error, and it accumulates
again. With the correct gain and matching integration, the position prediction
remains accurate to numerical tolerance. Define fix times exactly for this test;
do not confuse the small error on the preceding tick with the pre-correction
prediction at the fix time.

Record truth, prediction before correction, and belief after correction at
matching times. Recording only corrected position at fix times would hide the
effect. Then add noisy fixes, and finally let a position-seeking controller act
on belief to demonstrate the complete feedback loop.

Keep this research experiment in a workspace research crate such as
`crates/starter`, registered with the shared CLI. Its simple response model and
predictor do not need to become stock plugins immediately.

## Implementation sequence and checks

1. **Capture the baseline.** Retain existing correctness outputs and run
   `reference/perf.py` before changing the belief path.
2. **Introduce the ownership contract.** Add the Estimator registry/interface
   with its localization and tracking roles, perfect measurements and
   pass-through behavior for both, named inputs, belief-only
   decision contexts, and the defined update/initialization order. Migrate
   built-ins and the starter. Update `AGENTS.md` and book guidance when the API
   lands; the copied C++ guides stay unchanged.
3. **Check the migration before adding prediction.** Test ideal belief equality
   (including after collisions and removals), batch startup (every entity's
   first decision sees the whole initial population), mid-run spawns, slow
   measurements, multiple named sources, contact isolation, and
   delivery-to-decision timing. Rerun the RQ2 workflow example and confirm its
   result. Recheck existing missions and performance.
4. **Implement the small experiment with evaluation output.** Verify the correct
   and incorrect gain cases, the pre/post-correction position errors, and that
   sensor/estimator updates never directly modify physical state. Introduce a
   noisy-reading case separately, then the closed-loop controller case.
5. **Document results and choose the next extension.** Record actual evidence in
   the manuscript and retained paper data once tested. RQ1 capability descriptions
   must distinguish implemented behavior from remaining proposals.

Require identical Rust frames, events, and summaries at 1, 2, and 8 workers and
with recording enabled/disabled. Also compare estimator outputs across worker
counts; matching physical frames alone would miss errors in an unused belief.
Keep sensor RNG streams tied to stable entity/sensor identities.

Evaluation may inspect truth. Estimators may not. Compute position error at a
common epoch, distinguish unavailable/stale estimates, and retain final error
for removed entities. Add only the series and summary metrics the experiment
needs; full raw-measurement logging is not required for every mission.

## C++ comparisons and performance

Adding estimation does not prevent benchmarking against C++. It adds work to
the Rust implementation, which must remain included in the measured cost.

- **Matched baseline:** use the same mission, information available to decision
  code, recording settings, and physical workload. Verify outputs before treating
  timing ratios as implementation comparisons.
- **Noisy-state comparisons:** assess initialization, sample rates, source
  selection, and `Straight`'s old message path explicitly. A pass-through
  estimator alone does not prove every old mission remains equivalent.
- **Intentional changes:** document changed semantics and preserve the old
  evidence with its tested revision. Keep unaffected comparisons passing; do not
  loosen tolerances or silently relabel changed results as old parity.
- **New experiments:** benchmark their Rust cost separately. A fair C++ comparison
  requires an equivalent C++ workload; the existence of `SimpleINS` does not make
  it equivalent to this command-based predictor.

Use `reference_check.py` for affected comparisons and `perf.py` before/after the
migration. Run the full campaign before refreshing paper evidence. Do not combine
unrelated physics corrections with the initial belief-path change. The objective
is measured, explained overhead, not a promise of zero cost or universal parity.

## Later work

IMU/GPS integration, richer filters, learned environmental estimates, delayed
measurements, reusable sensor profiles, and richer tracking (association,
filtering) can follow a working
baseline. None is required to demonstrate the ownership and timing contracts.

Before an IMU uses current aircraft outputs, resolve missing acceleration and
inconsistent reported velocities/rates in the relevant motion models. Define
frames, gravity handling, units, sampling, and initialization, and test supported
models explicitly. Those corrections are separate from introducing Estimator.
