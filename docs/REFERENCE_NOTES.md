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
| `src/simcontrol/SimControl.cpp`, `run_single_step` / `run_entities` | Generate and log pre-step contacts; autonomy; all controller substeps; all motion substeps; sensors; interactions; networks; metrics; removal/output/time. Controller and motion substeps are not interleaved. |
| Same file, network/metrics phases | Metrics can consume interaction messages in the same tick. Delivery is not universally delayed to the next tick. |
| Same file, `run_logging` / finalization | Pre-step state is labeled `t + dt`; terminal logging repeats the final timestamp with terminal state. |
| `src/plugins/network/LocalNetwork/LocalNetwork.cpp` | Reachability requires the same non-null parent entity, not proximity. |
| `src/plugins/interaction/SimpleCollision/SimpleCollision.cpp` | Collision uses strict distance against `collision_range`, not summed entity/visual radii. Startup rejection is separate. |
| `src/entity/Entity.cpp` | Truth and belief initially alias; `set_state_belief` detaches belief. Rust uses owned belief without mutable aliasing, but legacy truth contacts remain accessible. |

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
- **AircraftPIDController with substeps:** C++ uses the full step as the
  controller timestep during substeps; Rust uses the substep. The compared
  FixedWing6DOF scenarios run without substeps.
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
