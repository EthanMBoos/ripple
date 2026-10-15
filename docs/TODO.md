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
(`simcontrol/world_plugins.rs`), missions declare LocalNetwork only when
needed, and GlobalNetwork's `delay_s` is a setting the disturbance sweeps use.
LocalNetwork's name hides that it delivers only within one entity; rename it
if a better name comes up.

1. **One MCAP file per run** (with 2). Today a run writes `frames.bin`
   (SCRIMMAGE's protobuf schema in `protocol/wire.rs`, length-prefixed with no
   schema in the file, read in Python by the hand-written `reference/frames.py`),
   `events.json`, `summary.csv`, `manifest.json`, with `--trace` a
   `trace.jsonl`, and unless `--no-rerun` a `recording.rrd`, a second, drawn
   copy of the same state.
   - **The record.** One `run.mcap` in Ripple's own protobuf schema, embedded
     in the file: frames on `/frames` and events on `/events` always; the
     resolved scenario and the end reason as MCAP metadata, replacing
     `manifest.json`. `--trace` adds channels for each agent's own believed
     state, sensor readings, message deliveries, and autonomy and controller
     outputs (after all substeps), as the trace records today. That is not
     all a policy observes: for learning data, also record believed contacts
     and received message payloads, and decide whether outputs are needed per
     substep. Today's
     `trace.jsonl` copies SCRIMMAGE's trace records (`simcontrol/trace.rs`);
     that shape moves to `reference/traces.py`, which converts the channels
     when comparing, as `scrimmage_xml.py` does for parameters.
   - **Time.** MCAP message times are unsigned nanoseconds; `start_s` may be
     negative, and the clock starts at `start_s - dt_s` (`simcontrol.rs`), so
     startup `EntityGenerated` events come before `start_s`. Message time is
     nanoseconds since that initial time, which the file records, and each
     message carries its exact simulation time as an f64.
   - **A failed run** that returns an error closes a valid partial file, as
     `frames.bin` keeps what was written now. A crash leaves a truncated file
     without the footer; `mcap recover` salvages it.
   - **Tested** (2026-10-15, scratch): an 11.5 MB `frames.bin`
     (`cpp-long/generated-000`, 80,004 entity rows) wrapped as MCAP by the
     `mcap` crate 0.25.0, twice, gave byte-identical files (no clock or random
     ID in the file), 7.0 MB with the default zstd, in under 1 s. Python read
     it into pandas in about 15 lines (`mcap`, `mcap-protobuf-support`),
     values identical to `frames.py`, in 0.4 s against 2.0 s.
   - **Tools.** The embedded schema lets MCAP tools (Foxglove, the `mcap`
     libraries) decode the data; it does not make them draw Ripple's
     vehicles, and the file is not a ROS 2 bag (ROS 2 uses its own encoding
     and bag conventions). Check what Foxglove and Rerun actually show.
   - **The display comes from the record.** `ripple replay` reads the MCAP
     and draws the picture into Rerun, as `--draw-trails` already rebuilds
     trails from `frames.bin` (`crates/cli/src/replay.rs`); `--viewer` still
     streams live from the same drawing code. A saved `.rrd` becomes an
     optional export, not written by default. Every run can then be viewed
     (sweeps and checks too), runs stop carrying a recording about 15 times
     the frames' size, and a drawing change reaches old runs. Drop the
     viewer's "legacy frame label".
   - **Why not `.rrd` as the record.** Rerun guarantees migration of files
     from 0.23 on (rerun.io/docs/concepts/logging-and-ingestion/rrd-format),
     but two identical runs wrote different `recording.rrd` bytes (268,028 and
     268,093 B), so the byte-for-byte proofs would break.
   - **Checks compare channels.** Determinism and equivalence require the
     `/frames` and `/events` channels to be byte-identical across workers,
     recording, and trace settings (a traced file has more channels, so whole
     files differ). Equivalence today also compares the summary, the trace,
     and the manifest; it keeps comparing each: metric results, trace
     channels (between traced runs), and the scenario and end reason in the
     metadata. The SCRIMMAGE comparison reads the trace channels. Only
     `reference/frames.py` still reads SCRIMMAGE's protobuf, for SCRIMMAGE's
     own output.
   - **Reference tools.** The scripts read runs through a few readers
     (`frames.read_frames` in 13 scripts, `traces.read_trace` in 3,
     `compare.read_summary`, `scenario.recorded`; `events.json` is parsed
     inline in 8). Add one reader, `run.read_run(path)`, returning frames,
     events, summary, scenario, and trace channels as the objects the scripts
     already use (`Frame`, `Entity`, ...); the old readers become thin
     wrappers, so most of the ~20 scripts change only file names and
     imports. SCRIMMAGE's own output keeps the old readers, so
     `reference_check.py`, `compare.py`, and `traces.py` read SCRIMMAGE the
     old way and Ripple from MCAP. `determinism.py` and `equivalence.py`
     compare channel bytes. The tools stay standard-library Python: write a
     small MCAP reader (the record format is documented and `frames.py`
     already decodes protobuf by hand) rather than depend on the `mcap`
     packages; the book's pandas example can still use them. On the Rust
     side: `ripple replay`, `sweep.rs`, `run.rs`, and the tests that read
     frames.
2. **SCRIMMAGE's entity fields** (with 1). Each frame carries three, and the
   public `Contact` type (`Contact::measured` in `estimator.rs`) carries two
   of them to plugins. Removing a field from the record and from the runtime
   types are separate decisions.
   - `sub_swarm_id` is the entity group's index (`generation.rs`, "entity
     block ID"), under SCRIMMAGE's name. Rename it `group`, or record each
     entity's group label once, when it spawns (live viewing has no final
     manifest), and drop the field.
   - `active` is false from the moment health reaches zero in interactions
     until removal at the tick's end; perfect contacts, NoisyContacts, and
     SimpleSpread skip such entities. It stays in the engine and in the
     record: `Simulation::new` runs interactions once before the first tick,
     so an entity can die there and appear inactive in the first frame
     (`spatial_lookup.rs` sets up such a collision), and a group may also be
     given `health: 0`. Reject `health` of 0 or less when the mission loads
     anyway, since an entity that starts dead means nothing.
   - `type` comes from `visual_model` (aircraft, quadrotor, sphere, mesh):
     display data. Record it once per entity when it spawns, not in every
     frame; decide whether `Contact` keeps it.
3. **Metric outputs.** The parameter-name scheme, applied to outputs, plus
   three of SCRIMMAGE's summary choices (`simcontrol/summary.rs`):
   - **Names.** SimpleCollisionMetrics: `non_team_coll`, `team_coll`,
     `ground_coll` to `non_team_collisions`, `team_collisions`,
     `ground_collisions`; `flight_time` to `flight_time_s`;
     `flight_time_norm` to `flight_time_fraction`. End conditions:
     `all_dead` to `no_entities`, `one_team` to `one_team_left`.
     SimpleSpread's names (`return`, `collisions`, `occupied_landmarks`,
     `landmark_distance`) are MPE's and stay, though `landmark_distance` is in
     metres.
   - **Precision.** Every value is written with 6 decimals (`{value:.6}`).
     Write full precision. SCRIMMAGE's CSV keeps 6, so the comparison checks
     values within a tolerance, not by relabeling columns.
   - **Missing values.** A metric with no value for a team is written as 0,
     indistinguishable from a real zero, in `summary.csv` and in sweep output
     (`sweep.rs`). Leave it out: an unset field in the MCAP, an empty cell in
     a CSV.
   - **`score`.** The summary's `score` sums every metric plugin's team score,
     so SimpleSpread's return and SimpleCollisionMetrics' weighted score add
     into a meaningless number when both run. Drop the combined score; each
     metric reports its own, under the metric's name (`sweep.rs` already reads
     each plugin's score).
   - **Readers.** `reference/compare.py` checks the summary against
     SCRIMMAGE's `summary.csv`, so the comparison tooling needs a column table
     like `KEYS` in `scrimmage_xml.py`. About a dozen reference scripts,
     `sweep.rs`, and three book pages read the summary. With item 1 the
     summary becomes an MCAP channel or metadata; decide together.
4. **Layout**, as two byte-identical commits.
   - **First:** rename `simcontrol.rs` and `simcontrol/` (SCRIMMAGE's
     SimControl) to `simulation.rs` and `simulation/`; the struct is already
     `Simulation`. Split `step_inner` (about 100 lines, every phase inline)
     into one function per phase, in the same order, so it reads like the
     book's phase table. The loop itself stays: its phase order is the
     standard sense-plan-act cycle, and rate-limited plugins and frame times
     are already correct. Named phases are only a start for the learning
     study's external stepping, which also needs a defined observation and
     action boundary and controller timing (see Learned policies). Also:
     `common/variable_io.rs` (SCRIMMAGE's VariableIO; it holds `Port`,
     `Ports`, `PluginIo`) to `ports.rs`; `entity_interaction.rs` to
     `interaction.rs`; `parse/mission.rs` and `parse/yaml_mission.rs` into one
     file (the split dates from two formats). Update `AGENTS.md` ("Organize
     around the C++ names") and the style guide ("keep the existing
     C++-familiar directories").
   - **Second:** everything about plugins under `plugin/`, one file per
     category. Today a category is split three ways: its trait at the crate
     root (`autonomy.rs`, `controller.rs`, `motion.rs`, `sensor.rs`,
     `metrics.rs`, `entity_interaction.rs`, `estimator.rs`, and
     `pubsub/network.rs`), its built-ins in `plugin/<category>.rs`, and the
     shared `Plugin` trait and contexts in `plugin_manager/entity_plugin.rs`
     (SCRIMMAGE's PluginManager and EntityPlugin; world plugins use it too).
     After: `plugin.rs` holds the shared API (`Plugin`, `PluginParams`,
     `StepTime`, `EntityInfo`, `Update`, `AgentContext`, `WorldContext`);
     `plugin/<category>.rs` holds each category's trait, context, and
     built-ins (`NetworkContext` stays in `pubsub`, re-exported);
     `plugin/registry.rs` holds `PluginRegistry`, `Catalog`, `CompiledPlugin`,
     and the built-in list; `supported_options.rs` becomes the plugin tests.
     `plugin_manager/` and the root trait files go. `ripple_core::plugin::*`
     exports the same names, so the user crates do not change; about 12
     internal `use` lines do.
5. **Small fixes.** FixedWing6DOF's module comment calls wind "added rather
   than subtracted, a quirk", but the code subtracts. Check `simcontrol.rs`'s
   "legacy missions can create entities one step before the start time".
   Compute substep time as `time_s + i * dt_s / N`, anchored to the tick's
   time, instead of adding the substep each time (SCRIMMAGE's way, still in
   `step_inner`); a separate commit from the layout, since it can move a
   time-threshold event by one substep, compared before and after.

## The book

For research and teaching: targeted trims, not a rewrite. Keep the beginner
material (tutorials, concepts, quick start) and write it as a person
explaining to a newcomer: short, plain, the key idea and why, with an example.
Exhaustive detail is left to the code, and a reader who wants the long version
can ask an LLM about the repo. Cut duplication (the phase order is told about
three times, plugin authoring four) and listings the code already states.
Do it after the MCAP and output changes (Remove SCRIMMAGE's shape, items 1
and 3), since three new pages describe outputs.

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
`decimation`; autonomy plugins become one way to fill the actions. Decide this
together with the run outputs (Remove SCRIMMAGE's shape, item 1).

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
