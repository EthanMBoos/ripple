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
timing ([benchmark.py](../reference/benchmark.py)) is done.

Done: explained the eight-worker slowdown (each phase's hand-off to the thread
pool costs more than light phases' work); benchmarked five workloads with memory
and C++'s multithreaded mode, in a Linux container and on bare-metal macOS; and
sped up single-worker phases and message addresses. Rust is now 1.1-1.8x faster
than C++ on one core where outputs match. Details are in the paper.

Done: spatial queries. World plugins ask `within(position, range)` (and
interactions `pairs_within(range)`); the mission's `spatial_lookup` answers by
scan (default) or a sparse grid, with byte-identical output. SimpleCollision
uses it: a step with 4,096 spread-out agents fell from 19.1 ms to 10.2 ms
(scan) and 5.0 ms (5 m grid); other workloads are unchanged. Design and results are in
[common/spatial.rs](../crates/core/src/common/spatial.rs), the book's
[Finding nearby agents](../book/src/reference/plugin-api.md#finding-nearby-agents),
and the paper. Options for later (range-limited sensors, a faster SphereNetwork,
`nearest`, skipping the grid for small teams) are comments where each would
apply.

Done: message routing by index. Each message visits only its channel's
subscribers (and, for `route_within_entity` networks such as LocalNetwork, only
its own entity's), with byte-identical output and traces. With NoisyState on
every aircraft, a 4,096-agent step fell from 388 ms to 9.7 ms (C++: 388 ms);
see [messages.rs](../crates/core/src/pubsub/messages.rs) and the paper. A topic
every agent publishes and subscribes to on a network that reaches everyone still
has one link per pair.

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
- [ ] Two estimation clean-ups, planned inline (`grep -rn "TODO(estimation)"
  crates reference`): remove the sensors' LocalNetwork messages, and make
  NoisyState measure honestly.
- Later estimation work, each for a concrete experiment: evaluation output that
  records truth, the prediction before correction, belief after it, and the
  readings at the same times (today tests compare `Entity::belief` with
  `Entity::truth`, and `trace.jsonl` holds belief); IMU and GPS sensors, real
  filters, delayed readings (tracking an older relative reading needs the
  observer's pose from when it was taken), reusable sensor profiles, and richer
  tracking (association, filtering). Before an IMU uses aircraft outputs, give
  the motion models acceleration and consistent velocity and rate outputs, and
  define frames, gravity, units, and initialization; that is a separate physics
  change. The design and worked example are in the book's
  [Belief vs truth](../book/src/concepts/belief-vs-truth.md) and
  [Your first localization estimator](../book/src/tutorial/noisy-localization.md).
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
