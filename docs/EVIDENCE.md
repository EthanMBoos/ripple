# Verification evidence

Run checks from the repository root. Results below are historical observations,
not fresh verification of the current tree. Generated reports live under ignored
`runs/`; the commands and limits here are enough to repeat the checks.

## Routine checks

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 -m unittest discover -s reference -p 'test_*.py'
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Focused suites are executable specifications; use `cargo test --locked -p
scrimmage-core --test <target>` for core integration targets below.

| Suite | What it checks |
| --- | --- |
| [plugin_contracts](../crates/core/tests/plugin_contracts.rs) | Seven plugin roles, registration errors, independent subscribers, phase visibility, delay/loss, stop/failure/removal and cleanup; [perception/communication fixtures](../crates/core/tests/support/perception_communication.rs) inspect measured contacts and auction histories. |
| [simulation_regression](../crates/core/tests/simulation_regression.rs) | All 14 curated XML missions: byte-identical frames, events, and summaries at 1/2/8 workers. |
| [noisy_state](../crates/core/tests/noisy_state.rs) | Owned belief, unchanged truth during sensing, next-tick feedback, and held estimates between sensor updates. Equation/RNG tests also live beside the sensor implementations. |
| [world_models](../crates/core/tests/world_models.rs), [navigation](../crates/core/tests/navigation.rs) | Boundary messages, ground removal/team scoring, shared route replacement, and final positions. |
| [yaml_missions](../crates/core/tests/yaml_missions.rs) | Four paired XML/YAML missions produce identical setup and outputs. Parser unit tests cover templates, overrides, defaults and invalid input. |
| CLI [run_output](../crates/cli/tests/run_output.rs) | Every shipped XML runs, recording on/off preserves simulation outputs, failed runs retain diagnostics, invalid missions allocate no output. Run with `-p scrimmage-rs --test run_output`. |
| CLI [sweep](../crates/cli/tests/sweep.rs) | Whole/sharded results agree, bad paths fail early, and the stock executable runs the starter plugin. Run with `-p scrimmage-rs --test sweep`. |
| [Starter](../crates/starter/tests/follow_nearest.rs) | FollowNearest catches its target; run `cargo test --locked -p starter`. |

Additional recorded checks on 2026-09-24/25:

- NoisyContacts measurements and lossy auction histories matched at 1/2/8 workers
  for two seeds. Different seeds/loss probabilities changed observations/delivery.
  No fresh C++ payload/event comparison was performed for these additions.
- Local bulk runs with 3 and 10 shards reproduced the eight-case waypoint sweep's
  complete `results.jsonl`; collection rejected missing/mismatched shards.
  The starter's four-case sweep also ran through the unchanged bulk launcher.
  Slurm submission is mock-tested; an actual cluster run remains open.

To repeat the bulk check after building the release binary:

```sh
cargo build --release --locked -p scrimmage-rs
python3 scripts/bulk_run.py local missions/waypoints-point-agents.sweep.yaml --jobs 3
```

## C++ comparisons

Read [reference/README.md](../reference/README.md) for Docker setup, the current
matrix, comparator tolerances, and report layout. The checker builds the committed
local `Ubuntu-24.04` branch headlessly; it does not patch simulator behavior.

```sh
python3 reference/reference_check.py
python3 reference/reference_check.py --mission missions/verification/noisy-state-bias.xml
python3 reference/reference_check.py --mission missions/noisy-state.xml
```

Inspect each case's `comparison.json`, plus `worker_equality` and
`recording_equality` in `report.json`. Source/image/binary provenance is recorded
with the run. Keep failure reports and diagnose each mismatch separately.

Historical Docker results, 2026-09-24, against C++ commit
`4bdf41fb06facaea358d478d9a07aa4d77fcac27`:

| Mission | Recorded C++ comparison | Maximum position error |
| --- | --- | --- |
| straight-no-gui | Pass | 0 m |
| straight_cpu | Pass | 0 m |
| straight_cpu_mul | Pass; retains the upstream multiplier typo | 0 m |
| aircraft-substeps-spawning | Spawn RNG mismatch | 8.41933 m |
| noisy-state-bias | Pass, 31 frames | 0 m |
| noisy-state | Independent sensor RNG mismatch | 5.35637 m |
| networks-local-global | Pass in the baseline batch, 301 frames | ≤ 4.28e-14 m |

The NoisyState batch and subsequent world-model baseline each had six cases;
all passed Rust worker/recording equality. The current default checker has
expanded to eight cases, including FixedWing6DOF and Multirotor. The initial
Multirotor comparison and fixture details are recorded in
[reference/README.md](../reference/README.md#default-comparison-matrix).

Current interpretation of these results:

- **Spawn RNG is an unresolved compatibility failure.** Ubuntu's engine/normal
  sampler differs from Rust's Apple-libc++ adapter; see
  [RNG findings](REFERENCE_NOTES.md#unresolved-spawn-rng-mismatch).
- **Stochastic sensor sequences intentionally differ.** Zero-standard-deviation,
  nonzero-bias fixtures check retained equations/feedback independently of RNG.
- **The old networks-local-global summary pass predates the survivor-flight-time
  correction.** Frames should still match; survivor `flight_time` and
  `flight_time_norm` now intentionally differ. Both summaries should report one
  ground collision on team 3. See
  [intentional differences](REFERENCE_NOTES.md#intentional-differences-from-c).
- Direct C++ event-stream and sensor-payload comparison remain absent. Rust-native
  waypoint tests do not claim C++ MotorSchemas equivalence. Native macOS passes
  do not establish Linux RNG parity.

## Visual checks

Generate a new recording and follow the [Viewer MCP procedure](../AGENTS.md#visual-verification-with-viewer-mcp):

```sh
cargo run --locked -p scrimmage-rs --bin scrimmage -- \
  run missions/networks-local-global.xml --workers 8 --headless
```

The 2026-09-24 review used Rerun 0.36.2 at 1920x1200, with checkpoints 0, 0.3,
0.6, 10, and 29.9 s. Map framing, labels, status, and ground removal were correct.
The nearly constant-speed plot had unreadable autoscaling/spikes despite a
20 m/s status value; recheck that issue before claiming full dashboard QA.
Artifacts were saved under `runs/visual-qa/section2/`.

Waypoint demos had numerical/recording checks but no completed screenshot review;
native wall-clock looping was not rechecked in that batch. A successful recording
or viewer launch alone is not visual verification.
