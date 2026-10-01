# C++ reference findings

Source review: sibling `../scrimmage`, branch `Ubuntu-24.04`, commit
`4bdf41fb06facaea358d478d9a07aa4d77fcac27` (2026-09-23/24). This identifies the
reviewed source, not a required pin. Follow the branch for new comparisons.
The [roadmap](TODO.md) defines selected scope; the [book](../book/src/SUMMARY.md)
describes current Rust APIs. Comparison results are in the [paper](../paper/README.md).

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
  fixture unchanged; `verification/aircraft-substeps-spawning.xml` tests real substeps.
- `SCRIMMAGE_PLUGIN_PATH` discovers optional XML defaults overlays, not code.
  YAML configuration does not depend on these overlays.
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
  network phase, so metrics credit survivors only until the latest removal.
  Rust credits them until report time. In `networks-local-global.xml`, surviving
  teams report 29.9 s instead of 0.4 s. Frames are unaffected; flight-time summary
  columns differ. See the regression in
  [SimpleCollisionMetrics](../crates/core/src/plugin/metrics/simple_collision_metrics/simple_collision_metrics.rs).
- **Teams that first spawn after t=0:** C++ SimpleCollisionMetrics lists teams
  at its first step, then totals later teams with no normalization time: their
  `flight_time_norm` is infinite and `score` NaN. Rust reports every team with a
  generated entity using the same normalization as other teams; `flight_time`
  and collision counts match. Seen in `verification/aircraft-substeps-scheduled.xml`.
- **FixedWing6DOF inertia:** C++'s bundled slug-unit default silently overrides a
  supplied SI matrix. Rust accepts either `inertia_matrix_slug_ft_sq` or
  `inertia_matrix`, rejects both together, and uses the slug default if neither
  is supplied.
- **FixedWing6DOF elevator drag key:** C++ reads `C_D_delta_elevator_` (with a
  trailing underscore), but every bundled parameter file writes
  `C_D_delta_elevator`, so C++ always uses the 0.01 default. That is harmless for
  the A-4D default (its file also says 0.01), but the Rascal file's 0.03 is
  never used. Rust reads the same underscore key and rejects the plain name, so
  a value is never silently ignored. `missions/fixed-wing-6dof-rascal.xml`
  leaves the key out and flies with 0.01, like C++.
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
  (`paper/data/aircraft-pid-upset/`): with no substeps (C++ and Rust
  identical) the altitude wandered 285 m and the heading 42 degrees; with 5
  substeps, Rust's timing gave 66 m, and C++'s, whose derivative is 5 times
  weaker, peaked at 1.4 m. With no substeps and the altitude D lowered to
  0.002 (or 0), altitude peaked at 1.4 m (1.5 m) and stayed within 0.6 m
  (0.8 m) after the first 5 s; lowering the pitch loop's D instead did not
  help. Measured with the Rust port on this one scenario; missions that fly
  AircraftPIDController differ from unmodified C++, so the checker only
  records their baseline equality.
- **Stochastic sensors/communication:** Rust uses per-plugin, mission-seeded
  streams rather than C++'s shared generator. Measured contacts use sorted typed
  vectors. SphereNetwork queries current post-motion positions rather than the
  earlier C++ R-tree. AuctionAssign has configurable network/deadline and an
  optional winner instead of a negative sentinel. These are not sample-for-sample
  C++ RNG ports.

Source details worth preserving: NoisyContacts copies angular velocity and uses
5I covariance; NoisyState uses zero angular velocity and identity covariance.
Both right-multiply attitude errors. SphereNetwork has strict range, inclusive
altitude-plane bounds, and same-parent geometry bypass that still draws for loss.
AuctionAssign includes self-bidding, keeps the first maximum on ties, and closes
strictly after its deadline. See [plugin behavior](../book/src/reference/plugin-api.md#noisycontacts).

## Findings from the comparison trace

- C++ Straight subscribes to `ContactsWithCovariances` and stores the map without
  reading it. Rust Straight subscribes and discards the messages so deliveries
  match; behavior is unaffected.
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
disables GPU acceleration in CMake (which also disables the GPU network
plugin). Rust interleaves too, refreshing `PassThrough`-of-perfect own-state
belief between substeps; other estimators, sensors, contacts, interactions, and
networks keep the outer tick, so more substeps do not mean fresher observations
or finer collision checks. `m` = 1 missions are unaffected. The benchmarks
(`reference/benchmark.py`, `paper/data/benchmark-native/`) now build the
comparison branch, and the paper's timings come from them. The history, the comparison with other simulators, and the
verification are in the paper (`sec:substeps`); `reference_check.py` requires
unmodified and fixed C++ to match only when `m` = 1 and no entity uses
AircraftPIDController.

## Ported models keep C++ numerics; new models do not

Ported plugins reproduce the C++ implementation, including its numerical
shortcuts, so comparisons stay exact. New plugins, such as new fixed-wing and
boat models, should be written to be correct, not
to match these.

**Frozen RK4 loads (FixedWing6DOF, Multirotor).** C++ `model(x, ...)` computes
forces and moments from `x_` (start of step), not the stage state `x`; only
kinematics and attitude use `x`. The loads are therefore first order per step.
These ported models keep this approximation for C++ compatibility, and the
substep-order fix does not remove it. The implementation is also rough: the
frozen loads are recomputed at every stage, and the gyroscopic term uses `x_`
while the kinematics use `x`. [`paper/data/rk4-loads`](../paper/data/rk4-loads/README.md)
measures it against loads recomputed at each stage. With constant controls,
the frozen error halves exactly with each halving of the step (first order),
while stage-evaluated loads reach the reference's resolution within one or two
halvings: 6.1 cm against 5e-10 m for Multirotor over 4 s at 0.01 s, and 17 m
against 1.5e-5 m for a FixedWing6DOF glider over 60 s at 0.05 s. In a gently
controlled 30 s flight, the largest difference between frozen and
stage-evaluated loads is 2.0 m at 0.01 s and 99 m at 0.05 s, and the frozen
version flips the aircraft at 0.1 s. A
suitable step size depends on the model, controller, and scenario; check
convergence by halving the step rather than relying on one fixed value.

Computing forces once per step is not wrong in itself. JSBSim does it at
120 Hz, with rectangular Euler for rotation and Adams-Bashforth methods, which
reuse past derivatives, for translation by default. What is careless here is the
mismatch: the models pay for four RK4 stages but get first-order accuracy in
the forces, recompute identical forces at each stage, mix start-of-step and
stage state, and never say so.

**Integrators for new motion models.** For smooth-flight models such as the
planned fixed-wing and boat, start with RK4 that recomputes forces and moments
from each stage's state, and verify convergence. Choose another integrator
when the dynamics warrant it: stiff or strongly damped terms and
discontinuities (contact, saturation) change the trade-off, and methods that
evaluate forces once per step can be the better choice there. Document the
choice, and do not freeze forces through RK4 stages. Whichever method you choose, check
convergence by running a representative scenario at `dt`, `dt/2`, and `dt/4`,
and confirm the trajectory stops changing at the step you intend to use.

**Evidence still needed for these claims.** Each claim above needs its own
side-by-side check, and the results belong in `paper/data/`:
- *Integration accuracy*: the same model run with frozen and with
  stage-evaluated forces at several step sizes, against a fine-step reference.
  Done in `paper/data/rk4-loads`: frozen loads converge at first order, and
  stage-evaluated loads reach the reference's resolution within one or two
  halvings. That shows a far smaller error, not a measured fourth-order rate.
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

## Spawn RNG mismatch (accepted)

The native macOS baseline matched; the Ubuntu Docker fixture diverges at
initialization. The inspected libstdc++ uses `minstd_rand0` (multiplier 16807)
and returns the second polar normal variate first. Rust's
[legacy RNG](../crates/core/src/common/random.rs) uses multiplier 48271 and returns
the first variate first, matching Apple libc++.

The recorded randomized-spawn comparison had 90/90 mismatched states and about
8.41934 m maximum position error. Rust outputs still matched across 1/2/8 workers
and recording on/off. This is an accepted difference of the unmodified Linux
build: C++ itself differs between libc++ and libstdc++. Comparisons remove it
with the instrumentation branch's opt-in libc++-compatible spawn stream
(`SCRIMMAGE_LIBCXX_SPAWN_RANDOM=1`), and keep a zero-variance twin that needs no
option. Do not change seeds or tolerances to hide randomized mismatches.

Use the [Docker workflow](../reference/README.md) for fresh comparisons. Its
traces compare message deliveries, lifecycle and collision events, sensor
payloads, beliefs, and plugin commands against the `benchmarking-edits` branch;
other message contents, such as auction bids, are compared by delivery only.
