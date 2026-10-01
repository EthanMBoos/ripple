# Roadmap

SCRIMMAGE-RS is at its initial release; the [paper](../paper/README.md) shows
how it agrees with C++ SCRIMMAGE, and [reference notes](REFERENCE_NOTES.md) list
the intentional differences. Keep the C++ comparison passing after any
simulation change.

## ArduPlane

Fly several ArduPlane SITL instances in lockstep with FixedWing6DOF over
ArduPilot's JSON backend. User guide:
[Flying ArduPlane](../book/src/guides/ardupilot.md).

`crates/ardupilot` has the `SitlState` sensor, the `ArduPilotBridge`
autonomy, one- and two-aircraft Rascal missions, the Docker image (`docker/`),
and `launch.py`, which starts one SITL container per aircraft, runs the
mission, and stops them. Aircraft are held at rest and catapulted at release.
One and two Rascals have flown LOITER, a looping AUTO waypoint route
(`launch.py --waypoints`), a GUIDED target (`--goto`), and a ground catapult
takeoff with real ArduPlane, with wind, and passed the pause and kill checks.
Per-aircraft routes and changing GUIDED
targets during a run are not done; add them when an experiment needs them.

## Terrain ground contact

- [ ] Add aircraft ground reactions for landing, rolling, and takeoff
  (needed for ArduPlane off a runway), using shared terrain geometry and
  off-grid behavior. Distinguish supported touchdown from crashes, and
  validate contact response and timestep convergence. Then fill in the
  ArduPlane guide's planned takeoff-and-landing section. Starting points are
  marked `TODO(terrain)` (`grep -rn "TODO(terrain)" crates`).
