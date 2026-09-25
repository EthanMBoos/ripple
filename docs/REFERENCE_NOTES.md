# Reference architecture and porting constraints

Reviewed the twelve documents in `../scrimmage/development-docs` on
2026-09-23. These notes distinguish useful architectural guidance from behavior
verified against SCRIMMAGE's `Ubuntu-24.04` branch. That review used commit
`4bdf41fb06facaea358d478d9a07aa4d77fcac27`; it is historical evidence, not a
required commit pin. The port follows the branch in the sibling `../scrimmage` repo.

## Selected contact and communication additions (2026-09-24)

The accepted follow-up to the [catalog audit](PLUGIN_COVERAGE_AUDIT.md) is only
NoisyContacts and SphereNetwork + AuctionAssign. Other proposals are not backlog.
The implementations use the existing delivery phases. Agent and network contexts
now carry each plugin's mission-seeded random stream, as sensor contexts already
did. See
[Rust plugin contracts](RUST_PLUGINS.md#noisycontacts) and [evidence](EVIDENCE.md).

Source-verified behavior: NoisyContacts copies angular velocity and uses 5I
covariance; attitude errors right-multiply roll, pitch, yaw. SphereNetwork's
range is strict (`RTree.cpp` uses `< dist`), while altitude-plane epsilon bounds
are inclusive. Same-parent delivery bypasses geometry but still draws for loss.
AuctionAssign starts from entity 1, includes self-bidding, retains the first
maximum on ties, and closes strictly after its deadline.

Rust differences: measured contacts are sorted typed vectors; each plugin
instance draws from its own mission-seeded RNG stream rather than a shared
generator; range checks use current post-motion snapshots rather than
C++'s earlier R-tree; AuctionAssign's network and duration are configurable, and
its result uses `Option<AuctionBid>` instead of negative sentinels. NoisyContacts
does not enforce truth isolation on downstream autonomy. Custom messages are
not automatically archived by stock outputs. There is no claim of C++ random
sequence, full mission compatibility, or fresh C++ payload/event parity for
these additions. No reference source or comparison tolerance was changed.

## Authority and scope

The 2026-09-24 [scope review and roadmap](TODO.md) supersedes older blanket
full-port commitments: compiled Rust extensions stay; runtime plugin loading,
legacy protobuf/string-map spawning, and the OpenCL path are excluded. Integration
candidates are ROS (including ROS 1 and ROS 2) and ArduPilot. JSBSim is limited
to offline reference fixtures for selected Rust flight models; its production
plugin/integration is excluded. Burn and later YAML/templates are directions
requiring focused implementation/validation.
The source findings and historical comparison evidence below remain applicable
to retained behavior; missing C++ mechanisms are not automatically obligations.

The user's requested functionality and `RUST_STYlE.md` govern this port. The
reference branch's source and headless mission results determine compatibility.
Synthesized documents provide a map and hypotheses to check, not a replacement
for those sources.

New comparisons use the [Docker reference workflow](../reference/README.md)
and the upstream Ubuntu slim dependency image. Native macOS results below are
historical checks, not claims of Ubuntu parity. The Rust legacy RNG currently
matches Apple libc++; Linux standard-library behavior must be verified separately.

The greenfield audit targets a different commit. Its proposed scope and phase
order are not decisions for this port; current intentional breaks come from
the user's scope review in TODO.md. Retain the selected XML behavior and staged
execution contracts. Rerun replaces VTK; CPU worker scheduling must be safe and
deterministic. No C++ GUI mission is required to run on this Mac.

## Reading map

All links below point to the original, user-maintained documents; this review
does not modify them.

| Documents | Guidance carried into the port |
| --- | --- |
| [Architecture](../../scrimmage/development-docs/ARCHITECTURE.md), [data flow](../../scrimmage/development-docs/DATA_FLOW.md) | Entity lifecycle; autonomy, controller, motion, sensor, interaction, network, and metrics responsibilities; separate state, command, message, and service paths. |
| [Mission configuration](../../scrimmage/development-docs/MISSION_CONFIG.md), [quick reference](../../scrimmage/development-docs/QUICK_REFERENCE.md) | Mission/default resolution, reusable templates, named control channels, environment paths, and user-facing workflows. |
| [Plugin development](../../scrimmage/development-docs/PLUGIN_DEVELOPMENT.md), [external projects](../../scrimmage/development-docs/EXTERNAL_PROJECTS.md) | Preserve extensibility and overlay workflows through Rust interfaces; do not mistake built-in mission support for a complete plugin system. |
| [Threading proposal](../../scrimmage/development-docs/THREADING_SIMPLIFICATION.md), [performance analysis](../../scrimmage/development-docs/PERFORMANCE_ANALYSIS.md) | Phase completion, failure handling, deterministic runs, and separate simulation throughput from rendering and wall-clock pacing. Performance estimates are not measured baselines. |
| [Plugin metadata proposal](../../scrimmage/development-docs/PLUGIN_METADATA_REFACTOR.md) | Keep plugin identity, execution order, source context, and validation diagnostics distinct. Its breaking API/protobuf changes were not present in the reviewed source. |
| [Greenfield audit](../../scrimmage/development-docs/GREENFIELD_RUST_SIMULATOR_AUDIT.md), [study guide](../../scrimmage/development-docs/ROBOTICS_SIMULATOR_LANDSCAPE_AND_STUDY_GUIDE.md) | Validate before constructing runtime state; make truth/observation/belief boundaries explicit; test visibility, timing, invalid input, and determinism. Their research plans and reduced product scope are not adopted. |
| [Container guide](../../scrimmage/development-docs/CONTAINERIZED_DEVELOPMENT.md) | Packaging and platform context; new reference checks use headless Ubuntu Docker. |

## Source-verified timing and dataflow

In `src/simcontrol/SimControl.cpp`, `run_single_step` and `run_entities` establish:

1. Generate entities, run simulator callbacks, and log pre-step contacts.
2. Handle pause/readiness and install autonomy contacts.
3. Run the autonomy phase across entities.
4. Run **all controller substeps**, then **all motion substeps**.
5. Run sensors, interactions, networks, then metrics.
6. Remove inactive entities, send visual updates, pace, advance time, and test
   termination.

Controller and motion substeps are not interleaved. A sensor update occurs
after that tick's autonomy. Network delivery is not universally delayed until
the next tick: metrics callbacks run after networks and can consume messages
within the same tick. The source explicitly documents this final-tick need.

`run_logging` labels pre-step state with `t + dt`; finalization logs again after
decrementing time, giving a repeated final timestamp with terminal state.
Preserve these semantics in compatibility recordings and comparison tests;
do not silently replace them with a cleaner-looking timeline.

Other checked distinctions:

- `src/plugins/network/LocalNetwork/LocalNetwork.cpp::is_reachable` requires
  the same non-null parent entity. Range-based connectivity is a separate
  network implementation, not LocalNetwork.
- `src/plugins/interaction/SimpleCollision/SimpleCollision.cpp` uses configured
  `collision_range`, with strict distance comparison, not summed visual/entity
  radii. Startup rejection has separate rules.
- `src/entity/Entity.cpp` initially aliases truth and belief and detaches belief
  on `set_state_belief`. Preserve observable plugin behavior without reproducing
  shared mutable pointers. Restrict new sensor/estimator interfaces explicitly;
  do not claim legacy autonomies already have enforced truth isolation.
- `include/scrimmage/parse/MissionParse.h` still contains `entity_attributes()`.
  Do not apply the metadata proposal's removal or altered GenerateEntity wire
  contract to this reference by assumption.

## Rust execution and validation rules

`ScenarioConfig -> ResolvedScenario -> Simulation` is the validation boundary.
The resolved type must not be publicly constructible without validation.
Named, unit-bearing physical state and commands belong in the core; protobuf
options and file handling stop at the boundary.

Workers own disjoint mutable entity state within a phase. A phase must join
every task, including when another task fails or panics, before returning.
Cross-entity reads and structural mutations need documented snapshot/commit
barriers as richer plugins arrive. Rayon alone does not prove those semantics.
Never move a whole entity's autonomy/controller/motion chain into one parallel
task and thereby remove the reference's global phase boundaries.

The current legacy RNG adapter preserves the reference's native generator and
coordinator draw order for spawn compatibility. It must never become a shared
worker RNG. New sensor randomness follows the style guide's stable engineering
identities. Porting a legacy stochastic plugin will require an explicit stream
mapping and parity evidence; changing its draw order silently is not acceptable.

Rerun is an output adapter, not an owner of simulation time or state. Recording
and visualization settings must not alter physics, random draws, or events.

## Intentional differences from C++

- **SimpleCollisionMetrics survivor flight time.** C++ publishes
  `EntityPresentAtEnd` in `finalize()` but never runs another network step, so
  its metrics never see it. Surviving entities are then credited only until the
  latest *removal* time. Rust credits an entity with no recorded end until the
  report time. Frames and events are unchanged; `summary.csv` differs only in
  missions where an entity is removed (currently `networks-local-global.xml`,
  where survivors now report 29.9 s instead of 0.4 s).
- **FixedWing6DOF inertia units.** C++ prefers `inertia_matrix_slug_ft_sq`
  whenever it is present, and its bundled defaults file always sets it, so a
  mission's SI `inertia_matrix` is silently ignored. Rust applies either key and
  rejects both together; with neither, it uses the C++ slug default. No curated
  mission sets either key.

## Current evidence and remaining work

The 2026-09-24 NoisyState batch adds a real legacy sensor, owned detachable
belief, and Straight's LocalNetwork state subscription. The new deterministic
bias/attitude mission passes against the unmodified Ubuntu Docker reference
with zero measured position error. All six checked missions are byte-identical
across Rust worker counts and recording on/off. The old spawn RNG failure remains;
the stochastic NoisyState demo also differs because Rust deliberately uses
independent per-sensor random streams. See
[the verification guide](EVIDENCE.md) for repeatable commands and recorded results.
No new visual QA or direct C++ observation/event-stream comparison is claimed.
The older catalog/evidence below describes the pre-NoisyState baseline.

All seven public plugin types now compile through the registry: Autonomy,
Controller, MotionModel, Sensor, Interaction, Network, and Metrics. Collision
and scoring are ordinary plugins, not simulator special cases. The public-API
plugin contract suite exercises all seven without engine edits and tests multiple
metric subscribers, custom delay/loss, phase visibility, failure, and shutdown.

Concrete built-ins cover Straight, SimpleAircraft, its PID controller,
SimpleCollision, SimpleCollisionMetrics, immediate LocalNetwork/GlobalNetwork,
and the illustrative NoisyPosition sensor. The latter is not a NoisyState port.

Native C++ frame and summary comparisons passed on 2026-09-23 (local date) for:

| Mission | Frames | Maximum position error |
| --- | ---: | ---: |
| straight-no-gui.xml | 489 | 1.18e-13 m |
| straight_cpu.xml | 2001 | 1.14e-13 m |
| straight_cpu_mul.xml | 2001 | 1.14e-13 m |
| verification/aircraft-substeps-spawning.xml | 31 | 1.59e-14 m |

The subsequent headless Ubuntu Docker check built the unpatched branch with
GCC 13.3 and ran all four missions. Straight and both CPU missions passed,
with zero measured position error. The randomized spawning fixture failed:
31 frames and 90 states were compared, with 90 state mismatches and maximum
position error 8.41934 m. It diverges at initialization, not worker scheduling.
The image's libstdc++ uses `minstd_rand0` (multiplier 16807) and returns the
second polar normal variate first; the current Rust adapter uses multiplier
48271 and returns the first variate first. This confirms an RNG compatibility
gap, not a reason to relax tolerances. All four missions retained byte-identical
Rust frames, events, and summaries at 1/2/8 workers and with recording on/off.
See the [verification guide](EVIDENCE.md) to reproduce the Docker check and inspect
its machine-readable `report.json`. This is a partial parity result, not an all-passing baseline.

After the core source reorganization, fresh Rust runs again passed against the
same native C++ baselines: `runs/compare-layout-complete-*.json`. Their frames,
events, and summaries are byte-identical to the pre-move `runs/final-seven-*`
Rust outputs. The C++ baseline was reused, not rerun for this structural check.
See [SOURCE_LAYOUT.md](SOURCE_LAYOUT.md) for the new navigation map.

After colocating each built-in's Rust implementation and XML defaults, all four
missions again produced byte-identical Rust frames, events, and summaries
before/after the move and passed against the saved native C++ baselines. The
[plugin-layout report](../runs/plugin-layout-qa/REPORT.md) records the checks,
new defaults/override regression tests, and recovery archive for the removed
reference copies. Public plugin imports remained unchanged.

The separate example application was subsequently removed. Its nine tests and
minimal model fixtures were retained in the core's public-API integration suite,
`crates/core/tests/plugin_contracts.rs`. The intended later application/core
boundary is documented in [LIBRARY_FIRST_REFACTOR.md](LIBRARY_FIRST_REFACTOR.md);
it is not a claim that the refactor is implemented.

Earlier local artifacts are `runs/compare-final-seven-*.json`. The CPU multiplier mission's
original `motion_multipler` typo is preserved; the separate verification
fixture actually exercises five substeps. Quaternion differences are below
6e-16 rad and summaries match at the comparator's six-decimal tolerance.
Direct C++ event-stream comparison remains absent.

Regression tests compare Rust frame bytes, events, and summaries exactly
across 1, 2, and 8 workers for those four missions. Scheduler tests cover
errors, panic, and empty phases. Plugin tests cover all seven categories,
same-entity/global routing, independent queues, silent delayed delivery,
invalid message types/delays, world plugin failure, and lifecycle cleanup.

The Rerun adapter records position, quaternion, heading arrows, team/ID labels,
and speed. `rerun rrd verify` accepted the headless straight recording.
That earlier smoke test used viewer 0.36.2 with SDK components 0.36.3 and
printed a newer-stream warning. Visual QA subsequently aligned the lockfile
with viewer 0.36.2; fresh recordings validate without that warning. See
the [QA report](../runs/visual-qa/simulation-first/REPORT.md)
for the later screenshot, looping, and viewer-on/off checks and their limits.

The native Rerun viewer was also launched for the 30-step substep fixture;
the live-streaming run completed and its recording passed verification.
Frames, events, and summaries were byte-identical with Rerun enabled and
disabled. This is a launch/transport smoke test, not a visual-quality review.

Remaining areas, subject to the current TODO scope and acceptance choices:

- Prioritized motion/controller/autonomy/sensor/interaction/metric/network models
  and selected optional integrations, not a blanket copy of every dependency.
- Services, runtime parameters, readiness, and callbacks where retained models
  actually require them. Owned detachable belief is implemented.
- Legacy monitoring and delay semantics. Rust queues now use fixed capacities
  and fail-on-overflow, not legacy queue-policy compatibility. Built-in networks
  reject nonnegative `comm_delay` and stochastic delay rather than silently
  reinterpret them. Custom networks support explicit delay and loss.
- Broader lifecycle semantics beyond the documented complete-tick stop/removal
  contract. Generic runtime library loading and legacy spawning protobufs are excluded;
  legacy frame protobuf I/O remains useful for comparisons.
- Terrain, visual meshes, arbitrary plugin shapes, wall-clock pacing, and
  Rerun/live simulation controls.
- Ubuntu/libstdc++ RNG compatibility and wider Docker reference coverage.

The copied C++ guides remain unchanged. [RUST_PLUGINS.md](RUST_PLUGINS.md)
describes the implemented authoring workflow and its explicit gaps. Passing
these aircraft missions does not establish whole-repository parity.
