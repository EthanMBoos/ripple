# Roadmap

What's left to build, in order: removing SCRIMMAGE leftovers (milestone A),
the disturbance models (milestone B), then the learning study. The study
itself (question, tasks, predictions, protocol) is in the
[paper](../paper/manuscript/main.tex); sources and version details are in
[LEARNING_LITERATURE.md](LEARNING_LITERATURE.md); intentional differences from
C++ SCRIMMAGE are in [reference notes](REFERENCE_NOTES.md). Keep the
[checks](../reference/README.md) passing after any simulation change.

## Milestone A: SCRIMMAGE leftovers

Interfaces and behavior ported to match SCRIMMAGE that get in the way of the
study. What stays only while the comparison does is listed in the [reference
notes](REFERENCE_NOTES.md#left-behind-when-the-comparison-is-retired).

### One random number generator, with keyed draws

Ripple has two generators. Spawns draw from `LegacyRng`
(`common/random.rs`), a copy of SCRIMMAGE's `minstd_rand` with Apple
libc++'s uniform and normal conversions, kept so randomized spawns match
SCRIMMAGE sample for sample (and even then only with
`SCRIMMAGE_LIBCXX_SPAWN_RANDOM=1`, since Linux's libstdc++ draws differently).
Every plugin draws from its own `PluginRandom` stream (SplitMix64 keyed by
seed, entity id, and plugin name), which SCRIMMAGE doesn't share, so sensor
noise is compared only by distribution (`noise_statistics.py`, a KS test).
Both are seeded and repeatable.

A stream has a flaw for paired comparisons: a plugin takes the next number
whenever it needs one, and some draw a varying number of times. NoisyContacts
draws per contact in view, SphereNetwork per reachable link. When two methods
on the same seed see different contacts at one tick, every later draw in that
stream shifts, and the two runs get unrelated noise for the rest of the
episode. Keyed draws fix this: each number is a function of a label saying
what it is for, `hash(seed, entity, plugin, tick, target, axis)`, so observer
1's error on target 2 along x at tick 50 is the same number in every run with
that seed, whatever else happened. This is the counter-based design of
Random123 (Philox) and JAX (Threefry); SplitMix64's mixer does the same job.

1. **Keyed draws in Ripple.** `random.normal(key, mean, sigma)` and
   `random.uniform(key)`: the engine supplies seed, entity id, plugin name,
   and tick; the plugin supplies the rest of the label (stable ids, never list
   positions; a different label for every number, such as the axis). The label
   parts are hashed in one fixed order, then mixed with SplitMix64; no
   draw order or counter remains. Convert NoisyState, NoisyContacts,
   NoisyPosition, SphereNetwork, and AuctionAssign (its bids).
   Contract test: two runs on one seed whose agents see different contacts
   give the same error for every (tick, observer, target, axis) both see.
2. **Spawns on the same scheme.** `hash(seed, "spawn", entity index, field)`;
   remove `LegacyRng` and the "preserve legacy spawn RNG draw order"
   invariant in `AGENTS.md`.
3. **C++ copy.** The same hash and conversions on `benchmarking-edits`,
   behind one opt-in switch that replaces `SCRIMMAGE_LIBCXX_SPAWN_RANDOM`,
   used by generation, NoisyState, NoisyContacts, and SphereNetwork. Labels,
   not draw order, have to match.
4. **Checks.** The noise missions compare traces sample for sample, within
   the usual tolerances: `ln` and `cos` can differ by one ulp between macOS
   and glibc, so draws agree to about 1e-16, not bit for bit (as the dynamics
   already do across platforms, `paper/data/rust-platform`). Keep the KS test
   as a second check. Missions without randomness must stay byte-identical
   (`reference/equivalence.py`); randomized outputs change once, and the
   evidence is refreshed with `--retain` after all checks pass.
5. **Book chapter.** A Concepts page, "Randomness and seeds": what the seed
   fixes, streams against keyed draws with the shifted-noise example, why
   paired comparisons need keyed draws (common random numbers), the two label
   rules, how a plugin asks for a number, and what is reproducible across
   platforms (bit for bit on one platform; about 1e-12 m across them, from the
   math library, not the generator). Update the plugin API reference and
   writing-plugins guide where they describe `context.random`.
6. **Notes.** Replace the "Spawn RNG mismatch" section of the reference notes
   and the RNG text in `reference/README.md`.

### Interfaces

1. **Sensor noise parameters.** Noise is a `"mean stddev"` string per axis,
   and NoisyState and NoisyContacts default to 5 m and 5 m/s, so every noise
   sweep has to zero each field explicitly. Use named fields
   (`position_stddev_m: [x, y, z]`, and so on) that default to no noise.
2. **SingleIntegrator's `max_speed`.** Negative means the command is used
   directly; zero or more rescales every command to exactly that speed, not a
   cap. Make it a plain speed cap. Point agents are the first learning task.
3. **Aircraft on the velocity interface.**
   - *Turn law.* SimpleAircraft turns at `speed / L * tan(bank)` with
     `turning_radius` L = 13 m: the kinematic bicycle model, a car's steering
     equation. An aircraft in a coordinated turn turns at
     `g * tan(bank) / speed`. At 25 m/s and 30 degrees of bank that is 0.23
     rad/s and a 110 m radius, against SimpleAircraft's 1.11 rad/s and 22.5
     m: about five times too tight, and the radius doesn't grow with speed
     squared (`radius_slope_per_speed`, off by default, is the patch). The
     fixed-wing rung exists to test methods on a vehicle that can't turn on a
     dime, so use the coordinated-turn law. A fix to ported code: on
     `benchmarking-edits` too, with the failing example, a focused test, and
     the comparisons after.
   - *Profiles.* SimpleAircraft takes physical limits (minimum and maximum
     speed, bank limit, climb and descent rates, acceleration) and an
     optional `airframe`, as FixedWing6DOF and AircraftPIDController do: a
     named, sourced set of those numbers that any key overrides, with no
     default aircraft. The turn follows from speed and bank, so a new plane
     is a new profile or its spec-sheet numbers in the mission.
   - *Fit to the Rascal.* The first profile, `rascal`: fit its limits to
     FixedWing6DOF's Rascal from 6-DOF runs (a script in `reference/`, its
     numbers sourced beside the profile), so the two fixed-wing rungs are
     the same aircraft and a change in results between them comes from
     fidelity alone. Every limit needs a physical unit for this: today speed
     changes at a unitless throttle in [-100, 100] divided by 5 (up to
     20 m/s², about 2 g).
   - *Velocity commands.* Learned and classical send the same velocity
     command on every rung. The aircraft follows it the way an autopilot
     does: heading is the command's direction (kept when the command is
     zero), speed is clamped between the minimum and maximum, climb is
     clamped to the climb limits, and the turn law limits how fast it turns.
     No special holding or tracking mode: when a command can't be obeyed,
     the aircraft does what it can and the metrics show the shortfall.
4. **Message delay.** SCRIMMAGE's fixed `comm_delay` works on every network
   but has defects: queued messages are released only while the same topic
   is being published, so they stall when publishing stops;
   `comm_delay="0"` queues instead of delivering; and `is_stochastic_delay`
   only reorders the queue. Ripple rejects these options. Its engine already
   delays correctly (`Delivery::After { delay_s }`): a message arrives at
   the first tick at or after its due time, and receivers see `sent_at_s`.
   Give SphereNetwork and GlobalNetwork a `delay_s` (default 0, immediate),
   keep LocalNetwork immediate, and remove the `comm_delay` and
   `is_stochastic_delay` fields. Random delay waits for keyed draws (above).
   Record the SCRIMMAGE defects in REFERENCE_NOTES. Loss already
   works (`prob_transmit`).

## Milestone B: disturbance models

Built before the learning work.

1. **Units.** MPE's arena has no scale (about ±1, agent radius 0.15). State
   how many metres one unit is, so metres of GPS error mean something there.
   GPS specs give a horizontal 95% radius; the YAML takes a per-axis
   standard deviation, which is about the 95% radius divided by 2.45.
2. **Design the first noise sweep (point agents).** The agent's own
   position is noisy (NoisyState); landmarks are exact (they come from a
   map). Decide how teammates are known: each agent broadcasting its own
   noisy position (shared GPS error), or NoisyContacts (independent
   perception error per observer). Contacts need a way to leave a team
   unperturbed. Noise defaults to none (milestone A), so the base mission
   sets only the fields it sweeps. White, zero-mean noise;
   belief from `PassThrough`, stated in the paper. A `.sweep.yaml` with
   `zip` over both horizontal axes (`pos_noise_0.1`, `pos_noise_1.1`).
3. **Wind.** Steady wind on the multirotor (drag on airspeed) and
   `SimpleAircraft`. FixedWing6DOF already has it (`wind_E/N/U`, the air's
   velocity). Gusts only if steady wind matters.
4. **Message delay sweep.** Delay from milestone A, swept like loss.
5. **A Kalman filter** for the aircraft rungs: raw GPS-like readings as
   belief make the Rascal's controller jitter (about 0.15 rad/s body rates).
   Point agents keep `PassThrough`.
   The sensors report no covariance; the filter needs the configured noise
   covariance.
6. **Realistic ranges** for each disturbance, from real specs, published
   wind limits, or ArduPilot SITL. These go in the paper's ranges figure.
7. **Later: drifting noise.** Real GPS error drifts slowly; a first-order
   Gauss-Markov error needs a measured magnitude and correlation time. The
   navigation prediction (targets swapping) is driven by jitter, so white
   noise tests it first.

## Learned policies

1. **Classical baseline on MPE2.**
   - Hungarian autonomy, re-solved every tick from each agent's own belief;
     cost is distance, ties go to the lowest ids.
   - Stopping-point controller: after a push F, an MPE2 agent stops at
     p + 4·v·dt + 4·F·dt², so use the F that puts that point on the target,
     clipped to ±5 per axis. Every number comes from MPE2; no fitted
     parameters.
   - Path cost in SimpleSpread (distance flown over the assignment's
     straight-line cost).
   - The same baseline in Python against MPE2 must give Ripple's scores on
     the same starts. Keep the script, starts, and scores as evidence.
2. **Learned side on MPE2.**
   - Train MAPPO with BenchMARL (PettingZoo 1.25, Python 3.12 or older,
     `max_cycles: 25`, continuous actions).
   - ONNX policy plugin: `tract-onnx` in an optional crate, actions sampled
     from the plugin's own random stream so output stays byte-identical at
     any worker count.
   - The exported policy's observations, actions, and returns match MPE2's
     on the same starts.
   - Then the benchmark comparison on paired episodes.
3. **Side check, outside Ripple.** Reproduce Papoudakis et al.'s MAPPO
   result with their code (about 5 hours), and score the classical baseline
   in their environment under the same protocol (their number is the best of
   41 evaluations averaged over 5 seeds). Start anytime.
4. **Velocity commands for point agents.** One velocity controller in Ripple
   and as an MPE2 wrapper, checked against each other. Retrain both sides for
   velocity commands and measure that switch against step 2, so the sweeps
   connect to the benchmark's ranking.
5. **Sensing on navigation**: run the noise sweep designed in milestone B,
   step 2, with both methods.
6. **Navigation on the Crazyflie, then fixed-wing**, with noise and wind
   sweeps. A landmark is covered while an aircraft loiters within a radius;
   define the radius and arena first.
7. **Training bridge**, for training at the realistic setting: an optional
   PyO3 crate shaped like a PettingZoo parallel environment.
   `Simulation::step` returns the pre-step state, so it needs its own step;
   reset rebuilds the simulation. Measure its speed first.
8. **Collision avoidance.** ORCA, NH-ORCA, constant-speed ORCA, a barrier
   function controller; circle and swap sweeps; separation metrics; a holding
   pattern at the goal for aircraft.
9. **Flocking.** Reynolds, Olfati-Saber, and Vásárhelyi plugins; neighbor
   messages with hop count and age; alignment and group metrics. Reproduce
   Tolstaya et al.'s range result first.

Two good tasks are still a paper.

### Rules for a fair comparison

- Learned and classical send the same command, read the same belief, and run
  on the same episodes. One observation definition per task, with a parity
  test against the training sim.
- No training code here: train with existing libraries, export to ONNX, run
  in Ripple.
- Train three ways: simplest setting, center of the realistic range, and
  randomized. Evaluate every policy across the whole sweep.
- Classical gets the same tuning budget as learned, and is also reported
  tuned at the simplest setting and frozen.
- One primary metric per task, chosen in advance; collisions, coverage, and
  the rest are reported separately, never combined into one score.
- 10 training seeds; seeds are the unit. Interquartile means with bootstrap
  intervals (`rliable`).
- Realistic ranges come from real specs, published wind limits, or
  ArduPilot SITL, never made-up numbers.
- The paper's predictions are fixed once experiments start; a result that
  contradicts one is reported as such.

## Other work

- Every new piece (vehicle, autopilot, sensor) is checked against an outside
  source, with versions, conversions, and errors written down. Matching a
  source is not matching reality: borrowed coefficients stay labeled.
- With new effects off, old missions run exactly as before.
- Unicycle dynamics.
- Legacy delay behavior, more sensor models, and plugin debug geometry.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK, ROS,
and JSBSim. ArduPilot and Burn stay optional, out of the default build.
Pursuit was dropped as a task: results depend on how each paper designs the
evader.
