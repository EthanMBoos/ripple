# Roadmap

## Initial release

SCRIMMAGE-RS is at its initial release. In the tested configurations of the
retained models, its closed simulation loop agrees with C++ SCRIMMAGE in
trajectories, selected sensor and command values, and message-delivery timing,
to floating-point rounding and subject to documented differences. The
[paper](../paper/README.md) shows how and states the limits, and
`python3 reference/campaign.py` reruns every check (see
[reference/README.md](../reference/README.md)).
Intentional differences from C++ are in [reference notes](REFERENCE_NOTES.md);
current usage is in the [book](../book/src/SUMMARY.md).

Everything below is optional: external integrations and core upgrades beyond
the release. Build each around a concrete experiment. The starter crate and the
shared run/sweep/Slurm workflow stay the normal development path, and any change
to the simulation must keep the C++ comparison passing.

## Scope decisions

| Area | Decision |
| --- | --- |
| Mission input | Keep XML single runs and existing overlays; YAML is native and gets templates/sweeps. |
| Runtime integrations | Optional ROS 1 and ROS 2 via a separate bridge; ArduPilot via UDP. |
| JSBSim | Offline flight-model verification fixtures. |
| GPU work | Evaluate Burn for one useful workload. |
| Viewer | Keep Rerun and its current recording playback. Terrain is an optional experiment. |
| C++ machinery excluded | Runtime plugin-library discovery/loading, legacy protobuf/string-map spawning, OpenCL, VTK, and production JSBSim integration. |

XML is frozen. Its inherited-motion replacement and entity-tag differences are
recorded in [reference notes](REFERENCE_NOTES.md#mission-compatibility).

## 1. Performance

`python3 reference/perf.py` tracks Rust speed against a committed history (see
[reference/README.md](../reference/README.md#performance-tracking)); record a
baseline before a core upgrade and compare after it. The paper's C++-versus-Rust
timing uses [benchmark.py](../reference/benchmark.py) and still needs these:

- [x] Explain the eight-worker slowdown: each phase's pool handoff (~35 µs on
  bare metal, ~10x in Docker's VM) exceeds light phases' work (see the paper).
- [x] Benchmark the four workloads across agent counts with peak memory and
  C++'s multithreaded mode, in a Linux container and on bare-metal macOS.
- [ ] Stop building message-endpoint names every step
  (`PluginStack::mailboxes`); about a fifth of main-thread time in `motion`,
  where native C++ is now 1.25x faster.
- [ ] Speed up spatial queries if profiling shows they matter:
  - Benchmark collision checks, spatial sensors, and routing on spread-out and
    clustered populations, keeping the simple scan as the correctness baseline.
  - Try one engine-owned read-only spatial lookup (grid or tree), refreshed at the
    snapshot each consumer reads, preserving event order and random draws.
  - Index subscribers by topic and, for LocalNetwork, by entity.

  C++ shares a pre-motion R-tree for SphereNetwork/Boids; don't inherit its stale
  timing for post-motion consumers.

Tested at scale: 5,000 agents spawning in one tick and 1,500 removed in one tick.
World plugins may publish up to 65,536 messages per step; an entity's plugins,
1,024. All-to-all messaging can still exceed 65,536 pending deliveries per network.

## 2. External integrations

Implement one end-to-end pilot at a time. Entities from a connection go through
`spawn(SpawnRequest)` in [generation.rs](../crates/core/src/simcontrol/generation.rs).
Adapters own protocol details and stay out of model equations and the default
build. Apply inputs at defined phases with explicit units and ENU/NED/body
frames; bound waits and queues; define failure, disconnect, and shutdown.

### First pilot: entities from a connection

- [ ] Supply the peer's starting state through `SpawnRequest`, adding velocity
  and attitude as needed.
- [ ] Bind one ROS robot session or ArduPilot connection to one live entity after
  a valid peer exchange. Duplicate packets, reconnects, and packets after removal
  must not create another entity.
- [ ] Let a YAML group be connection-bound (for example a `connection:` key) so it
  spawns only through its connection.
- [ ] Commit connection requests at the generation boundary in stable order,
  after scheduled spawns; the coordinator owns IDs, insertion, and events.
- [ ] Report creation success or failure; test missing groups, conflicting
  bindings, invalid state, removal, and cleanup. Fail on required-peer timeout.
- [ ] Record accepted inputs and their ticks for replay, separately from
  simulated network messages.

### ROS 1 and ROS 2

Use [rosbridge](https://github.com/RobotWebTools/rosbridge_suite) in separate
containers; evaluate [roslibrust](https://github.com/RosLibRust/roslibrust).

- [ ] Choose the first robot case: clock, odometry, selected sensors, and one
  command type.
- [ ] Test ROS 1 and ROS 2 separately: message names, headers, timestamps,
  frames, `/clock`, and ROS 2 QoS. No host ROS installation in Rust builds.

ROS 1 needs a pinned legacy environment: Noetic reached end of life on
2025-05-31 ([notice](https://www.ros.org/blog/noetic-eol/)).

### ArduPilot

Use a separate SITL process and its
[JSON simulator protocol](https://github.com/ArduPilot/ardupilot/blob/master/libraries/SITL/examples/JSON/readme.md):
actuator packets in, physics state out. Rust owns physics.

- [ ] Decode packets explicitly; test duplicates, loss, reset, timeout, and
  multiple vehicles. Never advance twice for a duplicate frame.
- [ ] Validate one aircraft with FixedWing6DOF or Multirotor: actuator scales and
  signs, attitude, velocity, body rates, and IMU specific force.
- [ ] Validate geographic origin and altitude datum, then fly a short closed-loop
  SITL case.

The C++ `arduplane.xml` uses JSBSim, so it does not establish FixedWing6DOF/SITL
compatibility.

### JSBSim reference fixtures

- [ ] Generate a small saved corpus with [standalone JSBSim](https://jsbsim-team.github.io/jsbsim/)
  for one aircraft and a few control schedules, with a pinned version and asset
  hashes. Ordinary Rust tests use the saved fixtures without JSBSim installed.
- [ ] Align frames, units, inertia, and control meanings, with declared
  tolerances and held-out cases.

Saved JSBSim outputs do not establish real-aircraft fidelity.

## 3. Core upgrades

- [ ] Record the effective configuration in each run's manifest: plugin values
  filled by defaults, overrides, registrations, and seeds.
- [ ] Add YAML forms of the remaining curated missions (four pairs exist; see
  [yaml_missions.rs](../crates/core/tests/yaml_missions.rs)).
- [ ] Let the installed command find missions and assets without the source
  checkout.
- [ ] Add the Rust and Python test suites to CI; the current workflow publishes
  the book only.
- [ ] Burn experiment: pick one workload (for example batched policy inference),
  measure the whole step against a CPU baseline, including transfers and
  synchronization, and keep it only for an end-to-end gain. See
  [Burn's backends](https://burn.dev/docs/burn/).

## 4. More models

Candidates and the shared capabilities they would need are in the
[plugin audit](PLUGIN_COVERAGE_AUDIT.md). Add a model for a research need, not
for catalog completeness. A model with a C++ counterpart joins the comparison
through the checklist in [reference/README.md](../reference/README.md).

## 5. Terrain visualization experiment

- [ ] Try streamed [Cesium](https://cesium.com/platform/cesiumjs/) terrain next
  to the Rerun panels, starting with one agent and a validated origin, in a
  [custom Rerun viewer](https://docs.rerun.io/dev/howto/visualization/extend-ui/)
  or a web dashboard. Check coordinates, shared playback time, and screenshots.

Tile loading must never affect physics, sensing, or simulation time. Check
Cesium's [pricing and redistribution terms](https://cesium.com/platform/cesium-ion/pricing/).
