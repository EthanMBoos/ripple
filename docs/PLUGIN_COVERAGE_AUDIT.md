# C++ model findings for future work

Source audit, 2026-09-24: C++ `Ubuntu-24.04` at
`4bdf41fb06facaea358d478d9a07aa4d77fcac27`. The selected NoisyContacts,
SphereNetwork, and AuctionAssign additions are complete; their behavior is in
the [plugin reference](../book/src/reference/plugin-api.md).

The remaining findings below are candidates to revisit when an experiment needs
them. Current model availability is in the [model reference](../book/src/reference/models.md);
selected implementation work is in [TODO.md](TODO.md).

## Missing shared capabilities

| Need | Current boundary | C++ source to revisit |
| --- | --- | --- |
| Physical loads and reset | `MotionContext` has mutable kinematics but no force/moment input or reset hook. Changing public velocity need not reset a model's private state. Stock Multirotor assumes no external contact force. | [ExternalForceField](../../scrimmage/src/plugins/interaction/ExternalForceField/ExternalForceField.cpp), [MotionModel](../../scrimmage/include/scrimmage/motion/MotionModel.h) |
| Current-tick dynamics for sensors | `KinematicState` has pose and velocities, not acceleration/wind. Motion messages arrive through networks after the sensor phase. Finite differences are not necessarily the model's physical sample. | [RigidBody6DOFStateSensor](../../scrimmage/src/plugins/sensor/RigidBody6DOFStateSensor/RigidBody6DOFStateSensor.cpp) |
| Camera target geometry | NoisyContacts works, but snapshots have no radius/geometry. Mounting pose can be sensor config; arbitrary target sizes require shared data or a model-specific map. | [ContactBlobCamera](../../scrimmage/src/plugins/sensor/ContactBlobCamera/ContactBlobCamera.cpp) |
| Occlusion/ray queries | No shared scene/query API. C++ RayTrace defines rays; BulletCollision performs queries and publishes point clouds. A sensor with private geometry is a narrower option. | [RayTrace](../../scrimmage/src/plugins/sensor/RayTrace/RayTrace.cpp), [BulletCollision](../../scrimmage/src/plugins/interaction/BulletCollision/BulletCollision.cpp), [LOSSensor](../../scrimmage/src/plugins/sensor/LOSSensor/LOSSensor.cpp) |
| Retiring one agent | `Update::Stop` ends the simulation. An autonomy/controller can send a request to an interaction that changes health, but that has message latency. | [MotionBattery](../../scrimmage/src/plugins/controller/MotionBattery/MotionBattery.cpp), [TrajectoryRecordPlayback](../../scrimmage/src/plugins/autonomy/TrajectoryRecordPlayback/TrajectoryRecordPlayback.cpp) |
| Custom records | `EventKind` is closed, metrics report by team, and contexts have no common output sink. Custom messages do not automatically appear in stock logs. | [CPA](../../scrimmage/src/plugins/metrics/CPA/CPA.cpp), [OpenAIRewards](../../scrimmage/src/plugins/metrics/OpenAIRewards/OpenAIRewards.cpp) |
| New physical ports/geography | Port names are extensible; `Unit` lacks force, torque, and angular acceleration. Contexts also lack a shared geographic origin/projection. | Rust [units](../crates/core/src/common/variable_io.rs), C++ [GPS](../../scrimmage/src/plugins/sensor/GPS/GPS.cpp) |

For a physical-load consumer, specify whether loads accumulate or replace,
how long they persist across substeps, and how reset updates private state.
For a dynamics sample, specify time/frame and distinguish inertial acceleration
from specific force. Implement the smallest common data needed by that consumer.

Truth isolation remains a modeling choice: `AgentContext` exposes
`contacts_truth` alongside sensor observations. Own-state belief is not a
per-agent estimated world model.

## Work that can stay inside plugins

- Neighborhood algorithms such as Boids can scan snapshots initially; a spatial
  index is an optimization. Autonomy/controller/sensor/network contexts already
  provide per-plugin random streams.
- TerrainGenerator/AltitudeAboveTerrain suggest a typed immutable height-map
  message. Late-spawned subscribers need an explicit delivery strategy because
  stock messages are not latched. Physical terrain is separate from viewer tiles.
- Capture/scoring rules can use interactions, typed events, and team metrics.
  Per-entity histories and custom diagnostic output still need a chosen sink.
- An explicit-port energy limiter fits a controller chain;
  [UUV6DOFLinearEnergy](../../scrimmage/src/plugins/controller/UUV6DOFLinearEnergy/UUV6DOFLinearEnergy.cpp)
  is a small reference. A full battery adds charge reporting and entity retirement.
- Rust's fixed plugin stack merges autonomy outputs in source order; it is not
  C++ MotorSchemas/AutonomyExecutor child-plugin activation. Use
  [ordinary components and state machines](../book/src/guides/autonomy-composition.md)
  for growing behaviors.

External readiness/creation belongs to the integration plan. No concrete
`posthumous` override was found in the audited C++ plugin tree. OpenAIRewards
provides reward output, not a complete episode/reset/action interface.

## Source quality cautions

Check these before choosing a new port; they are observations of the reviewed
source, not instructions to change the C++ reference.

- **MultirotorControllerPID:** writes equal hover motor speeds; attitude/yaw
  corrections are commented out. It does not establish closed-loop stabilization.
- **RigidBody6DOF:** disables gravity and some translational derivatives, and
  assigns P/Q/R by clamping U.
- **Ballistic:** puts `-mass * g` in the derivative of vertical acceleration,
  rather than implementing ordinary constant-gravity projectile motion.
- **SimpleCar:** uses `turn_rate` as a steering angle inside `tan`; its gravity
  branch multiplies acceleration by mass.
- **UUV6DOF:** transforms/clears external force into `force_ext_body_`, but the
  reviewed equations do not use it.
- **SimpleINS / LOSSensor:** SimpleINS divides velocity differences by `.01`
  and uses `rand()` for timing; LOSSensor seeds with wall time. Multi-rate and
  deterministic behavior need explicit treatment.

The audit inspected source and public interfaces, not numerical correctness of
all C++ models. Selected behavior and comparison evidence remain in [EVIDENCE.md](EVIDENCE.md).
