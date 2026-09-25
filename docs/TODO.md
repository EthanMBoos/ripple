# Remaining work

Current usage and implemented behavior are in the [book](../book/src/SUMMARY.md).
[Evidence](EVIDENCE.md) records checks and known failures;
[reference notes](REFERENCE_NOTES.md) explain C++ differences.

Priorities: fix the scale/RNG gaps, extend useful model coverage, then implement
one ROS or ArduPilot connection-to-entity pilot. Build new interfaces around a
concrete experiment. The existing starter and shared CLI/sweep/Slurm workflow
remain the normal development path.

## Scope decisions

| Area | Decision |
| --- | --- |
| Mission input | Keep XML single runs and existing overlays; YAML is native and gets templates/sweeps. |
| Runtime integrations | Optional ROS 1 and ROS 2 via a separate bridge; ArduPilot via UDP. |
| JSBSim | Offline flight-model verification fixtures. |
| GPU work | Evaluate Burn for one useful workload. |
| Viewer | Keep Rerun and its current recording playback. Terrain is an optional experiment below. |
| C++ machinery excluded | Runtime plugin-library discovery/loading, legacy protobuf/string-map spawning, OpenCL, VTK, and production JSBSim integration. |

Additional model candidates are recorded in the [plugin audit](PLUGIN_COVERAGE_AUDIT.md).
Select them for a research need rather than treating the C++ catalog as a checklist.

## 1. Mission input and experiments

The [mission guide](../book/src/guides/yaml-missions.md) covers the implemented
XML/YAML, template, override, comparison, and sweep behavior.

- [ ] Fix startup queue overflow: the recorded 1,500-agent case fills the 1,024
  message publisher queue with `GlobalNetwork/EntityGenerated` events. Recheck
  large populations and cleanup before promising thousand-agent runs.
- [ ] Add YAML forms of the remaining curated missions. The four existing pairs
  are listed in [yaml_missions.rs](../crates/core/tests/yaml_missions.rs).
- [ ] Add [direct Rust construction](LIBRARY_FIRST_REFACTOR.md) for experiments
  that calculate their setup in code, sharing validation with mission loading.
- [ ] Record effective configuration: filled plugin defaults, overrides, assets,
  registrations, seeds, and applicable adapter/backend versions. The current
  manifest records supplied plugin values, not every value filled by `Default`.
- [ ] Revisit explicit defaults/asset paths when needed. Preserve working
  `SCRIMMAGE_PLUGIN_PATH` XML overlays in the meantime.

XML is frozen. Its inherited-motion replacement and entity-tag differences are
recorded in [reference notes](REFERENCE_NOTES.md#mission-compatibility).

## 2. Optional integrations

Implement one end-to-end pilot at a time using `spawn(SpawnRequest)` in
[generation.rs](../crates/core/src/simcontrol/generation.rs).

### First pilot: creating entities from a connection

Do these with the first ROS or ArduPilot pilot, when there is a real peer to
test against:

- [ ] Resolve the configured group and supply the peer's starting state through
  `SpawnRequest`; add velocity and attitude fields as the pilot requires.
- [ ] Bind one configured ROS robot session or ArduPilot connection to one live
  entity after successful construction. Readiness means a valid peer exchange,
  not merely an open socket. Duplicate packets, reconnects, and packets after
  removal must not create another entity.
- [ ] Let a YAML group be connection-bound (for example a `connection:` key) so
  it spawns only through its connection, never through `count`. XML stays frozen.
- [ ] Commit connection requests at the generation boundary in stable order,
  after scheduled spawns; the coordinator owns IDs, insertion, and events.
- [ ] Return explicit creation success/failure. Test missing groups, conflicting
  bindings, invalid initial state, partial construction failure, removal, and
  cleanup. Bound pending requests and fail on required-peer timeout or
  disconnect; automatic reconnect/resume can wait.
- [ ] Record accepted inputs and their application ticks. Use a typed structural
  request path, separate from potentially lossy simulated network messages.

The adapter owns protocol details; generation receives ordinary Rust
`SpawnRequest`s. Legacy `frames.bin` protobuf output stays independent of spawning.
Adapters keep sockets and wire types outside model equations and default-build
dependencies. Apply typed inputs at defined phases with explicit units and
ENU/NED/body transforms. Bound waits/queues, reject stale inputs, and define
failure, disconnect, and shutdown behavior. Live pause/resume is later work.

### ROS 1 and ROS 2

Use [rosbridge](https://github.com/RobotWebTools/rosbridge_suite) in separate
Linux/container environments; evaluate
[roslibrust](https://github.com/RosLibRust/roslibrust) as the Rust client.

- [ ] Choose the first robot case: clock, odometry, selected sensor observations,
  and one command type. Keep configuration to endpoint, namespace/topic mappings,
  rates, and timeouts.
- [ ] Test ROS 1 and ROS 2 deployments separately: message names, headers,
  timestamps, frames, `/clock`, simulation-time settings, and ROS 2 QoS.
  Rust builds must not depend on a host ROS installation.
- [ ] Record accepted input ticks for replay. Asynchronous ROS traffic alone
  does not provide deterministic lockstep. Use local/private bridge endpoints.

ROS 1 requires a maintained, pinned legacy environment: Noetic reached EOL on
2025-05-31. Do not assume `ros1_bridge` works in the Ubuntu 24.04 reference image.
See the [EOL notice](https://www.ros.org/blog/noetic-eol/) and
[bridge compatibility](https://index.ros.org/p/ros1_bridge/).

### ArduPilot

Use a separate SITL process and its
[JSON simulator UDP protocol](https://github.com/ArduPilot/ardupilot/blob/master/libraries/SITL/examples/JSON/readme.md):
binary actuator packets in, timestamped JSON physics state out. Rust owns physics.

- [ ] Decode lengths, magic, counters, and channels explicitly. Test initial
  exchange, duplicate/lost packets, reset, rates/substeps, multiple vehicle
  endpoints, timeout, and cancellation. Never advance twice for a duplicate frame.
- [ ] Select and validate the first aircraft case using the existing
  FixedWing6DOF or Multirotor models. Map actuators with explicit scales, limits,
  and signs; provide attitude, velocity, body rates, and IMU specific force at
  the required sample times. Test gravity handling and avoid applying noise twice.
- [ ] Validate geographic origin, altitude datum, and frame conversions, then
  demonstrate a short closed-loop SITL case. Preserve global phase boundaries
  and keep offline physics tests independent of sockets.

The C++ `arduplane.xml` defaults to JSBSimControl; it does not establish
FixedWing6DOF/SITL compatibility. Its ArduPilot plugin uses an older native binary
physics packet. Source caveats for other flight models are in the
[plugin audit](PLUGIN_COVERAGE_AUDIT.md#source-quality-cautions).

### JSBSim reference fixtures

Use [standalone JSBSim](https://jsbsim-team.github.io/jsbsim/) to generate a small
saved reference corpus. The aircraft, envelope, and fidelity target remain open.

- [ ] Choose one aircraft and a few explicit control schedules: steady flight
  and small throttle/elevator/aileron changes. Compare either equivalent physics
  trajectories or declared response metrics for a simplified model.
- [ ] Add a fixture generator under `reference/`, with pinned JSBSim version,
  asset hashes, initial/trim state, model parameters, input schedules, and timing.
  Ordinary Rust tests consume saved fixtures without JSBSim installed.
- [ ] Align frames, units, inertia, gravity/wind, and control meanings. Test
  conversions independently, use declared tolerances and held-out cases, and
  retain analytical tests alongside the software reference.

Keep SimpleAircraft's existing C++ contract separate from aerodynamic model
validation. Saved JSBSim outputs do not establish real-aircraft fidelity.

## 3. Burn experiment

- [ ] Pick one workload, such as batched policy inference or a tensor-friendly
  sensor; establish a CPU baseline and test an optional Burn backend on this Mac.
- [ ] Measure the whole step, including packing, device startup, transfers,
  synchronization, readback, and memory. Require an end-to-end benefit.
- [ ] If batching helps, gather/execute/commit inside the appropriate phase,
  preserving entity-to-batch identity across spawn/removal and completing before
  downstream consumers run.
- [ ] Keep backend/tensor machinery inside the selected implementation. Record
  backend, hardware, precision, seeds, and numerical tolerances separately from
  exact CPU worker equality.

Start with [Burn's backend documentation](https://burn.dev/docs/burn/).

## 4. Verification gaps

Existing coverage and reproduction commands are in [EVIDENCE.md](EVIDENCE.md).

- [ ] Fix the Ubuntu/libstdc++ spawn RNG mismatch while preserving coordinator
  ownership and draw order. Rerun the current Docker matrix and retain failures;
  distinguish the intentional survivor-metrics difference from RNG regressions.
- [ ] Map every built-in to direct and composed tests. Expand meaningful turning,
  altitude, mixed-rate, spawn/removal, and scale coverage where missing.
- [ ] Extend [Docker comparisons](../reference/README.md) for selected behavior.
  The checker currently requires expanded XML; include packaging is separate.
- [ ] Add direct C++ lifecycle-event comparison when selected behavior needs it;
  frames alone do not prove event equivalence.
- [ ] Exercise populations/churn at increasing sizes: memory, queue use,
  throughput, and failure cleanup. Continue exact Rust frame/event/summary
  comparisons across 1/2/8 workers and recording on/off.
- [ ] Run an actual Slurm campaign; submission is currently mock-tested.

Retain `straight_cpu_mul.xml` as the upstream typo fixture; genuine substep
coverage comes from `verification/aircraft-substeps-spawning.xml`.

## 5. Packaging and checks

- [ ] Let the installed shared command find missions/assets without the source
  checkout. Plugin defaults are already compiled in.
- [ ] Add the verified Rust/Python test and comparison subset to CI, with known
  C++ differences explicit. The current workflow publishes the book only.

Keep the current Rerun view and replay controls. The recorded visual limitations
and checks still needed are in [EVIDENCE.md](EVIDENCE.md#visual-checks).

## 6. Spatial queries and routing performance

- [ ] Benchmark collision checks, spatial sensors, and routing on spread-out and
  clustered populations. Keep the simple implementation as the correctness baseline.
- [ ] Try one engine-owned read-only spatial lookup with collision and one other
  consumer. Compare a grid and a tree where query ranges differ substantially.
- [ ] Refresh at the snapshot each consumer reads: before autonomy, after motion,
  and after position-changing interactions as needed. Preserve candidate/event
  ordering and RNG draws across workers.
- [ ] Index subscribers separately by topic and, for LocalNetwork, entity.
  Range-limited networks can also use spatial candidates. Measure complete runs
  and compare outputs before expanding the API.

C++ shares an R-tree for SphereNetwork/Boids, but SimpleCollision and
ContactBlobCamera still scan entities. Its pre-motion rebuild is stale for
post-motion consumers; do not inherit that timing accidentally.

## 7. Optional terrain experiment

Try streamed Cesium terrain alongside Rerun debug panels in one window using
saved recordings. This is an unprototyped visualization experiment.

- [ ] Start with one agent and a validated origin. Evaluate a
  [CesiumJS](https://cesium.com/platform/cesiumjs/) webview in a
  [custom Rerun viewer](https://docs.rerun.io/dev/howto/visualization/extend-ui/),
  using [Wry](https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html#method.build_as_child).
- [ ] Check ENU-to-Earth-fixed positions, attitude and altitude datum, shared
  playback time, resizing, focus/overlays, scrubbing, looping, and screenshots.
  The webview is a separate rendering surface; capture and layout need testing.
- [ ] If it works cleanly, test multiple agents, spawn/removal, performance, and
  missing token/network behavior. Save screenshots and a short feasibility report.
- [ ] If native embedding is awkward, consider a web dashboard using Rerun's
  [playback API](https://ref.rerun.io/docs/js/0.36.2/web-viewer/classes/WebViewer.html),
  or stop the experiment.

Check [content eligibility/pricing](https://cesium.com/platform/cesium-ion/pricing/)
when trying it; preserve attribution and verify caching/redistribution rights.
Viewer tile loading must not determine physical terrain, collisions, sensing,
or simulation time.
