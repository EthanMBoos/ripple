# Working missions

Run from the repository root with `ripple run missions/<file> --headless`.
Replay the printed run directory with `ripple replay runs/<mission>/runNNN`.
`waypoints-point-agents.sweep.yaml` is a small example sweep (`ripple sweep`).
These [YAML missions](../book/src/guides/yaml-missions.md) use the compiled
built-ins. They are examples and regression inputs. Change a value for one run
with a dotted path, such as `entities.red.heading_deg:=170`.

| Mission | What to try or verify |
| --- | --- |
| `straight-no-gui.yaml` | Opposing aircraft, collision, removal, and team scores. |
| `fixed-wing-6dof.yaml` | Two aerodynamic aircraft: level eastbound and tilted northbound with wind. A reference fixture, not a trimmed flight demonstration. |
| `fixed-wing-6dof-rascal.yaml` | The Rascal RC plane (ArduPilot's JSBSim model), planned for ArduPlane: level flight, recovery from a 60° roll / 30° pitch upset, and a hands-off glide. No stall model; see `book/src/guides/ardupilot.md`. |
| `multirotor.yaml` | Open-loop quad hover, tilted flight, unequal motor speeds, and six-rotor climb. `MotorSpeeds` supplies constant rad/s commands; no autopilot or landing controller. |
| `multirotor-velocity.yaml` | Closed-loop flight: `MultirotorVelocityController` flies Crazyflie 2.0s and RotorS Fireflies through velocity steps, a 15° roll upset, and a 12 m/s cruise. `reference/multirotor_control.py` measures it; see the book's [Flying a multirotor](../book/src/guides/multirotor.md). |
| `networks-local-global.yaml` | NoisyState over LocalNetwork; Boundary over GlobalNetwork turns both aircraft; a third aircraft hits the ground. Set `entities.blue.autonomy.Straight.enable_boundary_control:=false` (and the same for `red`) or `interactions.GroundCollision.team:="'1'"` to see the difference. |
| `noisy-contacts.yaml` | Two route-following agents measure other contacts; typed LocalNetwork messages are available to downstream plugins. The route follower does not consume them. |
| `auction-sphere.yaml` | Stationary agents run a one-shot random-bid auction; the third is out of range. Try `networks.SphereNetwork.range_m:=300`, `networks.SphereNetwork.transmit_probability:=0.65`, or `run.seed:=42`. Results are typed messages, not stock summary/event fields. |
| `noisy-state.yaml` | Independent own-state noise and belief feedback. |
| `waypoints-aircraft.yaml` | Two aircraft follow a looping route; a shared GlobalNetwork replacement at 12 s changes both goals. Set `interactions.WaypointBroadcast.update_at_s:=100` to suppress that update. |
| `terrain-mcmillan.yaml` | Six aircraft fly waypoint routes over real terrain at McMillan Airfield (USGS 3DEP and NAIP imagery, in `terrain/`): three loop, climb, and descend safely; three fly into hills within the first minute. See the book's [Terrain](../book/src/guides/terrain.md) chapter. |
| `waypoints-point-agents.yaml` | Two SingleIntegrator agents receive a replacement at 3 s and stop at the final point. Set `interactions.WaypointBroadcast.update_at_s:=100` to retain the original route. |
| `verification/aircraft-long-horizon.yaml` | A longer aircraft run with a randomized start. |
| `verification/aircraft-substeps-spawning.yaml` | Actual substeps, update rates, scheduled spawning, and randomized state. |
| `verification/aircraft-substeps-scheduled.yaml` | Zero-variance twin with two spawn schedules. |
| `verification/noisy-state-bias.yaml` | Deterministic NoisyState biases, attitude, and closed-loop feedback. |
| `verification/single-integrator.yaml` | SingleIntegrator direct, capped, zero, and vertical commands from the `ConstantVelocity` test driver. |
| `verification/noisy-contacts-bias.yaml` | Deterministic NoisyContacts biases, attitude order, and covariance, compared through published payloads. |
| `verification/noisy-contacts-noise.yaml` | NoisyContacts with real noise, compared sample for sample and by distribution. |
| `verification/multirotor-fixes.yaml` | Three Multirotor contracts: a starting velocity that must move the vehicle, tilted and raised rotors (r x F), and motors held at idle. |
| `verification/collision-boundary.yaml` | SimpleCollision's strict range: pairs exactly at range survive, closing pairs collide (opposing and same team). |
| `verification/sphere-network-auction.yaml` | SphereNetwork reach (inside, exactly at, and beyond range) through AuctionAssign messages on a network named `CommsNetwork`. |
| `verification/sphere-network-loss.yaml` | SphereNetwork loss at `transmit_probability` 0.5, with keyed draws per message. |

The `verification/` missions are what `reference/missions.txt` and the other
checks run; [reference/README.md](../reference/README.md) describes each check.

WaypointFollower and WaypointBroadcast are small Rust replacements, not ports of
MotorSchemas or the legacy waypoint composition. Their routes are lists of
local ENU points in meters (`[[x, y, z], ...]`), not latitude/longitude. The
point example uses `max_speed_mps: -1` to follow commanded velocity; a nonnegative
SingleIntegrator `max_speed_mps` caps the speed. Fixed-wing aircraft cannot stop at
a point; their example repeats the route.

Malformed/missing-plugin cases live under `crates/*/tests/`, not this folder.

See the [paper's validation sections](../paper/README.md) for results,
reproduction, and comparison limits; [model
scope](../book/src/reference/models.md) explains the selection.
