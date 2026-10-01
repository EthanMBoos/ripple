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
- **Runs aren't repeatable byte for byte,** unlike other SCRIMMAGE runs: the
  output also depends on ArduPilot.
- **Sensor noise belongs to ArduPilot.** SITL is sent the true state on
  purpose. Don't put `NoisyState` on an ArduPilot aircraft; set ArduPilot's
  `SIM_` sensor-noise parameters instead (see [Parameters](#parameters)).

## Quick start

You need Docker running. The first launch builds the image, which compiles
ArduPlane; that takes about 25 minutes, and later launches reuse it. From the
repository root:

```sh
# One aircraft circling (LOITER)
python3 crates/ardupilot/launch.py crates/ardupilot/missions/rascal-sitl.xml end:=150

# Two aircraft, each flying a looping route (AUTO)
python3 crates/ardupilot/launch.py crates/ardupilot/missions/rascal-sitl-pair.xml end:=240 \
    --waypoints "600,0,0; 600,200,50; 0,200,50; 0,0,0"

# Fly to a point and circle it (GUIDED)
python3 crates/ardupilot/launch.py crates/ardupilot/missions/rascal-sitl.xml end:=180 --goto "800,400,50"

# Catapult takeoff from the ground, climbing to 50 m (TAKEOFF)
python3 crates/ardupilot/launch.py crates/ardupilot/missions/rascal-sitl.xml end:=120 \
    altitude:=0 --mode TAKEOFF

# Wind: 6 m/s from the north side (see "Wind" below for the sign)
python3 crates/ardupilot/launch.py crates/ardupilot/missions/rascal-sitl.xml end:=150 wind_north:=6
```

[launch.py](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/ardupilot/launch.py) does
four things:

1. It reads the mission's ArduPilot aircraft and their ports.
2. It starts one container per aircraft.
3. It runs `scrimmage run` (built with `--features ardupilot`).
4. It always stops the containers at the end.

Overrides (`key:=value`) are passed through to the mission. The launcher reads
XML missions with ArduPilot aircraft written out in their `<entity>` tags. It
rejects `param_common`, `entity_common`, and XInclude rather than guess at them.
Each run writes:

- `runs/<mission>/runNNN/`: the SCRIMMAGE run, as usual. View it with
  `scrimmage replay`.
- `runs/<mission>/sitlNNN/aircraft-<port>/`: one folder per SITL, holding:
  - `session.log`: ArduPilot's messages, arming, and errors;
  - `logs/*.BIN`: ArduPilot's own flight log;
  - its parameter storage.

Waypoints and GUIDED targets are `east,north,up` in metres **from that
aircraft's start**, so in a multi-aircraft run every aircraft flies its own
copy. AUTO routes repeat. `--waypoints` arms in AUTO and `--goto` arms in
GUIDED; otherwise `--mode` picks the mode (default LOITER). The launcher refuses
conflicting options, such as both together or `--goto` with `--mode AUTO`.

## What is supported

| | |
| --- | --- |
| Aircraft | The Rascal: FixedWing6DOF with C++ SCRIMMAGE's Rascal parameters (see [The aircraft](#the-aircraft)) |
| Number | Any; one container each. ArduPilot aircraft exist from t = 0. |
| Modes | Any ArduPlane mode by name with `--mode`. LOITER, AUTO (`--waypoints`), GUIDED (`--goto`), and TAKEOFF have been flown. |
| Starts | Held at rest, then catapulted at the release time, in the air or from the ground |
| Wind | Steady wind, also sent to ArduPlane as airspeed |
| Speed (macOS Docker) | About 9× real time for one aircraft, 5× for two, 2.5× for four |

Not supported:

- **ArduPilot aircraft spawned mid-run.** The containers start before the run.
- **Changing GUIDED targets during a run,** for example from a SCRIMMAGE
  autonomy.
- **A different route for each aircraft.**
- **Runway takeoff and landing** (see [the planned section](#takeoff-and-landing-on-terrain-planned)).
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

| Mission variable | Default | Meaning |
| --- | --- | --- |
| `release` | 40 | Release time, s. ArduPlane must be armed in its mode 1 s before, or the run fails and the launcher prints why. |
| `launch_speed`, `launch_accel` | 20, 20 | Catapult to 20 m/s at 20 m/s² (1 s) |
| `altitude` | 200 | Start height, m. With `altitude:=0` the aircraft sits on the ground. |
| `wind_east`, `wind_north` | 0 | Wind, m/s |
| `port`, or `port_a` / `port_b` | 9002, 9012 | Each aircraft's UDP port |

## ArduPlane basics

ArduPilot's documentation is the reference. These are the parts you need here.

**Flight modes** ([ArduPlane flight modes](https://ardupilot.org/plane/docs/flight-modes.html)):

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
- **[rascal.parm](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/ardupilot/docker/rascal.parm),
  on top:**
  - `AIRSPEED_MIN 15`, because the model has no stall;
  - `SERVO2_REVERSED` and `SERVO4_REVERSED`: FixedWing6DOF uses the standard
    convention, where positive elevator pitches the nose down and positive
    rudder yaws it left, which is the opposite of ArduPlane's outputs.

To change ArduPlane's behavior (speeds, loiter radius, takeoff altitude, sensor
noise), edit `rascal.parm`. The launcher rebuilds the image only when it is
missing, so run `docker build -t scrimmage-rs-arduplane crates/ardupilot/docker`
after an edit.

**More on SITL:** [SITL overview](https://ardupilot.org/dev/docs/sitl-simulator-software-in-the-loop.html),
[the JSON interface this crate uses](https://ardupilot.org/dev/docs/sitl-with-JSON.html),
and [MAVLink's mission protocol](https://mavlink.io/en/services/mission.html).

## The aircraft

The Rascal is a 6 kg RC plane, using C++ SCRIMMAGE's simplified Rascal, which
was derived from ArduPilot's JSBSim Rascal. `missions/fixed-wing-6dof-rascal.xml`
checks it against C++. Know its limits:

- **No stall.** Lift keeps rising with angle of attack. JSBSim's Rascal
  stalls near 8 m/s level (an estimate from its lift table), and higher in
  turns. Keep to moderate-angle flight around 20 m/s.
  This integration has not been checked against the real aircraft.
- **Thrust is a fixed 0-25 N with no propeller model,** so top level speed is
  about 38 m/s.
- **The SITL missions use radian surface limits:** elevator ±0.30, aileron
  and rudder ±0.35, inherited from C++'s Rascal file before it switched to ±1.
  With coefficients per radian, ±1 would allow about 3× more deflection.

## Adding ArduPilot to a mission

Start from
[rascal-sitl.xml](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/ardupilot/missions/rascal-sitl.xml).
An ArduPilot aircraft is an entity with count 1 and:

```xml
<autonomy port="9002">ArduPilotBridge</autonomy>      <!-- one port per aircraft -->
<sensor wind_E="0" wind_N="0">SitlState</sensor>       <!-- wind must match the motion model -->
<!-- no controller: the bridge writes throttle, elevator, aileron, rudder -->
<motion_model use_launcher="true" launch_time="40" launch_speed="20" launch_accel="20"
              ...Rascal parameters...>FixedWing6DOF</motion_model>
```

- **The mission needs `<network>LocalNetwork</network>`.** The sensor reaches
  the bridge through it.
- **Use dt = 0.0025 s (400 Hz),** a whole multiple of ArduPlane's 50 Hz loop.
- **Both plugins must run every tick,** so give them no `loop_rate`.
- **`ArduPilotBridge` maps each servo channel** as `"channel, pwm_min, pwm_max,
  output_min, output_max"`, with optional `throttle`, `elevator`, `aileron`,
  and `rudder` keys. The defaults are ArduPlane's outputs 1-4 on the Rascal's
  throws.
- **Wind sign:** FixedWing6DOF adds wind to velocity, so `wind_east=5` on an
  eastbound aircraft is a headwind. `SitlState` uses the same values to
  compute airspeed.

## Troubleshooting

| Symptom | Look at |
| --- | --- |
| `no new frame from ArduPilot SITL on UDP port …` | A SITL stopped. The launcher prints the session's `error:` line, such as "not armed in LOITER by the 40 s release". Then read that aircraft's `session.log`. |
| Never arms | `PreArm:` lines in `session.log` |
| Odd flight | ArduPilot's flight log. Decode a message type with the image's tools: `docker run --rm -v "$PWD/runs/<mission>/sitlNNN/aircraft-9002/logs:/logs" --entrypoint /opt/ardupilot-venv/bin/python scrimmage-rs-arduplane -m pymavlink.tools.mavlogdump --types ARSP --format csv /logs/00000001.BIN` (`ARSP` airspeed, `CTUN` attitude control, `TECS` speed and height control). |
| Containers left over after a crash | `docker ps --filter label=scrimmage-rs-sitl`, then `docker stop` |

For maintainers, the exchange rules (exactly one reply per frame, resends
ignored, a frame gap fails the run) and the frame conversions are documented in
the [crate's source](https://github.com/EthanMBoos/scrimmage-rs/tree/main/crates/ardupilot/src)
and covered by its tests, with no Docker needed: `cargo test -p ardupilot`, plus
`python3 -m unittest discover -s crates/ardupilot/docker` for the session's
command timing. The session paces its commands by ArduPilot's clock, not wall
time, because sim time runs many times faster.

## Takeoff and landing on terrain (planned)

Today the ground is FixedWing6DOF's simple flat spring at z = 0. It holds a
waiting aircraft, and the ground catapult takeoff works from it, but there is
no rolling, wheel, or landing contact.

Planned with [terrain ground contact](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/TODO.md):

- **Runway takeoff** in ArduPlane's TAKEOFF or AUTO mode, rolling on the
  terrain instead of being catapulted.
- **Landing** with ArduPlane's LAND or an AUTO mission ending in a landing,
  with touchdown told apart from a crash.
- **Starting on the terrain at its real height,** with the launcher home at
  the terrain origin.

<!-- TODO(terrain): fill this in once ground contact lands: mission settings,
     ArduPlane landing parameters (LAND_*, TKOFF_*), and a demo command. -->
