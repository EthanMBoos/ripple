# Roadmap

SCRIMMAGE-RS is at its initial release; the [paper](../paper/README.md) shows
how it agrees with C++ SCRIMMAGE, and [reference notes](REFERENCE_NOTES.md) list
the intentional differences. Keep the C++ comparison passing after any
simulation change.

## ArduPlane

- [ ] Fly ArduPlane SITL with FixedWing6DOF, as C++ SCRIMMAGE's ArduPilot
  autonomy plugin does (state to SITL over UDP, servo PWM back). Undecided:
  whether its Rascal parameters fly well enough, and what state SITL needs.
  Taking off from terrain needs terrain ground contact (below).

## Terrain ground contact

- [ ] Add aircraft ground reactions for landing, rolling, and takeoff
  (needed for ArduPlane off a runway), using shared terrain geometry and
  off-grid behavior. Distinguish supported touchdown from crashes, and
  validate contact response and timestep convergence. Starting points are
  marked `TODO(terrain)` (`grep -rn "TODO(terrain)" crates`).
