# C++ reference findings

Source review: sibling `../scrimmage`, branch `Ubuntu-24.04`, commit
`4bdf41fb06facaea358d478d9a07aa4d77fcac27` (2026-09-23/24). This identifies the
reviewed source, not a required pin. Follow the branch for new comparisons. The
[roadmap](TODO.md) defines selected scope; the [book](../book/src/SUMMARY.md)
describes current Rust APIs. Comparison results are in the
[paper](../paper/README.md).

## Corrections to the copied C++ guides

These findings come from source, rather than the synthesized guides in the
[book appendix](../book/src/appendix/cpp.md).

| Source in the C++ checkout | Finding |
| --- | --- |
| `src/simcontrol/SimControl.cpp`, `run_single_step` / `run_entities` | Generate and log pre-step contacts; autonomy; all controller substeps; all motion substeps; sensors; interactions; networks; metrics; removal/output/time. Controller and motion substeps are not interleaved on this branch; they were until 2025-12, and `benchmarking-edits` restores that (see [Substep order](#substep-order-a-c-regression-fixed-on-the-comparison-branch)). |
| Same file, network/metrics phases | Metrics can consume interaction messages in the same tick. Delivery is not universally delayed to the next tick. |
| Same file, `run_logging` / finalization | Pre-step state is labeled `t + dt`; terminal logging repeats the final timestamp with terminal state. |
| `src/plugins/network/LocalNetwork/LocalNetwork.cpp` | Reachability requires the same non-null parent entity, not proximity. |
| `src/plugins/interaction/SimpleCollision/SimpleCollision.cpp` | Collision uses strict distance against `collision_range`, not summed entity/visual radii. Startup rejection is separate. |
| `src/entity/Entity.cpp` | Truth and belief initially alias; `set_state_belief` detaches belief. Rust keeps belief separate and has estimators own it (see the estimation difference below). |

Preserve these observable timing/recording contracts during refactors. Current
Rust lifecycle and delivery rules are in the
[plugin reference](../book/src/reference/plugin-api.md#execution-and-lifecycle).

## Mission compatibility

- `<network name="CommsNetwork">SphereNetwork</network>` names a network
  instance, as upstream `missions/auction_assign.xml` does. Rust honors the
  attribute; the default name is the plugin name. Networks run in name order,
  like C++'s map.

- C++ can replace an inherited motion model. Rust's XML reader concatenates
  inherited and local declarations, then rejects the second model. It also does
  not retain entity `tag` values as spawn-template identities. XML is frozen;
  use YAML's whole-slot replacement for new missions.
- The original `straight_cpu_mul.xml` misspells `motion_multipler`. Keep the
  fixture unchanged; `verification/aircraft-substeps-spawning.xml` tests real
  substeps.
- `RIPPLE_PLUGIN_PATH` discovers optional XML defaults overlays, not code
  (C++ reads `SCRIMMAGE_PLUGIN_PATH`; the separate name keeps a C++ install's
  overlays from changing Ripple's defaults). `RIPPLE_MISSION_PATH` likewise
  replaces `SCRIMMAGE_MISSION_PATH`. YAML configuration does not depend on
  these overlays.
- Rust/YAML position variance defaults to zero; XML retains its legacy default
  of `[100, 100, 0]` m². The XML reader translates positive scalar `speed` into
  world-X velocity when the velocity vector is zero. Native scenarios specify
  `velocity_mps` directly. Spawn sampling and draw order are shared afterward.

## Intentional differences from C++

- **Estimation owns belief.** C++ lets any plugin set belief, falls back to
  truth when none does, and gives autonomy the true contacts. Rust has one
  localization and one tracking estimator per entity (default `PassThrough`)
  that turn sensor readings into belief at the end of each tick; autonomy and
  controllers see belief only (`context.state`, `context.contacts`); sensors
  can't write belief; several possible state or contact sensors is a load
  error rather than "last writer wins". With no sensor, a perfect measurement
  taken after interactions makes belief equal truth at the next decision, so
  existing missions give identical outputs: 75 runs (25 missions at 1, 2
  and 8 workers; 300 frame, event, summary and trace files) and the
  follow-nearest sweep matched the previous commit. Two consequences to
  know: decision plugins now initialize after the startup interactions (C++
  initializes them while building each entity), and an entity that spawns is
  unknown to others until the next tick. See the book's
  [Belief vs truth](../book/src/concepts/belief-vs-truth.md). Estimation
  experiments are timed on their own: C++ `SimpleINS` accumulates synthetic
  drift inside a sensor, so it is not an equivalent workload for a
  command-based predictor.
- **Straight and NoisyState messages.** C++ Straight steers by the newest
  `StateWithCovariance` message when one arrives. Rust Straight steers by
  belief, which holds the same measurement at the same time, and subscribes to
  the message only so deliveries match C++. NoisyState and NoisyContacts still
  publish their LocalNetwork messages for the same reason; removing them would
  change delivered-message counts and is left for a separate change.

- **Survivor flight time:** C++ publishes `EntityPresentAtEnd` after its last
  network phase, so metrics credit survivors only until the latest removal. Rust
  credits them until report time. In `networks-local-global.xml`, surviving
  teams report 29.9 s instead of 0.4 s. Frames are unaffected; flight-time
  summary columns differ. See the regression in
  [SimpleCollisionMetrics](../crates/core/src/plugin/metrics/simple_collision_metrics/simple_collision_metrics.rs).
- **Teams that first spawn after t=0:** C++ SimpleCollisionMetrics lists teams
  at its first step, then totals later teams with no normalization time: their
  `flight_time_norm` is infinite and `score` NaN. Rust reports every team with a
  generated entity using the same normalization as other teams; `flight_time`
  and collision counts match. Seen in
  `verification/aircraft-substeps-scheduled.xml`.
- **FixedWing6DOF inertia:** C++'s bundled slug-unit default silently overrides
  a supplied SI matrix. Rust accepts either `inertia_matrix_slug_ft_sq` or
  `inertia_matrix`, rejects both together, and uses the slug default if neither
  is supplied.
- **FixedWing6DOF elevator drag key:** C++ reads `C_D_delta_elevator_` (with a
  trailing underscore), but every bundled parameter file writes
  `C_D_delta_elevator`, so C++ always uses the 0.01 default. That is harmless
  for the A-4D default (its file also says 0.01), but the Rascal file's 0.03 is
  never used. Rust reads the same underscore key and rejects the plain name, so
  a value is never silently ignored. `missions/fixed-wing-6dof-rascal.xml`
  leaves the key out and flies with 0.01, like C++.
- **PID gain strings:** C++ `PID::init` splits gains on commas only and needs
  exactly four values; a space-separated string such as `"0.5 0 0.1 9"` logs
  `PID::init failed` and the loop keeps no gains, while the mission runs on.
  Rust accepts commas or spaces. Write gains with commas in any mission meant
  for both (a study input once differed from C++ by kilometres this way).
- **AircraftPIDController, fixed on the comparison branch:** unmodified C++
  ignores the timestep it is given and uses the full mission tick
  (`time_->dt()`), so with N substeps its integral terms act N times stronger
  and its derivative terms N times weaker: the controller's tuning changes with
  `motion_multiplier`. Rust and `benchmarking-edits` (commit `5777aeea1`) use
  the given substep, with which N substeps behave exactly like running the
  controller N times faster. That holds only for a controller without a
  `loop_rate`: in both engines a rate-limited controller still receives the
  substep, not the time since it last ran, so its tuning still changes with
  `motion_multiplier` (see `agent_step` in `entity/plugin_stack.rs`).

  Both also lower the default altitude derivative gain from 0.01 to 0.002.
  This is a tuning choice, not a fix: upstream's gain is too strong for the
  default 220 m/s A-4 (unmodified C++ can look better with substeps only by
  accident). The altitude loop turns climb rate into desired pitch, and at
  that speed its derivative path asks for about 2.2 rad of pitch change per
  rad of actual pitch change, so a pitch upset grows into an altitude and
  heading oscillation. One FixedWing6DOF, 45 s of `Straight` flight at 1000 m
  after a 5 degree roll and 3 degree pitch upset, 0.01 s ticks
  (`reference/inputs/aircraft-pid-upset.xml`): with no substeps (C++ and Rust
  identical) the altitude wandered 285 m and the heading 42 degrees; with 5
  substeps, Rust's timing gave 66 m, and C++'s, whose derivative is 5 times
  weaker, peaked at 1.4 m. With no substeps and the altitude D lowered to
  0.002 (or 0), altitude peaked at 1.4 m (1.5 m) and stayed within 0.6 m
  (0.8 m) after the first 5 s; lowering the pitch loop's D instead did not
  help. These figures predate the RK4 loads fix below; with it, D 0.01 still
  wanders 91 m and 0.002 peaks at 1.39 m, so the choice stands
  (`paper/data/rk4-loads/summary.md`, part C, which reruns this with held
  forces in unmodified C++ and stage forces in Rust). Measured on this one
  scenario; missions that fly
  AircraftPIDController differ from unmodified C++, so the checker only
  records their baseline equality.
- **Stochastic sensors/communication:** Rust uses per-plugin, mission-seeded
  streams rather than C++'s shared generator. Measured contacts use sorted typed
  vectors. SphereNetwork queries current post-motion positions rather than the
  earlier C++ R-tree. AuctionAssign has configurable network/deadline and an
  optional winner instead of a negative sentinel. These are not
  sample-for-sample C++ RNG ports.

Source details worth preserving: NoisyContacts copies angular velocity and uses
5I covariance; NoisyState uses zero angular velocity and identity covariance.
Both right-multiply attitude errors. SphereNetwork has strict range, inclusive
altitude-plane bounds, and same-parent geometry bypass that still draws for
loss. AuctionAssign includes self-bidding, keeps the first maximum on ties, and
closes strictly after its deadline. See [plugin
behavior](../book/src/reference/plugin-api.md#noisycontacts).

## Findings from the comparison trace

- C++ Straight subscribes to `ContactsWithCovariances` and stores the map
  without reading it. Rust Straight subscribes and discards the messages so
  deliveries match; behavior is unaffected.
- C++ `VariableIO::connect` replaces a plugin's output index with the next
  plugin's input index. Outputs the next plugin does not read are silently
  dropped, and reading them returns NaN with a warning. The trace records only
  outputs the next plugin reads, in both implementations.
- C++ logs the final tick twice: before its step and as the terminal frame in
  `finalize()`. Both traces record both.

## Substep order: a C++ regression, fixed on the comparison branch

From 2017 until `c0a9f42b2` ("added gpu support", 2025-12-16), `SimControl` ran
each substep as controllers, then motion. That commit ran all controller
substeps before any motion, so with `motion_multiplier` > 1 controllers get no
fresh state within a tick and a PID's derivative term is zero after the first
call. `benchmarking-edits` commit `7129e47f5` restores the interleaved order and
disables GPU acceleration in CMake (which also disables the GPU network plugin).
Rust interleaves too, refreshing `PassThrough`-of-perfect own-state belief
between substeps; other estimators, sensors, contacts, interactions, and
networks keep the outer tick, so more substeps do not mean fresher observations
or finer collision checks. `m` = 1 missions are unaffected. The benchmarks
(`reference/benchmark.py`, `reference/native_benchmark.py`) build the comparison
branch, and the paper's timings come from them. The history and the verification
are in the paper's C++ agreement appendix (`app:baseline`);
`reference_check.py` requires unmodified and fixed C++ to match only when `m` =
1 and no entity uses AircraftPIDController, FixedWing6DOF, or Multirotor.

## RK4 loads: a C++ defect, fixed on the comparison branch

C++ FixedWing6DOF and Multirotor integrate with RK4, whose `model(x, dxdt, t)`
receives each stage's state as `x`. Only the kinematics and attitude used it.
Every force and moment used the start-of-step member `x_` (and `quat_body_`,
`alpha_`, or `state_->quat()`), so the loads stayed fixed through the stages
and the step was first order: the models paid for four evaluations and
recomputed identical forces at each. It reads as an `x`/`x_` slip from the
original "finishing fixed 6dof" commit (DeMarco, 2018-01-12), not a design
choice: deliberately freezing forces would compute them once in `step()`, and
the gyroscopic term mixed `x_` rates with `x` kinematics. Multirotor repeats
the pattern for weight, drag, and the gyroscopic term.

`benchmarking-edits` commit `1c8595b91` computes them from `x`, and both Rust
models do the same. Missions with either model now differ from unmodified C++
by design, and `reference_check.py` only records their baseline equality.
`reference/rk4_loads.py` (evidence in
[`paper/data/rk4-loads`](../paper/data/rk4-loads/summary.md)) measures the
change, taking the held forces from unmodified C++, which still has them:

- **Accuracy.** With constant controls, frozen loads halve their error with each
  halving of the step, while stage-evaluated loads reach the fine-step
  reference's resolution within one or two halvings: 6.1 cm against 5e-10 m for
  Multirotor over 4 s at 0.01 s, and 17 m against 1.5e-5 m for a FixedWing6DOF
  glider over 60 s at 0.05 s. A gently controlled 30 s flight differs by 5.0 m
  at 0.01 s and 81 m at 0.05 s, and the frozen version flips the aircraft at
  0.1 s.
- **Speed.** Forces are now really computed four times per step instead of
  once. When first measured, whole runs of 128 aircraft took 22% longer for
  FixedWing6DOF and 7% for Multirotor; the study no longer times this, since
  the held-force version exists only in unmodified C++.
- **AircraftPIDController's altitude D gain.** Frozen loads did not cause the
  oscillation that lowered it from 0.01 to 0.002 (see AircraftPIDController
  above). With correct loads, D 0.01 still wanders 91 m after the 45 s upset at
  0.01 s ticks (285 m frozen), and tens of metres at smaller steps; 0.002
  flies the same either way (1.39 m against 1.41 m). The default stays 0.002.

A suitable step size still depends on the model, controller, and scenario;
check convergence by halving the step rather than relying on one fixed value.

Computing forces once per step is not wrong in itself. JSBSim does it at
120 Hz, with rectangular Euler for rotation and Adams-Bashforth methods, which
reuse past derivatives, for translation by default. What was wrong here was
the mismatch: four RK4 stages with first-order forces, unmentioned.

## Multirotor: what Ripple takes from C++

The code, the book, and the missions describe Ripple's multirotor on its own
terms; this section is the one record of how it relates to C++ SCRIMMAGE's
`Multirotor` (`src/plugins/motion/Multirotor/`). The module keeps the LGPL
attribution line it needs as a derived work.

- **Kept:** the model's structure (rigid body, thrust and reaction torque
  with rotor speed squared, commands clamped to [omega_min, omega_max]), the
  optional quadratic body drag (`c_D`), the `rotor_config` row format, and
  the `motor_0...` input ports, so C++ missions run unchanged.
- **Fixed:** RK4 loads (above) and three further defects (below), on both
  sides.
- **Not kept:** C++'s default vehicle. Ripple has no default vehicle and one
  built-in, the Crazyflie 2.0; any other must be given in full. The C++
  comparison missions (`multirotor.xml` and its YAML twin,
  `verification/multirotor-fixes.xml`) write C++'s numbers out; their
  comments call them illustrative test values. Also not exposed: the C++
  external-force input, teleport, acceleration output, and private CSV and
  shape drawing (`write_csv` and `show_shapes` are rejected).
- **Added, Rust-only:** RotorS's motor and rotor effects, the Crazyflie, and
  MultirotorVelocityController (see the motor and rotor section below;
  C++'s MultirotorControllerPID never stabilized anything).

Where C++'s default numbers came from, as far as its history shows: none is
cited. The first version (`ee26cb264`, 2017-11-29) was a 1.2 kg X quad with
0.175 m arms; the ArduPilot plugin commit (`e7a115c75`, 2017-12-04) made it
the 1.5 kg "+" quad of ArduPilot's SITL multicopter of the time (Copter 3.5,
`mass = 1.5f`, default `quad` frame "+"). The rest looks designed rather than
measured: c_T 5.45e-6 is exactly 0.8 kgf at the 1200 rad/s top speed;
omega_min 346.41 is 1200/sqrt(12), so idle thrust is 1/12 of the maximum; the
inertia, diag(0.0122, 0.0122, 0.0244), is four 0.2 kg point masses on the
arms (yaw exactly twice roll); c_Q 2.284e-7 and c_D 0.058 have no traceable
origin (the C++ drag line reads "TODO: Check source"). Searches for the exact
values found no published source.

## Multirotor: three C++ defects, fixed on the comparison branch

Reviewed one at a time in `src/plugins/motion/Multirotor/Multirotor.cpp`
before fixing. Rust (`plugin/motion/multirotor/`) and `benchmarking-edits`
(commit `74571470e`) have the same fixes, line for line.

- **Body and world velocity could disagree.** The state held body velocity
  (U, V, W, private, initially zero) and world velocity (Uw, Vw, Ww, read from
  the entity) and integrated both. Position followed body velocity; the
  reported velocity was the separately integrated world one. So a vehicle
  spawned or pushed with a velocity reported it but did not move. Body rates
  were private too. Fix: read body velocity and rates from the entity state at
  the start of every step (rotated into the body frame), and report world
  velocity as the rotated body velocity. With truth the only copy, the two
  cannot drift apart.
- **Moment arms.** Each rotor's moment was thrust times the full 3D offset
  length, in the x-y plane, and the rotor's roll/pitch/yaw tilt was parsed
  but unused (the C++ XML says so). Fix: each rotor pushes along its tilted
  axis (the rotor quaternion applied to body +z) and its moment is r x F,
  plus the reaction torque along the same axis. For flat, untilted rotors
  this is the old moment up to rounding.
- **Thrust at omega_min.** Commands are clamped to [omega_min, omega_max];
  thrust was then zeroed only when the clamped speed equalled omega_min
  ("servo is off"), while its yaw torque stayed: a jump in thrust at the
  bottom of the range and a motor that twists without pushing. We read
  omega_min as an armed motor's idle speed: thrust and torque both follow
  c w² there, with no jump. There is no "off"; set omega_min to 0 for motors
  that stop (the Crazyflie does).

`missions/multirotor.xml` triggers none of these (no initial velocity, flat
untilted rotors, motors above idle): before and after the fixes its frames
differ by at most 6e-12 m and 7e-12 m/s, rounding. The fixture
`verification/multirotor-fixes.xml` reaches all three (a starting velocity,
tilted and raised rotors, idle motors). Before the C++ fix it separated C++
from Rust at the first step on every vehicle; with it, the two agree to
3.7e-13 m (and `multirotor.xml` to 4.9e-12 m). Unmodified C++ keeps the
defects, so `reference_check.py` only records Multirotor missions' baseline
equality, as for the RK4 loads. The C++ commit leaves the `Uw`, `Vw`, `Ww`
states in the vector but no longer reads them.

## Multirotor motor and rotor effects, and the Crazyflie preset (Rust-only)

C++ has none of this, so there is nothing to compare with it; these effects
are checked against RotorS and Crazyflow only. Rust adds, as optional
parameters, RotorS's motor-plugin model (`rotor.rs`, ported from
`rotors_gazebo_plugins/src/gazebo_motor_model.cpp` at `cd813b7a8c37`,
Apache-2.0): first-order motor lag with separate up and down time constants,
rotor drag proportional to rotor speed and the hub's edgewise airspeed, and
the rolling moment. RotorS's reaction torque (moment constant times thrust)
is the C++ c_Q w² already. Rust evaluates the lagging speed exactly at each
RK4 stage; at the end of a step it equals RotorS's filter output. RotorS
applies drag at the rotor link, which Gazebo turns into an r x F moment, and
reads the link's own velocity, which includes the body's rotation; Rust does
both. `reference/rotors_motor.py` compiles RotorS's own filter and
`UpdateForcesAndMoments` against stand-in Gazebo links and compares 480
samples: largest errors 3.6e-15 N, 6.7e-16 N m, and 4.5e-13 rad/s.

Each effect is off unless the mission sets its key, for any vehicle, as
for FixedWing6DOF's stall tables and landing gear. With every effect at zero,
existing Ripple missions are unchanged: on
2026-10-12 all 39 missions and workloads of `equivalence.py` were
byte-identical before and after these effects and the controller were added.

The `crazyflie2` preset takes the vehicle's numbers from Förster's 2015 ETH
thesis; the book chapter "Flying a multirotor" lists each with its equation,
and `airframe.rs` derives c_T, c_Q, and the top speed from his fits. His
motor lag and drag coefficients are not part of the preset: missions write
them (the book lists them, and `airframe.rs`'s tests derive the lag).
Förster's drag also has an axial term, which RotorS lacks; it is a separate
key, `rotor_drag_coefficient_axial`, zero by default. RotorS's Hummingbird motor
constants are not used: they repeat its Firefly's digit for digit, with no
source.

## Ported models keep C++ numerics; new models do not

Ported plugins reproduce the C++ implementation, including its numerical
shortcuts, so comparisons stay exact; where a shortcut is a defect, it is fixed
on the comparison branch and in Rust together. New plugins, such as new
fixed-wing and boat models, should be written to be correct, not to match
these.

**Integrators for new motion models.** For smooth-flight models such as the
planned fixed-wing and boat, start with RK4 that recomputes forces and moments
from each stage's state, and verify convergence. Choose another integrator when
the dynamics warrant it: stiff or strongly damped terms and discontinuities
(contact, saturation) change the trade-off, and methods that evaluate forces
once per step can be the better choice there. Document the choice, and do not
freeze forces through RK4 stages. Whichever method you choose, check convergence
by running a representative scenario at `dt`, `dt/2`, and `dt/4`, and confirm
the trajectory stops changing at the step you intend to use.

**Evidence still needed for these claims.** Each claim above needs its own
side-by-side check, and the results belong in `paper/data/`:
- *Integration accuracy*: done in `reference/rk4_loads.py` (see "RK4 loads"
  above). It shows a far smaller error, not a measured fourth-order rate.
- *What JSBSim does*: cite the pinned `FGPropagate.cpp` source (default
  integrators, forces once per frame), not a summary of it.
- *That a new model is correct*: a second, independent evaluator of the same
  equations (for example, a JSBSim aircraft file that mirrors them) at fixed
  states and over short runs, with declared tolerances. None is planned.
  Agreement would show the equations are implemented consistently, not that
  the model matches a real aircraft.

## Terrain (Rust-only)

C++ terrain did not affect the simulation. The mission `<terrain>` tag loaded a
VTK mesh and texture (`data/gui/terrain/<name>/`) that only the viewer drew,
plus BulletCollision when `enable_terrain` was set (off by default, commented
out in `straight.xml`). The TerrainGenerator interaction published a random
height map that the AltitudeAboveTerrain sensor read, but no stock mission used
either. Rust ignores the XML `<terrain>` tag (XML is frozen); YAML missions name
a terrain header instead (`common/terrain.rs`, book chapter "Terrain"), which
world plugins read and TerrainCollision uses. Like C++'s `altitude_origin`,
local `z = 0` is the terrain's origin altitude. There is nothing in C++ to
compare it with.

FixedWing6DOF's `landing_gear` and TerrainCollision's `structure_points_frd_m`
are Rust-only too: C++ ground contact was the flat spring at `z = 0`
(`use_ground_model`), which stays unchanged for the comparison missions. The
gear follows JSBSim's FGLGear wheels (`landing_gear.rs`), so the comparison
does not cover it; its own tests do (`crates/core/tests/landing_gear.rs`).

FixedWing6DOF's `lift_table` and `drag_table` are Rust-only as well. C++ lift is
linear in angle of attack (`C_L0 + C_L_alpha * alpha`) at any angle, so it never
stalls, and its zero-lift drag is `C_D0 + C_D_alpha * |alpha|`. Tables of
`[alpha_rad, C_L]` and `[alpha_rad, C_D]` rows replace those terms, interpolated
linearly and held at their ends like a JSBSim `<table>`; induced and elevator
drag are unchanged. They are opt-in: no mission sets them, and the ArduPilot
book chapter lists JSBSim's Rascal values. Without tables the output is
unchanged: on 2026-10-05 `fixed-wing-6dof.xml` and `fixed-wing-6dof-rascal.xml`
wrote byte-identical frames, summaries, events, and traces before and after each
table was added.

## Unported C++ models: check before porting

From a source audit of the same commit (2026-09-24), not a numerical check of
every model. These are observations, not changes to make to the C++
reference.

- **MultirotorControllerPID** writes equal hover motor speeds; its attitude
  and yaw corrections are commented out, so it doesn't stabilize anything.
  Rust's `MultirotorVelocityController` is a new controller, not a port.
- **RigidBody6DOF** disables gravity and some translational derivatives, and
  sets P/Q/R by clamping U.
- **Ballistic** puts `-mass * g` in the derivative of vertical acceleration
  instead of ordinary constant-gravity motion.
- **SimpleCar** uses `turn_rate` as a steering angle inside `tan`; its gravity
  branch multiplies acceleration by mass.
- **UUV6DOF** stores an external force in `force_ext_body_` but never uses it.
- **SimpleINS** divides velocity differences by `.01` and uses `rand()` for
  timing; **LOSSensor** seeds from wall time. Neither is deterministic as
  written.
- **OpenAIRewards** gives a reward, not a full episode, reset, and action
  interface.

Things the C++ plugins need that Ripple doesn't have yet, for whoever ports
one: a force/moment input and reset hook on motion models
(ExternalForceField), acceleration and wind in the state sensors read
(RigidBody6DOFStateSensor), target size for cameras (ContactBlobCamera),
shared ray or occlusion queries (RayTrace, LOSSensor), retiring one agent
without ending the run (MotionBattery), and custom per-entity records (CPA).

## Spawn RNG mismatch (accepted)

The native macOS baseline matched; the Ubuntu Docker fixture diverges at
initialization. The inspected libstdc++ uses `minstd_rand0` (multiplier 16807)
and returns the second polar normal variate first. Rust's [legacy
RNG](../crates/core/src/common/random.rs) uses multiplier 48271 and returns the
first variate first, matching Apple libc++.

The recorded randomized-spawn comparison had 90/90 mismatched states and about
8.41934 m maximum position error. Rust outputs still matched across 1/2/8
workers and recording on/off. This is an accepted difference of the unmodified
Linux build: C++ itself differs between libc++ and libstdc++. Comparisons remove
it with the instrumentation branch's opt-in libc++-compatible spawn stream
(`SCRIMMAGE_LIBCXX_SPAWN_RANDOM=1`), and keep a zero-variance twin that needs no
option. Do not change seeds or tolerances to hide randomized mismatches.

Use the [Docker workflow](../reference/README.md) for fresh comparisons. Its
traces compare message deliveries, lifecycle and collision events, sensor
payloads, beliefs, and plugin commands against the `benchmarking-edits` branch;
other message contents, such as auction bids, are compared by delivery only.
