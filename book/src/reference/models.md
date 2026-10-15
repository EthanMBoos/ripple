# Selected v1 models

Keep a useful student research workflow, not a large compatibility catalog.
The stock registry covers all nine roles. Other
compiled Rust models can register through the same interfaces without editing
the simulation loop. No runtime library loading is planned.

| Role | Selected now | Later / excluded and why |
| --- | --- | --- |
| Autonomy | Straight; Rust-native WaypointFollower; AuctionAssign; Rust-only ForceReplay | ForceReplay repeats a list of forces, one per tick, for MpeParticle tests. More research behaviors when needed. MotorSchemas/nested plugin composition and GraphvizFSM are not prerequisites for navigation. Legacy protobuf spawning and camera windows are excluded. |
| Controller | SimpleAircraftControllerPID; SingleIntegratorControllerSimple; AircraftPIDController; Rust-only MotorSpeeds, MultirotorVelocityController, and AircraftVelocityController | SimpleAircraft PID roll/glide-slope modes deferred; unsupported modes are errors. MotorSpeeds supplies constant rotor commands for testing. MultirotorVelocityController flies Multirotor from the point agents' velocity command (velocity, attitude, rate, mixing); no position hold or yaw command. See [Flying a multirotor](../guides/multirotor.md). AircraftVelocityController turns the same velocity command into the heading, altitude, and speed that SimpleAircraftControllerPID and AircraftPIDController follow (below). |
| Motion | SimpleAircraft; SingleIntegrator; FixedWing6DOF; Multirotor; Rust-only MpeParticle | MpeParticle is the Multi-Agent Particle Environment's point (MPE2 v1.1.1): a force-driven unit mass with MPE's per-step damping, for matching learning benchmarks; checked step by step against MPE2 by `reference/mpe_parity.py`. Flight-model limitations are commented beside their equations. FixedWing6DOF has two built-in aircraft and no default: `airframe: a4d` (the A-4D Skyhawk, from Nelson's *Flight Stability and Automatic Control*) and `airframe: rascal` (ArduPilot's JSBSim Rascal), or a mission gives every number. It computes forces and moments at every RK4 stage, where C++ holds them for the whole step; FixedWing6DOF can also have landing gear (Rust-only, after JSBSim; see the Terrain chapter) and `lift_table` and `drag_table` rows against angle of attack that replace its linear lift and zero-lift drag, so the wing stalls (Rust-only; see the ArduPilot chapter); Multirotor has optional RotorS motor lag, rotor drag, and rolling moment and a built-in Crazyflie 2.0 (see [Flying a multirotor](../guides/multirotor.md)); external-force contact and acceleration sensor access are not exposed. JSBSim is an offline verification candidate, not a runtime plugin. |
| Sensor | NoisyState; NoisyContacts; NoisyPosition (Rust-only teaching example) | NoisyContacts measures every other live entity; no range/FOV, missed detections, or tracker. Cameras and richer sensors are not selected. |
| Localization, tracking | PassThrough (copies one reading; the default for both) | Filters, dead reckoning, and trackers are research plugins; see [Belief vs truth](../concepts/belief-vs-truth.md). |
| Interaction | SimpleCollision; GroundCollision; Boundary; Rust-native WaypointBroadcast and TerrainCollision; Rust-only MpeContact | MpeContact is MPE's soft push between overlapping MpeParticle agents. Physics-engine collisions, richer geometry, and games/scoring environments deferred. No integration-specific spawning here. |
| Network | GlobalNetwork; LocalNetwork; SphereNetwork | SphereNetwork uses direct 3D distance checks and optional altitude-plane filtering; no spatial index. Legacy delay modes remain rejected; compiled networks can use explicit delay/loss. A label with `plugin: SphereNetwork` names an instance, such as `CommsNetwork`. No OpenCL path. |
| Metrics | SimpleCollisionMetrics, including ground counts; Rust-only SimpleSpread | SimpleSpread scores MPE's `simple_spread`: the benchmark return, collisions, and landmark coverage as separate numbers. Add experiment-specific metrics as ordinary plugins. No generic statistics/service framework. |

## Deliberate limits

- Boundary publishes one axis-aligned cuboid. Faces are outside, as in C++.
  Sphere/polyhedron/plane, rotation, and drawing are not implemented; this is
  a behavioral region, not a physical wall. An autonomy must choose how to respond. Straight keeps its
  old heading if horizontally coincident with the center instead of normalizing
  a zero vector.
- GroundCollision uses a flat `ground_altitude_m` in local meters, optional
  team filtering, and removal. There is no force response or geodetic
  altitude.
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
- SingleIntegrator's `max_speed_mps` caps the speed: a faster command keeps its
  direction at `max_speed_mps`; negative (the default) means no limit.
- SimpleAircraft turns as a coordinated turn does, at g tan(bank) / speed, so
  its turn radius grows with speed squared. Its limits are physical:
  speeds (`min_speed_mps`, `max_speed_mps`), bank and pitch (`max_roll_deg`,
  `max_pitch_deg`), their rates, acceleration and deceleration
  (`max_acceleration_mps2`, `max_deceleration_mps2`), and climb and descent
  rates (`max_climb_rate_mps`, `max_descent_rate_mps`). `airframe: rascal`
  loads limits fit to FixedWing6DOF's Rascal (`reference/simple_aircraft_fit.py`
  fits and checks them), which any key overrides; without it the limits are
  generic round numbers, not any aircraft. Its `throttle` input asks for
  throttle / 5 m/s², within the acceleration limits.
- AircraftVelocityController: put it before SimpleAircraftControllerPID or
  AircraftPIDController to fly an aircraft from a velocity command. The
  heading is the command's direction (kept when the command has no horizontal
  part), the speed its magnitude, and the altitude target moves at its climb
  rate (staying within `max_altitude_error_m`, default 10 m, of the
  aircraft). The aircraft's own limits bound speed, climb, and turn; there is
  no holding mode, so a command it can't fly shows as a shortfall.
- SimpleAircraftControllerPID's altitude loop is stable only while
  speed × D × step < 2: with the default D of 0.8 and a 0.1 s step, below
  25 m/s; faster, pitch flips between its rate limits every step. Use motion
  substeps (`motion_multiplier: 5` gives 0.02 s and 125 m/s) for faster
  aircraft, such as the `rascal` profile at full speed.
- SimpleAircraft is a kinematic stand-in: no claim of a high-fidelity flight
  model.
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
for reading types, radio policies, random numbers, and examples.

## FixedWing6DOF parameters

Keys follow [Parameter names](../development/rust-style.md#parameter-names).
`airframe: a4d` or `airframe: rascal` fills every vehicle key; any key the
mission gives overrides one value. Without an airframe the mission gives them
all.

| Keys | Meaning |
| --- | --- |
| `mass_kg`, `inertia_kgm2` or `inertia_slug_ft2` | mass; inertia as three rows |
| `wing_span_m` (b), `wing_area_m2` (S), `mean_chord_m` (c̄), `oswald_efficiency` (e) | geometry; induced drag is C_L² / (π e AR), AR = b / c̄ |
| `min_thrust_n`, `max_thrust_n`; `min_throttle`, `max_throttle` | thrust, linear in throttle between them |
| `min_elevator_rad` ... `max_rudder_rad` | control surface limits |
| `air_density_kgpm3`, `gravity_mps2`, `wind_mps` | environment: 1.225, 9.81, and `[east, north, up]` of zero by default |

Each coefficient is a stability or control derivative, per radian and
dimensionless, in the notation of Beard and McLain (*Small Unmanned Aircraft*,
ch. 4) and Nelson (*Flight Stability and Automatic Control*). Rates enter
nondimensionalized, as p b / 2V, q c̄ / 2V, and r b / 2V.

| Key | Symbol | Meaning |
| --- | --- | --- |
| `c_lift_0`, `c_lift_alpha` | C_L0, C_Lα | lift at zero angle of attack, and its slope |
| `c_lift_q`, `c_lift_elevator` | C_Lq, C_Lδe | lift from pitch rate and from the elevator |
| `c_drag_0`, `c_drag_alpha`, `c_drag_elevator` | C_D0, C_Dα, C_Dδe | zero-lift drag, its rise with \|α\|, and elevator drag |
| `c_side_beta`, `c_side_rudder` | C_Yβ, C_Yδr | side force from sideslip and from the rudder |
| `c_roll_beta` | C_ℓβ | dihedral effect: roll from sideslip |
| `c_roll_p`, `c_roll_r` | C_ℓp, C_ℓr | roll damping; roll from yaw rate |
| `c_roll_aileron`, `c_roll_rudder` | C_ℓδa, C_ℓδr | roll from the ailerons and the rudder |
| `c_pitch_0`, `c_pitch_alpha` | C_m0, C_mα | pitching moment at zero α; static stability (negative is stable) |
| `c_pitch_q`, `c_pitch_elevator` | C_mq, C_mδe | pitch damping; elevator authority |
| `c_yaw_beta` | C_nβ | weathercock stability: yaw from sideslip |
| `c_yaw_p`, `c_yaw_r` | C_np, C_nr | yaw from roll rate; yaw damping |
| `c_yaw_aileron`, `c_yaw_rudder` | C_nδa, C_nδr | adverse yaw from the ailerons; rudder authority |

`lift_table` and `drag_table` (rows of `[alpha_rad, C]`) replace `c_lift_0 +
c_lift_alpha α` and `c_drag_0 + c_drag_alpha |α|`, so the wing can stall;
induced and elevator drag are still added. `enable_ground_spring` (on by
default) is a flat spring at z = 0, which `landing_gear` or terrain replaces;
`enable_launcher` with `launch_at_s`, `launch_speed_mps`, and
`launch_acceleration_mps2` is a catapult.
