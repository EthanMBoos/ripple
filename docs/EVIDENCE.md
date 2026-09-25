# Verification evidence

This is the commit-ready verification guide. Tests, mission inputs, and comparison
tools live in the repository; generated recordings and reports belong under ignored
`runs/`. You do not need somebody else's run directory to repeat these checks.

Run commands from the `scrimmage-rs` repository root. Recorded results below describe
the named batch, not a claim that every later change or the whole port is verified.

## Typed plugin parameters (2026-09-24)

Every plugin's parameters became a serde struct whose `Default` replaced the
bundled `PluginName.xml` defaults, and unknown keys became errors. This is a
readability change: simulation behavior must not move.

- All 14 curated missions, at 1 and 8 workers, produced byte-identical
  `frames.bin`, `events.json`, and `summary.csv` before and after.
- Each old manifest recorded the XML-derived parameters. For every plugin in
  every mission, all 716 of those values equal the new manifest's
  `effective_plugin_params`, which confirms the hand-written `Default` impls.
  NoisyPosition, used by no mission, was checked against its deleted XML.
- [params.rs](../crates/core/src/parse/params.rs) tests typed parsing, error
  messages that name the key, and unknown-key rejection;
  [mission.rs](../crates/core/src/parse/mission.rs) tests that a misspelled
  mission key fails to resolve.
- Review fixes, rechecked against the same byte-identical outputs: `u8`/`u16`/
  `i8`/`i16` fields parse; `f32` fields reject values that overflow to infinity;
  and FixedWing6DOF takes either `inertia_matrix_slug_ft_sq` or SI
  `inertia_matrix` (not both), falling back to the C++ slug default. Before this
  fix, the compiled slug default silently overrode an SI matrix.

## NoisyContacts and SphereNetwork + AuctionAssign (2026-09-24)

These are the only accepted additions from the plugin audit. Repeat the focused
checks with:

```sh
cargo test --locked -p scrimmage-core noisy_contacts
cargo test --locked -p scrimmage-core sphere_network
cargo test --locked -p scrimmage-core --test plugin_contracts perception_communication
cargo test --locked -p scrimmage-core --test simulation_regression
cargo test --locked -p scrimmage-rs --test run_output every_shipped_mission
```

Coverage:

- NoisyContacts equation tests cover bias, Gaussian moments, interleaved draw
  order, body-axis attitude errors, preserved angular velocity, 5I covariance,
  and invalid parameters. A public test autonomy drives from the delivered
  measurement: target x=50 with +2 m bias commands 52 m/s on the next tick,
  while own belief remains unchanged by the sensor.
- [Perception/communication contracts](../crates/core/tests/support/perception_communication.rs)
  check self exclusion, local subscriber isolation, scheduled targets appearing,
  empty snapshots after removal, an out-of-range agent excluded from the auction,
  highest-received-bid selection, one result after the strict deadline, and a
  short auction closing without bids and ignoring later arrivals.
- SphereNetwork tests cover strict 3D range, changed positions, inclusive
  altitude-plane epsilon bounds, same-entity geometry bypass, unreachable world
  endpoints, invalid configuration, and rejected legacy delay modes. Complete
  loss also suppresses same-entity auction traffic.
- Actual noisy measurements and lossy auction histories match at 1/2/8 workers
  for two seeds. Changed seed and transmission probability change observations
  and/or deliveries. Independent subscribers see the same loss-free result.
- All fourteen shipped missions have identical frame bytes, events, and summaries
  at 1/2/8 workers. CLI regression checks also compare those artifacts with
  headless Rerun recording on/off at 8 workers, including the two new missions.

Recorded on this Mac: `cargo test --workspace --locked --offline` passed **122
Rust tests** (including 18 public plugin contracts); `cargo clippy --workspace
--all-targets --locked --offline -- -D warnings` passed. The scheduler, state,
and delivery rules were not changed. Agent and network contexts gained each
plugin's mission-seeded random stream; sensor streams are unchanged, and all 14
shipped missions produced byte-identical frames, events, and summaries before
and after that change.

These are source-reviewed Rust ports with deliberate differences documented in
[REFERENCE_NOTES.md](REFERENCE_NOTES.md) and [RUST_PLUGINS.md](RUST_PLUGINS.md#noisycontacts).
No fresh C++ execution or direct C++ message/event comparison was performed for
this batch; stochastic streams and post-motion network geometry deliberately
differ. Stock frames/events do not record the new payloads, which is why the
public consumer tests inspect their actual values. Recording equality is not
visual QA, and no dashboard changes or visual verification are claimed.

## NoisyState, owned belief, and Straight's LocalNetwork consumption

### Check the Rust behavior

Requires Rust/Cargo. These checks do not need Docker or a viewer:

```sh
cargo test --locked -p scrimmage-core noisy_state
cargo test --locked -p scrimmage-core --test noisy_state
cargo test --locked -p scrimmage-core local_messages_override_belief
cargo test --locked -p scrimmage-core installing_and_clearing_belief
cargo test --locked -p scrimmage-core --test simulation_regression
```

What to inspect:

| Source / tests | What they establish |
| --- | --- |
| [NoisyState](../crates/core/src/plugin/sensor/noisy_state/noisy_state.rs) | Bias, Gaussian moments, seeded draw order, body-axis rotation order, invalid parameters, and the legacy payload's identity covariance/zero angular velocity. |
| [Belief integration tests](../crates/core/tests/noisy_state.rs) | Sensing does not directly change truth; biased altitude affects the next PID update; slower sensors hold belief while truth advances; no sensor retains ideal feedback. |
| [Straight tests](../crates/core/src/plugin/autonomy/straight/straight.rs) | Newest local message takes precedence over belief, does not reach another entity, and is not reused when no new message arrives. |
| [Sensor context tests](../crates/core/src/sensor.rs) | Installing/clearing an owned belief does not mutate truth. |
| [Mission regression tests](../crates/core/tests/simulation_regression.rs) | Identical frame bytes, events, and summaries at 1, 2, and 8 workers, including the sensor missions. |

Important distinction: NoisyState both updates belief directly and publishes a
copy on LocalNetwork, as C++ does. The PID reads belief; Straight prefers a new
message and otherwise reads belief. Dropping a network message does not suppress
the separate belief update. Explicit `contacts_truth` remains available to legacy
autonomies; this is not a claim that they cannot access perfect world information.

### Compare deterministic behavior with C++

Requires a running Docker engine and sibling `../scrimmage` with the local
`Ubuntu-24.04` branch. See [reference setup](../reference/README.md). The script
builds the headless reference in Docker; do not run the C++ GUI on this Mac.

```sh
python3 reference/reference_check.py \
  --mission missions/verification/noisy-state-bias.xml
```

The script prints its new `runs/reference-checkNNN` directory. Look at:

- `report.json`: overall `passed`, each case's `worker_equality` and
  `recording_equality`. Every artifact comparison should be true for this fixture.
- `00-noisy-state-bias/comparison.json`: matching frame counts, zero mismatches,
  and errors within the comparator's unchanged tolerances.
- `00-noisy-state-bias/cpp/logs/` and `rust-1/`: actual frames and team summaries.
- `source.json` and the provenance in `report.json`: C++ source, image/compiler
  details, and Rust source/binary hashes used for this particular check.

This fixture deliberately uses zero standard deviations and nonzero position,
velocity, and attitude means. It checks the equations and closed-loop feedback
without conflating them with random-number sequence compatibility. Do not change
the stochastic missions into deterministic ones to hide a failed comparison.

### Check and understand stochastic differences

```sh
python3 reference/reference_check.py --mission missions/noisy-state.xml
python3 reference/reference_check.py
```

The first command compares stochastic sensor behavior. Rust uses independent
per-sensor streams keyed by seed/entity/instance, rather than C++'s shared generator.
Sample-for-sample C++ comparison is expected to fail; Rust worker and recording
equality should still pass. Noise-distribution checks are in the Rust tests above.

The second command runs the default reference matrix. Its randomized spawning
fixture has a separate known Ubuntu/libstdc++ RNG compatibility gap. Preserve
the failed reports and inspect which checks failed; an expected RNG mismatch
does not excuse another regression. Neither command is currently an all-green
release gate. Do not loosen tolerances or alter the reference simulator.

### Recorded result: 2026-09-24 NoisyState batch

Reference source was Ubuntu-24.04 commit
`4bdf41fb06facaea358d478d9a07aa4d77fcac27` (evidence, not a project pin).

| Mission | C++ result | Maximum position error |
| --- | --- | --- |
| straight-no-gui | Pass | 0 m |
| straight_cpu | Pass | 0 m |
| straight_cpu_mul | Pass | 0 m |
| aircraft-substeps-spawning | Known spawn RNG mismatch | 8.41933 m |
| noisy-state-bias | Pass, 31 frames | 0 m |
| noisy-state | Independent sensor RNG mismatch | 5.35637 m |

All six cases had byte-identical Rust frames, events, and summaries at 1/2/8
workers and with headless recording on/off. The four preexisting missions also
matched before/after Rust outputs. The workspace had 73 passing Rust tests;
rustfmt and Clippy passed. The Python tooling had 22 passing tests.

These results do not establish full C++ parity, direct C++ sensor-payload/event
stream equivalence, or visual quality. Covariance/angular-velocity payload behavior
was source-reviewed and unit-tested. The subsequent baseline batch is recorded below.

## Aircraft/world/navigation baseline

### Repeat the focused checks

```sh
cargo test --locked -p scrimmage-core --test world_models
cargo test --locked -p scrimmage-core --test navigation
cargo test --locked -p scrimmage-core --test plugin_contracts
cargo test --locked -p scrimmage-core supported_options
cargo test --locked -p scrimmage-core pubsub::messages
cargo test --locked -p scrimmage-core single_integrator
cargo test --locked -p scrimmage-core simple_aircraft
cargo test --locked -p scrimmage-core --test simulation_regression
cargo test --locked -p scrimmage-rs --test run_output
```

What these establish:

- `world_models`: GlobalNetwork Boundary messages change both aircraft paths;
  the low aircraft hits ground, is removed, and contributes one ground collision
  to its team's metrics. A team-filter control suppresses that collision.
- `navigation`: both point agents accept the shared replacement and stop at its
  final `(-10,20,0)` meter waypoint. Suppressing the update makes them stop at
  `(10,10,0)` instead. Both aircraft paths respond to the shared route replacement.
- `plugin_contracts`: all seven extension types; independent subscribers;
  same/final-tick metrics; explicit delay/loss; complete-tick stop; initialization
  and step failure cleanup; removal and close-once behavior.
- `pubsub::messages`: bounded publisher/subscriber/network queues, descriptive
  overflow errors, independent subscribers, and discarded delayed delivery after
  receiver removal. No claim of legacy delay-policy compatibility.
- `supported_options`: meaningful excluded/unported options fail explicitly.
  Equation tests cover aircraft signs, limits and operation order, PID gains,
  and SingleIntegrator velocity/position propagation and speed normalization.
- `simulation_regression`: all ten retained missions give identical frame bytes,
  events, and summaries with 1/2/8 workers.
- `run_output`: every XML under `missions/` runs successfully and gives identical
  simulation outputs with recording on/off; each recording is nonempty. These
  temporary test outputs are removed automatically. This does not test visible UI.

### Reproduce the C++ comparison

```sh
python3 reference/reference_check.py --mission missions/networks-local-global.xml
python3 reference/reference_check.py
```

For the combined mission, inspect `comparison.json` for 301 matching frames and
no frame mismatches, and `rust-1/events.json` for entity 3's `GroundCollision`
and `EntityRemoved` at 0.4 s. Its summary comparison is now expected to fail on
`flight_time`/`flight_time_norm` for the surviving teams 1 and 2: Rust
intentionally credits survivors until the end of the run (see
[REFERENCE_NOTES.md](REFERENCE_NOTES.md#intentional-differences-from-c)). Both
summaries should still report one ground collision on team 3. Boundary is not a physical wall: aircraft turn after
crossing it. Direct C++ event-stream comparison is not implemented.

The full default matrix has six cases and currently exits **1**, solely because
of the known spawning RNG mismatch. Check each case; do not treat every failure
as expected. The Rust-native waypoint demos use analytical/control tests, not
claimed C++ MotorSchemas equivalence.

### Recorded result: 2026-09-24 baseline batch

The fresh Docker matrix (`reference-check004` locally) used the same C++ source
commit listed above, without simulator patches or tolerance changes:

| Mission | C++ result | Maximum position error |
| --- | --- | --- |
| straight-no-gui | Pass | 0 m |
| straight_cpu | Pass | 0 m |
| straight_cpu_mul | Pass | 0 m |
| aircraft-substeps-spawning | Known spawn RNG mismatch | 8.41933 m |
| noisy-state-bias | Pass | 0 m |
| networks-local-global | Pass, 301 frames | 4.28e-14 m or less |

All six Docker cases passed Rust worker/recording equality. All ten shipped
missions passed those checks in the Rust suites. Five preexisting fixtures were
also byte-identical to their preceding Rust baseline (frames, events, summaries).
The workspace passed 90 tests, rustfmt, and strict Clippy. Python tooling passed
22 tests. An earlier attempt rejected the legacy visual-only `show_text_label`
option and failed the CPU missions; the corrected implementation keeps Rerun's
labels and preserves those original inputs. The failed attempt was not overwritten.

NoisyState's stochastic C++ comparison was not rerun in this second batch;
its worker/recording checks were. The earlier stochastic evidence remains above.
See [model limits](MODEL_SCOPE.md) and [runtime contract](RUST_PLUGINS.md#execution-and-lifecycle).

### Visual inspection and limits

For a fresh recording, run:

```sh
cargo run --locked -p scrimmage-rs --bin scrimmage -- \
  run missions/networks-local-global.xml --workers 8 --headless
```

Follow [AGENTS.md](../AGENTS.md#visual-verification-with-viewer-mcp) to inspect
the resulting recording at 1920x1200. Check replay times 0, 0.3, 0.6, 10, and
29.9 seconds (select the final sample for the terminal frame). The third marker
and status row should disappear after ground contact, while both surviving
aircraft turn and remain within the full-run map framing. ENU meters, replay
time, and the separate legacy frame label should be readable.

These five screenshots were captured and inspected with Rerun 0.36.2 in a
dedicated headless viewer. Map framing, labels, status, and removal were correct.
The nearly constant-speed plot is **not** visually acceptable: autoscaling shows
vertical spikes and an unreadable value scale despite the status table reporting
20 m/s. Investigate plot scaling; do not mistake this for verified speed dynamics
or claim full dashboard QA. Boundary drawing remains deliberately unsupported.
The waypoint demos have numerical/recording evidence, not a completed screenshot
review. Native wall-clock playback wrapping was not rechecked in this batch.

Screenshots and notes are local artifacts under `runs/visual-qa/section2/`.
The commands and findings here remain available without that ignored directory.
The 60 removed unsupported mission copies are recoverable locally from
`runs/mission-cleanup-v1/missions-before-pruning.tar.gz`; upstream originals remain
in the C++ checkout. No unsupported demos are needed to run this curated suite.

## Whole-workspace checks

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 -m unittest discover -s reference -p 'test_*.py'
```

For a visual inspection, generate a fresh recording without launching a viewer:

```sh
cargo run --locked -p scrimmage-rs --bin scrimmage -- \
  run missions/noisy-state.xml --workers 8 --headless
```

Replay the run directory printed by that command with `scrimmage replay` (or the
same `cargo run ... -- replay` prefix). Follow the screenshot workflow in
[AGENTS.md](../AGENTS.md) for a visual verification claim. A completed recording
or successful replay launch alone is not visual QA.
