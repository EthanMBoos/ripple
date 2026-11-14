# Checks and release evidence

The reference suite checks output stability across workers and recording
modes, agreement with SCRIMMAGE and vehicle-model references, and speed
regressions. `reference/suite.py` runs each check by ID and keeps its inputs
and reports.

## Run the checks

```sh
python3 reference/suite.py --list                 # purpose and pass rule by ID
python3 reference/suite.py                        # quick set, about 30 minutes
python3 reference/suite.py --release              # all checks, several hours
python3 reference/suite.py --check cpp-agreement  # one check
```

A run writes `runs/suiteNNN/<check>/` with logs and a `summary.json`, keeps
failures with their inputs, and exits nonzero if any selected check fails.
Each script also runs alone; its docstring gives its options.

## Checks

| ID | Set | Question |
| --- | --- | --- |
| `rust-tests` | quick | Do Rust tests, format, lints, and checker tests pass? |
| `rust-determinism` | quick | Do worker count, recording, tracing, and spatial lookup leave output unchanged? |
| `cpp-agreement` | quick | Do matched missions agree with the SCRIMMAGE comparison branch? |
| `speed-regression` | quick | Has native Rust speed regressed, and what is peak memory? |
| `multirotor-control` | quick | Do the Crazyflie's controller steps and step-size checks meet their limits? |
| `simple-aircraft-fit` | quick | Does the simplified Rascal match the 6-DOF Rascal's measured maneuvers? |
| `rascal-control` | quick | Do the Rascal controller's chosen gains and limits behave as measured? |
| `aerosonde-book` | quick | Do two transcriptions of Beard and McLain agree at held states and in trim? |
| `cpp-generated` | release | Do 100 generated missions agree with SCRIMMAGE? |
| `cpp-long` | release | Do long missions run cleanly, and how do numerical differences grow? |
| `cpp-noise` | release | Does NoisyContacts agree sample by sample and in distribution? |
| `checker-sensitivity` | release | Does the comparison catch every planted porting defect? |
| `rust-platform` | release | How do native and Linux/amd64 outputs differ? |
| `speed-cpp-container` | release | How do Rust and C++ time and memory compare in one container? |
| `speed-cpp-native` | release | How do they compare on bare-metal macOS? |
| `speed-spatial` | release | What does the collision grid cost, with identical output? |
| `speed-messages` | release | What does all-agent message routing cost? |
| `rk4-loads` | release | What changes when forces are evaluated at every RK4 stage? |
| `rotors-motor` | release | Do rotor loads and lag agree with RotorS's motor code? |
| `crazyflow-crazyflie` | release | Do multirotor loads and open-loop flights agree with Crazyflow? |
| `jsbsim-rascal` | release | Do Rascal loads and glides agree with JSBSim? |
| `stall-landing` | release | What changes when the Rascal uses stall tables? |

[missions.txt](missions.txt) names every C++ case and what it exercises.
Each model script holds its own grids, constants, limits, and pinned source
versions. The vehicle checks compare Ripple with another simulator or a
second transcription of the same equations, not with a real aircraft.

A check runs its prerequisites first. The native C++ checks need macOS; the
JSBSim and Crazyflow checks pull pinned packages through uv; `stall-landing`
runs ArduPlane SITL in Docker with Ripple built `--features ardupilot`.

### Vehicle model references

`rotors_motor.py` runs RotorS's motor and rotor code in a small stand-in for
Gazebo and compares whole-vehicle loads for the Crazyflie and Firefly.
`crazyflow_check.py` compares held loads and open-loop flights with
Crazyflow; Ripple's per-rotor drag and Crazyflow's lumped drag differ, and
that difference is reported rather than judged. `jsbsim_check.py` flies
ArduPilot's Rascal files in JSBSim for held loads and open-loop glides.
`aerosonde_check.py` compares against its own Python transcription of Beard
and McLain, because the pinned MAVSIM code has no working force or motor
model.

## SCRIMMAGE comparison

Ripple does not need SCRIMMAGE for normal use. The C++ checks need a checkout
with the `Ubuntu-24.04` and `benchmarking-edits` branches, at `../scrimmage`
or wherever `--source PATH` points:

```sh
git clone git@github.com:gtri/scrimmage.git ../scrimmage
git -C ../scrimmage fetch origin Ubuntu-24.04:Ubuntu-24.04 benchmarking-edits:benchmarking-edits
```

They need Python 3.10+, Docker, and Cargo. Python before 3.14 also needs
`backports.zstd` to read Ripple's MCAP. C++ builds headless in Docker, so no
host toolchain; the first build downloads a large dependency image. Native
C++ timing is the exception and uses `native_build.sh` on macOS.

`reference_check.py` builds committed snapshots of both C++ branches with
`git archive`, so uncommitted C++ edits are not seen. The baseline branch is
upstream as is. The comparison branch adds opt-in tracing, keyed random
numbers, and the corrections listed in [SCRIMMAGE
reference](../docs/REFERENCE_NOTES.md), so upstream output is not expected to
match where those corrections matter. Each report records the commits,
hashes, compiler versions, and build logs it used.

For each [matrix mission](missions.txt), Ripple runs first and records its
resolved scenario; `scrimmage_xml.py` writes SCRIMMAGE's XML from that
record, so both simulators get the same values. Compared missions use default
estimators, no terrain, and explicit parameters where defaults differ. The
Docker build includes the upstream plugins these missions need plus two
small command fixtures, `MotorSpeeds` and `ConstantVelocity`, for missions
where no retained C++ controller supplies the command.

### Comparison protocol

Each case runs upstream C++ once and the comparison branch three times:
plain, with tracing, and with tracing plus keyed random draws. Tracing must
leave the comparison branch's `frames.bin` and `summary.csv` byte-identical.
Upstream equality is recorded, not required. The keyed run is what Ripple is
compared with. Rust runs at 1, 2, and 8 workers, then with tracing and with a
headless Rerun recording, and its simulation output must stay byte-identical
throughout.

The comparison requires the same entity IDs and removal times, and compares
position within 1e-6 m, velocity within 1e-6 m/s, orientation within 1e-7
rad, angular velocity within 1e-7 rad/s, and time within 1e-9 s. C++ summary
values get 1e-6 because C++ prints six decimals. A summary that differs
passes only when `metrics_model.py` reproduces both from the same metrics
deliveries under the documented C++ and Rust scoring rules.

`traces.py` compares message deliveries, sensor measurements, beliefs, and
plugin outputs. Deliveries keep sender and event-entity order; states use the
frame tolerances and outputs 1e-6 absolute. C++ sensor publications pair
with Rust sensor readings; C++ deliveries of those publications have no Rust
counterpart and are counted. Entities that SCRIMMAGE removes in its
pre-start collision pass leave first-tick records, which the checker reports
as such.

Upstream Linux spawning differs from Ripple's because its standard-library
random engine differs; the comparison branch uses keyed draws for spawning,
sensing, and network loss, and the upstream mismatch stays in the results.

Run one mission with:

```sh
python3 reference/reference_check.py --mission missions/multirotor.yaml
```

`--mission` repeats, and `--mission-dir` takes a generated set. A run writes
`runs/reference-checkNNN/report.json` and a folder per case with the input
YAML and XML, C++ logs, Rust variants, comparisons, traces, and hashes. The
checker keeps going after a failure. `--output`, `--jobs`, `--timeout`, and
`--build-timeout` set the report path, C++ build parallelism, run timeout,
and build timeout, respectively.

### Add a C++ comparison case

1. Register the Rust plugin and add its C++ build target to
   [Dockerfile](Dockerfile). Add a command fixture only if no retained C++
   plugin can drive it.
2. Add a mission under `missions/verification/` and a stable ID with a short
   coverage label to [missions.txt](missions.txt). Use only randomness the
   comparison can share.
3. If new sensor payloads matter, add them to the C++ and Rust traces. Commit
   C++ changes on `benchmarking-edits`; the checker builds committed sources.
4. Run the quick suite, then the release suite. Generated missions and a
   planted defect in `sensitivity.py` extend coverage.

### Compare saved runs

`compare.py` needs no Docker or Cargo. It reads SCRIMMAGE's `frames.bin` and
`summary.csv` and Ripple's `run.mcap`:

```sh
python3 reference/compare.py path/to/cpp-run path/to/rust-run \
  --report runs/comparison.json
```

Exit codes: 0 agreement, 1 mismatch, 2 invalid inputs. `frames.py` decodes
only the frame protobuf fields the comparison uses; `run.py` reads the MCAP
fields Ripple writes.

## Performance and behavior-preserving changes

`perf.py` runs the core-only [benchmark runner](benchmark_runner.rs) on six
workloads at two fleet sizes and 1 and 8 workers, and reports median step
time, its range, and peak memory. The accepted
[baseline](perf_baseline.json) records the workload hash, machine, and Rust
toolchain. `speed-regression` fails when a median is outside the baseline
range and more than 10% slower. An incompatible baseline fails for review
rather than being accepted.

```sh
python3 reference/perf.py           # compare with the baseline
python3 reference/perf.py --quick   # smallest sizes
python3 reference/perf.py --accept  # accept after review on an idle machine
```

`benchmark.py` times C++ and Rust together in one Linux container;
`native_benchmark.py` repeats that on macOS. `spatial_benchmark.py` checks
that scan and grid collision methods write identical output.
`message_benchmark.py` measures routing with all-agent traffic.

A refactor or rename must leave every output byte unchanged:

```sh
python3 reference/equivalence.py OLD_EXECUTABLE NEW_EXECUTABLE
```

This runs the shipped missions, the benchmark workloads, and a message-heavy
mission at 1, 2, and 8 workers and compares entities, frames, events,
summaries, traces, scenarios, and result metadata. Its report goes to
`runs/equivalenceNNN/`.

## Verification

`verus.py` proves the core crate's Verus contracts (`cargo verus verify -p
ripple-core`), then plants each fault it lists in the production source, one
at a time, and requires the proof to fail. The record goes to `runs/verusNNN/`
and, with `--retain`, to `paper/data/verification/`. It edits source files in
place and restores them, so do not build, run `equivalence.py`, or commit
while it runs. It needs `cargo-verus` and `verus` on the PATH; the Rust book's
verification chapter says how to install them.

```sh
python3 reference/verus.py
```

## Retained evidence

`paper/data/<check>/` holds compact reports, summaries, missions, and
timings; frames, traces, and recordings stay under `runs/`.
`paper/data/release.json` records each check's purpose, reference,
criterion, outcome, source hash, and external versions. The source hash and
the script names come from `project.py`.

```sh
python3 reference/suite.py --release --retain
python3 reference/suite.py --check rk4-loads --retain
```

`--retain` replaces evidence only after every selected check passes, and
updates `release.json` and `paper/data/SUMMARY.md`. Look at the diff and the
paper before committing. Failing checks stay in `runs/`.

The checker tools have their own tests, no Docker needed:

```sh
python3 -m unittest discover -s reference -p 'test_*.py'
```
