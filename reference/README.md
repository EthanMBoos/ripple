# Docker reference checks

These tools compare the Rust workspace with C++ SCRIMMAGE built in Docker.

## Quick start

After a core change, check speed and fidelity:

```sh
python3 reference/perf.py                # speed vs your history, about 2 minutes
python3 reference/campaign.py --quick    # still matches C++? about 20 minutes
python3 reference/campaign.py --bench    # everything, before committing, about two hours
python3 reference/perf.py --record --label "spatial index"   # once committed
```

What is in this folder:

| Files | Purpose |
| --- | --- |
| `campaign.py` | Runs everything below and refreshes the paper's data |
| `reference_check.py` | Compares Rust with C++ on `missions.txt` (or `--mission`, `--mission-dir`) |
| `perf.py` | Rust speed against your history in `perf_history.jsonl` |
| `compare.py` | Compares two saved runs |
| `generate_scenarios.py`, `sensitivity.py`, `noise_statistics.py`, `divergence.py`, `platform_check.py`, `benchmark.py` | Paper studies; `campaign.py` runs all but `benchmark.py` (see [Further checks](#further-checks)) |
| `frames.py`, `traces.py`, `metrics_model.py` | Helpers: read frames, compare traces, model the two scoring rules |
| `Dockerfile`, `fixtures/` | The C++ reference build and its command-test plugins |
| `rust.Dockerfile`, `benchmark_runner.rs` | Rust on Linux, for the platform check and benchmarks |
| `tests/` | Tests for these tools, run without Docker |

`perf.py` is described in [Performance tracking](#performance-tracking) below.
`campaign.py` reruns the C++ comparisons: the matrix, planted bugs, noise,
generated and long scenarios, and the platform check. It prints the headline
numbers before and after ("what moved") and replaces `paper/data/` only if every
step succeeded, so a failed run never overwrites good results. `--no-retain`
runs the checks without touching the paper; `python3 reference/reference_check.py`
runs the matrix alone. Expected differences (randomized spawning against the
unmodified C++ build, the noise mission's random payloads, late divergence in
long runs) are reported but are not failures; anything else exits non-zero, and
`runs/campaign-NNN/summary.json` lists each step's outcome.

The C++ checkout defaults to a sibling `../scrimmage`; pass `--source PATH` to
either script otherwise. It needs both branches locally, for example:

```sh
git clone git@github.com:gtri/scrimmage.git ../scrimmage
git -C ../scrimmage fetch origin Ubuntu-24.04:Ubuntu-24.04 benchmarking-edits:benchmarking-edits
```

The checker builds from these committed branches, never from uncommitted files.
`campaign.py` writes to `runs/campaign-NNN/`, replaces the folders under
`paper/data/`, and regenerates the manuscript tables; rebuild the paper and
check the prose afterwards. The missions compared are listed in
[missions.txt](missions.txt).

Requires Python 3.10+, a running Docker engine, and Rust/Cargo. No native C++
build is needed.
The first build downloads a large dependency image and compiles C++; later
checks reuse Docker's build cache. Allow several GB of disk space.

Checks are saved automatically as `runs/reference-check000`, `001`, and so on.
Use `--output path/to/new-check`, `--source path/to/scrimmage`, or `--jobs 4`
to override locations or compilation parallelism. Existing output directories
are never reused. `--timeout` controls each mission/comparison; `--build-timeout`
controls each Docker/Cargo build. Both are in seconds.

## Performance tracking

`perf.py` times the Rust simulation natively, without Docker or C++, so you can
see what a change does to speed:

```sh
python3 reference/perf.py                        # measure; compare with the last record
python3 reference/perf.py --record --label NAME  # measure and append to the history
python3 reference/perf.py --against NAME         # compare with a specific record
python3 reference/perf.py --history              # list this machine's records
python3 reference/perf.py --quick                # smallest sizes only, under a minute
```

It builds the release core-only runner (`benchmark_runner.rs`, no viewer) and
runs four workloads (`motion`, `substeps`, `sensing`, `churn`) at two agent
counts each and at 1 and 8 workers: a warm-up, then five timed runs. It reports
the median stepping time with its min-max range, and peak memory. A change is
flagged only when the new median lies outside the old range and differs by more
than 5%.

Records go in `perf_history.jsonl` (the first `--record` creates it; commit it), with
the commit, date, machine, and label. Compare only records from the same machine
(the script does this automatically) on an idle system with the same power
settings. Record a baseline from a clean commit; `--record` notes uncommitted
changes. Do not edit a workload in place: that makes its history incomparable, so
add a new workload name instead.

`benchmark.py` is different: it times C++ against Rust in the reference Docker
image for the paper (see [C++ versus Rust benchmark](#c-versus-rust-benchmark)).

## Two C++ builds

The checker builds two committed branches of `../scrimmage`:

- `Ubuntu-24.04`, the unmodified reference.
- `benchmarking-edits`, created from it, which adds only opt-in
  comparison support: `SCRIMMAGE_TRACE=<file>` writes a per-tick trace, and
  `SCRIMMAGE_LIBCXX_SPAWN_RANDOM=1` draws entity generation from a dedicated
  `std::minstd_rand` stream with libc++'s normal-variate ordering. With neither
  variable set, the branch behaves exactly like the reference.

Each mission runs C++ four times. The unmodified build, the instrumented build,
and the instrumented build with tracing must write byte-identical `frames.bin`
and `summary.csv` (`cpp_non_interference` in `result.json`); a difference fails
the case. The fourth run, with tracing and the libc++-compatible spawn stream, is
the one compared with Rust. Rust runs once more with `--trace`, which must leave
its frames, events, and summary byte-identical (`trace_equality`).

The spawn option reproduces the unmodified C++ on macOS (libc++) when spawning
is the only consumer of the shared generator, which holds for the matrix. It
lets randomized spawning be compared sample for sample on Linux. Other draws,
such as sensor noise, stay on the original engine.

`traces.py` compares the traces (`trace-comparison.json`):

| Record | When | Compared |
| --- | --- | --- |
| `delivery` | A message reaches a subscriber | Per tick, receiver, network, and topic: senders and event entity IDs, in order |
| `publication` | A plugin publishes a sensor payload | Contact IDs in order; state within frame tolerances; covariance within 1e-9 |
| `belief` | Each entity when its frame is logged | Within frame tolerances |
| `output` | Each autonomy/controller port after all controller substeps | Within 1e-8 absolute |

Endpoints are `[entity ID or null, plugin]` using C++ names (sensors carry their
order index, as in `NoisyState0`). Non-finite numbers are written as the strings
`NaN`, `Infinity`, and `-Infinity`. Delayed deliveries are recorded when queued
(`scheduled_delivery`); no matrix mission uses network delay.

## What gets built

The script reads the exact
[upstream slim dependency Dockerfile](https://github.com/gtri/scrimmage/blob/Ubuntu-24.04/ci/dockerfiles/ubuntu-24.04-slim-dependency-only)
from the local branch snapshot and builds it without edits. Despite its name,
that image includes VTK/Qt and other integration dependencies. Its JSBSim setup
downloads amd64 packages, so the workflow explicitly uses `linux/amd64`, including
emulation on Apple Silicon. These packages are installed inside Docker only.

A second image per branch builds a `git archive` of that local branch. It does not copy
uncommitted changes, modify the checkout, or fetch/update your local branch.
The branch is not pinned in this project;
each check records the actual commit, source archive hash, Dockerfile hashes,
image IDs, Linux package/compiler versions, Rust compiler, and Rust source hashes.
The upstream dependency Dockerfile itself clones GitHub for its setup scripts;
that is distinct from the archived source used for the reference executable.

[Dockerfile](Dockerfile) disables optional VTK, gRPC, OpenCV, ROS, and GPU build
paths and builds the executable plus the eighteen upstream C++ plugins needed by the current
matrix. This is a headless comparison build, not verification of the full C++
plugin catalog. Model equations are never patched. Two small command drivers in
`fixtures/` are built into both images: `MotorSpeeds` (the old upstream PID
writes a different control interface than Multirotor reads) and
`ConstantVelocity` (no retained upstream autonomy writes a nonzero velocity).
They supply constant commands to unchanged upstream models; neither is an
autopilot. Rust has matching stock test drivers.
Fixture source hashes are recorded alongside the upstream source provenance.
`VTK_FOUND=OFF` is set explicitly because upstream CMake expands that variable
inside conditionals even when VTK discovery is disabled.

Docker image tags/layers remain cached. There is no automatic Docker pruning.
Mission containers run without network access and can write only to their
dedicated host output mount; they are removed after the run.

## Add a C++ plugin to the comparison

1. Port the plugin to Rust and register it (see the book's plugin guide).
2. Add its C++ build target to the `cmake --build` lists in [Dockerfile](Dockerfile).
   If it needs a command source no retained C++ plugin provides, add a small
   driver to [fixtures/](fixtures/) and the fixture loop in the Dockerfile, with
   a matching Rust driver.
3. Write a fixture mission under `missions/verification/` that exercises it with
   no randomness it can't share (zero spawn variance, zero sensor noise), and add
   it to [missions.txt](missions.txt) with a short label. The Rust regression
   tests pick up every mission under `missions/` automatically.
4. If it publishes a new sensor message whose contents should be compared, add
   the payload to both traces: `src/common/Trace.cpp` on the C++ branch and
   `crates/core/src/simcontrol/trace.rs` here (see `sensor_states` in
   `pubsub/messages.rs`). Commit C++ changes on `benchmarking-edits`;
   the checker builds committed branches only.
5. Optionally add the plugin to `generate_scenarios.py` for random coverage and a
   one-line defect for it to `sensitivity.py`.
6. Run `python3 reference/campaign.py --quick`, then the full campaign.

## Default comparison matrix

The matrix is [missions.txt](missions.txt):

| Mission | Coverage |
| --- | --- |
| `straight-no-gui.xml` | Opposing aircraft, collision and removal |
| `test_missions/straight_cpu.xml` | Longer single-aircraft run |
| `test_missions/straight_cpu_mul.xml` | Original mission including its `motion_multipler` typo |
| `verification/aircraft-substeps-spawning.xml` | Actual five-substep execution, rates, randomized state, and scheduled spawning, compared with the libc++-compatible spawn stream |
| `verification/aircraft-substeps-scheduled.xml` | The same substeps and rates with two zero-variance spawn schedules; deterministic on either engine |
| `verification/noisy-state-bias.xml` | NoisyState deterministic bias/attitude equations and belief feedback, without stochastic sequence differences |
| `networks-local-global.xml` | Boundary response over GlobalNetwork, local own-state feedback, ground removal and metrics |
| `fixed-wing-6dof.xml` | Aerodynamic flight, tilted/northbound attitude, nonzero rates and wind |
| `multirotor.xml` | Quad hover, tilted flight, unequal motor speeds and a six-rotor configuration |
| `verification/single-integrator.xml` | SingleIntegrator direct, normalized, zero, and vertical commands from `ConstantVelocity` |
| `verification/noisy-contacts-bias.xml` | NoisyContacts biases, attitude order, and covariance through published payloads |
| `verification/collision-boundary.xml` | SimpleCollision strict range at the boundary, and in-run opposing and same-team collisions |
| `verification/sphere-network-auction.xml` | SphereNetwork reach inside, at, and beyond range through AuctionAssign messages on `CommsNetwork` |

Each case runs C++ headless and single-threaded. Rust runs with 1, 2, and 8
workers without Rerun, plus an eight-worker headless Rerun recording.

For just the Multirotor port, run
`python3 reference/reference_check.py --mission missions/multirotor.xml` and
`cargo test -p scrimmage-core multirotor`. The initial four-vehicle check compared
4,001 frames / 16,004 states: maximum position error 8.63e-15 m, velocity error
4.82e-13 m/s, matching summaries, and byte-identical Rust worker/recording outputs.
This establishes the selected open-loop cases, not a working upstream autopilot
or external-force/landing integration. Model-specific quirks are commented inline.

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

`--mission path/to/mission.xml` can be repeated to replace the default matrix.
Additional missions must use plugins included in this build and have expanded
XML: XIncludes are rejected explicitly. The script saves the input and effective
C++ mission, with all headless/logging overrides recorded. It does not change
physics, seeds, rates, parameter defaults, or the multiplier typo.

The build also includes NoisyState. Its deterministic bias/attitude fixture can
be checked with `--mission missions/verification/noisy-state-bias.xml`.
`missions/noisy-state.xml` uses Rust's independent sensor random streams and is
not expected to match C++ sample-for-sample. Keep stochastic mismatch reports;
use the deterministic fixture and sensor unit tests to verify equations/noise
without pretending the random sequences are identical.

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
defect through (`sensitivity.py`). Frames are paired by index, retaining repeated terminal
timestamps; entities and summary rows are matched by ID. Quaternion signs are
equivalent. Malformed/nonfinite state is rejected. `frames.py` decodes the
length-delimited proto3 frame subset from the reference branch's schemas; it is
not a general protobuf library. `traces.py` compares event streams separately.

## Results and failure handling

The command exits zero only when every requested case passes. `report.json`
contains the overall result and per-mission checks; case directories retain
`comparison.json`, input missions, C++ logs, Rust variants, hashes, and recordings.
Build logs and source/image provenance are kept at the check root. A failed
mission does not prevent the remaining cases from being checked.

Rust's legacy RNG reproduces Apple libc++. Ubuntu's libstdc++ uses a different
default engine (multiplier 16807, not 48271) and returns the other polar-method
variate first, so the unmodified Linux build diverges on randomized spawning.
The comparison run removes that platform effect with the instrumentation
branch's spawn option instead of changing seeds or tolerances. The
zero-variance twin `verification/aircraft-substeps-scheduled.xml` stays in the
matrix as a check that does not rely on that option.

Recorded outcomes, with retained reports under `paper/data/`, are in the
[paper](../paper/README.md).

## Further checks

- `generate_scenarios.py` writes seeded random missions from retained models;
  run them with `reference_check.py --mission-dir DIR`. `--horizon 1000` makes
  long runs (fixed-wing capped at 100 s).
- `divergence.py CHECK --output DIR` explains long-run divergence: it compares
  when C++ and Rust exceed the tolerance with when Rust diverges from itself
  after one-ulp changes to initial roll, commanded speed, and turning radius.
- `platform_check.py --output DIR` builds Rust for Linux/amd64
  (`rust.Dockerfile`, target `cli`) and compares every curated mission's outputs and
  traces byte for byte with the native build, at 1 and 8 workers.
- `sensitivity.py --check CHECK --output DIR` plants one-line defects in a copy
  of the Rust source and reports which comparison layer detects each.
- `noise_statistics.py CASE` compares NoisyContacts error distributions
  (two-sample Kolmogorov-Smirnov) for `verification/noisy-contacts-noise.xml`,
  whose random samples differ by design.

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

```sh
python3 reference/benchmark.py --output runs/new-benchmark   # --quick: smallest sizes only
```

This builds the unmodified C++ branch and the Rust core-only runner for this
machine's own architecture and runs both in one Linux container, so neither is
emulated (on Apple Silicon, C++ is built without JSBSim, which no workload uses).
It runs `perf.py`'s four workloads at three agent counts with C++ single-threaded,
C++ in its own `multi_threaded` mode at 8 threads, and Rust at 1 and 8 workers:
one warm-up and three timed runs each. Times are whole processes minus a one-step
run of the same mission, which removes startup and parsing; peak memory is
recorded too. Rust output must match single-threaded C++, except in churn and
sensing, whose random spawns and sensor noise the unmodified C++ build draws
differently; every run must repeat its warm-up output.

C++'s multithreaded mode sometimes deadlocks (its worker waits on a condition
variable without a predicate, so a wake-up can be lost; 7 of 40 runs of one
mission hung). Such runs are killed after 120 s, counted, and retried. The
paper's retained results are in `paper/data/benchmark/`.
