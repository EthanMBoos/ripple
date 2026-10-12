# Selected v1 models

Keep a useful student research workflow, not a large compatibility catalog.
The stock registry covers all nine roles. Other
compiled Rust models can register through the same interfaces without editing
the simulation loop. No runtime library loading is planned.

| Role | Selected now | Later / excluded and why |
| --- | --- | --- |
| Autonomy | Straight; Rust-native WaypointFollower; AuctionAssign | More research behaviors when needed. MotorSchemas/nested plugin composition and GraphvizFSM are not prerequisites for navigation. Legacy protobuf spawning and camera windows are excluded. |
| Controller | SimpleAircraftControllerPID; SingleIntegratorControllerSimple; AircraftPIDController; Rust-only MotorSpeeds and MultirotorVelocityController | SimpleAircraft PID roll/glide-slope modes deferred; unsupported modes are errors. MotorSpeeds supplies constant rotor commands for testing. MultirotorVelocityController flies Multirotor from the point agents' velocity command (velocity, attitude, rate, mixing); no position hold or yaw command. See [Flying a multirotor](../guides/multirotor.md). |
| Motion | SimpleAircraft; SingleIntegrator; FixedWing6DOF; Multirotor | Flight-model limitations are commented beside their equations. FixedWing6DOF has two built-in aircraft and no default: `airframe: a4d` (the A-4D Skyhawk, from Nelson's *Flight Stability and Automatic Control*) and `airframe: rascal` (ArduPilot's JSBSim Rascal), or a mission gives every number. It computes forces and moments at every RK4 stage, where C++ holds them for the whole step; FixedWing6DOF can also have landing gear (Rust-only, after JSBSim; see the Terrain chapter) and `lift_table` and `drag_table` rows against angle of attack that replace its linear lift and zero-lift drag, so the wing stalls (Rust-only; see the ArduPilot chapter); Multirotor has optional RotorS motor lag, rotor drag, and rolling moment and a built-in Crazyflie 2.0 (see [Flying a multirotor](../guides/multirotor.md)); external-force contact and acceleration sensor access are not exposed. JSBSim is an offline verification candidate, not a runtime plugin. |
| Sensor | NoisyState; NoisyContacts; NoisyPosition (Rust-only teaching example) | NoisyContacts measures every other live entity; no range/FOV, missed detections, or tracker. Cameras and richer sensors are not selected. |
| Localization, tracking | PassThrough (copies one reading; the default for both) | Filters, dead reckoning, and trackers are research plugins; see [Belief vs truth](../concepts/belief-vs-truth.md). |
| Interaction | SimpleCollision; GroundCollision; Boundary; Rust-native WaypointBroadcast and TerrainCollision | Physics-engine collisions, richer geometry, and games/scoring environments deferred. No integration-specific spawning here. |
| Network | GlobalNetwork; LocalNetwork; SphereNetwork | SphereNetwork uses direct 3D distance checks and optional altitude-plane filtering; no spatial index. Legacy delay modes remain rejected; compiled networks can use explicit delay/loss. XML `name="CommsNetwork"` names an instance, as in C++. No OpenCL path. |
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
  Startup rejection is separately configurable and, like C++, has no team
  filter.
- TerrainCollision removes agents that reach the mission's terrain, checking the
  path moved each step; see [Terrain](../guides/terrain.md).
- AircraftPIDController takes the same `airframe` and loads the gains and
  attitude limits tuned for it; without one, the mission gives them all.
  `a4d`'s altitude derivative gain is 0.002: at its 220 m/s, 0.01 made each
  altitude correction larger than the motion it reacts to, so a pitch or roll
  upset grew into swings of hundreds of metres in a 45 s test; at 0.002 they
  die out ([measurement](https://github.com/EthanMBoos/ripple/blob/main/paper/data/rk4-loads/summary.md),
  part C). `rascal` flies the same gains, measured adequate on the Rascal at
  20 m/s (the derivative gain makes no difference there from 0 to 0.02), with
  ArduPlane's attitude limits: pitch 20 and roll 45 degrees instead of 45 and
  60, which let an altitude step pitch the Rascal to 48 degrees. Its PIDs use
  the substep as their timestep.
- WaypointBroadcast sends an initial route and optionally one timed replacement.
  WaypointFollower owns a list and current index. No services, route planner, or
  generic state-machine framework. Initial/updated broadcasts are not latched
  for future subscribers: late-spawned agents use their configured fallback
  route until a later message. Boundary has the same one-shot delivery
  limitation.
- The waypoint demos update autonomy each tick. Arrival/overshoot behavior with
  slower autonomy rates, tight aircraft turns, or noisy position at the final
  point is not a navigation guarantee. Choose sensible speed/radius/turn
  geometry.
- SingleIntegrator preserves C++'s unusual `max_speed`: negative means direct
  velocity; nonnegative means normalize to that magnitude, not clamp. Zero
  command stays zero. `override_heading=true` is not implemented.
- SimpleAircraft preserves legacy model roll/pitch signs, pre-integration
  limits, and its reported vertical-velocity versus altitude-derivative
  discrepancy. No claim of a high-fidelity flight model. `show_text_label` in
  legacy Straight inputs is superseded by Rerun's labels, not a plugin drawing
  API.
- NoisyState estimates own state, not probabilistic target detection. Rust's
  independent sensor RNG streams differ from C++; its inherited covariance
  payload is not calibrated uncertainty. See the [paper's
  validation](https://github.com/EthanMBoos/ripple/blob/main/paper/README.md).

ROS and JSBSim integrations are not planned; ArduPlane SITL is optional; see
[Flying ArduPlane](../guides/ardupilot.md). Terrain is covered in
[Terrain](../guides/terrain.md).

The plugin audit's selected additions are complete: NoisyContacts and
SphereNetwork + AuctionAssign. The other proposed ports and the force, dynamics
telemetry, battery, game, and geometry extensions they motivate are not selected
backlog. Keep their limitations documented; do not add them for completeness.
AuctionAssign is a single random-bid messaging demonstration with stationary
agents, not a task planner. See [plugin contracts](plugin-api.md#noisycontacts)
for reading types, radio policies, random streams, and examples.
