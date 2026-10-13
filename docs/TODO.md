# Roadmap

What's left to build, in order: a correct core (milestone A), the
disturbance models (milestone B), then the learning study. The study itself
(question, tasks, predictions, protocol) is in the
[paper](../paper/manuscript/main.tex);
sources and version details are in
[LEARNING_LITERATURE.md](LEARNING_LITERATURE.md); intentional differences from
C++ SCRIMMAGE are in [reference notes](REFERENCE_NOTES.md). Keep the
[checks](../reference/README.md) passing after any simulation change.

## Done

C++ agreement, determinism, fixed-wing with ArduPlane, noisy sensors, lossy
networks, sweeps, the Crazyflie with a velocity controller, and MPE's point
agents matched step by step against MPE2 (`reference/mpe_parity.py`).

## Kept from SCRIMMAGE

Ripple ported SCRIMMAGE's behavior, defects included, so the two could be
compared. [Reference notes](REFERENCE_NOTES.md#behavior-kept-from-scrimmage)
list what was kept: simplifications that can stay, and defects with the
condition under which each matters. Milestone A fixes the defects. Message
delay is the one SCRIMMAGE feature not ported yet (milestone B).

## Milestone A: a correct core

The current checks don't exercise these paths; the study will. Each fix
states a contract and proves it three ways: the failing example before, a
focused test of the contract, and the comparisons after. A fix to code ported
from SCRIMMAGE goes on the `benchmarking-edits` branch too, so the two still
agree to rounding; the fix and its evidence go in REFERENCE_NOTES.md.

1. **NoisyState angular velocity.** It reports zero, so belief says the
   vehicle never rotates, and the multirotor velocity controller damps its
   attitude with that. *Contract:* a sensor with no angular-rate noise
   reports the true angular velocity. Also fly a noisy Crazyflie before and
   after. *Not exercised:* every NoisyState mission flies SimpleAircraft,
   whose controller doesn't read rates.
2. **NoisyState first reading.** It is the exact truth, so the first
   decision is noise-free at every noise level: 1 of 25 in an MPE episode.
   *Contract:* the first reading is sampled like every later one.
3. **SimpleAircraft climb rate.** It integrates altitude with -sin(pitch)
   but reports +sin(pitch). *Contract:* reported velocity matches the change
   in position over a small step. *Not exercised:* its controller uses only
   speed, where the sign drops out, and altitude from position.
4. **SimpleAircraft turn rates.** It reports zero. *Contract:* reported
   angular velocity matches the change in attitude over a small step.
   *Not exercised:* its controller steers from attitude.
5. **SimpleAircraft state.** It integrates from a private copy, so a change
   an interaction makes to truth is overwritten. *Contract:* the change is
   still there on the next step. *Not exercised:* the collision interactions
   remove entities rather than push them.
6. **FixedWing6DOF wind sign.** It adds wind to velocity, so missions must
   pass the negated wind. *Contract:* airspeed is velocity minus wind. Fix
   the ArduPilot sample's airspeed with it, and the missions that set wind.
   Needed before any wind sweep.
7. **Rate-limited plugins.** A plugin with `loop_rate` receives the phase
   dt, not the time since it last ran; WaypointFollower caps its speed with
   that dt, so it can overshoot. *Contract:* dt is the time since the plugin
   last ran.
8. **Spawn sampling.** Each sampled heading becomes the next spawn's mean (a
   random walk), and values are rounded to 6 decimals. *Contract:* every
   spawn is sampled around the configured mean, unrounded. Update
   `reference/mpe_parity.py`, which rounds MPE2's starts to match.
9. **Time.** Time accumulates by `+= dt` and drifts in the last digits, and
   is wound back one step at the end, so the final timestamp repeats.
   *Contract:* time is the step count times dt, and every frame's timestamp
   is unique.
10. **MpeContact timing.** A sensor runs before the contact push, so on
    contact steps a zero-noise reading differs from perfect belief. Compute
    the push for the next step and hand it to MpeParticle. *Contract:*
    zero-noise readings equal perfect belief, and contact episodes still
    match MPE2 (`reference/mpe_parity.py`); MPE2 is the proof, not SCRIMMAGE.
    *Not exercised:* the parity check replays forces with no sensor.
11. **Cleanup: duplicate sensor messages.** NoisyState and NoisyContacts
    publish a LocalNetwork copy of each reading that only the comparison
    trace reads; Straight subscribes and discards both. Make the trace record
    readings, then remove them (on both sides). *Proof:* readings and every
    decision are unchanged; only the unused messages disappear.

## Milestone B: disturbance models

Built after milestone A and before the learning work.

1. **Units.** MPE's arena has no scale (about ±1, agent radius 0.15). State
   how many metres one unit is, so metres of GPS error mean something there.
   GPS specs give a horizontal 95% radius; the YAML takes a per-axis
   standard deviation, which is about the 95% radius divided by 2.45.
2. **Design the first noise sweep (point agents).** The agent's own
   position is noisy (NoisyState); landmarks are exact (they come from a
   map). Decide how teammates are known: each agent broadcasting its own
   noisy position (shared GPS error), or NoisyContacts (independent
   perception error per observer). Contacts need a way to leave a team
   unperturbed. The base mission sets every noise field to zero explicitly,
   since both sensors default to 5 m and 5 m/s. White, zero-mean noise;
   belief from `PassThrough`, stated in the paper. A `.sweep.yaml` with
   `zip` over both horizontal axes (`pos_noise_0.1`, `pos_noise_1.1`).
3. **Wind.** Steady wind on the multirotor (drag on airspeed) and
   `SimpleAircraft`. FixedWing6DOF already has it; it adds wind to velocity
   (kept from C++), so missions pass the negated wind. Gusts only if steady
   wind matters.
4. **Message delay.** The engine already delivers late
   (`Delivery::After { delay_s }`); give SphereNetwork a `delay_s` and
   replace the legacy `comm_delay` errors. Loss already works
   (`prob_transmit`).
5. **Aircraft on the velocity interface.** Velocity becomes heading and
   speed above a minimum; holding and moving-reference goals. Fit
   `SimpleAircraft` (minimum speed, turn radius, climb) to the Rascal from
   6-DOF runs, so both fixed-wing rungs are the same aircraft.
6. **A Kalman filter** for the aircraft rungs: raw GPS-like readings as
   belief make the Rascal's controller jitter (about 0.15 rad/s body rates).
   Point agents keep `PassThrough`.
   The sensors' covariance is a placeholder (identity, 5I); the filter
   needs the configured noise covariance.
7. **Realistic ranges** for each disturbance, from real specs, published
   wind limits, or ArduPilot SITL. These go in the paper's ranges figure.
8. **Later: drifting noise.** Real GPS error drifts slowly; a first-order
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
