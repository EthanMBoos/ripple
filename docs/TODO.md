# Roadmap

What's left to build, in order: the disturbance models, then the learning
study. Missions are YAML only ([reference
notes](REFERENCE_NOTES.md#missions-are-yaml)), and each plugin parameter has
one name ([Rust style](../book/src/development/rust-style.md#parameter-names)).
The interfaces that got in the way of the study are fixed ([reference
notes](REFERENCE_NOTES.md#interfaces-fixed-for-the-study)). The study itself
(question, tasks, predictions, protocol) is in the
[paper](../paper/manuscript/main.tex); sources and version details are in
[LEARNING_LITERATURE.md](LEARNING_LITERATURE.md); intentional differences from
C++ SCRIMMAGE are in [reference notes](REFERENCE_NOTES.md). Keep the
[checks](../reference/README.md) passing after any simulation change.

## Remove SCRIMMAGE's shape

Like the parameter rename: change what only makes sense to someone who knows
SCRIMMAGE, list everything each change touches, settle the decisions, then
prove it (byte-identical outputs, or the same numbers after a stated
relabeling). Breaking old missions and outputs is fine. Networks stay as they
are: GlobalNetwork is already added when a mission leaves it out
(`simulation/world_plugins.rs`), missions declare LocalNetwork only when
needed, and GlobalNetwork's `delay_s` is a setting the disturbance sweeps use.
LocalNetwork's name hides that it delivers only within one entity; rename it
if a better name comes up.

1. **Small fixes.** Rename `sub_swarm_id`, the index of the mission group an
   entity came from, to `group_index`, the record's name for it; nothing in
   the simulation reads it. Drop the mission `color` key (9 missions set it):
   the viewer colors by team and never reads it. FixedWing6DOF's module
   comment calls wind "added rather than subtracted, a quirk", but the code
   subtracts. Check `simulation.rs`'s "legacy missions can create entities one
   step before the start time". Compute substep time as `time_s + i * dt_s /
   N`, anchored to the tick's time, instead of adding the substep each time
   (SCRIMMAGE's way, still in `controllers_and_motion`), in its own commit,
   since it can move a time-threshold event by one substep, compared before
   and after.

## The book

For research and teaching: targeted trims, not a rewrite. Keep the beginner
material (tutorials, concepts, quick start) and write it as a person
explaining to a newcomer: short, plain, the key idea and why, with an example.
Exhaustive detail is left to the code, and a reader who wants the long version
can ask an LLM about the repo. Cut duplication (the phase order is told about
three times, plugin authoring four) and listings the code already states.

- **Keep:** both tutorials, tightened only where they repeat; coordinate
  frames; belief vs truth; randomness; YAML missions; Rust scenarios;
  multirotor; measured-data vehicle models; Rust style.
- **Merge:** the architecture overview, tick, information-flow, and messages
  pages, plus the plugin API's lifecycle section, into one "How a run works",
  after the paper's tick figure. User plugins, Writing plugins, and the
  plugin API's authoring parts into one plugin guide; the API reference keeps
  only traits, contexts, ports, and messages. The plugin API's model sections
  (NoisyState, NoisyContacts, SphereNetwork, AuctionAssign) into one models
  reference: each model, its parameters, what it is checked against, and its
  limits, replacing "Selected v1 models" and its SCRIMMAGE scope table.
- **Delete:** Fighting autonomy plugin bloat (SCRIMMAGE research advice with
  sketch code; at most a paragraph survives); Visual checks with Rerun (agent
  and MCP instructions, to `AGENTS.md` or `docs/`); the plugin API's "What
  remains" backlog; the C++-to-Rust table in the source map, which becomes a
  short human-written map of where things are.
- **Optional section:** ArduPlane SITL; Terrain, trimmed (its comparison with
  other simulators to the reference notes).
- **Add:** sweeps and results (sweep files, shards, the output folder, reading
  a run in Python); disturbances (where each enters the tick, its settings,
  realistic ranges); how models are checked (the paper's table in words, the
  suite, what agreement does and does not show); learning, once it exists.
- **Introduction:** lead with what Ripple is for in the paper's terms; the
  quick start shows a sweep, not `straight-no-gui`.
- **SCRIMMAGE mentions:** about 60 remain across the pages; the merges and
  deletions remove most, and the rest go except where a page cites the
  comparison.

## Disturbance models

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
   unperturbed. Noise defaults to none, so the base mission sets only the
   fields it sweeps (an override replaces a value the mission has). White,
   zero-mean noise; belief from `PassThrough`, stated in the paper. A
   `.sweep.yaml` with `zip` over both horizontal axes
   (`position_stddev_m.0`, `position_stddev_m.1`).
3. **Wind.** Steady wind on the multirotor (drag on airspeed) and
   `SimpleAircraft`. FixedWing6DOF already has it (`wind_E/N/U`, the air's
   velocity). Gusts only if steady wind matters.
4. **Message delay sweep.** `delay_s` on GlobalNetwork and SphereNetwork,
   swept like loss. Random delay, if needed, as a keyed draw per message.
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

Decide first how trained policies run in Ripple: exported into an autonomy
plugin, or driven from Python. For the second, stop the tick at the decision
point (observations out, actions in), as Isaac Lab's `ManagerBasedRLEnv` and
Gymnasium's `step(actions)` do, with `motion_multiplier` as their
`decimation`; autonomy plugins become one way to fill the actions.

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
     as keyed random numbers so output stays byte-identical at any worker
     count and paired runs share their samples.
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

- Rerun the five speed checks and accept a new baseline (`perf.py --accept`):
  renaming the workload missions' keys changed their hash, so
  `speed-regression` reports different workloads until then. Outputs are
  byte-identical, so no change is expected.
- Every new piece (vehicle, autopilot, sensor) is checked against an outside
  source, with versions, conversions, and errors written down. Matching a
  source is not matching reality: borrowed coefficients stay labeled.
- With new effects off, old missions run exactly as before.
- Unicycle dynamics.
- More sensor models and plugin debug geometry.
- The viewer draws each Boundary box from the recorded scenario, with a
  toggle in the display to hide it.
- ArduPilotBridge's servo maps are still positional
  (`throttle: [3, 1000, 2000, 0, 1]`, channel then PWM and output ranges);
  give them named fields like the PID gains.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK, ROS,
and JSBSim. ArduPilot and Burn stay optional, out of the default build.
Pursuit was dropped as a task: results depend on how each paper designs the
evader.
