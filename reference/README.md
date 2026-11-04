# Checks and release evidence

The reference suite tests four things: Ripple's own behavior is stable across
workers and recording modes; the shared models agree with SCRIMMAGE; vehicle
models agree with their stated references; and performance has not regressed.
`reference/suite.py` runs each check by ID and keeps its inputs and reports.

## Run the checks

```sh
python3 reference/suite.py --list                 # purpose and pass rule by ID
python3 reference/suite.py                        # quick set, about 30 minutes
python3 reference/suite.py --release              # all checks, several hours
python3 reference/suite.py --check cpp-agreement  # one check
```

A run creates `runs/suiteNNN/<check>/`, with logs and a `summary.json`. It
never reuses an earlier folder. Failures keep their inputs and diagnostics;
the command exits nonzero if any selected check fails. Each script can also
run alone; its module docstring gives its options. `suite.py --list` reads the
check registry, which is the source of truth for each check's pass criterion.

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

The [mission matrix](missions.txt) names every C++ case and the behavior it
exercises. The model scripts hold their grids, constants, limits, and source
versions. Their output reports show the measured errors. `aerosonde-book`
compares two transcriptions of a published model, not a second simulator or
flight data; the other vehicle checks also establish model agreement, not
agreement with a real aircraft.

A check runs its prerequisites first. The native C++ checks need macOS; the
JSBSim and Crazyflow checks use pinned packages through uv. `stall-landing`
uses ArduPlane SITL in Docker and builds Ripple with `--features ardupilot`.

### Vehicle model references

`rotors_motor.py` runs RotorS's motor and rotor calculations in a small
stand-in for Gazebo, then compares whole-vehicle loads for the Crazyflie and
Firefly. `crazyflow_check.py` compares held loads and open-loop flights with
Crazyflow. Its no-drag and supplied-hub-drag variants test matching equations;
the difference between Ripple's per-rotor drag and Crazyflow's lumped drag is
reported rather than judged as an error.

`jsbsim_check.py` uses ArduPilot's Rascal files in JSBSim for held loads and
open-loop glides. It judges the matching elevator-drag form and reports the
airframe as flown separately. `aerosonde_check.py` uses a separate Python
transcription of Beard and McLain because the pinned MAVSIM code has no
working force or motor implementation. It checks held states and a flown
longitudinal trim; agreement is between two transcriptions.

## SCRIMMAGE comparison

Ripple does not need SCRIMMAGE for normal use. The C++ checks need a checkout
with local `Ubuntu-24.04` and `benchmarking-edits` branches. They default to
`../scrimmage`; pass `--source PATH` when it lives elsewhere. For example:

```sh
git clone git@github.com:gtri/scrimmage.git ../scrimmage
git -C ../scrimmage fetch origin Ubuntu-24.04:Ubuntu-24.04 benchmarking-edits:benchmarking-edits
```

The checks require Python 3.10+, Docker, and Cargo. Python before 3.14 also
needs `backports.zstd` to read Ripple's MCAP. The C++ build runs headless in
Docker; it needs no host C++ toolchain. The first Docker build downloads a
large dependency image, so allow several GB of disk space. Native C++ timing
is the exception and uses `native_build.sh` on macOS.

`reference_check.py` builds committed snapshots of both C++ branches with
`git archive`. It neither reads uncommitted C++ edits nor changes the source
checkout. The baseline branch shows upstream behavior. The comparison branch
has opt-in tracing and keyed random numbers plus the documented corrections
in [SCRIMMAGE reference](../docs/REFERENCE_NOTES.md); upstream output is not
expected to match when those corrections matter. Each report records the
actual commits, source and image hashes, compiler versions, and build logs.
The dependency image can change as upstream packages change; its saved image
ID identifies the environment used for a retained result.

For each [matrix mission](missions.txt), Ripple runs first and records its
resolved scenario. `scrimmage_xml.py` writes SCRIMMAGE's XML from that record,
so both simulators receive the same mission values. Compared missions use
default estimators, no terrain, and explicit parameters where defaults differ.
The Docker build includes the upstream plugins these missions need and two
small command fixtures, `MotorSpeeds` and `ConstantVelocity`. They provide
inputs when no retained C++ controller supplies the required command; neither
changes a model equation.

### Comparison protocol

Each case runs upstream C++ once and the comparison branch three times:
without instrumentation, with tracing, and with tracing plus keyed random
draws. Tracing must leave the comparison branch's `frames.bin` and
`summary.csv` byte-identical. Upstream equality is recorded, not required.
The keyed run is compared with Ripple. Rust runs at 1, 2, and 8 workers, then
with tracing and with a headless Rerun recording; its simulation output must
remain byte-identical. A nonempty RRD is checked, but viewer screenshots are
a separate visual check.

The comparison requires the same entity IDs and removal times, and compares
position within 1e-6 m, velocity within 1e-6 m/s, orientation within 1e-7
rad, angular velocity within 1e-7 rad/s, and time within 1e-9 s. C++ summary
values have a 1e-6 tolerance because C++ prints six decimals. A summary
that differs passes only when `metrics_model.py` reproduces both summaries
from the same metrics deliveries under the documented C++ and Rust scoring
rules, with identical entity and collision counts.

`traces.py` compares message deliveries, sensor measurements, beliefs, and
plugin outputs. Deliveries retain sender and event-entity order; state values
use the frame tolerances and outputs use 1e-6 absolute. C++ sensor
publications and Rust sensor readings are paired as measurements. C++
deliveries of those publications have no Rust counterpart and are counted.
The checker also reports the known first-tick records of entities removed in
SCRIMMAGE's pre-start collision pass, rather than treating those records as
Rust outputs. No matrix mission uses delayed network delivery.

The unmodified Linux build's randomized spawning differs from Ripple's
because its C++ standard-library engine differs. The comparison branch uses
keyed draws for spawning, sensing, and network loss. The suite retains the
upstream mismatch as a result rather than changing a seed or tolerance.

Run a specific mission with:

```sh
python3 reference/reference_check.py --mission missions/multirotor.yaml
```

`--mission` can be repeated, and `--mission-dir` takes a generated set. A run
writes `runs/reference-checkNNN/report.json` and a folder per case with input
YAML and XML, C++ logs, Rust variants, comparisons, traces, and hashes. The
checker runs remaining cases after one fails. Use `--output` for a new output
path, `--jobs` for C++ build parallelism, and `--timeout` or `--build-timeout`
for run and build limits.

### Add a C++ comparison case

1. Register the Rust plugin and add its C++ build target to
   [Dockerfile](Dockerfile). Add a minimal command fixture only if the
   retained C++ plugins cannot drive it.
2. Add a mission under `missions/verification/` and a stable ID with a short
   coverage label to [missions.txt](missions.txt). Use only randomness the
   comparison can share.
3. If new sensor payloads matter, add them to the C++ and Rust traces. Commit
   C++ changes on `benchmarking-edits`; the checker builds committed sources.
4. Run the quick suite, then the release suite. Generated missions and a
   planted defect in `sensitivity.py` can extend coverage.

### Compare saved runs

`compare.py` needs no Docker or Cargo. It reads SCRIMMAGE's `frames.bin` and
`summary.csv` and Ripple's `run.mcap`:

```sh
python3 reference/compare.py path/to/cpp-run path/to/rust-run \
  --report runs/comparison.json
```

The JSON report lists mismatches. Exit codes are 0 for agreement, 1 for a
mismatch, and 2 for invalid inputs. Existing reports are not overwritten.
`frames.py` decodes only the frame protobuf fields used by this comparison;
`run.py` reads the MCAP fields Ripple writes.

## Performance and behavior-preserving changes

`perf.py` runs the core-only [benchmark runner](benchmark_runner.rs) on six
workloads at two fleet sizes and 1 and 8 workers. It reports median step
time, its range, and peak memory. The accepted
[baseline](perf_baseline.json) records the workload hash, machine, and Rust
toolchain. A median fails `speed-regression` when it is outside the baseline
range and more than 10% slower. An incompatible baseline fails for review;
it is never accepted automatically.

```sh
python3 reference/perf.py           # compare with the baseline
python3 reference/perf.py --quick   # smallest sizes
python3 reference/perf.py --accept  # accept after review on an idle machine
```

`benchmark.py` times C++ and Rust together in a Linux container with the same
workloads; `native_benchmark.py` repeats them on macOS. `spatial_benchmark.py`
checks that scan and grid collision methods write identical output.
`message_benchmark.py` measures routing with all-agent traffic. Timing and
memory results are measurements, not claims of real-time performance.

A refactor or rename must leave all output bytes unchanged:

```sh
python3 reference/equivalence.py OLD_EXECUTABLE NEW_EXECUTABLE
```

This runs shipped missions, benchmark workloads, and a message-heavy mission
at 1, 2, and 8 workers. It compares entities, frames, events, summaries,
traces, scenarios, and result metadata; unavailable API missions are listed
as skipped. Its report goes to `runs/equivalenceNNN/` and is development
evidence, not release evidence.

## Retained evidence

`paper/data/<check>/` holds compact reports, summaries, missions, and timings.
Frames, traces, and recordings stay under `runs/`. `paper/data/release.json`
records each check's purpose, reference, criterion, outcome, source hash, and
external versions. The source hash comes from `project.py`; names and paths
for the checking scripts are defined there too.

```sh
python3 reference/suite.py --release --retain
python3 reference/suite.py --check rk4-loads --retain
```

`--retain` replaces evidence only after every selected check passes. It
updates `release.json` and `paper/data/SUMMARY.md`; inspect the diff and the
paper before committing. Never retain a failing check or loosen its inputs
to make it pass. The suite keeps failure reports in `runs/`.

Check the checker tools without Docker:

```sh
python3 -m unittest discover -s reference -p 'test_*.py'
```
