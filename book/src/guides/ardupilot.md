# Flying ArduPlane (SITL)

The optional `ardupilot` crate lets real ArduPlane firmware fly FixedWing6DOF
aircraft in a SCRIMMAGE mission. Each aircraft gets its own ArduPilot SITL
(software in the loop) running in a Docker container. You can fly one aircraft
or many, in LOITER, AUTO, GUIDED, or TAKEOFF, with wind.

## How to think about it

Two programs share each aircraft:

| | Owns |
| --- | --- |
| **SCRIMMAGE** | The world: time, physics (FixedWing6DOF), other entities, interactions, metrics, Rerun. Also sensing that ArduPilot doesn't do, such as other aircraft (`NoisyContacts`). |
| **ArduPlane** | The aircraft's flight controller. Its sensors (IMU, GPS, barometer, compass, airspeed) are simulated from the true state it is sent; its EKF estimates the state; its control loops and flight modes fly. |

Each SCRIMMAGE tick:

1. The aircraft's true state goes to its SITL.
2. ArduPlane runs one step and answers with servo outputs (PWM).
3. SCRIMMAGE applies them to the control surfaces and throttle.

This is **lockstep**. SCRIMMAGE advances only when every aircraft's SITL has
answered, so all aircraft share one clock, and ArduPlane's clock follows
SCRIMMAGE's. Three things follow from that:

- **A slow SITL slows the whole run.** A dead one fails it after 10 s with "no
  new frame from ArduPilot SITL".
- **Repeatability depends on ArduPilot too,** so it is not guaranteed as it is
  for other SCRIMMAGE runs. In practice the lockstep exchange makes runs
  repeat: three runs of `rascal-runway.yaml` on one machine (2026-10-05) wrote
  byte-identical frames, events, and summaries. Check before relying on it
  across machines or ArduPilot versions.
- **Sensor noise belongs to ArduPilot.** SITL is sent the true state on
  purpose. Don't put `NoisyState` on an ArduPilot aircraft; set ArduPilot's
  `SIM_` sensor-noise parameters instead (see [Parameters](#parameters)).

## Quick start

You need Docker running. Build `ripple` with the ArduPilot feature; the
first run builds the SITL image, which compiles ArduPlane. That takes about 25
minutes, and later runs reuse it. From the repository root:

```sh
cargo build --release -p ripple --features ardupilot
S=target/release/ripple
M=crates/ardupilot/missions
B=autonomy.ArduPilotBridge   # shortens the override paths below

# One aircraft circling (LOITER)
$S run $M/rascal-sitl.yaml --headless run.end_s:=150

# Two aircraft: one flying a looping route (AUTO), one circling
$S run $M/rascal-sitl-pair.yaml --headless --workers 2 run.end_s:=240 \
    "entities.rascal_a.$B.waypoints:=[[600,0,0], [600,200,50], [0,200,50], [0,0,0]]"

# Fly to a point and circle it (GUIDED)
$S run $M/rascal-sitl.yaml --headless run.end_s:=180 "entities.rascal.$B.goto:=[800,400,50]"

# Catapult takeoff from the ground, climbing to 50 m (TAKEOFF)
$S run $M/rascal-sitl.yaml --headless run.end_s:=120 \
    "entities.rascal.position_m:=[0,0,0]" entities.rascal.$B.mode:=TAKEOFF

# Wind: 6 m/s from the north side (see "Wind" below for the sign); the sensor
# and the motion model both take it
$S run $M/rascal-sitl.yaml --headless run.end_s:=150 \
    entities.rascal.sensors.SitlState.wind_N:=6 entities.rascal.motion_model.FixedWing6DOF.wind_N:=6
```

`ripple run` starts one SITL container per `ArduPilotBridge` aircraft once the
mission has loaded, and stops them all when the run ends, fails, or is
interrupted with Ctrl-C
([sitl_launcher.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/ardupilot/src/sitl_launcher.rs)).
It reads the resolved mission, so templates and overrides apply, and the
containers always match it. `ripple sweep` starts no SITL. Each run
writes `runs/<mission>/runNNN/` as usual, plus `sitl/<port>/` inside it: one
folder per SITL, holding

- `session.log`: ArduPilot's messages, arming, and errors;
- `logs/*.BIN`: ArduPilot's own flight log;
- its parameter storage.

Give multi-aircraft runs one worker per aircraft (`--workers 2`), so their SITL
waits overlap; with one worker they take turns and the run is slower.

The SITL session settings are bridge parameters:

| Key | Default | Meaning |
| --- | --- | --- |
| `mode` | AUTO with `waypoints`, GUIDED with `goto`, else LOITER | ArduPlane mode to arm in |
| `waypoints` | none | AUTO route `[[east, north, up], ...]`; it repeats |
| `goto` | none | GUIDED target `[east, north, up]`, sent once armed |
| `takeoff_altitude` | none | Start the AUTO route with a takeoff to this height, m |
| `land` | false | Make the last waypoint a landing; the route does not repeat. Its `up` is the ground's height there relative to the start (0 on the same runway). |
| `speedup` | 100 | SITL's cap on running faster than real time |
| `ardupilot_params` | none | ArduPilot parameters for this aircraft, `{NAME: value}`, loaded after `rascal.parm` |

Points are in metres **from that aircraft's start**, so each aircraft flies its
own route. Conflicting settings, such as both `waypoints` and `goto`, or `goto`
with mode AUTO, are refused. Home, where local (0, 0, 0) is, is the mission
terrain's origin, or McMillan Airfield without terrain.

## What is supported

| | |
| --- | --- |
| Aircraft | The Rascal: FixedWing6DOF's `airframe: rascal` (see [The aircraft](#the-aircraft)) |
| Number | Any; one container each. ArduPilot aircraft exist from t = 0. |
| Modes | Any ArduPlane mode by name with `mode`. LOITER, AUTO (`waypoints`), GUIDED (`goto`), and TAKEOFF have been flown. |
| Starts | Held at rest, then catapulted at the release time, in the air or from the ground; or on wheels from a runway ([Takeoff and landing](#takeoff-and-landing)) |
| Wind | Steady wind, also sent to ArduPlane as airspeed |
| Speed (macOS Docker) | About 9× real time for one aircraft, 5× for two, 2.5× for four |

Not supported:

- **ArduPilot aircraft spawned mid-run.** The containers start before the run.
- **Changing GUIDED targets during a run,** for example from a SCRIMMAGE
  autonomy.
- **A ground station on the host.** MAVLink stays inside each container.

## Starting: hold, release, catapult

ArduPlane needs the aircraft **still** while it boots:

- **It won't arm while the aircraft maneuvers.** Its pre-arm checks fail.
- **It steers the surfaces even when disarmed,** so a free-flying start dives
  into the ground.
- **It zeroes its airspeed sensor at boot,** assuming the aircraft is still.
  Holding it at 20 m/s instead leaves that sensor 20 m/s wrong for the rest of
  the flight.

So the SITL missions use FixedWing6DOF's launcher.

- **Until the release time,** every force is off and the aircraft hangs still.
  ArduPlane aligns, gets a GPS fix, and arms.
- **At release,** the aircraft is catapulted to flying speed, and ArduPlane
  takes over.

| Setting | Default | Meaning |
| --- | --- | --- |
| FixedWing6DOF `launch_time` | 40 | Release time, s. ArduPlane must be armed in its mode 1 s before, or the run fails and prints why. |
| FixedWing6DOF `launch_speed`, `launch_accel` | 20, 20 | Catapult to 20 m/s at 20 m/s² (1 s) |
| `position_m` | [0, 0, 200] | Start position, m. With height 0 the aircraft sits on the ground. |
| `wind_E`, `wind_N` on `SitlState` and FixedWing6DOF | 0 | Wind, m/s |
| ArduPilotBridge `port` | 9002, 9012 | Each aircraft's UDP port |

## ArduPlane basics

ArduPilot's documentation is the reference. These are the parts you need here.

**Flight modes** ([ArduPlane flight
modes](https://ardupilot.org/plane/docs/flight-modes.html)):

| Mode | What it does |
| --- | --- |
| LOITER | Circles the point where the mode started, at `WP_LOITER_RAD` (80 m), holding altitude and `AIRSPEED_CRUISE` (22 m/s) |
| AUTO | Flies the uploaded mission ([missions](https://ardupilot.org/plane/docs/common-planning-a-mission-with-waypoints-and-events.html)) |
| GUIDED | Flies to one target point and circles it ([GUIDED](https://ardupilot.org/plane/docs/guided-mode.html)) |
| TAKEOFF | Climbs to `TKOFF_ALT` (50 m), then loiters; used with the ground catapult |
| FBWA, CRUISE, MANUAL | Pilot-driven; the session script only sends centred sticks |

**Arming** ([arming](https://ardupilot.org/plane/docs/arming-your-plane.html)).
The session script waits for a GPS fix and a healthy EKF, then arms normally.
A failed pre-arm check shows up as `PreArm:` lines in `session.log`.

**Home.** ArduPlane moves home to where it arms, and waypoint altitudes are
relative to home. That is why routes and targets are given relative to each
aircraft's start.

**Airspeed.** ArduPlane flies on airspeed, not ground speed. In wind its ground
speed varies around a circle while airspeed stays near 22 m/s.

<a id="parameters"></a>**Parameters** ([parameter list](https://ardupilot.org/plane/docs/parameters.html)).
SITL loads two files:

- **`plane.parm`:** ArduPilot's own SITL plane defaults, taken from the pinned
  ArduPilot checkout in the image.
- **[rascal.parm](https://github.com/EthanMBoos/ripple/blob/main/crates/ardupilot/docker/rascal.parm),
  on top:**
  - `AIRSPEED_MIN 15`, because the model has no stall;
  - `SERVO2_REVERSED` and `SERVO4_REVERSED`: FixedWing6DOF uses the standard
    convention, where positive elevator pitches the nose down and positive
    rudder yaws it left, which is the opposite of ArduPlane's outputs.

To change ArduPlane's behavior (speeds, loiter radius, takeoff altitude, sensor
noise), edit `rascal.parm`, or set them for one mission with the bridge's
`ardupilot_params`. `ripple run` rebuilds the image from
`crates/ardupilot/docker` before every run, so edits (and a newer session
script after an update) are always used; Docker's cache makes that about a
second when nothing changed.

**More on SITL:** [SITL
overview](https://ardupilot.org/dev/docs/sitl-simulator-software-in-the-loop.html),
[the JSON interface this crate
uses](https://ardupilot.org/dev/docs/sitl-with-JSON.html), and [MAVLink's
mission protocol](https://mavlink.io/en/services/mission.html).

## The aircraft

The Rascal is a 6 kg RC plane: FixedWing6DOF's `airframe: rascal`, built from
ArduPilot's JSBSim Rascal. `airframe.rs` cites each value and lists where it
differs from JSBSim (span, angle-of-attack drag, induced and elevator drag,
elevator range).
Flown by AircraftPIDController instead of ArduPlane, `airframe: rascal` on the
controller gives gains measured on the Rascal and ArduPlane's attitude limits
(pitch 20, roll 45 degrees). Know its limits:

- **No stall by default.** Lift keeps rising with angle of attack. JSBSim's
  Rascal stalls near 8.3 m/s level (from its lift table), and
  higher in turns. Keep to moderate-angle flight around 20 m/s. This
  integration has not been checked against the real aircraft.

  **Optional stall:** FixedWing6DOF takes tables against angle
  of attack that replace its linear lift and zero-lift drag, interpolated
  and held at their ends like a JSBSim `<table>`. No mission sets them; to
  fly the Rascal with JSBSim's values, add to its `FixedWing6DOF` block:

  ```yaml
  # [alpha_rad, C_L]: C_L0 + C_L_alpha * alpha up to the 0.23 rad peak, then falling
  lift_table: [[-0.2, -0.75], [0.0, 0.25], [0.23, 1.4], [0.6, 0.71]]
  # [alpha_rad, C_D]: zero-lift drag, rising steeply past the stall
  drag_table: [[-1.57, 1.5], [-0.26, 0.056], [0.0, 0.028], [0.26, 0.056], [1.57, 1.5]]
  ```

  Use both: induced drag follows `C_L`, so with `lift_table` alone drag
  falls past the stall. Induced and elevator drag are still added to the
  table's. Accepted limitations: there is no spin model, and JSBSim's
  sideslip drag is left out. A deep stall or flying far sideways counts as
  already a failure; the model is not meant to recover from either
  faithfully.
- **Thrust is a fixed 0-25 N with no propeller model,** so top level speed is
  about 38 m/s.
- **Surface limits in radians:** elevator ±0.30, aileron and rudder ±0.35,
  matching the bridge's default PWM maps. Aileron and rudder are JSBSim's;
  its elevator moves −0.35 to +0.30, so ±0.30 is a simplification.

## Adding ArduPilot to a mission

Start from
[rascal-sitl.yaml](https://github.com/EthanMBoos/ripple/blob/main/crates/ardupilot/missions/rascal-sitl.yaml).
An ArduPilot aircraft is an entity group with count 1 and:

```yaml
autonomy:
  ArduPilotBridge:
    port: 9002                  # one port per aircraft
sensors:
  SitlState:
    wind_E: 0                   # wind must match the motion model
    wind_N: 0
# no controller: the bridge writes throttle, elevator, aileron, rudder
motion_model:
  FixedWing6DOF:
    airframe: rascal
    use_launcher: true
    launch_time: 40
```

- **The mission needs the `LocalNetwork` network.** The sensor reaches
  the bridge through it.
- **Use dt = 0.0025 s (400 Hz),** a whole multiple of ArduPlane's 50 Hz loop.
- **Both plugins must run every tick,** so give them no `loop_rate`.
- **`ArduPilotBridge` maps each servo channel** as `[channel, pwm_min, pwm_max,
  output_min, output_max]`, with optional `throttle`, `elevator`, `aileron`,
  and `rudder` keys. The defaults are ArduPlane's outputs 1-4 on the Rascal's
  throws.
- **Wind** is the air's velocity over the ground (ENU), so `wind_E: 5` on an
  eastbound aircraft is a tailwind. `SitlState` uses the same values to
  compute airspeed.

## Troubleshooting

| Symptom | Look at |
| --- | --- |
| `no new frame from ArduPilot SITL on UDP port …` | A SITL stopped. `ripple run` prints the session's `error:` line, such as "not armed in LOITER by the 40 s release". Then read that aircraft's `session.log`. |
| Never arms | `PreArm:` lines in `session.log` |
| Odd flight | ArduPilot's flight log. Decode a message type with the image's tools: `docker run --rm -v "$PWD/runs/<mission>/runNNN/sitl/9002/logs:/logs" --entrypoint /opt/ardupilot-venv/bin/python ripple-arduplane -m pymavlink.tools.mavlogdump --types ARSP --format csv /logs/00000001.BIN` (`ARSP` airspeed, `CTUN` attitude control, `TECS` speed and height control). |
| Containers left over after `ripple` was killed (Ctrl-C stops them) | `docker stop $(docker ps -q --filter label=ripple-sitl)` |

For maintainers, the exchange rules (exactly one reply per frame, resends
ignored, a frame gap fails the run) and the frame conversions are documented in
the [crate's
source](https://github.com/EthanMBoos/ripple/tree/main/crates/ardupilot/src) and
covered by its tests, with no Docker needed: `cargo test -p ardupilot`, plus
`python3 -m unittest discover -s crates/ardupilot/docker` for the session's
command timing. The session paces its commands by ArduPilot's clock, not wall
time, because sim time runs many times faster.

## Takeoff and landing

[rascal-runway.yaml](https://github.com/EthanMBoos/ripple/blob/main/crates/ardupilot/missions/rascal-runway.yaml)
takes off from the runway south of McMillan Airfield, flies a left-hand circuit,
and lands:

```sh
target/release/ripple run crates/ardupilot/missions/rascal-runway.yaml --headless
```

- **On its wheels.** The Rascal has FixedWing6DOF landing gear with JSBSim's
  Rascal values (see the Terrain chapter's
  [Landing gear](terrain.md#landing-gear)), on the McMillan terrain. It
  starts at rest on the runway, so there is no hold or catapult: still on the
  ground, ArduPlane boots, aligns, and arms. The starting height and the
  14° nose-up attitude are set by hand, from the ground height there.
- **The route.** The bridge's `takeoff_altitude` starts the AUTO mission with
  a takeoff to 50 m, and `land: true` makes its last waypoint a landing. With
  a takeoff, the session arms on the ground first and then switches to AUTO,
  as a pilot would: in AUTO while still disarmed, ArduPlane runs through the
  mission and then refuses to arm "in landing sequence". After landing it
  disarms and stays down.
- **Crash or touchdown.** TerrainCollision checks the propeller and wingtips,
  not the centre, so rolling on the wheels is not a crash.
- **Tuning, in the mission's `ardupilot_params`:** ground steering
  (`GROUND_STEER_ALT` and `STEER2SRV_*`, ArduPilot's recommended starting
  values with the Rascal's turning circle as `STEER2SRV_P`), and
  `LAND_PITCH_DEG 14` to flare to the three-point attitude. These are not in
  `rascal.parm`: ArduPlane measures `GROUND_STEER_ALT` from home, so an air
  start, which arms at its start height, would steer its rudder for the
  ground in flight.
- **The tail wheel turns with the rudder** (`steered_by_rudder`), as on RC
  taildraggers. JSBSim's castors, and a castering tail wheel ground-looped on
  the landing roll.

What a run looks like (run on 2026-10-01): ArduPlane rolls 22 m and lifts off at
12 m/s within 0.3 m of the centreline, climbs out, flies the circuit, and
touches down at 13 m/s after a 3.9° glide. The rollout stays within 1.6 m of the
centreline and stops 0.7 m from it, aligned with the runway. The landing is not
pretty: it lands level at 1.4 m/s sink and bounces (up to 1.7 m) before
settling. With ArduPilot's default 1° flare it touches down once, tail-high at
17 m/s, but swerves 14 m off the centreline when the tail drops.

The bounce is not a missing stall. Rerun on 2026-10-05 with and without the
optional `lift_table` added to the mission, the landing was the same to within
centimetres: first touch at 15.4 m/s and 1.45 m/s sink, bounces up to 2.0 m,
rollout within 1.7 m of the centreline. (These numbers come from a different
touchdown measure than the run above: the first frame after the circuit with the
centre within 5 cm of its resting height.) Adding `drag_table` too, the extra
drag at small angles helped a little: first touch at 15.1 m/s, bounces up to 1.9
m, rollout within 1.2 m. Airborne, the angle of attack peaks at 11.2°, below the
13.2° stall, from 15 s before touchdown on; only rolling tail-down on the wheels
(above 5 m/s) does it pass the stall angle, at 14.5°. The unmodified mission
repeats byte for byte, so these differences are the tables' effect, not
run-to-run noise (`reference/stall_landing.py`, evidence in
`paper/data/stall-landing/`). Each touchdown pitches the nose up by 10–17°, as a
taildragger's mains ahead of the centre of gravity do. A likely cause is the
approach at about 18.5 m/s, more than twice the stall speed, but that is
untested.
