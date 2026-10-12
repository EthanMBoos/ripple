# Roadmap

Ripple is at its initial release; the
[baseline report](../paper/manuscript/baseline.tex) records its comparison
with C++ SCRIMMAGE, [reference notes](REFERENCE_NOTES.md) list the intentional
differences, and the [checks](../reference/README.md) rerun all of it. Keep the
C++ comparison passing after any simulation change.

## Integration and evidence

The goal is to complete the capabilities below in one multi-agent robotics
testbed, so experiments can compare model and environment fidelity. The
[paper](../paper/manuscript/main.tex) follows that work. Each implemented stage
needs an external baseline and a clear statement of what it establishes:

- Compare the integrated component with its source on matched inputs, then
  exercise it in a representative closed-loop mission. Record source versions,
  parameters, frame and unit conversions, timestep, errors, and known gaps.
- Keep source agreement separate from physical validation. Use independent
  measured data where available; borrowed, estimated, or computed coefficients
  retain those labels even when the source comparison passes.
- Preserve existing behavior when optional effects are off. Document deliberate
  corrections and maintain the corresponding reference comparisons.
- Record the comparison results and execution cost in the paper, with retained
  data under `paper/data/`. Selected multi-agent experiments then test whether
  added fidelity changes outcomes or strategy rankings, and at what cost.

A changed ranking is not required to justify completing an integration. A
useful capability with a checked baseline, or evidence that a simpler model is
adequate in a stated regime, is a result too. The learned-policy gate below
still applies. The roadmap can support several bounded papers; completing
every item is not a prerequisite for each submission.

## Learned policies

Background and options: [learning simulator
landscape](LEARNING_SIMULATOR_LANDSCAPE.md).

Learning is for decisions the classical stack cannot hand-code: weighing
many tracks and their uncertainty, teammates' messages, vehicle limits,
terrain, and timing at once. Not for re-solving what classical strategies
already do. Such a policy can only learn from the inputs it will face, so it
trains here, in worlds with the real sensing, networks, belief, and vehicles.

- **No RL algorithms or training loops in this repository.** PPO, MAPPO, and
  the like come from existing libraries (RLlib, BenchMARL, PufferLib, SB3,
  CleanRL).
- **Measure decision throughput first.** Put a decision every second in the
  paper's information-sharing workload and count decisions per second per
  core, including observation building and resets, at a few fidelity levels.
  That says whether training here takes hours or months. Pick the decision to
  learn and record why classical strategies (nearest known target,
  `AuctionAssign`) are not enough for it.
- **Learn the high-level decision; classical behaviors are the actions.** The
  policy chooses which target, waypoint, message, or hand-off; the existing
  autonomy, controllers, and motion carry it out at their own rates. Decisions
  at about 1 Hz instead of every physics step cut the samples needed by orders
  of magnitude, and the policy cannot fly the vehicle into the ground. Add a
  decision hook so one decision inside a classical autonomy can be learned
  without rewriting it.
- **Training bridge.** An optional PyO3 crate, out of the default build like
  `ardupilot`, exposing Gymnasium (single agent) and PettingZoo parallel
  (multi-agent) environments to an outside learner. Its step applies the
  decisions and runs until the next decision point (`Simulation::step`
  returns pre-step state, so the bridge needs its own step); reset rebuilds
  the simulation. Many single-worker worlds in parallel give the scale; the
  sweep and Slurm tooling already fans runs out.
- **Truth for the critic, belief for the policy.** Centralized training with
  decentralized execution: during training a critic may read truth, while
  each agent's policy sees only its own belief and messages, which Ripple
  already keeps apart.
- **One observation and action definition**, shared by the bridge and the
  evaluation plugin: belief encoded at a fixed size or as a set, frames,
  normalization, action meanings, decision period, termination. Rewards come
  from the metrics plugins, so the training reward and the evaluation score
  agree.
- **Evaluation in the same setup.** An optional autonomy plugin (or decision
  hook) that runs a frozen ONNX policy (Burn or `ort`), so learned and
  classical strategies run side by side in one sweep under identical sensing,
  network, and vehicle rules. Inference must keep output byte-identical at
  any worker count: a fixed thread count, and stochastic policies draw from
  the plugin's own random stream.
- **Fidelity curriculum.** Train mostly at low fidelity, then fine-tune and
  evaluate at higher fidelity (stall tables, wind, noisy sensing, lossy
  networks). How much fidelity training needs, and whether a learned policy
  holds up as fidelity rises where a classical one does, are questions for
  the paper.
- **Position against GPU simulators.** CPU worlds with a GPU learner is the
  classic setup (Atari, SMAC, OpenAI Five; RLlib, Sample Factory,
  PufferLib); GPU-resident simulators (Isaac Lab, Brax, MJX, JaxMARL) are
  faster but simplify the sensing, belief, networks, and vehicles these
  decisions depend on. Expect reviewers to ask why not train there: answer
  with measured throughput and learning curves, one standard benchmark task
  as an anchor, and the hybrid: train cheaply wherever possible (including a
  GPU simulator), fine-tune and evaluate here against the classical stack.
  If a cheap simulator's policy holds up here, report that too.

## Vehicle fidelity: scope and extension points

Build each vehicle to the level established open simulators reach (ArduPilot
and PX4 SITL, RotorS, Fossen's MSS), with every number's source stated. Going
beyond that needs measured data for one real vehicle; that is not planned
here, but the code shows where it plugs in:

- **A fidelity header on every motion model**, with the same headings:
  *Source* (where its equations and numbers come from), *Validated against*
  ("nothing" is allowed), *Valid range*, *Not modeled*, and *To go further*
  (the data needed, who has it, and what to change). Measured-data leads live
  there, next to the code they concern, rather than in this roadmap.
- **Effects left out are named terms, not absences.** Where a higher-fidelity
  effect belongs in a force sum (slipstream over a wing, ground effect), the
  code computes it as an explicit zero with a comment saying what a model of
  it needs.
- **Geometry and coefficients are parameters, not assumptions:** rotor axes
  as vectors, aerodynamics through the table-or-linear pattern of the stall
  tables, so a measured model arrives as data or one function.
- **A book page, "Adding a measured-data vehicle model":** implement the
  source's equations as a new motion model; reproduce its published tables or
  predictions as a test; enforce or warn on its stated valid ranges; keep
  "matches its source" separate from "matches reality"; add a suite check so
  the result is evidence.

## VTOL

Goal: VTOL vehicles whose transitions (where the wing can stall) are
plausible at ArduPilot and PX4's level. No C++ comparison exists, so each
model states its data source and its limits.

- **Quadplane at ArduPilot/PX4 level (plausible, not validated).** Add optional
  `lift_rotors` to FixedWing6DOF, like `landing_gear`: each rotor's position,
  axis, spin direction, and thrust and torque coefficients, reusing
  Multirotor's rotor equations. Add ArduPilot's axial-inflow thrust loss
  (`SIM_Motor.cpp`) and the rotor drag from the multirotor item below (RotorS's
  `gazebo_motor_model.cpp`, which PX4's Gazebo motor model derives from). Use
  the `lift_table` and `drag_table` stall. Missions without rotors stay
  byte-identical. Fly it with ArduPlane quadplane SITL (`Q_ENABLE=1`): the
  bridge needs the quad motor channels and Q modes; ArduPilot owns the
  transition logic. Check it against ArduPilot's own SITL quadplane on the
  same mission; that shows ArduPilot can fly our model, not that the physics
  is right (ArduPilot's plane sim calls itself "not aerodynamically
  accurate"). Accepted limitations, as in ArduPilot and PX4: no prop wash over
  the wing, no tilting rotors or wings.
- **Leave the next steps one change away.** Each rotor's axis is a fixed
  vector parameter; a tilt-rotor later makes it follow a control input, with
  the rotor equations unchanged. Slipstream lift over the wing is a named zero
  term. The fidelity header's *To go further* records the measured-data lead:
  NASA LA-8 (tandem tilt-wing, 8 propellers), the only open source of
  transition-regime data found (NTRS search, September–October 2026). Its
  mass properties, a propeller model valid from 0 to 180 degrees incidence
  (Simmons, J. Aircraft), the control approach (Cook et al., VFS 2021), and
  wind-tunnel plots (NASA TM-2020-5003178) are published; the whole-aircraft
  aero-propulsive model is printed at one of eight airspeeds (q = 3.5 psf),
  and rate damping, NASA's simulation, and flight data are not. Someone with
  access (NASA Langley, Flight Dynamics Branch) would follow the book page
  above.

## Multirotor fidelity

`Multirotor` is the C++ port: thrust and torque proportional to rotor speed
squared, quadratic body drag, speed limits, gyroscopic terms. It is not a
validated flight model and nothing flies it closed-loop (C++'s
MultirotorControllerPID only holds hover speed).

- **A Rust cascaded controller** (position, velocity, attitude, rate, motor
  mixing), so `Multirotor` flies at all, and at swarm scale. ArduCopter SITL
  through the bridge comes later, for a few high-fidelity vehicles; the bridge
  needs copter motor channels, modes, arming, and takeoff.
- **Fix C++ defects first, on both sides.** Fix in Rust and on the C++
  `benchmarking-edits` branch (precedent: substep order, AircraftPIDController,
  RK4 loads), record in `docs/REFERENCE_NOTES.md`, and rerun the suite so the
  `multirotor` case still compares. Each needs an in-depth review before the
  fix:
  - two velocity states: body velocity (starts at zero, moves the vehicle) and
    world velocity (reported) are integrated separately and can disagree;
  - moment arms use the full 3D rotor offset (height above the centre of
    gravity adds leverage) and ignore the configured rotor tilt; use r x F;
  - thrust drops to zero only when speed equals `omega_min` exactly, while
    yaw torque stays on; make thrust continuous and torque follow thrust.
- **Structure: ETH RotorS's motor and rotor kernel** (Apache-2.0,
  `rotors_gazebo_plugins/src/gazebo_motor_model.cpp` and `FirstOrderFilter`),
  as optional parameters with C++ defaults kept byte-identical: asymmetric
  first-order motor lag, rotor drag against wind-relative motion across the
  rotor, rolling moment, and reaction torque. These are new physics, not C++
  defects, so the `multirotor` case stays unchanged. Rotor drag (linear in
  speed, Martin & Salaün 2010) dominates at typical quad speeds, but RotorS
  has no body drag, so keep C++'s quadratic body drag as a second term (it
  matters at high speed, per NeuroBEM). RotorS's reaction torque (moment
  constant times thrust) equals C++'s c_Q w^2; adopt its form. Not ported:
  `rotor_velocity_slowdown_sim`, a Gazebo display workaround. Add ArduPilot's
  thrust change with airflow along the rotor axis, which RotorS lacks. Check
  the port sample by sample against the RotorS kernel. The same plant is
  planned for `../UnrealVehicleDynamics` (`docs/COPTER_LANDING.md`); keep the
  two aligned. Ground effect and downwash between vehicles are named zero
  terms.
- **Two parameter sets.** C++'s defaults stay, unchanged, for the C++
  comparison. Add a Crazyflie 2.x set (27 g) as the recommended real-vehicle
  mission, from Förster's 2015 ETH thesis ("System Identification of the
  Crazyflie 2.0 Nano Quadrocopter", handle 20.500.11850/214143): thrust and
  torque against rotor speed measured on a force/torque sensor with a laser
  tachometer, inertia, and motor dynamics. Cite each value. gym-pybullet-drones'
  `cf2x.urdf` (kf = 3.16e-10, km = 7.94e-12 per rpm squared) cites this thesis
  but mixes in ground-effect, drag, and downwash coefficients from other
  sources (Luis & Le Ny, arXiv 1608.05786; Landry, MIT 2015): check each one's
  source or leave it out. Do not use RotorS's Hummingbird numbers: its mass,
  arm, and inertia come from a paper on the real vehicle (Wang et al.), but its
  motor, moment, time, drag, and rolling-moment constants are identical, digit
  for digit, to its Firefly hexacopter's, with no source (checked October
  2026; the rolling moment is a placeholder 1e-6).
- **Extension point: validation.** The fidelity header says "validated
  against: nothing yet" and records the path to a stated prediction error:
  replay recorded motor commands from NanoBench (arXiv 2603.09908,
  CC-BY-4.0, 172 Crazyflie 2.1 flights with Vicon truth, motor PWM, and
  battery telemetry) through the model and compare the motion. A second
  vehicle is the NeuroBEM racing quad (752 g, UZH RPG, RSS 2021: 96 flights
  with motion capture and motor speeds, and its own blade-element model).

## Surface vessels

There is no boat. C++ SCRIMMAGE has none either: its only marine model is
UUV6DOF (a REMUS underwater vehicle whose depth is clamped at `surface_height`;
nothing floats), and C++ users fake boats with the 2D Unicycle. Neither is
ported.

- **Port Fossen's Otter** (MSS toolbox, a small catamaran), the same model
  `../UnrealVehicleDynamics` uses (`core/src/fossen_otter.cpp`). It gives
  calm-water manoeuvring (coupled 6-axis mass matrix with added mass, hull
  damping, two propellers with lag, current) and floating: hydrostatic
  stiffness from metacentric heights restores heave, roll, and pitch. Check it
  sample by sample against the MSS reference; drive it with ArduRover through
  the bridge later. Claim: an educational calm-water model. Its numbers are
  estimates (checked October 2026 in MSS's `otter.py`: real dimensions and
  masses, but added mass as a rule-of-thumb fraction of mass and 1 s sway and
  yaw time constants); the fidelity header says so.
- **Waves at Gazebo/VRX level.** Sea state is a world resource beside
  terrain: elevation, slope, and water velocity are a sum of components from
  a standard spectrum (JONSWAP or Pierson-Moskowitz) with random phases drawn
  once from the mission seed on their own stream, so the sea is a pure
  function of position and time: byte-identical at any worker count,
  read-only, truth (sensors see it only through readings). Mission YAML sets
  spectrum, significant height, peak period, and direction. Wave loads are
  hull-point buoyancy against the wave surface (Froude-Krylov), as Gazebo's
  Surface plugin and VRX's WAM-V do: plausible, with no added mass,
  radiation, or diffraction, which are named zero terms. Boats do not make
  waves that reach other boats (the usual simplification).
- **Drawing the sea in Rerun** (output only): Rerun cannot evaluate the wave
  function, so the sea is logged as a mesh, and its cost scales with area and
  resolution, not fleet size (hulls are logged once, then poses). Benchmark
  recording size and viewer frame rate against grid resolution, logging rate,
  and covered area (one grid over the fleet or a patch per boat) before
  deciding whether the sea is saved or live-only like trails; record the
  result in a DEVNOTE, as `viewer/trails.rs` does, and follow the visual QA
  chapter. Aim: a small fleet.
- **Going further is an extension point.** The Otter's *To go further*
  records the path: Fossen's superposition of calm-water manoeuvring and
  wave-frequency motion from motion RAOs (MSS's WAMIT/ShipX templates), then
  linear seakeeping (Cummins equation: force RAOs, frequency-dependent added
  mass and damping, radiation memory as vessel state, as in WEC-Sim), with
  hydrodynamic tables from an offline panel solver (Capytaine or NEMOH, via
  WEC-Sim's BEMIO format), checked first on MSS's bundled S175 ShipX data.
  These are computed, not measured; public tank data for small autonomous
  boats has not been searched for. Related work: UVD's `docs/WATER.md`, VRX.
