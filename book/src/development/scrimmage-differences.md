# Comparing with SCRIMMAGE

Ripple began as a port of SCRIMMAGE. Running the same mission in both
simulators checks more than isolated equations: a wrong sign, default, phase
order, or message delivery changes the whole run. The comparison compares
states, events, messages, beliefs, commands, and summaries.

A difference is not automatically a porting error, because Ripple changes
parts of SCRIMMAGE on purpose. This chapter lists those decisions, the
defects corrected in both implementations, and the inherited behavior kept
as it is, so that a mismatch can be read against them. The source audit used
SCRIMMAGE's `Ubuntu-24.04` branch at `4bdf41fb`; the checker records the
commit it ran. Normal Ripple use needs no SCRIMMAGE checkout. The optional
comparison defaults to a sibling `../scrimmage` checkout and takes `--source
PATH` for one elsewhere. The [reference
guide](../../../reference/README.md)
explains the commands and pass criteria, and the retained results are under
[paper/data](../../../paper/data).

## What the comparison runs

Ripple reads a YAML mission into `ScenarioConfig` and records the resolved
scenario. `reference/scrimmage_xml.py` writes an XML mission from that record
for SCRIMMAGE. A compared mission uses the default estimators and no terrain,
and spells out every plugin parameter whose default differs between the two.
SCRIMMAGE's XML receives the plugin values the mission supplies, apart from
named airframe values that SCRIMMAGE lacks. The writer owns the parameter
names, units, and XML defaults.

The checker builds two committed SCRIMMAGE branches:

- `Ubuntu-24.04` is the upstream reference. Its output is retained to show
  the effect of the comparison branch. It is not expected to match Ripple
  where a documented correction changes behavior.
- `benchmarking-edits` adds opt-in tracing and keyed random draws and fixes
  the defects listed below. With those options off, its outputs are
  unchanged. The checker compares Ripple with this branch. Only opt-in,
  output-preserving instrumentation and the documented fixes may go on it,
  and the `Ubuntu-24.04` branch is never modified.

The branch disables GPU acceleration for the phase-order correction. The
comparison is headless and covers the plugins in the mission matrix, not
SCRIMMAGE's whole plugin catalog. `reference/suite.py` runs the checks and
`reference/reference_check.py` builds and compares the missions. The suite
also tests worker-count and tracing invariance, generated and long missions,
and whether planted defects are caught.

Do not change a seed, branch, tolerance, or metric rule to make a result
pass. A remaining mismatch is reported with its inputs and trace. When a
fix is a genuine correction, apply it to `benchmarking-edits` too, prove it
with a failing example, a focused test, and new comparisons, and add it to
the corrections below.

## Deliberate differences

**Belief and sensors.** SCRIMMAGE lets plugins set belief and otherwise
falls back to truth, and its autonomy can read true contacts. Ripple gives
each entity a localization and a tracking estimator. Sensors produce
readings, the estimators update belief after delivery, and autonomy and
controllers read only that belief. With no sensor, a perfect reading through
`PassThrough` gives the usual perfect-information behavior. An entity that
spawns becomes known to others after their next measurement. The C++
`Straight` steers from a sensor message; Ripple's steers from belief. See
[Belief vs truth](../concepts/belief-vs-truth.md).

**Autonomy composition.** SCRIMMAGE switches and blends autonomy plugins
through `GraphvizFSM`, `AutonomyExecutor`, and `MotorSchemas`, and can load
behaviors during a run. Ripple keeps a vehicle's phases and callable skills
inside one autonomy plugin; the delivery `Courier` is an example. Several
autonomy plugins can still run on an entity, in mission order, but the last
one declaring a command port sets that port. There is no `is_controlling`
flag and no runtime plugin loading.

**Port values and the pass-through controller.** SCRIMMAGE's VariableIO
gives each plugin its own input and output buffers and connects an autonomy
to the motion model only through a controller, so a velocity autonomy
driving SingleIntegrator needs `SingleIntegratorControllerSimple`, which
copies each command through unchanged. Ripple keeps one map of port values
per entity, requires one writing plugin per port name, and connects the
motion model to the autonomies directly when a mission lists no controller;
it has no pass-through plugin. `reference/scrimmage_xml.py` adds that
controller on the C++ side, and `reference/traces.py` skips its output
records in the C++ trace, reporting the count; the values it copied are
compared as the autonomy's outputs. Decided 2026-11-14 with the port-value
redesign, whose `DEVNOTE` is in `crates/core/src/ports.rs`.

**Mission and output formats.** Ripple reads YAML and writes MCAP;
SCRIMMAGE reads XML and writes `frames.bin` and `summary.csv`. Ripple keeps
each metrics plugin's score at full precision; SCRIMMAGE sums plugin scores,
prints six decimals, and writes zero for missing columns. The comparator
reconstructs the C++ summary rule rather than comparing unlike formats as
text.

**Two scoring rules.** SCRIMMAGE's `SimpleCollisionMetrics` credits a
survivor only through the latest removal, because its end-of-run presence
message arrives after the final network phase. It also fixes the set of
teams at its first step, so a team that spawns later has no scoring window.
Ripple credits survivors through report time and scores every team that
generated an entity. `reference/metrics_model.py` applies both rules to the
same trace, and the checker accepts a summary difference only when that
model explains it and the entity and collision counts match.

**Vehicles and controls.** SCRIMMAGE's FixedWing6DOF defaults to an A-4D.
Ripple requires a named airframe or explicit parameters. The shared
fixed-wing equations are compared with both A-4D and Rascal parameters, and
the Rascal's additional JSBSim terms have their own check. Ripple's
SimpleAircraft takes physical speed, turn, climb, and bank limits, and
`SingleIntegrator` treats `max_speed` as a cap. The comparison branch has
the matching SimpleAircraft turn and SingleIntegrator corrections. The
controllers, vehicle presets, and optional effects are in [Built-in
models](../reference/models.md).

**Network timing.** Ripple delivers a delayed message at the first tick at
or after its due time, even if the publisher goes quiet. SCRIMMAGE releases
its delay queue only while processing new publications on the topic, so no
compared mission uses delay. Ripple's `SphereNetwork` reads positions after
motion; SCRIMMAGE builds its R-tree earlier, so a moving pair near range can
disagree. The comparison records that gap rather than changing the route or
the tolerance to hide it.

**Terrain and estimation.** Ripple's terrain, landing gear, estimators, and
RotorS-style multirotor effects have no counterpart in the C++ mission
matrix. They have their own tests and reference checks where one exists.
The C++ `SimpleINS` adds synthetic drift inside a sensor, so it is not an
equivalent baseline for a predictor that uses commands and readings.

## Corrections made in both implementations

These changes are in Ripple and on `benchmarking-edits`. They correct the
compared behavior; they are not evidence that upstream SCRIMMAGE produces
the same output. Focused tests and the C++ agreement matrix are run by the
suite.

| Area | Upstream behavior | Correction |
| --- | --- | --- |
| Time and startup | Frames were labeled one tick ahead and the terminal time repeated; some spawn and rate schedules missed their due tick. | Time follows the step count; labels are unique; generation and rate-limited plugins run on the intended tick. The first entities still start one step before `start_s` for comparison. |
| NoisyState | The initial reading skipped its noise, and a later reading could report zero angular velocity while the vehicle turned. | Every reading uses the measured state and reports the true rate when rate noise is zero. Sensors keep their noise model; autonomy still receives belief. |
| SimpleAircraft | Reported velocity and angular rate could disagree with motion; externally changed state was overwritten; the bank and turn signs were inconsistent. | Reported rates follow motion, state changes persist, and a positive bank turns right. The limits apply within integration stages. |
| SingleIntegrator | `max_speed` replaced the magnitude of every nonzero command. | It caps commands above the limit and leaves slower commands alone. |
| FixedWing6DOF | Wind could have the wrong physical sign, and wind-axis forces omitted sideslip and angle-of-attack cross terms during rotation. | Airspeed is velocity minus wind; forces use the full wind-to-body rotation. `reference/jsbsim_check.py` checks the Rascal's loads separately. |
| AircraftPIDController | With motion substeps it used the outer tick as its timestep, changing its integral and derivative gains. | It uses the elapsed substep time. Both implementations also lower the default A-4D altitude derivative gain from 0.01 to 0.002; that is a tuning choice, checked by `reference/rascal_control.py` and `reference/rk4_loads.py`. |
| FixedWing6DOF and Multirotor | RK4 held most loads at the start-of-step state, and their quaternions carried a nonphysical length-correction term. | Every RK4 stage computes loads from its own state, and the quaternion is normalized at the rotation and truth boundaries. |
| Multirotor | Private body velocity could disagree with reported world velocity; rotor tilt and moment arms were wrong; idle thrust vanished while yaw torque remained. | Truth supplies body velocity and rates; each rotor pushes along its tilted axis with moment `r × F`; thrust and torque both follow rotor speed at idle. |

### Substep order

For several years SCRIMMAGE interleaved controllers and motion within each
substep. A later GPU change ran all controller substeps before all motion
substeps, so a PID read no new state after its first call within a tick.
Ripple and `benchmarking-edits` restore controllers, then motion, for every
substep. Only the own-state perfect reading refreshes between substeps;
sensors, other contacts, interactions, and networks keep the outer tick. A
larger motion multiplier therefore gives finer dynamics and control, not
finer collision checks or new sensor observations.

### RK4 loads

SCRIMMAGE passed an RK4 stage state `x` into FixedWing6DOF and Multirotor,
but most forces and moments read the start-of-step member `x_`, so the four
stages held those loads fixed. Ripple and `benchmarking-edits` compute every
load from `x`. Their trajectories differ from upstream SCRIMMAGE on purpose.
`reference/rk4_loads.py` checks the effect by comparing the held-load
upstream model with stage-evaluated loads at smaller steps.

Computing forces once per step can be a valid choice with a suitable
integrator. The defect was using four RK4 stages while leaving their forces
at the first stage. The model, controller, and mission still set the step
size a run needs.

### Random numbers

SCRIMMAGE normally draws from one shared stream, so adding one sensed
contact changes later noise on unrelated contacts. Ripple keys a random
number by seed, entity, plugin, tick, and what is being measured. For the
comparison, `SCRIMMAGE_KEYED_RANDOM=1` opts the C++ branch into the same
scheme for generation, NoisyState, NoisyContacts, and SphereNetwork; with
the option off, its draws are unchanged. The same contact on the same tick
then gets the same noise in both runs even when other contacts differ. See
[Randomness and seeds](../concepts/randomness.md).

Keyed loss is per sending and receiving entity, topic, and sequence, so all
subscribers on one receiving entity hear a given message or none do;
upstream SCRIMMAGE draws separately per subscriber. AuctionAssign's private
bid draws still use the C++ stream, so its bids are compared through
deliveries rather than sample for sample. Normal-number conversion can
differ in the last bit between platforms.

## Behavior kept for comparison

Some simplified behavior is left as SCRIMMAGE has it. It belongs to the
model being compared and can affect a mission:

| Behavior | Consequence |
| --- | --- |
| `NoisyContacts` measures every entity without a range or field of view. | A research sensor should model visibility explicitly; the stock one is an ideal contact sensor with configurable noise. |
| `Boundary` publishes its region once. | An entity spawned later needs another source of the boundary to learn it. |
| `AuctionAssign` runs on entity 1, includes its own bid, keeps the first maximum on ties, and closes strictly after its deadline. | It is a demo dispatcher with these deterministic choices. |
| `SimpleCollision` uses strict distance below `collision_range`, independent of visual radius. | Equality at the boundary does not collide; startup rejection is separate. |
| `LocalNetwork` requires the same parent entity. | Its name means entity-local delivery, not spatial proximity. |

SCRIMMAGE's sensor messages carry a placeholder covariance, an identity or a
constant, rather than that of the noise. Ripple's sensors report the
covariance of the noise they add, and its beliefs and tracks carry one
([Uncertainty](../concepts/uncertainty.md)). The comparison compares the
means only, since SCRIMMAGE has nothing to compare a covariance with. Other
ported numerical conventions stay in the model code and its focused tests.

## What agreement does and does not establish

The comparison tests the shared execution path and equations on matched
missions. It does not validate a vehicle against flight data, cover
SCRIMMAGE's unported plugins, or test every Rust-only component. [How
models are checked](../reference/checks.md) names the independent
implementations and measurements used for those.
