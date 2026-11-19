# Flying PX4 (SITL)

The optional `px4` crate lets PX4 firmware fly FixedWing6DOF aircraft in a
Ripple mission, the way the [ArduPilot bridge](ardupilot.md) does with
ArduPlane. Each aircraft gets its own PX4 SITL (software in the loop) running
in a Docker container. The missions are twins of the ArduPilot ones: swap
`ArduPilotBridge` for `Px4Bridge` and the aircraft, the runway, and the
circuit stay the same.

## How to think about it

Two programs share each aircraft:

| | Owns |
| --- | --- |
| **Ripple** | The world: time, physics (FixedWing6DOF), other entities, interactions, metrics, Rerun. Also the aircraft's sensor readings: PX4 is sent readings, not the true state. |
| **PX4** | The aircraft's flight controller. Its estimator fuses the readings; its control loops and flight modes fly. |

Each Ripple tick:

1. `Px4Bridge` turns the aircraft's true state into the readings PX4 expects
   (IMU, magnetometer, barometer, pitot, and GPS at 5 Hz), with sensor noise,
   and sends them.
2. PX4 runs one step and answers with its actuator controls.
3. Ripple maps them onto the control surfaces and throttle.

This is **lockstep**: PX4's clock follows the time stamps Ripple sends, and
Ripple advances only when every aircraft's PX4 has answered, so all aircraft
share one clock. Three things follow from that:

- **A slow SITL slows the whole run.** A dead one fails it after 10 s with "no
  controls from PX4 on TCP port …".
- **Runs do not repeat byte for byte.** PX4 boots on wall time while Ripple
  runs on, so its modules start at a slightly different simulated time each
  run and its estimator settles differently. The flights agree closely: three
  runs of `rascal-sitl.yaml` (2026-11-17) were within a metre of each other
  after 150 s. The ArduPilot runs, by contrast, have repeated exactly.
- **Sensor noise belongs to the bridge.** PX4 takes readings, so the bridge
  adds the noise, with the defaults of PX4's Gazebo Classic plugin. Don't put
  `NoisyState` on a PX4 aircraft; set the bridge's noise keys instead (see
  [Quick start](#quick-start)).

## Quick start

You need Docker running. Build `ripple` with the PX4 feature; the first run
builds the SITL image, which compiles PX4. That takes a few minutes, and later
runs reuse it. From the repository root:

```sh
cargo build --release -p ripple --features px4
S=target/release/ripple
M=crates/px4/missions
B=autonomy.Px4Bridge   # shortens the override paths below

# One aircraft: catapulted at 40 s, climbs 20 m, and circles (TAKEOFF, then LOITER)
$S run $M/rascal-sitl.yaml --headless run.end_s:=150

# Two aircraft: one flying a looping route (MISSION), one circling
$S run $M/rascal-sitl-pair.yaml --headless run.end_s:=240 \
    "entities.rascal_a.$B.waypoints_m:=[[600,0,0], [600,200,50], [0,200,50], [0,0,0]]"

# A PX4 parameter for one run: circle at 150 m instead of 80 m
$S run $M/rascal-sitl.yaml --headless run.end_s:=150 "entities.rascal.$B.px4_params.NAV_LOITER_RAD:=150"

# Wind: 6 m/s from the south, so the air moves north (see "Wind" below for the
# sign); the sensor and the motion model both take it
$S run $M/rascal-sitl.yaml --headless run.end_s:=150 \
    entities.rascal.sensors.SitlState.wind_mps.1:=6 entities.rascal.motion_model.FixedWing6DOF.wind_mps.1:=6
```

`ripple run` starts one SITL container per `Px4Bridge` aircraft once the
mission has loaded, and stops them all when the run ends, fails, or is
interrupted with Ctrl-C
([px4_launcher.rs](../../../crates/px4/src/px4_launcher.rs)).
It reads the resolved mission, so templates and overrides apply, and the
containers always match it. `ripple sweep` starts no SITL. Each run writes
`runs/<mission>/runNNN/` as usual, plus `sitl/<port>/` inside it: one folder
per SITL, holding

- `session.log`: PX4's messages, arming, and errors;
- `log/<date>/*.ulg`: PX4's own flight log, with every parameter it used.

PX4 keeps its parameter and mission files inside the container. With them
on the mounted run folder, which Docker Desktop writes slowly, one runway
run in six lost every interval-scheduled PX4 module (airspeed selector, load
monitor, battery) at the moment of arming and flew without airspeed; with
them inside, nine runs showed no stall. The cause is not proven, so if
modules go quiet in `session.log` ("Airspeed selector module down" after
arming), suspect this first.

The PX4 missions have the same file names as their ArduPilot twins, so
`runs/rascal-sitl/` holds runs of both. The first line of `session.log`
tells them apart, as does the SITL folder's name: PX4 uses ports 4560 to
4569, ArduPilot 9002 and up.

With several aircraft the engine waits for each SITL in turn within a tick, so
a pair runs slower than one aircraft.

The SITL session settings are bridge parameters:

| Key | Default | Meaning |
| --- | --- | --- |
| `mode` | MISSION with `waypoints_m`, else TAKEOFF | PX4 mode to arm in |
| `waypoints_m` | none | MISSION route `[[east, north, up], ...]`; it repeats |
| `takeoff_altitude_m` | none | Start the route with a takeoff to this height, m, towards the first waypoint |
| `land` | false | Make the last waypoint a landing; the route does not repeat. Its `up` is the ground's height there relative to the start (0 on the same runway). |
| `speedup` | 100 | PX4's cap on running faster than real time |
| `px4_params` | none | PX4 parameters for this aircraft, `{NAME: value}`, set after the airframe file |
| `home` | McMillan Airfield, 300 m | Where local (0, 0, 0) is: `[latitude, longitude, altitude]`, for the GPS and barometer. It must be the terrain's origin; `ripple run` checks. |
| `magnetic_field_ned_gauss` | McMillan, 2026 | The Earth's field at home, north-east-down |
| `gps_rate_hz` | 5 | GPS reading rate |
| `gyro_noise_radps_per_rthz`, `accel_noise_mps2_per_rthz`, `mag_noise_gauss_per_rthz` | PX4's Gazebo Classic defaults | White-noise densities, scaled to the tick; 0 turns one off |
| `baro_noise_pa` | 1 | Barometer noise per reading, Pa |

Points are in metres **from that aircraft's start**, so each aircraft flies its
own route. Conflicting settings, such as `mode: LOITER` with `waypoints_m`,
are refused.

## What is supported

| | |
| --- | --- |
| Aircraft | The Rascal: FixedWing6DOF's `airframe: rascal` (see the ArduPilot guide's [The aircraft](ardupilot.md#the-aircraft)) |
| Number | Any; one container each. PX4 aircraft exist from t = 0. |
| Modes | Any PX4 mode by its MAVLink name with `mode` (MANUAL, STABILIZED, ACRO, ALTCTL, POSCTL, LOITER, MISSION, RTL, LAND, TAKEOFF, OFFBOARD). TAKEOFF and MISSION (`waypoints_m`) have been flown. |
| Starts | Held at rest, then catapulted at the release time, in the air or from the ground; or on wheels from a runway ([Takeoff and landing](#takeoff-and-landing)) |
| Wind | Steady wind, also felt by PX4 as airspeed |
| Speed (macOS Docker) | About 5× real time for one aircraft |

Not supported:

- **PX4 aircraft spawned mid-run.** The containers start before the run.
- **A target sent during a run,** such as ArduPlane's GUIDED point, for
  example from a Ripple autonomy.
- **A ground station on the host.** MAVLink stays inside each container.

## Starting: hold, release, catapult

PX4 needs the aircraft **still** while it boots: its estimator aligns from
the first readings, its pre-flight checks must pass before it arms, and it
steers the surfaces as soon as it is armed. So the SITL missions use
FixedWing6DOF's launcher, as the ArduPilot ones do.

- **Until the release time,** every force is off and the aircraft hangs still.
  PX4 aligns, gets a GPS fix, and the session script arms it in its mode.
- **At release,** the aircraft is catapulted to flying speed, and PX4 takes
  over. In TAKEOFF it climbs 20 m above where it armed and then circles there.

| Setting | Default | Meaning |
| --- | --- | --- |
| FixedWing6DOF `launch_at_s` | 40 | Release time, s. PX4 must be armed in its mode 1 s before, or the run fails and prints why. |
| FixedWing6DOF `launch_speed_mps`, `launch_acceleration_mps2` | 20, 20 | Catapult to 20 m/s at 20 m/s² (1 s) |
| `position_m` | [0, 0, 200] | Start position, m. With height 0 the aircraft sits on the ground. |
| `wind_mps` on `SitlState` and FixedWing6DOF | [0, 0, 0] | Wind, m/s, `[east, north, up]` |
| Px4Bridge `port` | 4560, 4561 | Each aircraft's TCP port: 4560 plus its PX4 instance |

The session script's order is: PX4 boots into Hold for want of sticks, so the
script first puts it in STABILIZED for 2 s of PX4 time, uploads the route if
there is one, sets the mode, and arms. The STABILIZED step matters: Hold runs
PX4's height controller from the first estimator output, before the estimator
has an altitude, and the controller then chases the real altitude at its
climb-rate limit for over a minute of flight. In STABILIZED the controller is
off, and it starts afresh after a second unused.

## PX4 basics

PX4's documentation is the reference. These are the parts you need here.

**Flight modes** ([PX4 fixed-wing flight
modes](https://docs.px4.io/main/en/flight_modes_fw/)):

| Mode | What it does |
| --- | --- |
| TAKEOFF | Runway takeoff: climbs to `MIS_TAKEOFF_ALT` (20 m) above where it armed, then becomes LOITER there. Used with the catapult too. |
| MISSION | Flies the uploaded route ([missions](https://docs.px4.io/main/en/flight_modes_fw/mission.html)) |
| LOITER | PX4's Hold: circles where it is at `NAV_LOITER_RAD` (80 m). Entered on the ground it never gets a target, so arm in TAKEOFF to circle. |
| STABILIZED, MANUAL, ACRO | Pilot-driven; the session script sends no sticks |

**Arming.** The session script waits for a GPS fix and a position, then arms
normally. A failed pre-flight check shows up as `px4: Preflight Fail:` lines
in `session.log`.

**Home.** PX4 sets home where it arms, and waypoint altitudes are relative to
home. That is why routes are given relative to each aircraft's start.

**Airspeed.** PX4 flies on airspeed, not ground speed, at `FW_AIRSPD_TRIM`
(20 m/s here). In wind its ground speed varies around a circle while airspeed
stays near 20 m/s.

<a id="parameters"></a>**Parameters** ([parameter
reference](https://docs.px4.io/main/en/advanced_config/parameter_reference.html)).
SITL loads the airframe file
[4100_rascal](../../../crates/px4/docker/4100_rascal),
which starts from PX4's own JSBSim Rascal airframe (`1033_jsbsim_rascal`,
the Rascal PX4 flies in JSBSim). What it changes, none of which PX4's
documentation would tell you:

- **`CA_AIRFRAME 1`, a fixed wing.** PX4's Rascal file leaves the control
  allocator on its multirotor default, so the surfaces never moved and the
  first flight dived.
- **The Rascal's speeds:** `FW_AIRSPD_MIN 15`, `FW_AIRSPD_TRIM 20`,
  `FW_AIRSPD_MAX 30`, `FW_AIRSPD_STALL 10`. PX4's defaults (10, 15, 20)
  flew the model at its stall in turns; its level stall is near 8.3 m/s.
- **Reversed outputs.** `PWM_MAIN_REV` reverses the rudder, the wheel, the
  left aileron, and the elevator: FixedWing6DOF uses the standard
  convention, where positive elevator pitches the nose down and positive
  rudder yaws it left, which is the opposite of PX4's outputs, and a tail
  wheel turned right swings the nose left. Output 4 carries the steered
  wheel (`PWM_MAIN_FUNC4 440`) in place of the unused left flap.
- **`FW_W_EN 0`.** PX4's wheel controller takes its heading when the landing
  starts, mid-turn here; see [Takeoff and landing](#takeoff-and-landing).
- **`COM_RC_IN_MODE 4`,** no sticks, so PX4 skips its stick checks. It still
  boots into Hold, which the session script handles (see
  [Starting](#starting-hold-release-catapult)).
- **`MIS_TKO_LAND_REQ 0`.** PX4's fixed-wing defaults require a landing item
  in every mission; a looping route has none.
- **`NAV_DLL_ACT 0` and `NAV_RCL_ACT 0`.** The script is the only ground
  station and sends no RC, so losing either must not trigger a return.
- **`FW_LND_USETER 0`.** No range finder: land by barometric height, not
  terrain, or the landing aborts.

Around the file, the launcher and the image do four more things worth
knowing:

- **The IMU rate is the tick rate** (`IMU_INTEG_RATE`, `IMU_GYRO_RATEMAX`),
  so PX4 integrates and runs its control loop once per tick and answers each
  reading with one controls message.
- **The pre-flight disarm timeout is off** (`COM_DISARM_PRFLT -1`). PX4
  scales it by the speed factor, and a 40 s hold still outlasts it.
- **Mission parameters go in as `PX4_PARAM_*` environment entries,** which
  PX4's start-up script applies *before* the airframe file. So the file uses
  `set-default` only, which loses to them; a plain `set` would overwrite the
  mission's values.
- **PX4's start-up script needs `bc`** for that scaling, so the image has
  it.

To change PX4's behaviour (speeds, loiter radius, takeoff altitude), edit the
airframe file, or set parameters for one mission with the bridge's
`px4_params`. `ripple run` rebuilds the image from `crates/px4/docker` before
every run, so edits (and a newer session script after an update) are always
used; Docker's cache makes that about a second when nothing changed.

**More on SITL:** [PX4
simulation](https://docs.px4.io/main/en/simulation/), [the simulator MAVLink
API this crate uses](https://mavlink.io/en/messages/common.html#HIL_SENSOR)
(`HIL_SENSOR`, `HIL_GPS`, `HIL_ACTUATOR_CONTROLS`), and [MAVLink's mission
protocol](https://mavlink.io/en/services/mission.html).

## The aircraft

The Rascal is the ArduPilot guide's: see [The
aircraft](ardupilot.md#the-aircraft) there for its limits (no stall without
the tables, fixed 0 to 25 N thrust, JSBSim's surface limits). The bridge's
default maps span ±0.30 rad for the elevator, so PX4 never asks for the
model's last 0.05 rad of up-elevator, as with ArduPilot.

## Adding PX4 to a mission

Start from
[rascal-sitl.yaml](../../../crates/px4/missions/rascal-sitl.yaml).
A PX4 aircraft is an entity group with count 1 and:

```yaml
autonomy:
  Px4Bridge:
    port: 4560                  # 4560 plus the PX4 instance; one per aircraft
sensors:
  SitlState:
    wind_mps: [0, 0, 0]         # wind must match the motion model
# no controller: the bridge writes throttle, elevator, aileron, rudder, wheel
motion_model:
  FixedWing6DOF:
    airframe: rascal
    enable_launcher: true
    launch_at_s: 40
```

- **The mission needs the `LocalNetwork` network.** The sensor reaches
  the bridge through it.
- **Use dt = 0.0025 s (400 Hz),** or another whole-number rate: PX4's IMU
  and control loop run at the tick rate.
- **Both plugins must run every tick,** so give them no `loop_rate_hz`.
- **`Px4Bridge` maps each output channel** with optional `throttle`,
  `elevator`, `aileron`, `rudder`, and `wheel` keys, each `[channel,
  input_min, input_max, output_min, output_max]` (PX4's value, 0 to 1 for the
  motor and −1 to 1 for servos, maps linearly onto the output; swap the
  outputs to reverse a channel). The defaults are the airframe file's
  outputs 3 to 8 on the Rascal's throws.
- **`home` must be the terrain's origin,** or McMillan Airfield without
  terrain, since the GPS readings are made from it.
- **Wind** is the air's velocity over the ground (ENU), so
  `wind_mps: [5, 0, 0]` on an eastbound aircraft is a tailwind. `SitlState`
  uses the same values to compute airspeed.

## Troubleshooting

| Symptom | Look at |
| --- | --- |
| `no controls from PX4 on TCP port …` or `PX4 closed the connection` | A SITL stopped. `ripple run` prints the session's `error:` line, such as "not armed in TAKEOFF by the 40 s release". Then read that aircraft's `session.log`. |
| `PX4 … did not connect` | Docker is not running, or the image failed to build; `docker.log` in the SITL folder has the container's errors. |
| Never arms | `px4: Preflight Fail:` lines in `session.log` |
| Odd flight | PX4's flight log. Turn it into CSV files with the image's tools: `docker run --rm -v "$PWD/runs/<mission>/runNNN/sitl/4560/log:/log" --entrypoint /opt/px4-venv/bin/ulog2csv ripple-px4 -o /log/csv /log/<date>/<time>.ulg`, one file per topic (`tecs_status` speed and height control, `airspeed_validated`, `actuator_outputs`, `vehicle_attitude`). |
| Containers left over after `ripple` was killed (Ctrl-C stops them) | `docker stop $(docker ps -q --filter label=ripple-sitl)` |

For maintainers, the exchange rules (one controls message per tick, stamped
with the readings it answers; older stamps ignored; a later stamp or silence
fails the run) and the reading conversions are documented in the [crate's
source](../../../crates/px4/src) and
covered by its tests against a fake PX4, with no Docker needed: `cargo test
-p px4`, plus `python3 -m unittest discover -s crates/px4/docker` for the
session's command timing. The session paces its commands by PX4's clock, not
wall time, because sim time runs many times faster.

## Takeoff and landing

{{#include ../demos/runway-px4/embed.html}}

[rascal-runway.yaml](../../../crates/px4/missions/rascal-runway.yaml)
is the twin of the ArduPilot runway mission: the same Rascal on its wheels,
the same runway south of McMillan Airfield, and the same left-hand circuit,
flown by PX4:

```sh
target/release/ripple run crates/px4/missions/rascal-runway.yaml --headless
```

- **On its wheels.** As in the ArduPilot twin, the aircraft starts at rest on
  the runway with FixedWing6DOF landing gear, so there is no hold or
  catapult: still on the ground, PX4 boots, aligns, and arms.
- **The route.** `takeoff_altitude_m` starts the MISSION with a runway
  takeoff to 50 m, which PX4 rolls and climbs towards the first waypoint, and
  `land: true` makes the last waypoint a landing. PX4 glides the final leg
  from the previous waypoint, 30 m over 500 m here (3.4°, under its
  `FW_LND_ANG` 5° limit). After landing it disarms and stays down.
- **Arming order.** PX4 arms in the mode it will fly, so the session uploads
  the route, sets MISSION, and then arms; the takeoff starts on arming.
- **The tail wheel follows the rudder,** as on the ArduPilot twin. PX4 has
  its own wheel controller (the bridge's `wheel` output, with a wheel
  `steered_by_input` and `FW_W_EN 1`), and it held the takeoff roll within
  4°, but it takes its heading when the landing starts, which on this circuit
  is mid-turn onto final, so it arrived hard over and ground-looped the
  rollout (2026-11-17). The airframe file turns it off.

What a run looked like (2026-11-17, four runs within 0.2 m of each other at
the end): PX4 rolls 57 m and lifts off at 17 m/s within 2.5 m of the
centreline, climbs out, flies the circuit at 20 m/s, and comes down a 2.8°
approach to touch down at 10 m/s and 1.8 m/s sink, without a bounce, 7 m left
of the centreline. The rollout is 52 m and wanders to 14 m off the
centreline before it stops, aligned within 4° of the runway. The ArduPlane
twin lands faster and bounces, but stays within 3 m of the centreline; PX4's
approach is still correcting when it reaches the ground, and its rudder
steering on the roll is weak at low speed. Neither has been tuned for it.
