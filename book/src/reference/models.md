# Selected v1 models

Keep a useful student research workflow, not a large compatibility catalog.
The stock registry currently contains 22 models across all seven roles. Other
compiled Rust models can register through the same interfaces without editing
the simulation loop. No runtime library loading is planned.

| Role | Selected now | Later / excluded and why |
| --- | --- | --- |
| Autonomy | Straight; Rust-native WaypointFollower; AuctionAssign | More research behaviors when needed. MotorSchemas/nested plugin composition and GraphvizFSM are not prerequisites for navigation. Legacy protobuf spawning and camera windows are excluded. |
| Controller | SimpleAircraftControllerPID; SingleIntegratorControllerSimple; AircraftPIDController; Rust-only MotorSpeeds | SimpleAircraft PID roll/glide-slope modes deferred; unsupported modes are errors. MotorSpeeds supplies constant rotor commands for testing, not stabilization or navigation. |
| Motion | SimpleAircraft; SingleIntegrator; FixedWing6DOF; Multirotor | Flight-model limitations are commented beside their equations. Multirotor retains C++ behavior; external-force contact and acceleration sensor access are not exposed. JSBSim is an offline verification candidate, not a runtime plugin. |
| Sensor | NoisyState; NoisyContacts; NoisyPosition (Rust-only teaching example) | NoisyContacts measures every other live entity; no range/FOV, missed detections, or tracker. Cameras and richer sensors are not selected. |
| Interaction | SimpleCollision; GroundCollision; Boundary; Rust-native WaypointBroadcast | Physics-engine collisions, richer geometry, and games/scoring environments deferred. No integration-specific spawning here. |
| Network | GlobalNetwork; LocalNetwork; SphereNetwork | SphereNetwork uses direct 3D distance checks and optional altitude-plane filtering; no spatial index. Legacy delay modes remain rejected; compiled networks can use explicit delay/loss. No OpenCL path. |
| Metrics | SimpleCollisionMetrics, including ground counts | Add experiment-specific metrics as ordinary plugins. No generic statistics/service framework. |

## Deliberate limits

- Boundary publishes one axis-aligned cuboid. Faces are outside, as in C++.
  Sphere/polyhedron/plane, rotation, and drawing are not implemented. Use
  `show_boundary=false` (the Rust default); this is a behavioral region, not
  a physical wall. An autonomy must choose how to respond. Straight keeps its
  old heading if horizontally coincident with the center instead of normalizing
  a zero vector.
- GroundCollision uses a flat `ground_collision_z` in local meters, optional
  team filtering, and removal. Force response and geodetic altitude are errors.
  Startup rejection is separately configurable and, like C++, has no team filter.
- WaypointBroadcast sends an initial route and optionally one timed replacement.
  WaypointFollower owns a list and current index. No services, route planner, or
  generic state-machine framework. Initial/updated broadcasts are not latched
  for future subscribers: late-spawned agents use their configured fallback
  route until a later message. Boundary has the same one-shot delivery limitation.
- The waypoint demos update autonomy each tick. Arrival/overshoot behavior with
  slower autonomy rates, tight aircraft turns, or noisy position at the final
  point is not a navigation guarantee. Choose sensible speed/radius/turn geometry.
- SingleIntegrator preserves C++'s unusual `max_speed`: negative means direct
  velocity; nonnegative means normalize to that magnitude, not clamp. Zero
  command stays zero. `override_heading=true` is not implemented.
- SimpleAircraft preserves legacy model roll/pitch signs, pre-integration limits,
  and its reported vertical-velocity versus altitude-derivative discrepancy.
  No claim of a high-fidelity flight model. `show_text_label` in legacy Straight
  inputs is superseded by Rerun's labels, not a plugin drawing API.
- NoisyState estimates own state, not probabilistic target detection. Rust's
  independent sensor RNG streams differ from C++; its inherited covariance
  payload is not calibrated uncertainty. See [evidence](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/EVIDENCE.md).

ROS 1/2 and ArduPilot are separate future adapter work. They are not reasons to
add default ROS/C++ build dependencies. Burn, terrain, and
spatial acceleration remain in [Roadmap](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/TODO.md), outside this v1 slice.

The plugin audit's selected additions are complete: NoisyContacts and
SphereNetwork + AuctionAssign. The other proposed ports and the force, dynamics
telemetry, battery, game, and geometry extensions they motivate are not selected
backlog. Keep their limitations documented; do not add them for completeness.
AuctionAssign is a single random-bid messaging demonstration with stationary
agents, not a task planner. See [plugin contracts](plugin-api.md#noisycontacts)
for observation types, radio policies, random streams, and examples.
