# C++ reference findings

Source review: sibling `../scrimmage`, branch `Ubuntu-24.04`, commit
`4bdf41fb06facaea358d478d9a07aa4d77fcac27` (2026-09-23/24). This identifies the
reviewed source, not a required pin. Follow the branch for new comparisons. The
[roadmap](TODO.md) defines selected scope; the [book](../book/src/SUMMARY.md)
describes current Rust APIs. Comparison results are in the
[paper](../paper/README.md).

## Corrections to SCRIMMAGE's documentation

These findings come from source, rather than SCRIMMAGE's documentation
(`../scrimmage/docs/source`).

| Source in the C++ checkout | Finding |
| --- | --- |
| `src/simcontrol/SimControl.cpp`, `run_single_step` / `run_entities` | Generate and log pre-step contacts; autonomy; all controller substeps; all motion substeps; sensors; interactions; networks; metrics; removal/output/time. Controller and motion substeps are not interleaved on this branch; they were until 2025-12, and `benchmarking-edits` restores that (see [Substep order](#substep-order-a-c-regression-fixed-on-the-comparison-branch)). |
| Same file, network/metrics phases | Metrics can consume interaction messages in the same tick. Delivery is not universally delayed to the next tick. |
| Same file, `run_logging` / finalization | Pre-step state is labeled `t + dt`; terminal logging repeats the final timestamp with terminal state. Both are fixed on `benchmarking-edits` and in Ripple ([core defects](#core-defects-fixed-on-both-sides)). |
| `src/plugins/network/LocalNetwork/LocalNetwork.cpp` | Reachability requires the same non-null parent entity, not proximity. |
| `src/plugins/interaction/SimpleCollision/SimpleCollision.cpp` | Collision uses strict distance against `collision_range`, not summed entity/visual radii. Startup rejection is separate. |
| `src/entity/Entity.cpp` | Truth and belief initially alias; `set_state_belief` detaches belief. Rust keeps belief separate and has estimators own it (see the estimation difference below). |

Preserve these observable timing/recording contracts during refactors. Current
Rust lifecycle and delivery rules are in the
[plugin reference](../book/src/reference/plugin-api.md#execution-and-lifecycle).

## Missions are YAML

Ripple reads only YAML. SCRIMMAGE reads only XML, so the comparison writes
SCRIMMAGE's mission from the scenario a Ripple run records in its
`run.mcap` (`reference/scrimmage_xml.py`): Ripple runs the YAML first,
then SCRIMMAGE runs the written XML.

- **What is written.** Every entity value, since the two simulators' entity
  defaults differ (SCRIMMAGE spreads spawns over 100 m² unless told
  otherwise). Plugin parameters only as the mission gives them, so a compared
  mission writes out any whose default differs (as
  `reference/inputs/fixed-wing-recovery.yaml` does for its altitude gains).
  NoisyState and NoisyContacts noise as SCRIMMAGE's `pos_noise_*`,
  `vel_noise_*`, and `orient_noise_*` keys, every axis written (SCRIMMAGE's own
  default is 5 m and 5 m/s of noise). A network's YAML label becomes its
  `name`; networks run in name order on both sides. Lists are written
  space-separated, the one form every compared plugin reads. A compared
  mission uses the default estimators and no terrain, which XML cannot name.
- **One reader for both.** SCRIMMAGE's input now comes through Ripple's YAML
  loading, so a loading error would reach both simulators rather than show up
  as a difference. The loader is derived from the scenario types, and rejects
  unknown keys. Before Ripple's XML reader was removed, every mission's YAML
  form was proven to set up the same simulation as its XML and to write
  byte-identical frames, events, and summaries (`ripple compare`, also
  removed): the 21 missions, the three `reference/inputs/`, the three test
  fixtures under every override their tests use, the 100 generated scenarios,
  and the speed workloads. The XML written from each YAML mission was read back
  by the XML reader and proven the same way. The matrix then passed 15 of 15
  on the written XML, largest position difference 4.93e-12 m, as before.
- **Two missions changed shape, not output.** `aircraft-multiplier-typo` is
  gone: it ran SCRIMMAGE's `straight_cpu_mul.xml`, whose misspelled
  `motion_multipler` both simulators ignored, which a YAML mission cannot
  express; its YAML form is `aircraft-long-horizon`. In
  `verification/collision-boundary.yaml` the closing agents have one
  ConstantVelocity rather than a stationary one followed by a moving one, since
  a template's slot is replaced, not appended to; frames, events, and summary
  are byte-identical. SCRIMMAGE's own test missions are in its
  `test/test_missions/`.
- **Ripple's parameter names, SCRIMMAGE's keys.** Each plugin parameter has
  one name ([Rust style](../book/src/development/rust-style.md#parameter-names)):
  `speed_mps`, not `speed`; `c_roll_p`, not `C_LP`; PID gains as
  `{kp, ki, kd, integrate_within_*}`, not `"P I D band"`; `wind_mps` as one
  `[east, north, up]`. SCRIMMAGE's names live only in `KEYS` in
  `reference/scrimmage_xml.py`, which writes each parameter under SCRIMMAGE's
  key, sorted by it as before, and leaves a zero wind component to SCRIMMAGE's
  default. Options Ripple never implemented were deleted rather than kept as
  fields that reject a value, among them SCRIMMAGE's display switches. The
  rename was proven like the YAML move: 142 cases (every mission, the
  reference inputs, the fixtures, the speed workloads, and the 100 generated
  scenarios), the old build on the old missions against the new build on the
  new ones, wrote byte-identical frames, events, summaries, and traces. The
  written XML is identical except where a mission set a deleted option:
  `world-networks` and 20 generated scenarios lose `show_boundary="false"`,
  and `aircraft-long-horizon` loses `show_text_label="true"` and
  `generate_entities="false"`. `show_boundary` and `show_text_label` only draw
  in SCRIMMAGE's GUI, which the comparison never opens, and
  `generate_entities` is false by default.

## Metric outputs

SCRIMMAGE's `summary.csv` adds every metric plugin's team score into one
`score`, prints six decimals, and writes 0 for a value a team doesn't have.
Ripple's `/summary` keeps each plugin's own score and columns, at full
precision, and leaves out a missing value. SimpleCollisionMetrics' columns are
`flight_time_s`, `flight_time_fraction`, `non_team_collisions`,
`team_collisions`, and `ground_collisions` (SCRIMMAGE's `flight_time`,
`flight_time_norm`, `non_team_coll`, `team_coll`, `ground_coll`), and the end
conditions `no_entities` and `one_team_left` (`all_dead`, `one_team`), whose
termination reasons are now `NoEntities` and `OneTeamLeft`.
`compare.py`'s `scrimmage_summary` rebuilds SCRIMMAGE's table from Ripple's
as `SimControl::output_summary` writes it: every plugin's declared columns in
order under SCRIMMAGE's names (a name two plugins declare appears twice), the
summed score, and per team one map of values where the first plugin to set a
name keeps it and an unset name is 0. `scrimmage_xml.py` writes its
end-condition names; summaries still agree within 1e-6, since C++ rounds.

- **Proof (2026-10-15).** The 23 missions under `missions/` (all but
  `terrain-mcmillan`) and the starter mission, old build against new, wrote
  byte-identical entities, frames, and events; the old summary text was
  reproduced exactly from the new summary by the deleted rule after renaming
  the columns; result metadata and scenario differ only by the renamed
  termination reasons and end conditions. Both sweeps' `results.jsonl`, already
  at full precision, are identical after renaming.

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
- **Straight and the sensor messages.** C++ Straight steers by the newest
  `StateWithCovariance` message, which holds the same measurement as belief.
  Rust sensors publish no message (readings go to estimators), and Rust
  Straight steers by belief.

- **Survivor flight time:** C++ publishes `EntityPresentAtEnd` after its last
  network phase, so metrics credit survivors only until the latest removal. Rust
  credits them until report time. In `networks-local-global.yaml`, surviving
  teams report 29.9 s instead of 0.4 s. Frames are unaffected; flight-time
  summary columns differ. See the regression in
  [SimpleCollisionMetrics](../crates/core/src/plugin/metrics/simple_collision_metrics/simple_collision_metrics.rs).
- **Teams that first spawn after t=0:** C++ SimpleCollisionMetrics lists teams
  at its first step, then totals later teams with no normalization time: their
  `flight_time_norm` is infinite and `score` NaN. Rust reports every team with a
  generated entity using the same normalization as other teams; `flight_time`
  and collision counts match. Seen in
  `verification/aircraft-substeps-scheduled.yaml`.
- **FixedWing6DOF inertia:** C++'s bundled slug-unit default silently overrides
  a supplied SI matrix. Rust accepts either `inertia_slug_ft2` or
  `inertia_kgm2` (three rows; the comparison writes them as C++'s
  `inertia_matrix_slug_ft_sq` and `inertia_matrix`), rejects both together,
  and uses the named airframe's if neither is supplied.
- **FixedWing6DOF elevator drag key:** C++ reads `C_D_delta_elevator_` (with a
  trailing underscore), but every bundled parameter file writes
  `C_D_delta_elevator`, so C++ always uses the 0.01 default. That is harmless
  for the A-4D default (its file also says 0.01), but the Rascal file's 0.03 is
  never used. Rust reads the same underscore key and rejects the plain name, so
  a value is never silently ignored. `missions/fixed-wing-6dof-rascal.yaml`
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
  controller N times faster. A rate-limited controller receives the time since
  it last ran, summed over substeps, so its tuning does not change with
  `motion_multiplier` either (`agent_step` in `entity/plugin_stack.rs`; C++
  `EntityPlugin::loop_dt`, commit `719c0940c`; see [Core defects fixed on both
  sides](#core-defects-fixed-on-both-sides)).

  Both also lower the default altitude derivative gain from 0.01 to 0.002.
  This is a tuning choice, not a fix: upstream's gain is too strong for the
  default 220 m/s A-4 (unmodified C++ can look better with substeps only by
  accident). The altitude loop turns climb rate into desired pitch, and at
  that speed its derivative path asks for about 2.2 rad of pitch change per
  rad of actual pitch change, so a pitch upset grows into an altitude and
  heading oscillation. One FixedWing6DOF, 45 s of `Straight` flight at 1000 m
  after a 5 degree roll and 3 degree pitch upset, 0.01 s ticks
  (`reference/inputs/aircraft-pid-upset.yaml`): with no substeps (C++ and Rust
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
  earlier C++ R-tree: C++ builds the tree when entities are generated
  (before motion), queries it with the publisher's post-motion position, and
  caches the answer for both directions of the pair, so whichever endpoint
  publishes first decides the link. Results differ only for moving vehicles
  near the range edge; exact parity would need C++'s publisher order.
  AuctionAssign has configurable network/deadline and an
  optional winner instead of a negative sentinel. These are not
  sample-for-sample C++ RNG ports.

Source details worth preserving: both sensors keep the true angular velocity
(NoisyState did not in unmodified SCRIMMAGE) and right-multiply attitude
errors. SphereNetwork has strict range, inclusive
altitude-plane bounds, and same-parent geometry bypass that still draws for
loss. AuctionAssign includes self-bidding, keeps the first maximum on ties, and
closes strictly after its deadline. See [plugin
behavior](../book/src/reference/plugin-api.md#noisycontacts).

## Behavior kept from SCRIMMAGE

Ripple's code comments explain each behavior on its own terms. This is the
list of what is there because SCRIMMAGE does it. While the comparison is a
check, a correctness fix to ported code is made on `benchmarking-edits` too,
so the two still agree; record each fix and its evidence here.

Simplifications that can stay:

| Behavior | Where | Why |
| --- | --- | --- |
| NoisyContacts measures every entity, at any range | `noisy_contacts.rs` | An ideal sensor; range and field of view are a modeling choice |
| SingleIntegrator draws positive pitch nose-down; yaw snaps to east at zero velocity | `single_integrator.rs` | Drawing only; the motion doesn't depend on attitude |
| Boundary publishes once | `boundary.rs` | Matters only if a mission spawns late |
| AuctionAssign runs only on entity 1 | `auction_assign.rs` | A demo plugin |
| Degrees to radians as `deg * PI / 180` (`Angles::deg2rad`) | `math.rs` | As correct as `to_radians`; keeps outputs bit-identical |

SCRIMMAGE's defects were kept until they were fixed on both sides; see [core
defects](#core-defects-fixed-on-both-sides). One is left, for the disturbance
models in the [roadmap](TODO.md):

| Defect | Where | Matters when |
| --- | --- | --- |
| Neither sensor reports its noise covariance (SCRIMMAGE's messages carry a placeholder, identity or 5I) | sensors | A filter needs it (the roadmap's Kalman filter) |

## Left behind when the comparison is retired

These exist so Ripple and SCRIMMAGE can be compared directly. They are
awkward but contained, so they stay while the comparison is a check; once the
comparison has proven what it needs to, they go.

| What | Where | Replaced by |
| --- | --- | --- |
| Spawn spread given as a variance (`position_variance_m2`, `heading_variance_deg2`) | `entity.rs`, `scenario.rs` | A standard deviation in metres and degrees |
| The first entities are created one step before the start time (generation runs at start − dt) | `simcontrol.rs` | Generation at the start time |
| SCRIMMAGE's mission shapes: `randomize_every_spawn` (its `use_variance_all_ents`); PID gains in its order, with the integral band in degrees | `simcontrol/generation.rs`, `common/pid.rs` | Named gains and one spawn rule |
| SCRIMMAGE's XML, written from a run's recorded scenario | `reference/scrimmage_xml.py`, `reference/scenario.py` | Nothing |

## Core defects fixed on both sides

Each fix states a contract, has a focused test of it that fails on the code
before the fix (`6d7cae3`, with the new tests copied in), and is made the same
way on `benchmarking-edits` (one commit; files below), so the C++ comparison
still agrees. The numbers in "Before" are those failing tests' output.

| Fix | Contract | Before | Test | SCRIMMAGE files |
| --- | --- | --- | --- | --- |
| NoisyState angular velocity | With no rate noise, the reading has the true angular velocity | Belief rate 0 while turning; a Crazyflie with a zero-noise NoisyState tumbles (below) | `belief_reports_the_true_angular_velocity` | `NoisyState.cpp` |
| NoisyState first reading | Sampled like every later one, from the state the motion model set up | First belief exactly truth (bias 3 m read as 0) | `the_first_reading_is_sampled_from_the_starting_state` | `NoisyState.cpp`; `Entity.cpp` initializes sensors after the motion model |
| SimpleAircraft climb rate | Reported velocity is the change in position | Climbing at 4.3 m/s, reported 0 (or the opposite sign) | `reported_velocity_is_the_change_in_position` | `SimpleAircraft.cpp/.h` |
| SimpleAircraft turn rates | Reported angular velocity is the change in attitude | 0.67 rad/s turn reported as 0 | `reported_angular_velocity_is_the_change_in_attitude`, `an_angle_held_at_its_limit_reports_no_rate` | same |
| SimpleAircraft state | A change to position, attitude, or speed carries into the next step (velocity direction follows the nose) | A 50 m push undone the next step | `a_change_to_position_attitude_or_speed_carries_into_the_next_step` | same |
| FixedWing6DOF wind | Airspeed is velocity minus wind | Flight in wind w at v + w differed from still air by 5.7 m in 2 s | `flight_in_wind_is_flight_in_still_air_carried_by_the_wind`, `airspeed_is_speed_through_the_air` | `FixedWing6DOF.cpp` |
| Rate-limited plugins | dt is the time since the plugin last ran; it runs once per period from its first tick | 5 Hz at 10 Hz ran at 0, 0.1, 0.3, 0.5 s, each with dt 0.1 | `a_rate_limited_plugin_runs_once_per_period_...`, `loop_rate` unit tests | `EntityPlugin.cpp/.h`, `SimControl.cpp` |
| Spawn sampling | Every spawn is sampled around the configured start, unrounded | Mean heading of 200 spawns 49.8 degrees for a configured 90 | `every_spawn_is_sampled_around_the_configured_start` | `SimControl.cpp` |
| Time | Time is start + steps x dt; a frame is labeled with its state's time; every label is unique | Frame 0 labeled 0.1 s; the final label repeated | `time_is_the_step_count_times_dt_...`, `a_spawn_rate_equal_to_the_tick_rate_spawns_every_tick` | `SimControl.cpp/.h` |
| MpeContact timing | A zero-noise reading is the true state, contact or not | Reading 0.547 m/s against a true 0.447 m/s on a contact step | `a_zero_noise_reading_is_the_true_state_through_mpe_contact` | none (Rust-only; MPE2 is the reference) |
| Duplicate sensor messages | Readings and decisions unchanged; the unused messages are gone | NoisyState and NoisyContacts published a LocalNetwork copy only Straight received, and discarded | the comparison below | none: in SCRIMMAGE the message is the sensor's output |

Details worth knowing:

- **Limits in SimpleAircraft.** RK4's inner stages carried roll past its
  limit, so at a 30 degree bank the aircraft turned as if banked up to about
  36 degrees (12% faster at dt 0.1 s) and the reported attitude overshot. Now
  a value at a limit stays there while the command pushes outward (within
  1e-9 rad, which absorbs reading an angle back from the quaternion), and the
  limits apply after each step too. Without that allowance one-ulp
  differences between macOS and Linux math libraries decided whether a held
  bank moved, and `world-networks` diverged at 4.2 s. Initialization also no
  longer negates the mission's roll, and its initial velocity includes pitch.
- **SimpleAircraft's roll sign.** SCRIMMAGE kept roll with the opposite sign
  to the state's (positive banked left, to suit `yaw rate = v / R tan(roll)`),
  negated it when writing the state but not when reading it, and
  SimpleAircraftControllerPID's roll mode carried a compensating minus. The
  model now uses the state's roll (positive banks right and turns right), the
  turn rate carries the minus, and the controller's heading loop negates its
  output; `a_positive_roll_rate_banks_right_and_turns_right` checks it. A
  `roll_rate` command now has the opposite sign; no mission sends one.
  Trajectories are unchanged, since negation is exact in floating point:
  `reference/equivalence.py` (42 cases, 1, 2, and 8 workers) found frames,
  events, and summaries byte-identical. Traces differ in exactly two ways:
  the traced `roll_rate` output is negated, and a level aircraft's belief
  quaternion holds 0.0 where it held -0.0 (the old negated zero roll). On
  `benchmarking-edits` it is a second commit (`SimpleAircraft.cpp/.h`,
  `SimpleAircraftControllerPID.cpp`); the C++ comparison passes 15 of 15.
- **Rate and spawn schedules round.** With time counted rather than summed,
  t + 1/rate can land just after the tick it means. A rate-limited plugin and
  a spawn batch now run on the tick nearest their due time; without that a
  10 Hz group in a 10 Hz simulation skipped a spawn at 1.3 s, and a 0.2 Hz
  sensor ran 0.1 s late.
- **Missions.** `fixed-wing-6dof.yaml` and `reference/inputs/fixed-wing-recovery.yaml`
  now give the wind's physical sign (`wind_E="-3" wind_N="2"`), so they fly
  exactly as before. In `networks-local-global.yaml` the low aircraft's ground
  hit came only from the exact first reading (a +20 m altitude bias present
  from the start makes Straight hold its true altitude); it now starts nose
  down instead.
- **Trace.** Ripple records each sensor reading; `reference/traces.py` pairs
  it with SCRIMMAGE's publication of the same measurement and skips that
  publication's deliveries. Non-interference is now traced against untraced
  SCRIMMAGE; the comparison branch no longer matches unmodified SCRIMMAGE on
  any mission (frame labels alone differ), which `cpp_baseline_equality`
  records.
- **Crazyflie with NoisyState.** `missions/multirotor-velocity.yaml` with a
  zero-noise NoisyState (and LocalNetwork, which the old sensor needed), over
  2-10 s: before, the moving vehicles reached 90-150 degrees of tilt at
  5 rad/s and ended 15-42 m/s off their commands; after, at most 7.2 degrees
  and 0.15 rad/s, as with perfect belief (velocity error 0.004 m/s).
- **Comparisons after.** The C++ matrix (15 missions, against
  `benchmarking-edits` with these fixes) passes: positions agree to
  4.9e-12 m, 812 sensor states match, and 722 SCRIMMAGE sensor-message
  deliveries are skipped as described. MPE2 parity: all 100 episodes
  bit-identical (56 before; the 44 others were contact episodes). The other
  checks run in the full suite.

## Interfaces fixed for the study

Interfaces and behavior kept to match SCRIMMAGE that got in the way of the
study (the roadmap's milestone A). Where SCRIMMAGE's behavior changes, it
changes on `benchmarking-edits` too (commits "Replace the libc++ spawn stream
with keyed random draws" and "Turn SimpleAircraft as a coordinated turn and
make SingleIntegrator's max_speed a cap"), so the comparison still agrees;
the rest are Ripple interfaces whose defaults reproduce SCRIMMAGE. The numbers
in "Before" are the focused tests' output on the code before the change
(`cc6661a`, with the new tests copied in).

| Change | Contract | Before | Test | SCRIMMAGE files |
| --- | --- | --- | --- | --- |
| Keyed random numbers | An observer's error on a target doesn't depend on what else it sees | Target 2's x error at tick 1: -8.566 m in one run, -2.563 m with a third target | `an_observers_error_on_a_target_does_not_depend_on_what_else_it_sees` | `KeyedRandom.h` (new), `Random.cpp/.h`, `Time.h`, `EntityPlugin.h`, `Entity.cpp`, `NoisyState.cpp/.h`, `NoisyContacts.cpp`, `Network.cpp/.h`, `SphereNetwork.cpp/.h`, `SimControl.cpp/.h`; opt-in (below) |
| SimpleAircraft turn | Turn rate g tan(bank) / speed: 110 m radius at 25 m/s and 30 degrees, growing with speed squared | speed / 13 m x tan(bank): a 22.5 m radius at every speed | `a_banked_turn_is_a_coordinated_turn_whose_radius_grows_with_speed_squared` | `SimpleAircraft.cpp/.h/.xml` |
| SingleIntegrator `max_speed` | A cap: a slower command is flown as commanded, a faster one at `max_speed` | A 5 m/s command with `max_speed` 10 flew at 10 m/s | `velocity_integrates_position_and_max_speed_only_caps_it` | `SingleIntegrator.cpp` |
| Noise parameters | A sensor with no parameters measures exactly; named bias and standard deviation per axis | 5 m and 5 m/s of noise unless every axis was zeroed | `without_parameters_there_is_no_noise`, `scrimmage_noise_keys_become_named_fields_with_scrimmage_defaults` | none: XML keeps `pos_noise_0="mean stddev"` and SCRIMMAGE's defaults |
| SimpleAircraft limits | Physical limits (speeds, bank, acceleration, climb, descent); `airframe: rascal` flies like FixedWing6DOF's Rascal within 5% | Throttle in [-100, 100] / 5; no climb limit but pitch | `reference/simple_aircraft_fit.py` | none: the defaults are SCRIMMAGE's numbers |
| Velocity commands for aircraft | Heading along the command (kept at zero), speed and climb within the aircraft's limits, turns no tighter than its bank allows | none: aircraft took heading, altitude, and speed | `crates/core/tests/aircraft_velocity.rs` | none (Rust-only controller) |
| Message delay | `delay_s` on GlobalNetwork and SphereNetwork arrives at the first tick at or after it | delay rejected | `global_network_delay_arrives_at_the_first_tick_at_or_after_it` | none (below) |

Details worth knowing:

- **Keyed random numbers** are described under [Random
  numbers](#random-numbers-keyed-draws-on-both-sides).
- **The turn law.** The fixed-wing rung exists to test methods on a vehicle
  that can't turn on a dime; SCRIMMAGE's law turned five times too tight at
  25 m/s and 30 degrees. `turning_radius`, `speed_target`, and
  `radius_slope_per_speed` are gone on both sides; the generated scenarios vary
  the bank limit instead, and `divergence.py` nudges it.
- **SimpleAircraft's `rascal`.** The Rascal holds 47 degrees of bank but turns
  at 26.3 deg/s, 13% slower than a coordinated turn at that bank (it doesn't
  fly perfectly coordinated). SimpleAircraft's bank only sets its turn rate, so
  its bank limit is 43 degrees, the bank that gives 26.3 deg/s. Its pitch
  limit, 30 degrees, only has to let the climb and descent limits bind at
  cruise: SimpleAircraft keeps its speed in a climb or dive (it has no energy
  model), where the Rascal slows to 17.4 m/s climbing and speeds up to
  32 m/s descending.
- **SCRIMMAGE's noise keys.** The comparison writes the named fields as
  `pos_noise_*`, `vel_noise_*`, and `orient_noise_*`, every axis given, since a
  SCRIMMAGE sensor without them measures with 5 m of horizontal noise.
- **SCRIMMAGE's message delay** (not ported; Ripple's engine delays
  correctly). `comm_delay` works on every network but: queued messages are
  released only inside the per-publisher loop when that topic has new
  messages, so they stall when publishing stops (`src/pubsub/Network.cpp`,
  `Network::step`); `comm_delay="0"` queues instead of delivering (only -1,
  the default, is immediate); `is_stochastic_delay` only sorts the queue (the
  base delay is constant); the due time is written onto the message object
  all subscribers share (`msg->time`); trimming a device's queues to its
  `max_queue_size` subtracts the limit from the undelivered queue's size
  without checking which is larger, so a short undelivered queue underflows
  (`NetworkDevice::enforce_queue_size`); and the `DelayedComms` plugin
  `missions/delay-test.xml` names doesn't exist. No compared mission uses
  delay, so nothing changes on `benchmarking-edits`.
- **Comparisons after.** The C++ matrix (16 missions, with the new
  `sphere-network-loss`) passes against `benchmarking-edits` with these
  changes. `reference/equivalence.py` found the 30 of 42 cases that draw no
  random numbers byte-identical after the keyed draws; the other 12 (noise,
  randomized spawns, churn, collision) change once.
- **Planted bugs.** "LocalNetwork delivers across entities" stopped being
  detectable once sensors no longer published readings on LocalNetwork (the
  duplicate-message fix above): no compared mission sends a LocalNetwork
  message. `sensitivity.py` replaces it with a defect in the keyed loss draws
  (sender and receiver swapped in the key), which `sphere-network-loss`
  compares draw for draw.

## Findings from the comparison trace

- C++ Straight subscribes to `ContactsWithCovariances` and stores the map
  without reading it, and steers by `StateWithCovariance`, which equals belief.
  Rust Straight subscribes to neither; the trace skips those deliveries.
- C++ `VariableIO::connect` replaces a plugin's output index with the next
  plugin's input index. Outputs the next plugin does not read are silently
  dropped, and reading them returns NaN with a warning. The trace records only
  outputs the next plugin reads, in both implementations.
- C++ logged the final tick twice, before its step and as the terminal frame
  in `finalize()`; with the time fix the terminal frame has its own time.

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

## Fixed wing: built-in aircraft instead of C++'s default

C++ FixedWing6DOF's defaults (`FixedWing6DOF.xml`) are the A-4D Skyhawk; the
identical `FixedWing6DOF-A-4D.xml` cites Nelson, *Flight Stability and
Automatic Control*, 2nd ed., p. 405, and Pratt and McGhee's NPSNET paper.
`FixedWing6DOF-Rascal.xml` cites "Ardupilot JSBSim Model". Ripple has no
default aircraft: `airframe: a4d` and `airframe: rascal` load those numbers
(`plugin/motion/fixed_wing_6dof/airframe.rs`), or a mission gives every
vehicle number; environment and option keys keep their defaults.
AircraftPIDController takes the same `airframe` for its gains and limits.

- **Missions that flew the default** name `airframe="a4d"` on the motion model
  and the controller (`fixed-wing-6dof.yaml`, `reference/inputs/`, generated
  scenarios). C++ ignores the key, so the C++ comparison is unchanged: on
  2026-10-12 `fixed-wing-a4` and `fixed-wing-rascal` agreed to 6.9e-13 m and
  2.3e-13 m, as before.
- **The Rascal's numbers** match JSBSim's `Rascal.xml` (tables read at zero
  angle of attack and low Mach) except: mass 5.91 kg (JSBSim's 13 lb is 5.90
  kg); span 2.70 m (JSBSim 2.795 m); C_D_alpha 0 (JSBSim's drag table rises
  about 0.11 per rad); induced drag through efficiency 0.995, inherited from
  the A-4D default (JSBSim states 0.04 C_L^2); elevator drag 0.01 times
  |elevator| in radians (C++ never reads its file's 0.03, and JSBSim's 0.03
  multiplies the signed normalized deflection, elevator / 0.30, so units and
  sign treatment differ too); elevator limits +-0.30 rad (JSBSim's elevator
  moves -0.35 to +0.30; aileron and rudder +-0.35 match). These are inherited
  modeling choices, kept because they are what has been flown. Its thrust
  range (0 to 25 N) has no JSBSim counterpart. Every mission flies these
  radian limits. C++'s `FixedWing6DOF-Rascal.xml` sets every surface limit to
  +-1 ("use normalized actuators for rascal model", `2cab6d1190`, 2018), but
  FixedWing6DOF never converts a normalized command: it multiplies the clamped
  deflection straight into per-radian coefficients, so +-1 is +-1 rad, about
  3x the real travel. `fixed-wing-6dof-rascal.yaml` kept those limits until
  2026-10-14; with the radian limits its level and glide aircraft fly
  identical paths and its upset aircraft differs by at most 2.89 m. The
  comparison writes the Rascal's numbers out for C++, which has no airframe
  profiles (`reference/scrimmage_xml.py`).
- **AircraftPIDController's `rascal` settings** are `a4d`'s gains with
  ArduPlane's attitude limits (`ROLL_LIMIT_DEG` 45, `PITCH_MAX` 20,
  `ArduPlane/config.h` at `ccdfd945aefb`). Measured on the Rascal at 20 m/s
  (`reference/rascal_control.py`, evidence in
  [`paper/data/rascal-control`](../paper/data/rascal-control/summary.md)):
  heading, altitude, and speed steps all settle; the altitude derivative gain
  flies identically from 0 to 0.02 after 5/3 and 60/30 degree upsets at 0.01,
  0.005, and 0.0025 s steps (the derivative path scales with airspeed, so the
  A-4D's reason for 0.002 does not apply); and the inherited 45/60 degree
  limits let a 20 m altitude step pitch it to 48 degrees, where 20/45 settle
  the same step in 3.4 s at 23 degrees. Unmodified C++ (D 0.01) flies the same
  upsets as well as Ripple with no substeps; with 5 substeps it ends 53 to 60
  degrees off heading at D 0.01 and at 0.002 alike, so its substep defects,
  not the gain, matter there. `fixed-wing-6dof-rascal.yaml` keeps the 45/60
  limits as overrides.
- **Outputs unchanged.** Before and after (2026-10-12): all 31 missions under
  `missions/` and `reference/inputs/` wrote byte-identical frames,
  summaries, and events; the three ArduPilot missions' vehicle blocks, flown
  20 s under AircraftPIDController, wrote identical frames; and the 21
  fixed-wing scenarios of `generate_scenarios.py --seed 1` were identical.

## Multirotor: what Ripple takes from C++

The code, the book, and the missions describe Ripple's multirotor on its own
terms; this section is the one record of how it relates to C++ SCRIMMAGE's
`Multirotor` (`src/plugins/motion/Multirotor/`). The module keeps the LGPL
attribution line it needs as a derived work.

- **Kept:** the model's structure (rigid body, thrust and reaction torque
  with rotor speed squared, commands clamped to [omega_min, omega_max]), the
  optional quadratic body drag (`c_D`), each rotor's direction, hub position,
  and tilt (C++'s `rotor_config` rows, now the `rotors` list, written back as
  rows for the comparison), and the `motor_0...` input ports.
- **Fixed:** RK4 loads (above) and three further defects (below), on both
  sides.
- **Not kept:** C++'s default vehicle. Ripple has no default vehicle and one
  built-in, the Crazyflie 2.0; any other must be given in full. The C++
  comparison missions (`multirotor.yaml`,
  `verification/multirotor-fixes.yaml`) write C++'s numbers out; their
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

`missions/multirotor.yaml` triggers none of these (no initial velocity, flat
untilted rotors, motors above idle): before and after the fixes its frames
differ by at most 6e-12 m and 7e-12 m/s, rounding. The fixture
`verification/multirotor-fixes.yaml` reaches all three (a starting velocity,
tilted and raised rotors, idle motors). Before the C++ fix it separated C++
from Rust at the first step on every vehicle; with it, the two agree to
3.7e-13 m (and `multirotor.yaml` to 4.9e-12 m). Unmodified C++ keeps the
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
either. YAML missions name a terrain header instead (`common/terrain.rs`, book chapter "Terrain"), which
world plugins read and TerrainCollision uses. Like C++'s `altitude_origin`,
local `z = 0` is the terrain's origin altitude. There is nothing in C++ to
compare it with.

FixedWing6DOF's `landing_gear` and TerrainCollision's `structure_points_frd_m`
are Rust-only too: C++ ground contact was the flat spring at `z = 0`
(`use_ground_model`, Ripple's `enable_ground_spring`), which stays unchanged
for the comparison missions. The
gear follows JSBSim's FGLGear wheels (`landing_gear.rs`), so the comparison
does not cover it; its own tests do (`crates/core/tests/landing_gear.rs`).

FixedWing6DOF's `lift_table` and `drag_table` are Rust-only as well. C++ lift is
linear in angle of attack (`C_L0 + C_L_alpha * alpha`) at any angle, so it never
stalls, and its zero-lift drag is `C_D0 + C_D_alpha * |alpha|`. Tables of
`[alpha_rad, C_L]` and `[alpha_rad, C_D]` rows replace those terms, interpolated
linearly and held at their ends like a JSBSim `<table>`; induced and elevator
drag are unchanged. They are opt-in: no mission sets them, and the ArduPilot
book chapter lists JSBSim's Rascal values. Without tables the output is
unchanged: on 2026-10-05 `fixed-wing-6dof.yaml` and `fixed-wing-6dof-rascal.yaml`
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

## Random numbers: keyed draws on both sides

Ripple keys every random number by what it is for, never by draw order (see
[Randomness and seeds](../book/src/concepts/randomness.md)). SCRIMMAGE draws
everything from one shared `std::default_random_engine`, so a number depends on
every draw before it, in any plugin. Ripple used to have two generators of its
own: spawns drew from a copy of Apple libc++'s `minstd_rand` and its
uniform and normal conversions (so randomized spawns matched SCRIMMAGE on
macOS), and every plugin drew from its own SplitMix64 stream.

A stream breaks paired comparisons. In
`an_observers_error_on_a_target_does_not_depend_on_what_else_it_sees`
(`crates/core/tests/core_contracts.rs`), one observer measures two targets with
NoisyContacts, then the same mission runs with a third target spawned last.
On the streams (`cc6661a`), the observer's x error on target 2 at tick 1 was
-8.566 m in the first run and -2.563 m in the second: measuring the third
target shifted every later number. With keyed draws every error on the two
shared targets is identical, at every tick and axis.

`benchmarking-edits` copies the scheme behind `SCRIMMAGE_KEYED_RANDOM=1`
(`include/scrimmage/common/KeyedRandom.h`), which replaced
`SCRIMMAGE_LIBCXX_SPAWN_RANDOM` and its `minstd_rand` stream. Generation,
NoisyState, NoisyContacts, and SphereNetwork use it; the comparison run sets
it, so randomized spawns, noise, and loss are compared sample for sample.
Labels, not draw order, have to match: `Time` carries the step count, sensors
carry Ripple's identity (`NoisyState:0`, the name and its count among the
entity's sensors of that name), and `Network::step` names each message by its
topic and the sender entity's sequence on it. The labels and hashes are
integer arithmetic and agree exactly; the normal conversion's `log` and `cos`
can differ in the last bit between macOS and glibc, well inside the frame
tolerances. Without the option every C++ draw is unchanged. AuctionAssign's
bids still come from the shared engine in C++; bids are compared by delivery
only.

Keyed draws change two things about what is random, on both sides:

- **SphereNetwork loss is per receiving entity.** A message's draw is keyed by
  the sending and receiving entities, the topic, and the sequence, so all
  subscribers on one entity hear it or none do, as with one radio. SCRIMMAGE
  draws once per subscriber. Each subscriber still receives with probability
  `transmit_probability` (SCRIMMAGE's `prob_transmit`).
- **NoisyState's first reading has its own key.** It is taken when the entity
  is created, in the tick of the sensor phase's next reading; without a
  separate key both readings would carry the same errors.

The unmodified Linux build still differs on randomized spawns: libstdc++'s
default engine is `minstd_rand0` (multiplier 16807) and its polar method
returns the second variate first. The suite reports that comparison
(`unmodified-linux-randomized-spawning.json`) and expects it to fail. Do not
change seeds or tolerances to hide randomized mismatches.

Use the [Docker workflow](../reference/README.md) for fresh comparisons. Its
traces compare message deliveries, lifecycle and collision events, sensor
payloads, beliefs, and plugin commands against the `benchmarking-edits` branch;
other message contents, such as auction bids, are compared by delivery only.
