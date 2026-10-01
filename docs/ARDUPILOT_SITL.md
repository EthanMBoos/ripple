# ArduPilot SITL: how lockstep works and how to fly several vehicles

Research for the ArduPlane roadmap item in [TODO.md](TODO.md). It covers:

1. what ArduPilot's source says;
2. what C++ SCRIMMAGE did, and why;
3. what [UnrealVehicleDynamics](https://github.com/EthanMBoos/UnrealVehicleDynamics)
   (UVD) does, including an audit;
4. a recommended design for scrimmage-rs.

ArduPilot paths are on `master` of `github.com/ArduPilot/ardupilot`, read
2026-09-30. Claims were checked against the source rather than the docs,
where the two differ. Anything that is a judgment rather than a sourced fact
is marked as such.

## 1. ArduPilot SITL

### Backends

SITL can talk to many external simulators (`libraries/AP_HAL_SITL/SITL_cmdline.cpp`).

- **JSON** (`--model JSON`, `libraries/SITL/SIM_JSON.cpp`) is the general
  interface for any simulator, added 2020-05-20. The
  [JSON page](https://ardupilot.org/dev/docs/sitl-with-JSON.html) calls it
  "designed to be easy to implement for those developing new physics backend".
  ArduPilot's current Gazebo plugin
  ([ardupilot_gazebo](https://github.com/ArduPilot/ardupilot_gazebo)) uses it.
- **Older backends** each had their own binary packet, such as the Gazebo
  one, `last_letter` and `scrimmage`. The SCRIMMAGE wiki page was marked
  "Archived" on 2026-08-27 and "may not work with current ArduPilot code".

### How ArduPilot's simulator interfaces evolved

Dates are the first commit of each backend file in `libraries/SITL/`. Notes
on how a backend worked are from memory unless a source is given.

- **2015: one backend per simulator.** Andrew Tridgell added CRRCSim,
  JSBSim, `last_letter` and RealFlight (FlightAxis) within months, and John
  Hsu added the first Gazebo shim. Each had its own packet format, and none
  had a frame counter.
- **2016: built-in models.** Tridgell added `SIM_Plane` ("very simple fixed
  wing simulator"), which runs inside SITL, plus X-Plane. For ArduPilot's own
  testing, external simulators became optional. Built-in models step in
  lockstep by construction, because they run in the same process.
- **2018-2019: more one-off backends:** Morse (2018), then AirSim,
  SilentWings, Webots and SCRIMMAGE (2019). In September 2019 there are
  commits fixing SITL's internal lockstep scheduling, so lockstep was already
  a working concept inside SITL.
- **2020: JSON** (2020-05-20, Iampete1). One general text protocol with a
  frame counter, resends and sim-owned time. A new simulator no longer needs
  a new ArduPilot backend. SCRIMMAGE's last ArduPilot-side changes landed that
  same spring, just before JSON.
- **2021 onward: JSON becomes the standard.**
  - 2021: `JSON_Master` lets several SITLs share one simulator connection.
    It is for ride-along controllers, not multiple vehicles.
  - The current `ardupilot_gazebo` runs over JSON with lockstep; its history
    was not checked.
  - 2023: Webots moved to a new Python backend.
- **2025-2026: lockstep by default, with an escape hatch.** A JSON
  non-lockstep mode was added in November 2025 for simulators that cannot
  wait. In July 2026 lockstep was extended to simulated CAN peripherals in
  ArduPilot's tests.

Overall, ArduPilot went from a custom backend per simulator with loose
timing, to built-in models for its own testing, to one standard lockstep
protocol for everything else.

### The JSON protocol

Servo packet, SITL to sim, little-endian binary:

```
uint16 magic = 18458   // 29569 with SERVO_32_ENABLE=1 and pwm[32]
uint16 frame_rate      // SITL's current rate estimate, see "Rate" below
uint32 frame_count
uint16 pwm[16]
```

State reply, sim to SITL: one JSON object with a newline **before and after**
it. Without the leading newline, SITL drops the first reply (SIM_JSON.cpp:339-347).

| Field | Units and frame | Required |
| --- | --- | --- |
| `timestamp` | seconds of physics time, absolute | yes |
| `imu.gyro` | rad/s, body FRD | yes |
| `imu.accel_body` | m/s², body FRD, specific force (reads (0, 0, -g) at rest) | yes |
| `velocity` | m/s, NED | yes |
| `attitude` or `quaternion` | roll, pitch, yaw in rad, or q1..q4 (the quaternion wins) | yes |
| `position` | m, NED from SITL's home | in practice |
| `airspeed`, `velocity_wind` | m/s | optional; without them SITL assumes zero wind |
| `rng_1..6`, `windvane`, `rc`, `battery`, `no_time_sync`, `no_lockstep` | | optional |

- **Home.** `position` is relative to SITL's `--home`, and the sim's own origin
  is invisible to SITL (SIM_JSON.cpp:402-403). So the sim must pass its
  lat/lon origin as `--home`. The code also has a branch meant to set home
  from the first lat/lon received, but it never runs in normal SITL.
- **Sensors.** SITL makes up GPS, compass and baro from position and attitude.
  ArduPlane needs nothing beyond the required fields; send `airspeed` if the
  sim has wind.

### Lockstep

The rules below are checked in the source.

- **SITL sends, then waits.** It sends one servo packet, then polls every
  100 ms until a reply arrives. There is no overall timeout.
- **Resends.** After about **1 s** of silence SITL resends the same frame, and
  keeps resending about every 1.1 s (SIM_JSON.cpp:321-330). The readme and a
  code comment say 10 s; that is stale.
- **Frame counter.** `frame_count` only advances after a reply parses
  successfully (:527). A reply that fails to parse means the same frame is
  sent again.
- **The sim owns time.** SITL advances its clock by
  `timestamp - last_timestamp` on every reply (:506-514). Details:
  - A timestamp that goes backwards freezes time for that step and prints
    "Detected physics reset". It does not reset ArduPilot.
  - The first reply measures from 0, so start the sim's timestamps near 0.
  - A step of 0.1 s or more still advances time; it only skips the rate
    adjustment and wall-clock pacing.
- **One reply per servo packet, exactly.** Replies carry no frame number, so
  SITL cannot tell which frame a reply answers. SITL reads one UDP datagram per step
  and parses the last complete line in it. An extra reply is not discarded:
  it queues in SITL's socket and is consumed next step as the answer to the
  next frame. SITL then runs one frame stale for the rest of the run. This is
  the easiest way to get lockstep subtly wrong.
- **Restarts.** SITL never checks for `frame_count` going backwards; that is
  the sim's job. A restarted SITL simply starts again at 0.
- **Rate.**
  - `--rate` sets `SIM_RATE_HZ`, which defaults to 1200.
  - SITL resets its rate estimate to 1/dt from each reply's timestamps, and
    `frame_rate` just reports it back, so a sim with its own fixed dt is fine.
  - ArduPlane's main loop runs at 50 Hz (`SCHED_LOOP_RATE`) and runs several
    physics frames per loop.
  - A physics rate that is a whole multiple of 50 Hz (200, 400) keeps loop
    periods even. 120 Hz alternates between 16.7 and 25 ms.
- **Speed.**
  - `SIM_SPEEDUP` (`--speedup`) only sleeps, to keep SITL from running faster
    than real time × speedup. It never makes a slow sim faster.
  - The default is 1, so a lockstepped run is capped at real time.
  - To run fast, pass a large `--speedup`. 100 is common, though the parameter
    doc says 1-10.
  - Above speedup 1, SITL also stalls while its MAVLink serial0 output queue is
    full, so a MAVLink client must keep reading.
- **Two flags to leave off.**
  - `no_time_sync: true` turns off pacing and changes the default
    `AHRS_EKF_TYPE` to 10 (SITL's built-in estimator), so it is not a real EKF
    test.
  - `no_lockstep: true` makes SITL advance on its own when no reply arrives.

### Ports and multiple instances

- **Ports per instance.** `-I N` adds `10*N` to every port. MAVLink serial0 is
  `5760+10N`, and the JSON sim port is `9002+10N` (SITL_cmdline.cpp:261-266).
- **Which options matter.** `--sim-port-out` sets the port the sim listens on.
  `--sim-address` sets the sim's IP. `--sim-port-in` does nothing for JSON.
- **Reply address.** SITL sends from an ephemeral port and accepts replies from
  any sender, so the sim must **reply to the source address and port** of
  each servo packet. One socket per instance is normal.
- **System ids.** `--sysid` sets the MAVLink system id.
  `sim_vehicle.py --count N --auto-sysid` runs N instances.
- **Keeping several vehicles in step.** Nothing in ArduPilot keeps several
  SITLs on one world clock. Each blocks on its own reply, so the world must
  collect one frame from every vehicle before it steps.
- **ardupilot_gazebo's approach, for comparison.** Each model binds its own
  port, set by hand in SDF. It drains the socket and keeps only the newest
  servo packet. On a duplicate frame it resends the last state without
  stepping, and it replies to the captured sender address.

## 2. C++ SCRIMMAGE

### History

Sources: git history of `../scrimmage` and gtri/scrimmage, and ArduPilot pull
requests.

- **2013-2016:** Michael Day (Georgia Tech Research Institute, GTRI, earlier
  at NPS on UAV swarms) made 54 ArduPilot commits: rally points, geofence and
  TECS landing.
- **2017-12-04:** Day added SCRIMMAGE's ArduPilot plugin (e7a115c75). Daniel
  Magree followed with the JSBSim ArduPlane integration (2018-04-12) and
  hardware-in-the-loop support (2019-05-08).
- **2019-08-13:** ArduPilot merged
  [#11887](https://github.com/ArduPilot/ardupilot/pull/11887) by Anthony
  Velte (GTRI), which added `SIM_Scrimmage.cpp` with exactly the plugin's
  packet struct. SCRIMMAGE therefore had its own backend in ArduPilot, run with
  `-f scrimmage-plane` / `scrimmage-copter`.
  - At first ArduPilot started SCRIMMAGE itself, one SCRIMMAGE per vehicle.
  - One reviewer objected that "ArduPilot should [not be] starting the external
    simulator".
  - One tester's plane "seems to just spin on the Z axis".
- **2020-04-28:** [#13405](https://github.com/ArduPilot/ardupilot/pull/13405)
  (Arash Negahdar) stopped ArduPilot from launching SCRIMMAGE and allowed "a
  single scrimmage process to connect to multiple ArduPilot processes".
  `sim_vehicle.py` now writes a SCRIMMAGE mission from
  `Tools/autotest/template/scrimmage.xml.j2`: one entity per instance, with
  explicit ports 9002+10i and 9003+10i, dt = 0.001, real time
  (sim_vehicle.py:1913-1966).
- **2020-05-07:** Kevin DeMarco added `ardupilot_port_base` /
  `port_instance_multiplier` and `missions/arduplane_multiple.xml` to
  SCRIMMAGE. Those ports (5514/5515, 5524/5525) do not match SITL's defaults,
  and nothing documents the SITL command that pairs with them.
- **2020-05-20:** ArduPilot added the JSON backend.
- **2023-07-03:** Magree added `asynchronous_mode=false`, a synchronous option
  (#573). Asked whether asynchronous would stay the default, he said: "Yes,
  asynchronous was and still is the default." Before 2023 the plugin could only
  free-run.

#13405 includes real multi-instance launch code, but no posted test shows
several SCRIMMAGE vehicles, and we have not run it.

### The protocol

- **Packets.** SCRIMMAGE sends a raw host-endian struct:
  - `uint64 timestamp_us`
  - lat, lon, alt
  - heading
  - NED speeds
  - accelerations
  - body rates
  - roll, pitch, yaw
  - airspeed

  It receives `uint16 servos[16]` back.
- **Accelerations.** The ArduPilot side subtracts gravity itself
  (`accel_body = accel - R^T * g`, `SIM_Scrimmage.cpp`), so sending kinematic
  acceleration without gravity is correct.
- **Airspeed** is passed straight through. SCRIMMAGE computes it as
  `|vel + wind|`, which is right or wrong depending on its wind sign
  convention; not checked.

### Timing

On the ArduPilot side (`SIM_Scrimmage::update`), SITL sends servos, then
blocks until one state packet arrives. It advances its clock by the packet's
timestamps, so SITL time was always driven by SCRIMMAGE.

On the SCRIMMAGE side there is no thread; Asio receives are polled inside
`step_autonomy`.

- **Default:** SCRIMMAGE does not wait.
  - Each step sends state, drains the queue, and uses the newest servos.
  - Those servos can belong to any earlier state, so the delay depends on load.
  - Servos are 0 before the first packet arrives, which is outside the
    1000-2000 PWM range.
- **`asynchronous_mode=false` (since 2023):** it waits for one servo packet
  per step. That is full lockstep, but the receive has no timeout.
- **Sync signal:** the only one is `timestamp_us`. There is no frame counter.

### Ports and PWM mapping

- **Ports.** In the plugin, `from_port = base + multiplier * entity_id` and
  `to_port = from_port + 1`. The mission `sim_vehicle.py` generates ignores
  this and sets each entity's ports directly.
- **PWM mapping.** Each `servo_map` row is
  `[name channel in_min in_max out_min out_max coeff]`, scaled by `AxisScale`.
  The defaults are throttle on channel 2 (0..1), elevator on 1 (×-1), aileron
  on 0 and rudder on 3 (×-1). The shipped missions fly JSBSim through
  DirectController, not FixedWing6DOF.

### Why lockstep was not the default

It was half lockstep. ArduPilot's side always waited: `SIM_Scrimmage` blocks
for one state packet per step, and its clock follows SCRIMMAGE's timestamps.
Only SCRIMMAGE's side did not wait.

The only stated reason is backward compatibility: when synchronous mode
arrived in 2023, asynchronous stayed the default because it "was and still
is". Even then the protocol was not changed to make waiting safe: there is
still no frame counter and no timeout. The integration was being maintained,
not redesigned. Why the original 2017 plugin free-ran is not written down.

My judgment is that lockstep was fragile with that protocol:

- **A lost packet hangs everything.** Over UDP, with no frame counter and no
  resend, one lost packet leaves both sides waiting forever.
- **Startup order.** A waiting SCRIMMAGE hangs at t = 0 until SITL starts.
- **One sim, many agents.** A stalled SITL would freeze every entity.
- **The cost was small.** At dt = 0.001 in real time, SITL usually kept up.
  What was lost was repeatability, which mattered less for demos.

JSON's resend and frame counter remove the first reason, which is why
lockstep is now practical. They are retry machinery, not an exactly-once
protocol: a reply carries no frame number, so the sim still has to send exactly
one reply per frame (section 4).

### Current status

The integration is unmaintained on ArduPilot's side.

- ArduPilot archived the wiki page in August 2026. For multi-vehicle work it
  points to Webots, Gazebo or PteroSim instead.
- The backend is still in ArduPilot's source.
- The SCRIMMAGE plugin's last functional fix was in 2024. Style and logging
  changes followed (2025-12-19, 2026-03-30).
- Multi-vehicle support existed. The 2020 pull request has real multi-instance
  launch and mission-generation code.
- We have not validated that it still works, and its free-running default
  gives no synchronization guarantee.
- The hand-written `arduplane_multiple.xml` uses ports that no documented
  launcher matches, but that says nothing about the generated missions.

The archive warning is reason enough to use a maintained interface. What
scrimmage-rs would add is multi-vehicle lockstep with checks that it holds.

### Defects

- The rate counters are file-static, so all vehicles share them.
- There is no stale-link check.
- There is no endian handling (a TODO in the code).

## 3. UnrealVehicleDynamics

UVD works well in practice, but it is not a reference. It was audited here
against SIM_JSON and ardupilot_gazebo. `VC` means
`unreal/Plugins/UnrealVehicleDynamics/Source/UnrealVehicleDynamics/Private/UvdVehicleComponent.cpp`.

### How it works

- **SITL launch.** SITL runs with
  `--model JSON:<host> --speedup 1 --rate 120 --sim-port-in/--sim-port-out --sysid`
  (`ardupilot/session.py:132-155`).
- **Physics.** Chaos runs a fixed 120 Hz step.
- **Each physics tick** (`VC:1528-1536`):
  1. send the state from the last step;
  2. block for a new frame;
  3. apply it;
  4. step.

  This pairing is correct: SITL receives the state that results from its last
  frame, the same pairing as ardupilot_gazebo.
- **Frames** (`VC:793-816`):
  - a repeated frame gets the cached reply;
  - a gap is counted;
  - a counter going backwards is fatal;
  - 10 s of silence fails the vehicle.
- **Two vehicles** (`./run.py dual`):
  - Each runs in its own container with its own port and sysid.
  - Each has one blocking socket.
  - `run.py` refuses mismatched dt, origin or duration.
- **Release.**
  1. Each container's `session.py` waits for GPS and EKF, sets FBWA, arms and
     writes a ready file.
  2. `run.py` writes `start`.
  3. Each session sends a UDP `release`.
  4. A barrier puts both vehicles into play on the same step (`VC:588-610`).

  Before release, the plane flies its trim command.

### Audit findings

**Real bugs:**

- **Lag after a pause or stall.** This is traced from code, not observed. A
  pause or a stall over 1 s lets SITL's resends pile up in the socket. On
  resume, UVD answers each queued duplicate with the cached reply, and SITL
  takes every extra reply as a new frame, so it stays k frames stale. UVD
  still sees consecutive frame numbers, so its lockstep check passes.
  - Fix: reply exactly once per new frame and never answer duplicates (see
    section 4). Draining and answering the newest duplicate does not fix it,
    because a resend can already be queued behind the normal reply.
- **A bad channel mapping hangs silently** (`VC:811-813`). SITL keeps
  resending the same frame, so the 10 s timeout never fires.

**Questionable:**

- **120 Hz** is enough for ArduPlane, but it is not a whole multiple of the
  50 Hz loop.
- **Gravity constant.** `accel_body` is a finite difference, which is the
  right quantity and step. But it subtracts UVD's own gravity constant while
  Chaos applies Unreal's world gravity. Any mismatch gives a constant bias.
- **Varying start state.** Before release the plane flies on trim for however
  long boot and arming take in wall time, so the state at release varies from
  run to run.
- **Same step for both vehicles** relies on Chaos calling each vehicle's
  physics tick one after another. That is general Unreal behaviour, not
  enforced in UVD, and it makes the blocking waits add up in series.
  `dual_report.py` would catch a skipped tick, but not the stale-frame lag
  above.
- **The backwards-frame check is fatal.** That is fine for scripted runs, but
  it prevents restarting SITL.

**Worth copying:**

- the per-step exchange log (frame, step, sim time, PWM);
- the explicit pass/fail report;
- one solver owning each vehicle's state.

## 4. Recommendation for scrimmage-rs

Use the JSON backend. Don't copy the C++ protocol: it needs a
SCRIMMAGE-specific ArduPilot backend that ArduPilot has archived, and its
default mode isn't lockstep. This section includes corrections from a second
review of this document.

### Aircraft: the Rascal, not the default FixedWing6DOF

FixedWing6DOF's default parameters are the A-4D Skyhawk jet, which cites
Nelson's *Flight Stability and Automatic Control*, 2nd ed., p. 405
(`FixedWing6DOF-A-4D.xml`). C++ also ships `FixedWing6DOF-Rascal.xml`, a 6 kg
RC plane that Daniel Magree added on 2018-04-24.

No C++ mission ever used the Rascal file; the ArduPilot missions flew JSBSim's
Rascal instead. scrimmage-rs now has it as `missions/fixed-wing-6dof-rascal.xml`,
and it is in the C++ comparison.

| | Rascal | Default (A-4D) |
| --- | --- | --- |
| Mass | 5.91 kg | 7,973 kg |
| Wingspan / area | 2.70 m / 0.98 m² | 8.38 m / 24.2 m² |
| Thrust | 0 to 25 N | -36,000 to +36,000 N, so zero throttle is full reverse thrust |
| Surface limits | ±1, "normalized" | radians: ±0.52 elevator and aileron, ±0.26 rudder |
| `C_MQ` (pitch damping) | -12 | -3.6 |

Use the Rascal. It is C++'s simplified Rascal, derived from the JSBSim
configuration that ArduPlane's SITL uses. Treat ArduPlane's supplied tuning as
a starting configuration to test, not as known to fit. The jet would need a
full retune.

**The values are copied from C++ as-is**, including the 25 N thrust and 2.70 m
span, whose sources are unclear. The one exception is `C_D_delta_elevator`. C++
never reads it because of a key typo (it reads `C_D_delta_elevator_`), and
scrimmage-rs rejects the plain name. Leaving it out makes both use the 0.01
default.

**Relation to JSBSim.** The source is ArduPilot's
`Tools/autotest/aircraft/Rascal/Rascal.xml`. Mass, inertias, wing area, chord
and the stability and control derivatives were copied from it. Where JSBSim uses
a table, the file takes the table's slope or its zero-angle value.

The dynamics are still not JSBSim's. JSBSim also has:

- a stall table;
- drag tables for angle of attack and sideslip;
- Mach-dependent control derivatives;
- fuel mass on top of the 5.9 kg empty mass;
- a propeller and engine model;
- landing gear.

**Surface units: the one real error.** The coefficients are per radian.

- Until commit `2cab6d119` (Magree, 2018-07-23, "use normalized actuators for
  rascal model"), the C++ file limited the surfaces in radians: elevator ±0.30,
  aileron and rudder ±0.35.
- That commit changed the limits to ±1 "normalized" without rescaling the
  coefficients, so the largest allowed deflection is about 3× too big. Small
  commands are unaffected. In level flight AircraftPIDController uses about
  0.003 rad of elevator, but an upset drives it to the ±1 limit.
- The comparison mission keeps ±1, to match C++.
- **The SITL mission should use C++'s original radian limits, and the bridge
  should map full-throw PWM onto them.**

**Model limits.** These apply to FixedWing6DOF in general.

- **No stall behavior.** Lift keeps rising with angle of attack, whereas JSBSim's
  lift peaks at CL 1.4. That is a level-flight stall near 8.3 m/s, higher in
  turns. Use the model for moderate-angle flight around 20 m/s.
- **Drag** rises with lift, through the induced-drag term. What it lacks is
  JSBSim's extra angle-of-attack drag table (`C_D_alpha` is 0).
- **The `alpha_dot` terms multiply zero,** because C++ never computes alpha_dot.
- **Aspect ratio is span ÷ chord,** not span² ÷ area.
- **Wind is added to velocity,** not subtracted, so `wind_E=+10` on a plane
  flying east is a headwind.
- **Forces are computed once per step,** then held through the RK4 stages.

**Exploratory stress runs.** These are not verification. They were Rust only,
used AircraftPIDController for 60 s, and started at 300 m. Unlike the mission,
they used elevator drag 0.03. The controller's gains were hand-tuned in 2018 on
the A-4D at 220 m/s.

- From 12 to 35 m/s, level flight holds altitude within about 1 m.
- A 60°/30° roll and pitch upset recovers, losing about 1 m. With C++'s
  original radian limits it loses about 2 m.
- An inverted start recovers, losing 15 m. With radian limits it loses 36 m.
- Hands off, it settles into a glide of about 8:1.
- After an upset, dt = 0.005 ends 5 cm from dt = 0.001. Use dt = 0.005 or
  0.0025, which are also whole multiples of Plane's 50 Hz loop.
- The altitude loop oscillates steadily when its gains are raised 4×.
- The controller holds ground speed, not airspeed.

None of this shows that the recoveries are physically accurate, especially
without stall.

### Each tick

SCRIMMAGE's phases already give multi-vehicle lockstep: every phase waits
for all entities to finish, so no tick starts until every vehicle has
answered. N vehicles share one clock without extra threads.

The plan follows the existing plugin contexts:

- **A small SITL sensor.** After motion, it reads the entity's state and
  publishes one sample on LocalNetwork, as C++ did with
  `RigidBody6DOFStateSensor`. The sample holds:
  - NED position and velocity;
  - the body-to-NED attitude;
  - the body FRD gyro;
  - specific force (a finite difference of world velocity over dt, rotated to
    body and minus gravity);
  - the timestamp.

  `KinematicState` has no acceleration and stores angular velocity in the
  world frame, so the sensor does these conversions. Autonomy then sees
  readings, not truth, and no Rust estimator is needed, because SITL has its
  own EKF.
- **A bridge autonomy.** On the next tick it:
  1. sends the latest sample as the one reply to the frame it applied last
     tick (skipped on the first tick);
  2. waits for the next new frame;
  3. converts the PWM to the four FixedWing6DOF inputs through a named
     calibration.

  The chain stays empty, so there is no need to port `DirectController`.

### The exchange rule: exactly one reply per new frame

A reply carries no frame number, so a second reply to the same frame makes
SITL skip ahead for the rest of the run. The rule is:

- **Reply once to each new frame, and never answer a duplicate.**
- **A duplicate** (a SITL resend after about 1 s) is dropped. Once our reply
  arrives, SITL moves on.
- **A frame gap or a lower frame count** fails the run with a clear error. In
  strict lockstep, neither can happen unless something is wrong.
- **A lost reply** leaves SITL resending forever. The bridge's timeout then
  fails the run. UDP over loopback or Docker's bridge almost never drops
  packets, so a clear failure is better than retry logic.

The state needed is one stored number: the last frame answered.

Draining the socket and answering the newest duplicate does not work. A
resend can already sit behind the normal reply. The reply goes out, the
queued resend is then answered too, and SITL takes both.

### Workers and speed

The scheduler's default is one worker, so the vehicles' blocking waits run one
after another. More workers than vehicles would let them overlap. The phase
barrier keeps the world in step either way; only speed changes.

Measure two instances first. If round trips dominate, split the exchange:
send every vehicle's reply, then collect every vehicle's next frame. All the
SITLs then compute at the same time. Add per-vehicle threads or async only if
a measurement demands it.

### Startup and release

A shared clock does not mean every autopilot is armed and ready. The simplest
collective rule is a **fixed release time**, after which every aircraft flies.
A launcher script per container arms the aircraft and sets its mode over
MAVLink before then, keeping MAVLink out of the Rust sim. If any aircraft is
not armed and in its mode by the release time, the run fails. Per-aircraft
triggers, such as a servo channel crossing a threshold, would release aircraft
on different ticks.

**Holding uses FixedWing6DOF's existing launcher** (`use_launcher`, inherited
from C++), so no new freeze mechanism is needed:

- **Before `launch_time`,** every force, gravity included, is zero, so the
  aircraft keeps its starting velocity.
  - Started at 0 m/s it sits still, like an aircraft on the ground.
  - Started at cruise speed it moves straight at constant speed, like steady
    level flight.

  Either way, the bridge's sample is self-consistent: specific force reads
  (0, 0, -g), and velocity matches the motion.
- **At `launch_time`,** it accelerates along the nose at `launch_accel` until
  it reaches `launch_speed`, then flies freely.

That gives three starts:

| Start | Settings | Notes |
| --- | --- | --- |
| Flying immediately | no launcher, start at about 20 m/s | Simplest. ArduPlane is not armed for its first 10-30 s of sim time, so the aircraft glides with neutral surfaces and idle throttle meanwhile; start high. |
| Air start at the release time | launcher, start at about 20 m/s, `launch_speed` 0, small `launch_accel` | Straight, constant-speed flight until release. With `launch_speed` 0 the launcher still pushes for one tick at release (`launch_accel × dt`), so keep `launch_accel` small. |
| Catapult launch | launcher, start at 0 m/s, `launch_speed` about 20 | Sits still until release, then is catapulted. ArduPlane supports catapult takeoff. It can start at ground level on the flat-ground model. |

A wheeled runway takeoff needs terrain ground contact (see the roadmap).

**End of the run.** Replies go out on the tick after a frame is applied, so the
last frame never gets a reply: the run ends first. SITL then sits waiting, so
the launcher must stop the containers when SCRIMMAGE exits, whether it succeeded
or failed. It must never wait for SITL to finish on its own.

**Both plugins run every tick.** `SitlState`'s acceleration divides one tick's
velocity change by one tick, and the bridge must answer every frame. Each one
fails the run if a `loop_rate` makes it skip a tick.

### Settings

- **Processes:** one SITL process and one socket per aircraft, with distinct
  `--sysid` values and separate parameter and log directories. An external
  launcher handles startup and cleanup.
- **Ports:** the socket for entity index i is on `9002 + 10*i`. Each SITL runs
  `--model JSON:<host IPv4> --sim-port-out <port> --home <shared home>`.
- **Rate:** dt of 1/200 or 1/400 s, a whole multiple of Plane's 50 Hz loop.
  Set `--rate` to match.
- **Timestamps:** `timestamp = t`, starting at 0. Every reply starts and ends
  with a newline.
- **Speed:** a large `--speedup` for headless runs. Never set `no_time_sync`
  or `no_lockstep`.
- **Timeouts:** longer than SITL's 1 s resend, for example 10 s per step, and
  longer for the first frame while SITL boots.
- **Build:** an optional crate outside the default build, per AGENTS.md.

### Docker and launch: what to reuse from UVD

UVD's ArduPlane container setup was audited at `fb95941`, in `ardupilot/Dockerfile`,
`entrypoint.sh`, `session.py`, `uvd.parm` and `run.py`. Most of it carries over
to the Rascal. Here is what to copy, and what to change.

**Copy as-is:**

- **Image.** A two-stage Dockerfile that builds `arduplane` for the SITL board
  at a pinned ArduPilot commit. The final image holds only the binary and a
  Python virtual environment with pymavlink. One image serves every aircraft.
- **One container per aircraft.** Each container gets its own run directory
  (bind-mounted), its own `--sim-port-out` and its own `--sysid`. Keep `-I 0`
  inside each container. Separate directories keep `eeprom.bin`, logs and
  parameters from leaking between aircraft and between runs.
- **Resolve the host address to a numeric IPv4 address before starting SITL.**
  `entrypoint.sh` runs `getent ahostsv4 host.docker.internal`, then starts
  `--model JSON:<ip>`. On Linux, `run.py` adds
  `--add-host host.docker.internal:host-gateway`.
- **Run the `arduplane` binary directly** from a small Python session script,
  not through `sim_vehicle.py`, with `--serial0 udpclient:127.0.0.1:14550`.
  MAVLink stays inside the container, and the script talks to it with
  pymavlink.
- **The readiness check before arming:** a GPS fix of at least 3, and EKF
  flags `1|2|4|16|32` (attitude, horizontal and vertical velocity, absolute
  horizontal and vertical position).
- **The session script keeps reading MAVLink until SITL exits.** Above
  `--speedup 1`, SITL stalls when its serial0 output queue fills, so this
  matters more for us than for UVD.
- **Host-side cleanup:** stop each container with a timeout in a `finally`
  block, and fail the run if any container exits early.

**Do differently:**

- **Load ArduPilot's own plane defaults.** UVD passes only `uvd.parm` (servo
  functions and trims for its airframe), then force-arms with the magic code
  21196, which skips arming checks.
  - ArduPilot's SITL frames for SCRIMMAGE and JSBSim load
    `Tools/autotest/models/plane.parm`. It holds tuned attitude loops, airspeed
    limits (cruise 22, minimum 10, maximum 30 m/s, `ARSPD_USE 1`) and small
    accelerometer calibration offsets, which "we need so INS is recognised as
    being calibrated".
  - Start from `plane.parm`, arm normally, and treat a failed arming check as
    a real error rather than forcing past it.
  - Consider raising `AIRSPEED_MIN` from 10 to about 15, because FixedWing6DOF
    has no stall.
- **Servo directions: decide in one place.**
  - ArduPilot's `jsbsim` frame (the JSBSim Rascal) also loads
    `plane-reverse-servo-pitch-yaw.parm` (`SERVO2_REVERSED 1`,
    `SERVO4_REVERSED 1`), plus the matching RC reversals.
  - C++'s `servo_map` applies the same flip on its side (elevator and rudder
    ×-1).
  - FixedWing6DOF's sign convention decides which is needed. Set it either in
    the ArduPilot parameters or in the bridge's calibration, never both.
  - The "control directions" check settles it: command positive aileron and
    elevator in FBWA, and confirm the aircraft rolls right and pitches up.
- **Speed.** UVD hard-codes `--speedup 1`, because Unreal runs in real time.
  Headless SCRIMMAGE should use a large `--speedup`.
- **Drop `--sim-port-in`.** The JSON backend ignores it. UVD passes its
  release-control port there, which is harmless but misleading.
- **Release.** UVD uses:
  - ready, start and done files in the run directory;
  - a UDP `release` message;
  - a barrier in Unreal;
  - a trim flight before release, timed in wall-clock time.

  With a fixed sim-time release, the session script only has to reach armed,
  in the right mode, before the release time; the simulator holds the aircraft
  until then. No files and no UDP control socket are needed.
- **Flight mode.** UVD's demo flies FBWA, with RC-override roll commands and a
  fixed throttle. For the first Rascal demo, FBWA or CRUISE with neutral RC is
  the simplest steady flight. It needs no mission upload. AUTO needs a waypoint
  mission uploaded over MAVLink first.
- **Send `airspeed` in the JSON reply.** With `ARSPD_USE 1`, ArduPlane flies on
  airspeed. SITL's fallback assumes zero wind, and FixedWing6DOF has wind, which
  it adds to velocity. So compute airspeed from the model's own air velocity.
- **Home.** SITL places the aircraft at `--home` plus the NED position the
  bridge sends, so local (0, 0, 0) must mean `--home`. scrimmage-rs XML missions
  have only a local frame; only terrain YAML missions define a lat/lon origin.
  So the launcher chooses one home (the terrain origin when there is one) and
  passes the same value to every SITL.
- **Start every run in a fresh directory.** UVD deletes `eeprom.bin` only after
  a completed run. Stored parameters carry over otherwise, and fresh run
  directories also follow this repo's "never overwrite a run" rule.

### Checks before calling it working

A successful flight does not show synchronization. The run must show:

1. two simultaneous instances staying on the same step for the whole run;
2. correct control directions, meaning each PWM channel moves the right
   surface the right way, and correct specific force (reads (0, 0, -g) at
   rest);
3. a pause longer than the 1 s resend, after which no frames are skipped;
4. a bounded, clear failure when one SITL is killed.

First demo: two Rascals, zero wind, air start, and no terrain takeoff, which
needs terrain ground contact.

### Still open

- **Determinism.** Output also depends on SITL, so ArduPilot runs lose
  SCRIMMAGE's usual promise of identical output across runs. Document this.
- **Docker on macOS.** Each step costs one UDP round trip per vehicle through
  Docker's VM. How many SITLs can run faster than real time at 200-400 Hz has
  to be measured.
