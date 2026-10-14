# Roadmap

What's left to build, in order: YAML as the only mission format, the disturbance
models, then the learning study. The interfaces that got in the way of the study
are fixed ([reference
notes](REFERENCE_NOTES.md#interfaces-fixed-for-the-study)). The study itself
(question, tasks, predictions, protocol) is in the
[paper](../paper/manuscript/main.tex); sources and version details are in
[LEARNING_LITERATURE.md](LEARNING_LITERATURE.md); intentional differences from
C++ SCRIMMAGE are in [reference notes](REFERENCE_NOTES.md). Keep the
[checks](../reference/README.md) passing after any simulation change.

## YAML only

Ripple's missions are YAML, but 21 files under `missions/` are still XML,
kept because the SCRIMMAGE comparison runs the same file in both simulators
and SCRIMMAGE reads only XML. Move the comparison to YAML and remove the XML
reader.

1. **A YAML-to-SCRIMMAGE-XML writer** in `reference/` (Python). Every value is
   written explicitly, since XML's defaults differ from YAML's (spawn variance
   100 m² against 0, noise 5 m against none): positions, variances, heading,
   velocity, spawn schedules, networks and their names, and each plugin's
   parameters under its XML name and format (`pos_noise_0="mean stddev"`, PID
   gains in SCRIMMAGE's order, waypoints as `"x,y,z; x,y,z"`, matrices and
   rotor configurations as bracketed text). `reference_check.py` runs Ripple
   on the YAML and SCRIMMAGE on the written XML.
2. **Convert every XML mission** (`missions/`, 21 files; the test fixtures
   `perception_communication`, `plugin_contracts`, `plugin_defaults`; the
   three in `reference/inputs/`) to YAML, each proven to run identically with
   `ripple compare` before its XML is deleted. Four already have YAML forms.
   Templates replace `entity_common` and `param_common`; overrides replace
   `${name=default}`; `randomize_every_spawn` replaces
   `use_variance_all_ents`.
3. **The generated missions.** `perf.py`'s workloads (an XML template),
   `generate_scenarios.py`, `divergence.py`'s nudges, `benchmark.py`,
   `message_benchmark.py`, and `rk4_loads.py` write or edit XML; they write
   YAML instead. A changed workload definition needs a new speed baseline:
   accept it only after the old and new workloads give byte-identical output
   and the same times.
4. **Proof.** The C++ matrix, the generated scenarios, and the long runs pass
   unchanged on the written XML; `reference/equivalence.py` finds every
   output byte-identical before and after.
5. **Remove.** `parse/xml.rs` and `parse/xml_mission.rs` (675 lines), the
   text-value branch of plugin parameters (`PluginValues::Text`, the text
   deserializer in `parse/params.rs`), the XML-only conversions (noise keys,
   scalar `speed`, the 100 m² default, `use_variance_all_ents`), plugin
   overlays (`RIPPLE_PLUGIN_PATH`, `RIPPLE_MISSION_PATH`), `ripple compare`'s
   XML-to-YAML purpose, the rule that a `.yaml` beside a `.xml` runs
   identically, and the XML tests. The 39 unit tests that configure a plugin
   from text (`PluginParams::text`) take YAML values instead.
6. **Keep.** Every contract test that came from a past defect (time as steps
   x dt, spawn sampling, rate-limited dt, the coordinated turn, keyed noise,
   zero-noise readings, and the rest in the reference notes): they describe
   correct behavior, not SCRIMMAGE's. Keep the comparison plumbing while the
   comparison is a check (`frames.bin` in SCRIMMAGE's protobuf format, the
   trace); the reference notes list it under "Left behind when the
   comparison is retired". Retained evidence keeps its `input.xml` files as
   records of what ran.
7. **Elsewhere.** SCRIMMAGE's verbatim missions (`test_missions/`, with the
   `motion_multipler` typo) and the C++ test drivers in `reference/fixtures/`
   can live on `benchmarking-edits` instead of here. Rewrite the book's XML
   passages (the YAML guide, plugin API reference, missions README) and
   `AGENTS.md`'s XML rules.

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

- Every new piece (vehicle, autopilot, sensor) is checked against an outside
  source, with versions, conversions, and errors written down. Matching a
  source is not matching reality: borrowed coefficients stay labeled.
- With new effects off, old missions run exactly as before.
- Unicycle dynamics.
- More sensor models and plugin debug geometry.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK, ROS,
and JSBSim. ArduPilot and Burn stay optional, out of the default build.
Pursuit was dropped as a task: results depend on how each paper designs the
evader.
