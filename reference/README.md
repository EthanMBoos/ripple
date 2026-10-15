# Checks and release evidence

This folder holds the checks for Ripple: Rust tests and
determinism, behavioral agreement with C++ SCRIMMAGE built in Docker, speed and
memory, and focused studies. One runner, `suite.py`, runs them by ID.

## Quick start

```sh
python3 reference/suite.py --list        # every check: purpose, reference, pass criterion
python3 reference/suite.py               # quick set after a change, about 30 minutes
python3 reference/suite.py --release     # every check before a release, several hours
python3 reference/suite.py --check cpp-agreement    # one check (repeatable)
```

Each run writes `runs/suiteNNN/<check>/` with a log per step and
`summary.json` last; earlier runs are never reused, and a failed check keeps
its diagnostics there. The suite prints the C++ agreement numbers before
(from the release evidence) and after ("what moved"), and exits non-zero if any
check failed. Every script also runs on its own; its docstring says how.

## Checks

| ID | Set | What it checks | Against | Passes when |
| --- | --- | --- | --- | --- |
| `rust-tests` | quick | `cargo test`, `fmt`, `clippy`, and these tools' tests | expected values | all pass |
| `rust-determinism` | quick | 1, 2, 8 workers, recording, tracing, scan and grid lookup (`determinism.py`) | the one-worker run | byte-identical on every mission |
| `cpp-agreement` | quick | the curated matrix ([missions.txt](missions.txt), `reference_check.py`) | C++ in Docker | see [Criteria](#criteria) |
| `speed-regression` | quick | Rust speed and memory (`perf.py`), first, on an idle machine | the accepted baseline | no workload slower beyond noise |
| `cpp-generated` | release | 100 random scenarios, seed 1 | C++ | all pass |
| `cpp-long` | release | ten 1,000 s scenarios, seed 7, and `divergence.py` | C++; nudged Rust | runs clean, summaries explained |
| `cpp-noise` | release | NoisyContacts noise, sample for sample and by distribution (`noise_statistics.py`) | C++ samples | the case passes; no KS test below the threshold |
| `checker-sensitivity` | release | 12 planted porting bugs (`sensitivity.py`) | `cpp-agreement`'s C++ runs | every bug caught |
| `rust-platform` | release | Linux/amd64 against native (`platform_check.py`) | the native build | runs; differences reported |
| `speed-cpp-container` | release | C++ and Rust in one Linux container (`benchmark.py`) | C++ comparison branch | Rust deterministic, matches C++ where comparable |
| `speed-cpp-native` | release | the same on bare-metal macOS (`native_benchmark.py`) | native C++ | as above |
| `speed-spatial` | release | collision cost, scan against grids (`spatial_benchmark.py`) | the scan | identical output |
| `speed-messages` | release | routing cost with all-agent messages (`message_benchmark.py`) | native C++ | runs |
| `rk4-loads` | release | forces at every RK4 stage against held forces (`rk4_loads.py`) | unmodified C++ | runs |
| `multirotor-control` | quick | MultirotorVelocityController step responses and step-size convergence (`multirotor_control.py`) | the commands; Förster's drag; 20 substeps | whole velocity error within 5%; drag tilt within 2%; within 5 mm |
| `simple-aircraft-fit` | quick | SimpleAircraft's `rascal` limits flown against FixedWing6DOF's Rascal (`simple_aircraft_fit.py`) | the Rascal under AircraftPIDController | every number within 5% (or 0.2) |
| `rascal-control` | quick | AircraftPIDController's `rascal` settings: altitude D after upsets, steps under each attitude limit (`rascal_control.py`) | the commands; the `a4d` limits | D changes error under 1 cm; steps settle, pitch under 25 degrees |
| `rotors-motor` | release | Multirotor motor lag and rotor loads (`rotors_motor.py`) | RotorS's motor-plugin source in Docker | every sample within 1e-12 relative |
| `crazyflow-crazyflie` | release | Multirotor's Crazyflie against Crazyflow's dynamics (`crazyflow_check.py`) | Crazyflow, pinned, via uv | no-drag case within 1e-6 m |
| `stall-landing` | release | stall tables in ArduPlane's Rascal landing (`stall_landing.py`) | the mission without tables | the repeat is byte-identical |

A check that needs another runs it first (`checker-sensitivity` needs
`cpp-agreement`). The native checks need macOS and the arm64 C++ image that
`speed-cpp-container` builds; `native_build.sh` compiles C++ on the host on
first use. `stall-landing` builds `ripple` with `--features ardupilot` and runs
ArduPlane SITL in Docker.

## Release evidence

`paper/data/<check>/` holds each check's compact evidence: reports, summaries,
input missions, and timings, with local paths replaced by `<checkout>` and
`<run>`. Frames, traces, and recordings stay in the run folder.
`paper/data/release.json` records, per check, its purpose, reference, pass
criterion, outcome, date, and the hash of the source that ran (a hash of every
source file, `project.source_identity`, so no Git history is needed), plus
the external reference versions (C++ commits and images, the Rust compiler).

```sh
python3 reference/suite.py --release --retain   # refresh all evidence
python3 reference/suite.py --check rk4-loads --retain
```

`--retain` changes the evidence only when every selected check passed. It
stages the new evidence, swaps it in for the checks that ran, updates
`release.json`, and regenerates the readable summary of the evidence,
`paper/data/SUMMARY.md` (`paper/data/summarize.py`). Review the diff and the
paper's prose before committing. Failures are never retained: fix the cause, or record a known gap
in `docs/REFERENCE_NOTES.md`; do not change seeds, tolerances, or the
reference branches to pass.

## Performance baseline

`perf.py` times six workloads (`motion`, `substeps`, `sensing`, `churn`,
`collision`, and `collision-grid`, the same collision mission with a 5 m
spatial grid) natively with the release core-only runner
(`benchmark_runner.rs`, no viewer), at two agent counts and 1 and 8 workers: a
warm-up, then five timed runs. It reports the median stepping time with its
min-max range, and peak memory.

```sh
python3 reference/perf.py           # compare with reference/perf_baseline.json
python3 reference/perf.py --quick   # smallest sizes only, under a minute
python3 reference/perf.py --accept  # make this measurement the baseline
```

The baseline is one committed record: the workloads (a hash of their
definitions), the machine, the Rust compiler, and each workload's times. A
workload is SLOWER when its median lies outside the baseline's range and is
more than 10% higher (identical code drifted up to 6% between sessions); any
slower workload fails `speed-regression`. A baseline
from another machine or toolchain, or for changed workloads, is not compared at
all: the check fails with "no compatible baseline" until someone reviews the
numbers and runs `--accept` on an idle machine. A slowdown is never accepted
automatically.

## Refactors and renames

A change that should not change behavior must leave every output byte for byte
unchanged:

```sh
python3 reference/equivalence.py OLD_EXECUTABLE NEW_EXECUTABLE
```

Build the old executable from any trusted source (for example an exported copy
of the previous release) and the new one from this checkout. Every mission,
perf.py's workloads, and a message-heavy mission run at 1, 2, and 8 workers with
the comparison trace; frames, events, summaries, and traces must be identical,
and so must manifests except the documented metadata keys (`source`, the
mission's path), each of which is reported. The report goes to
`runs/equivalenceNNN/`; it is development evidence, not release evidence.

Names and paths live in one place, [project.py](project.py): the package and
executable names, Docker image prefix, and the runs and evidence folders, all
resolved from this checkout, so the suite runs the same from any directory name.

## Tools

| Files | Purpose |
| --- | --- |
| `suite.py`, `project.py`, `retain.py` | The runner, names and paths, and evidence copying |
| `reference_check.py` | Compares Rust with C++ on `missions.txt` (or `--mission`, `--mission-dir`) |
| `scrimmage_xml.py`, `scenario.py` | C++'s XML mission from the scenario a Rust run recorded; recorded scenarios back to YAML missions |
| `compare.py`, `frames.py`, `traces.py`, `metrics_model.py` | Compare two saved runs; read frames and traces; model the two scoring rules |
| `determinism.py`, `equivalence.py` | Rust against itself: variants of one build, or two builds |
| `generate_scenarios.py`, `divergence.py`, `sensitivity.py`, `noise_statistics.py`, `platform_check.py` | Release checks described [below](#the-scripts-behind-the-release-checks) |
| `perf.py`, `benchmark.py`, `native_benchmark.py`, `native.py`, `native_build.sh`, `spatial_benchmark.py`, `message_benchmark.py` | Speed and memory |
| `rk4_loads.py`, `stall_landing.py`, `inputs/` | Focused studies and their input missions |
| `rascal_control.py` | The Rascal's controller settings |
| `simple_aircraft_fit.py` | SimpleAircraft's `rascal` limits, fit to and checked against the Rascal |
| `multirotor_control.py`, `rotors_motor.py`, `rotors/`, `crazyflow_check.py`, `crazyflow/` | Multirotor checks; `rotors/` builds RotorS's motor-plugin source with a stand-in for Gazebo; `crazyflow/` runs Crazyflow's dynamics |
| `Dockerfile`, `fixtures/` | The C++ reference build and its command-test plugins |
| `rust.Dockerfile`, `benchmark_runner.rs` | Rust on Linux, for the platform check and benchmarks |
| `tests/` | Tests for these tools, run without Docker |

The C++ checkout defaults to a sibling `../scrimmage`; pass `--source PATH`
otherwise. It needs both branches locally, for example:

```sh
git clone git@github.com:gtri/scrimmage.git ../scrimmage
git -C ../scrimmage fetch origin Ubuntu-24.04:Ubuntu-24.04 benchmarking-edits:benchmarking-edits
```

The checker builds from these committed branches, never from uncommitted files.
Requires Python 3.10+, a running Docker engine, and Rust/Cargo. No native C++
build is needed except for the native timings. The first build downloads a
large dependency image and compiles C++; later checks reuse Docker's build
cache. Allow several GB of disk space.

`reference_check.py` alone writes `runs/reference-checkNNN`. Use `--output
path/to/new-check`, `--source path/to/scrimmage`, or `--jobs 4` to override
locations or compilation parallelism. Existing output directories are never
reused. `--timeout` controls each mission/comparison; `--build-timeout` controls
each Docker/Cargo build. Both are in seconds.

## Two C++ builds

The checker builds two committed branches of `../scrimmage`:

- `Ubuntu-24.04`, the unmodified reference.
- `benchmarking-edits`, the comparison branch created from it. It adds opt-in
  comparison support: `SCRIMMAGE_TRACE=<file>` writes a per-tick trace, and
  `SCRIMMAGE_KEYED_RANDOM=1` makes generation, NoisyState, NoisyContacts, and
  SphereNetwork draw Ripple's keyed random numbers ([reference
  notes](../docs/REFERENCE_NOTES.md#random-numbers-keyed-draws-on-both-sides)).
  It also fixes
  one C++ regression: each substep runs controllers, then motion, as SCRIMMAGE
  did until GPU support split them (GPU motion models are disabled; see
  [reference
  notes](../docs/REFERENCE_NOTES.md#substep-order-a-c-regression-fixed-on-the-comparison-branch)).
  AircraftPIDController uses its given timestep with a lower default altitude
  derivative gain ([reference
  notes](../docs/REFERENCE_NOTES.md#intentional-differences-from-c)). And
  FixedWing6DOF and Multirotor compute forces and moments at every RK4 stage
  instead of once per step ([reference
  notes](../docs/REFERENCE_NOTES.md#rk4-loads-a-c-defect-fixed-on-the-comparison-branch)).
  Multirotor also reads body velocity and rates from the entity state, takes
  rotor moments as r x F along each tilted rotor axis, and keeps idle thrust
  at `omega_min` ([reference
  notes](../docs/REFERENCE_NOTES.md#multirotor-three-c-defects-fixed-on-the-comparison-branch)).
  And it fixes the core defects: time, frame timestamps, spawn sampling,
  rate-limited plugins, NoisyState, SimpleAircraft, and the wind sign
  ([reference
  notes](../docs/REFERENCE_NOTES.md#core-defects-fixed-on-both-sides)).
  Every mission's frame timestamps change, so no mission's output matches the
  reference.

### Criteria

Each mission runs C++ four times. The instrumented build with and without
tracing must write byte-identical `frames.bin` and `summary.csv`
(`cpp_non_interference` in `result.json`); a difference fails the case. The
unmodified build differs by design, and `cpp_baseline_equality` records it.
The fourth run, with tracing and keyed random numbers, is the one compared
with Rust. Rust runs once more with `--trace`, which must
leave its frames, events, and summary byte-identical (`trace_equality`).

Keyed draws make randomized spawns, sensor noise, and network loss the same
numbers on both sides. AuctionAssign's bids stay on C++'s shared engine; bids
are compared by delivery only.

`traces.py` compares the traces (`trace-comparison.json`):

| Record | When | Compared |
| --- | --- | --- |
| `delivery` | A message reaches a subscriber | Per tick, receiver, network, and topic: senders and event entity IDs, in order |
| `publication` (C++), `reading` (Rust) | A sensor measures own state or contacts; C++ publishes it, Rust hands it to estimators | Paired by tick and sensor: contact IDs in order; state within frame tolerances. C++'s deliveries of the publication are skipped and counted |
| `belief` | Each entity when its frame is logged | Within frame tolerances |
| `output` | Each autonomy/controller port after the last substep | Within 1e-8 absolute |

Endpoints are `[entity ID or null, plugin]` using C++ names (sensors carry their
order index, as in `NoisyState0`). Non-finite numbers are written as the strings
`NaN`, `Infinity`, and `-Infinity`. Delayed deliveries are recorded when queued
(`scheduled_delivery`); no matrix mission uses network delay.

## What gets built

The script reads the exact [upstream slim dependency
Dockerfile](https://github.com/gtri/scrimmage/blob/Ubuntu-24.04/ci/dockerfiles/ubuntu-24.04-slim-dependency-only)
from the local branch snapshot and builds it without edits. Despite its name,
that image includes VTK/Qt and other integration dependencies. Its JSBSim setup
downloads amd64 packages, so the workflow explicitly uses `linux/amd64`,
including emulation on Apple Silicon. These packages are installed inside Docker
only.

A second image per branch builds a `git archive` of that local branch. It does
not copy uncommitted changes, modify the checkout, or fetch/update your local
branch. The branch is not pinned in this project; each check records the actual
commit, source archive hash, Dockerfile hashes, image IDs, Linux
package/compiler versions, Rust compiler, and Rust source hashes. The upstream
dependency Dockerfile itself clones GitHub for its setup scripts; that is
distinct from the archived source used for the reference executable.

[Dockerfile](Dockerfile) disables optional VTK, gRPC, OpenCV, ROS, and GPU build
paths and builds the executable plus the eighteen upstream C++ plugins needed by
the current matrix. This is a headless comparison build, not verification of the
full C++ plugin catalog. Model equations are never patched. Two small command
drivers in `fixtures/` are built into both images: `MotorSpeeds` (the old
upstream PID writes a different control interface than Multirotor reads) and
`ConstantVelocity` (no retained upstream autonomy writes a nonzero velocity).
They supply constant commands to unchanged upstream models; neither is an
autopilot. Rust has matching stock test drivers. Fixture source hashes are
recorded alongside the upstream source provenance. `VTK_FOUND=OFF` is set
explicitly because upstream CMake expands that variable inside conditionals even
when VTK discovery is disabled.

Docker image tags/layers remain cached. There is no automatic Docker pruning.
Mission containers run without network access and can write only to their
dedicated host output mount; they are removed after the run.

## Add a C++ plugin to the comparison

1. Port the plugin to Rust and register it (see the book's plugin guide).
2. Add its C++ build target to the `cmake --build` lists in
   [Dockerfile](Dockerfile). If it needs a command source no retained C++ plugin
   provides, add a small driver to [fixtures/](fixtures/) and the fixture loop
   in the Dockerfile, with a matching Rust driver.
3. Write a fixture mission under `missions/verification/` that exercises it with
   no randomness it can't share (zero spawn variance, zero sensor noise), and
   add it to [missions.txt](missions.txt) with a case ID that says what it tests
   and a short label. The Rust regression tests pick up every mission under
   `missions/` automatically.
4. If it publishes a new sensor message whose contents should be compared, add
   the payload to both traces: `src/common/Trace.cpp` on the C++ branch and
   `crates/core/src/simcontrol/trace.rs` here (see `sensor_states` in
   `pubsub/messages.rs`). Commit C++ changes on `benchmarking-edits`;
   the checker builds committed branches only.
5. Optionally add the plugin to `generate_scenarios.py` for random coverage and
   a one-line defect for it to `sensitivity.py`.
6. Run `python3 reference/suite.py`, then `--release`.

## Default comparison matrix

The matrix is [missions.txt](missions.txt):

| Case | Mission | Coverage |
| --- | --- | --- |
| `aircraft-collision` | `straight-no-gui.yaml` | Opposing aircraft, collision and removal |
| `aircraft-long-horizon` | `verification/aircraft-long-horizon.yaml` | Longer single-aircraft run |
| `substeps-random-spawn` | `verification/aircraft-substeps-spawning.yaml` | Actual five-substep execution, rates, randomized state, and scheduled spawning, with keyed spawn draws |
| `substeps-scheduled-spawn` | `verification/aircraft-substeps-scheduled.yaml` | The same substeps and rates with two zero-variance spawn schedules; deterministic on either engine |
| `noisy-state-bias` | `verification/noisy-state-bias.yaml` | NoisyState deterministic bias/attitude equations and belief feedback, without stochastic sequence differences |
| `world-networks` | `networks-local-global.yaml` | Boundary response over GlobalNetwork, local own-state feedback, ground removal and metrics |
| `fixed-wing-a4` | `fixed-wing-6dof.yaml` | Aerodynamic flight, tilted/northbound attitude, nonzero rates and wind |
| `fixed-wing-rascal` | `fixed-wing-6dof-rascal.yaml` | The Rascal's parameters, an upset, and a glide |
| `multirotor` | `multirotor.yaml` | Quad hover, tilted flight, unequal motor speeds and a six-rotor configuration |
| `multirotor-fixes` | `verification/multirotor-fixes.yaml` | The three Multirotor fixes: starting velocity, tilted and raised rotors, idle motors |
| `single-integrator` | `verification/single-integrator.yaml` | SingleIntegrator direct, normalized, zero, and vertical commands from `ConstantVelocity` |
| `noisy-contacts-bias` | `verification/noisy-contacts-bias.yaml` | NoisyContacts biases, attitude order, and covariance through published payloads |
| `sphere-network-auction` | `verification/sphere-network-auction.yaml` | SphereNetwork reach inside, at, and beyond range through AuctionAssign messages on `CommsNetwork` |
| `sphere-network-loss` | `verification/sphere-network-loss.yaml` | SphereNetwork loss at `prob_transmit` 0.5, keyed draw for draw, through the same auction |
| `collision-boundary` | `verification/collision-boundary.yaml` | SimpleCollision strict range at the boundary, and in-run opposing and same-team collisions |

Case folders and reports use the case ID, so a mission file can move without
changing what the evidence calls it. Missions given with `--mission` or
`--mission-dir` use their file name.

Each case runs C++ headless and single-threaded. Rust runs with 1, 2, and 8
workers without Rerun, plus an eight-worker headless Rerun recording.

For just the Multirotor port, run
`python3 reference/reference_check.py --mission missions/multirotor.yaml
--mission missions/verification/multirotor-fixes.yaml` and `cargo test -p
ripple-core multirotor`. `multirotor.yaml` covers the quad open loop and
reaches none of the three Multirotor fixes; `multirotor-fixes.yaml` reaches all
three. The Rust-only motor and rotor effects, the Crazyflie preset, and
MultirotorVelocityController have no C++ counterpart; they are checked against
RotorS, Förster's data, and Crazyflow (`rotors_motor.py`,
`crazyflow_check.py`, `multirotor_control.py`).

- C++/Rust frames and team summaries use `reference/compare.py`. Frames must
  contain the same entity IDs, so spawn and removal ticks are compared.
- Summaries must agree within 1e-6. A differing summary passes only if entity
  and collision counts match exactly and `metrics_model.py`, applied to the Rust
  run's own metrics deliveries, reproduces the C++ summary under the C++ scoring
  rule and the Rust summary under Rust's documented rule (`summary_agreement` in
  `result.json`). No column is masked.
- Entities C++ destroys in its pre-start collision pass keep stepping for one
  tick and are reported removed twice (a C++ defect); `traces.py` excludes only
  those entities' first-tick records and reports the count.
- Rust frames, events, and summaries must be byte-identical across worker counts
  and with recording enabled/disabled. The recording must be nonempty.
- Traces compare the message stream, sensor payloads, beliefs, and plugin
  outputs as above. Headless recording is not a native-viewer playback or
  screenshot test; use `AGENTS.md` for visual QA.

`--mission path/to/mission.yaml` can be repeated to replace the default matrix.
Additional missions must use plugins included in both builds, the default
estimators, and no terrain. Each case saves its inputs in `input/`: the YAML
mission (`input.yaml`), the C++ mission written from the scenario Ripple's run
recorded (`input.xml`, by [scrimmage_xml.py](scrimmage_xml.py)), and that
mission with the headless/logging overrides (`effective.xml`), all recorded.
It does not change physics, seeds, rates, or parameter defaults.

The build also includes NoisyState. Its deterministic bias/attitude fixture can
be checked with `--mission missions/verification/noisy-state-bias.yaml`. Noisy
missions draw keyed random numbers on both sides, so they compare sample for
sample too; keep any stochastic mismatch report rather than loosening a
tolerance.

## Compare saved runs

Comparison is development tooling, not a simulator subcommand. It needs only
Python 3.10+ (standard library); no Docker, Cargo, Rerun, or Python packages:

```sh
python3 reference/compare.py path/to/cpp-run path/to/rust-run \
  --report runs/comparison.json
```

Both directories must contain `frames.bin` and `summary.csv`. JSON is printed
to stdout; `--report` also saves it, including mismatches, to a new file. Parent
directories are created automatically; existing reports are never overwritten.
Exit codes are 0 for a match, 1 for a mismatch, and 2 for invalid inputs or I/O
errors. Invalid inputs do not produce a comparison report.

Tolerances are set from measured agreement, not inherited: position 1e-8 m,
velocity 1e-8 m/s, orientation 1e-9 rad, angular velocity 1e-9 rad/s, timestamp
1e-9 s, and summary values 1e-6 (C++ prints six decimals). The largest measured
differences are 1.4e-10 m, 9.6e-11 m/s, 2.2e-12 rad, and 8.9e-12 rad/s. The
original comparator used 0.01 m and 1e-3, which let a sub-micrometre spawn
defect through (`sensitivity.py`). Frames are paired by index, retaining
repeated terminal timestamps; entities and summary rows are matched by ID.
Quaternion signs are equivalent. Malformed/nonfinite state is rejected.
`frames.py` decodes the length-delimited proto3 frame subset from the reference
branch's schemas; it is not a general protobuf library. `traces.py` compares
event streams separately.

## Results and failure handling

The command exits zero only when every requested case passes. `report.json`
contains the overall result and per-mission checks; case directories retain
`comparison.json`, input missions, C++ logs, Rust variants, hashes, and
recordings. Build logs and source/image provenance are kept at the check root. A
failed mission does not prevent the remaining cases from being checked.

The unmodified Linux build draws spawns from libstdc++'s default engine
(`minstd_rand0`, multiplier 16807, which also returns the other polar-method
variate first), so it diverges on randomized spawning; the suite records that
comparison and expects it to fail. The comparison run uses keyed draws instead
of changing seeds or tolerances. The zero-variance twin
`verification/aircraft-substeps-scheduled.yaml` stays in the matrix as a check
that does not rely on them.

Recorded outcomes are the release evidence under `paper/data/`, described in
the [paper README](../paper/README.md).

## The scripts behind the release checks

- `generate_scenarios.py` writes seeded random missions from retained models;
  run them with `reference_check.py --mission-dir DIR`. `--horizon 1000` makes
  long runs (fixed-wing capped at 100 s).
- `divergence.py CHECK --output DIR` explains long-run divergence: it compares
  when C++ and Rust exceed the tolerance with when Rust diverges from itself
  after one-ulp changes to initial roll, commanded speed, and bank limit,
  and after scaling each controller loop's proportional gain by 1 + 1e-12
  (one-ulp parameter changes alone can miss a flight that is sensitive only in a
  controller loop).
- `platform_check.py --output DIR` builds Rust for Linux/amd64
  (`rust.Dockerfile`, target `cli`) and compares every mission's outputs and
  traces byte for byte with the native build, at 1 and 8 workers.
- `sensitivity.py --check CHECK --output DIR` plants one-line defects in a copy
  of the Rust source and reports which comparison layer detects each.
- `noise_statistics.py CASE` compares NoisyContacts error distributions
  (two-sample Kolmogorov-Smirnov) for `verification/noisy-contacts-noise.yaml`,
  a second check beside the sample-for-sample comparison.

The archive has no `.git` directory, so upstream's optional Git metadata
collection prints warnings in C++ console logs. `source.json` and image
provenance identify the actual source instead. The unused JSBSim integration
also warns about its unset model path; no JSBSim mission is tested here.

The upstream base tag and package repositories are mutable. Saved image IDs
and package inventories identify a check's environment; rebuilding months later
is not guaranteed to reproduce that exact dependency image byte-for-byte.

Test the checker itself without Docker:

```sh
python3 -m unittest discover -s reference -p 'test_*.py'
```

## C++ versus Rust benchmark

`benchmark.py` (check `speed-cpp-container`) builds the C++ comparison branch
(`benchmarking-edits`, whose substeps interleave controllers and motion as Rust
does; tracing stays off) and the Rust core-only runner for this machine's own
architecture and runs both in one Linux container, so neither is emulated (on
Apple Silicon, C++ is built without JSBSim, which no workload uses). It runs
`perf.py`'s five workloads at three agent counts with C++ single-threaded, C++
in its own `multi_threaded` mode at 8 threads, and Rust at 1 and 8 workers: one
warm-up and three timed runs each. Times are whole processes minus a one-step
run of the same mission, which removes startup and parsing; peak memory is
recorded too. Rust must write identical output at 1 and 8 workers, and must
match single-threaded C++ except in churn, sensing, and collision, whose random
spawns and sensor noise C++ draws differently without the comparison spawn
stream; every run must repeat its warm-up output. `native_benchmark.py` (check
`speed-cpp-native`) runs the same on bare-metal macOS against
`native_build.sh`'s build.

C++'s multithreaded mode sometimes deadlocks (its worker waits on a condition
variable without a predicate, so a wake-up can be lost). Such runs are killed
after 120 s, counted, and retried. To save disk, a timed run's output is deleted
once it matches the warm-up's, and each build replaces the previous
`ripple-benchmark` image.

## Focused studies

These answer one question each from the current checkout, against an external
reference rather than an earlier revision of this project:

- `spatial_benchmark.py`: collision checking cost with the scan (every contact)
  and with grids of several cell widths, on crowded and spread-out missions;
  every variant must write identical output.
- `message_benchmark.py`: what all-agent messages cost per step, in native C++
  and in Rust, with `sample` profiles of both on macOS.
- `rk4_loads.py`: what computing forces at every RK4 stage changes. The held
  ("frozen") forces come from the unmodified C++ build, which still has them.
- `stall_landing.py`: ArduPlane's Rascal landing with and without stall tables,
  with a repeat that must be byte-identical.
- `rotors_motor.py`: Multirotor's motor lag and rotor loads against RotorS's
  own motor-plugin code (`rotors/`: an extractor that copies RotorS's
  `FirstOrderFilter` and `UpdateForcesAndMoments` verbatim from a pinned
  commit, and a harness with stand-in Gazebo links). It writes the samples
  that the Rust test `rotors_motor_samples` reproduces; `--update-fixture`
  replaces the committed copy.
- `crazyflow_check.py`: the `crazyflie2` preset flown open loop in Ripple and
  in Crazyflow's `first_principles` dynamics (an independent implementation,
  given the same constants through `crazyflow/integrate.py`), with rotor drag
  off (must agree to integration precision) and on (Ripple's per-rotor drag
  against Crazyflow's lumped drag matrix; reported). Crazyflow runs in a uv
  environment at a pinned commit; `--python` uses an existing one.
- `rascal_control.py`: the measurements behind AircraftPIDController's
  `rascal` settings: the altitude derivative gain after upsets at three step
  sizes, and heading, altitude, and speed steps under the `a4d` and `rascal`
  attitude limits.
- `simple_aircraft_fit.py`: SimpleAircraft's `rascal` limits. It flies
  FixedWing6DOF's Rascal through maneuvers that each reach one limit (full
  throttle, idle, a long climb and descent, a 180 degree turn, and level
  flight asked at 7 to 14 m/s), then flies SimpleAircraft with
  `airframe: rascal` through the same ones and compares speeds, acceleration,
  climb, descent, turn rate (and the bank a coordinated turn needs for it),
  and roll and pitch rates.
- `multirotor_control.py`: MultirotorVelocityController's step responses on
  the Crazyflie, its steady tilt against Förster's rotor drag, and
  convergence at 5, 10, and 20 substeps.
