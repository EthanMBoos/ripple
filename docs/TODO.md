# Roadmap

SCRIMMAGE-RS is at its initial release; the [paper](../paper/README.md) shows
how it agrees with C++ SCRIMMAGE, and [reference notes](REFERENCE_NOTES.md) list
the intentional differences. Keep the C++ comparison passing after any
simulation change.

## ArduPlane

Fly several ArduPlane SITL instances in lockstep with FixedWing6DOF over
ArduPilot's JSON backend. The design, history and reasons are in
[ARDUPILOT_SITL.md](ARDUPILOT_SITL.md); don't copy the C++ plugin's protocol.

`crates/ardupilot` (built with `--features ardupilot`) has the `SitlState`
sensor, the `ArduPilotBridge` autonomy, and `missions/rascal-sitl.xml` (the
Rascal with C++'s original radian surface limits) and `rascal-sitl-pair.xml`
(two of them). Its tests run the lockstep exchange against fake SITLs over UDP,
including two aircraft at 1 and 2 workers; it has not flown real ArduPlane yet.

- [ ] Starts, using FixedWing6DOF's existing launcher (see "Startup and
  release" in ARDUPILOT_SITL.md): flying immediately (no launcher), an air
  start released at a fixed time (launcher, cruise speed, `launch_speed` 0), and
  a catapult launch (launcher, start at rest, `launch_speed` about 20).
- [ ] Send `airspeed` in the reply, so wind can be nonzero.
- [ ] A launcher, reusing UVD's Docker setup (see ARDUPILOT_SITL.md). It reads
  the aircraft count and ports from the mission and starts the containers
  before the run, so ArduPilot aircraft always exist from t = 0; mid-run
  spawning is out of scope. Changes from UVD:
  - one container per aircraft with its own port (`9002+10*i`), `--sysid` and
    fresh run directory;
  - one shared `--home` meaning local (0, 0, 0) (the terrain origin when
    there is one), `--rate` = 1/dt and a large `--speedup`;
  - ArduPilot's `models/plane.parm` (copied from the image's pinned checkout)
    plus a small `rascal.parm` on top (`--defaults plane.parm,rascal.parm`):
    elevator and rudder reversal if done there rather than in the bridge (not
    both), `AIRSPEED_MIN`, and the `SIM_` sensor-noise settings. Sensor noise
    belongs to SITL; don't add noise on the SCRIMMAGE side;
  - normal arming instead of UVD's force-arm;
  - FBWA or CRUISE, reached before the release time;
  - stop every container when SCRIMMAGE exits, whatever the outcome: the last
    frame is never answered, so SITL never finishes on its own.
- [ ] Checks:
  - two aircraft stay on the same step all run;
  - control directions and specific force are right;
  - a pause longer than 1 s skips no frames;
  - killing one SITL fails the run clearly;
  - measure speed with two instances first. If round trips dominate, send all
    replies before collecting frames.

  First demo: two Rascals, zero wind, air start, moderate-angle flight around
  20 m/s (FixedWing6DOF has no stall). Taking off from terrain needs terrain
  ground contact (below).

## Terrain ground contact

- [ ] Add aircraft ground reactions for landing, rolling, and takeoff
  (needed for ArduPlane off a runway), using shared terrain geometry and
  off-grid behavior. Distinguish supported touchdown from crashes, and
  validate contact response and timestep convergence. Starting points are
  marked `TODO(terrain)` (`grep -rn "TODO(terrain)" crates`).
