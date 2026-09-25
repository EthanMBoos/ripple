# C++ reference findings

Source review: sibling `../scrimmage`, branch `Ubuntu-24.04`, commit
`4bdf41fb06facaea358d478d9a07aa4d77fcac27` (2026-09-23/24). This identifies the
reviewed source, not a required pin. Follow the branch for new comparisons.
The [roadmap](TODO.md) defines selected scope; the [book](../book/src/SUMMARY.md)
describes current Rust APIs. Comparison results are in [EVIDENCE.md](EVIDENCE.md).

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
- **FixedWing6DOF inertia:** C++'s bundled slug-unit default silently overrides a
  supplied SI matrix. Rust accepts either `inertia_matrix_slug_ft_sq` or
  `inertia_matrix`, rejects both together, and uses the slug default if neither
  is supplied.
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

## Unresolved spawn RNG mismatch

The native macOS baseline matched; the Ubuntu Docker fixture diverges at
initialization. The inspected libstdc++ uses `minstd_rand0` (multiplier 16807)
and returns the second polar normal variate first. Rust's
[legacy RNG](../crates/core/src/common/random.rs) uses multiplier 48271 and returns
the first variate first, matching Apple libc++.

The recorded randomized-spawn comparison had 90/90 mismatched states and about
8.41934 m maximum position error. Rust outputs still matched across 1/2/8 workers
and recording on/off. Fix engine/distribution compatibility while preserving
coordinator draw order; changing seeds or tolerances would hide the problem.

Use the [Docker workflow](../reference/README.md) for fresh comparisons. Direct
C++ event-stream comparison remains absent; historical native passes and recorded
frames alone do not establish it.
