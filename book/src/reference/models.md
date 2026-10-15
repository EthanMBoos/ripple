# Built-in models

Every built-in, by type: what it does, its main settings, what it is checked
against, and its limits. Each one's full parameter list is the parameter
struct at the top of its source file, under `crates/core/src/plugin/`.
"The comparison" is the check of Ripple's core against SCRIMMAGE, the C++
simulator it began as a port of; [How models are checked](checks.md)
describes it and the other checks.

## Autonomy

- **Straight** flies toward a distant point along its starting heading,
  holding altitude and speed (`speed_mps`). With
  `enable_boundary_control`, it turns back toward the centre of a Boundary
  region when it leaves it. Checked in the comparison.
- **WaypointFollower** follows a list of points (`waypoints_m`, `speed_mps`,
  `arrival_radius_m`, `repeat`), or a route that WaypointBroadcast sends. It
  writes both aircraft commands (heading, altitude, speed) and a velocity
  command, so it drives point agents, multirotors, and aircraft alike. An
  aircraft's route should repeat, since a fixed-wing aircraft can't stop at
  the last point. Arrival is not guaranteed with slow autonomy rates, tight
  turns, or noisy position; choose sensible speed and radius.
- **AuctionAssign** is a single-round auction, a messaging example. Entity 1
  starts it (`auctioneer`), every recipient bids a random number in (0, 10),
  and after `auction_duration_s` (default 5) the auctioneer publishes the
  highest bid received. It runs on the network named by `network` (default
  SphereNetwork). There are no retries, no task execution, and one round at
  a time; agents hold still. Its payloads and topics (`AuctionStart`,
  `AuctionBid`, `AuctionResult`) are exported from `plugin::autonomy`.
  Checked in the comparison.
- **ConstantVelocity** writes a fixed velocity command (`velocity_mps`), and
  **ForceReplay** a fixed list of forces, one per tick (`forces_n`). Both are
  test drivers, not behaviors: ForceReplay is how `reference/mpe_parity.py`
  feeds MPE2's actions to MpeParticle.

## Controllers

- **SimpleAircraftControllerPID** turns desired heading, altitude, and speed
  into throttle and roll and pitch rates for SimpleAircraft (`heading_pid`,
  `altitude_pid`, `speed_pid`). Its altitude loop is stable only while speed ×
  D × step < 2: with the default D of 0.8 and a 0.1 s step, below 25 m/s. For
  faster aircraft, use substeps (`motion_multiplier: 5` allows 125 m/s).
- **AircraftPIDController** flies FixedWing6DOF with nested heading/altitude
  and roll/pitch PID loops. `airframe: a4d` or `rascal` loads gains and
  attitude limits tuned for that aircraft; without one, the mission gives them
  all. The `a4d` altitude derivative gain is 0.002, since 0.01 turned upsets
  into growing swings at 220 m/s. `rascal` uses ArduPlane's attitude limits
  (pitch 20°, roll 45°). Checked by `rascal_control.py`.
- **AircraftVelocityController** goes before either aircraft controller and
  turns a velocity command into heading, speed, and a climbing altitude
  target (within `max_altitude_error_m`, default 10 m). The aircraft's own
  limits bound what it can fly; a command it can't fly shows as a shortfall.
- **MultirotorVelocityController** flies Multirotor from a velocity command:
  velocity, attitude, body rate, then motor mixing. It carries its own copy of
  the vehicle's numbers, so they can be set apart from the true vehicle to
  study model error. No position hold or yaw command. Checked by
  `multirotor_control.py`; see [Flying a multirotor](../guides/multirotor.md).
- **SingleIntegratorControllerSimple** passes a velocity command to
  SingleIntegrator, and **MotorSpeeds** writes constant rotor speeds
  (`speeds_radps`) for tests.

## Motion

- **SingleIntegrator** moves at the commanded velocity at once. With
  `max_speed_mps` set, a faster command keeps its direction at that speed.
  No mass, forces, or lag. Checked in the comparison.
- **MpeParticle** is the Multi-Agent Particle Environment's agent (MPE2
  v1.1.1): a unit mass pushed by a force, with MPE's per-step `damping`. It is
  valid only at MPE's 0.1 s step with one substep, since the damping is per
  step. Checked step by step against MPE2 (`mpe_parity.py`).
- **SimpleAircraft** is a kinematic aircraft: speed, roll, and pitch follow
  commands within rate and range limits, and it turns as a coordinated turn
  does, at g tan(bank) / speed. `airframe: rascal` loads limits fit to
  FixedWing6DOF's Rascal (within 3%, `simple_aircraft_fit.py`); without it the
  limits are generic round numbers. Its `throttle` input asks for throttle ×
  5 m/s² of acceleration, within the limits. A stand-in, not a flight model.
  Checked in the comparison.
- **FixedWing6DOF** is a rigid-body aircraft under aerodynamic forces from
  linear stability derivatives, integrated with RK4. Two aircraft are built
  in: `airframe: a4d` (the A-4D Skyhawk, from Nelson) and `airframe: rascal`
  (ArduPilot's JSBSim Rascal); any key overrides one value, and without an
  airframe the mission gives every number. Optional: `lift_table` and
  `drag_table` so the wing stalls, `landing_gear` (see
  [Terrain](../guides/terrain.md#landing-gear)), steady `wind_mps`, and a
  catapult. Not modelled: propeller effects, ground effect, spins, unsteady
  aerodynamics, engine dynamics, and gusts. Checked in the comparison on both
  aircraft and for step-size convergence (`rk4_loads.py`).
  Its parameters are [below](#fixedwing6dof-parameters).
- **Multirotor** is a rigid-body multirotor with thrust and torque growing
  with rotor speed squared, RK4, and a built-in Crazyflie 2.0
  (`airframe: crazyflie2`). Optional RotorS effects: motor lag, rotor drag, and
  rolling moment. Checked in the comparison and against RotorS's
  motor code, Crazyflow's dynamics, and the Crazyflie's measured
  identification; see [Flying a
  multirotor](../guides/multirotor.md).

## Sensors

- **NoisyState** measures the entity's own state with Gaussian error: a bias
  and a standard deviation per axis for position (`position_bias_m`,
  `position_stddev_m`), velocity (`velocity_bias_mps`,
  `velocity_stddev_mps`), and attitude (`attitude_bias_rad`,
  `attitude_stddev_rad`, roll, pitch, yaw). All default to zero, so set only
  the noise you want. Angular velocity is exact. Each error is keyed by the
  reading, quantity, and axis. Checked in the comparison, sample by
  sample. Try `missions/noisy-state.yaml`.
- **NoisyContacts** measures every other live entity, with the same settings.
  No range or field of view, missed detections, or tracks. Each error is keyed
  by the contact it measures. Checked in the comparison, sample by sample and
  by distribution. Try `missions/noisy-contacts.yaml`.
- **NoisyPosition** measures world position (`bias_m`, `stddev_m`): a small
  example for writing a sensor.

Neither NoisyState nor NoisyContacts reports a covariance, which a filter
would need.

## Estimators

**PassThrough** is the default for both localization and tracking. It copies
the latest reading into belief and has no model of its own, so it adds no
model error. With the default perfect measurement, belief equals truth at
every decision.

## Interactions

- **SimpleCollision** destroys both agents of a pair closer than
  `collision_range_m`, for team and non-team pairs as enabled, and checks
  spawn positions at startup. Checked in the comparison.
- **GroundCollision** removes agents at or below a flat `ground_altitude_m`,
  optionally for one `team`. No force response. Checked in the comparison.
- **TerrainCollision** removes agents that reach the mission's terrain,
  checking the path moved each step against the interpolated ground; see
  [Terrain](../guides/terrain.md).
- **Boundary** publishes an axis-aligned box (`center_m`, `lengths_m`). It is
  a region autonomies respond to, not a wall. It publishes once, so agents
  that spawn later don't receive it. Checked in the comparison.
- **WaypointBroadcast** publishes a route (`waypoints_m`) and optionally one
  replacement (`update_waypoints_m` at `update_at_s`). Agents that spawn after
  a broadcast use their own configured route until the next one.
- **MpeContact** is MPE's soft push between overlapping MpeParticle agents
  (`radius_m`, `contact_force`, `contact_margin_m`), applied as a force on the
  next step. Checked with MpeParticle against MPE2.

## Networks

- **GlobalNetwork** delivers to every subscriber, after `delay_s` (default
  0). Every mission has one.
- **LocalNetwork** delivers only between plugins on the same entity.
- **SphereNetwork** delivers between entities closer than `range_m` (default
  100 m), using positions after motion. Each delivery succeeds with
  `transmit_probability` (default 1), drawn per receiving entity, and arrives
  after `delay_s`. `enable_altitude_split` allows a link only when both ends
  are on the same side of `split_altitude_m`, within `split_margin_m`. World
  plugins have no position, so they can't use it; give their traffic to
  GlobalNetwork. Checked in the comparison, loss included.

## Metrics

- **SimpleCollisionMetrics** counts, per team, entities, flight time, and
  team, non-team, and ground collisions, and scores them with
  `flight_time_weight`, `team_collision_weight`, and
  `non_team_collision_weight`. Checked in the comparison.
- **SimpleSpread** scores MPE's `simple_spread`: the benchmark return,
  collisions, and landmark coverage as separate numbers. Checked against
  MPE2's returns.

## FixedWing6DOF parameters

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
