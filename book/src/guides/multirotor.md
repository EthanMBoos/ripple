# Flying a multirotor

`Multirotor` is a rigid-body multirotor model with optional motor and rotor
effects ported from RotorS, and two built-in vehicles: a Crazyflie 2.0
measured by Förster, and RotorS's Firefly, a 1.5 kg hexacopter.
`MultirotorVelocityController` flies it from the same velocity command point
agents take, so an autonomy written for `SingleIntegrator` (or a learned policy
with velocity actions) can fly a quad unchanged.

## A Crazyflie

Give the motion model and the controller the same `airframe`, and switch on
the effects you want to fly with (here Förster's motor lag and rotor drag;
see [Optional effects](#optional-effects)):

```yaml
entities:
  cf:
    position_m: [0, 0, 1]
    autonomy:
      ConstantVelocity:
        velocity_mps: [1, 0, 0]
    controller:
      MultirotorVelocityController:
        airframe: crazyflie2
    motion_model:
      Multirotor:
        airframe: crazyflie2
        motor_time_constant_up_s: 0.0647
        motor_time_constant_down_s: 0.0647
        rotor_drag_coefficient: 9.1785e-7
        rotor_drag_coefficient_axial: 10.311e-7
```

Leave the four effect lines out and the same Crazyflie flies with instant
motors and no rotor drag.

Run the controller fast: its attitude and rate loops run at every motion
substep, so `dt_s: 0.01` with `motion_multiplier: 5` gives 500 Hz, the
Crazyflie firmware's rate. [`missions/multirotor-velocity.yaml`] flies
Crazyflies through velocity steps and an upset.

[`missions/multirotor-velocity.yaml`]: https://github.com/EthanMBoos/ripple/blob/main/missions/multirotor-velocity.yaml

## The vehicle

A mission either names a built-in vehicle with `airframe: crazyflie2` or
`airframe: firefly`, or gives every vehicle number itself: `mass_kg`, `inertia_kgm2`,
`thrust_coefficient` (c_T), `torque_coefficient` (c_Q), `min_rotor_speed_radps`,
`max_rotor_speed_radps`, and `rotors` (`gravity_mps2` defaults to 9.81).
Inertia is three rows; each rotor gives its spin direction, hub position in
body forward/left/up axes, and, if tilted, its tilt:

```yaml
inertia_kgm2: [[0.0122, 0, 0], [0, 0.0122, 0], [0, 0, 0.0244]]
rotors:
  - {direction: ccw, position_flu_m: [0, -0.175, 0]}
  - {direction: cw, position_flu_m: [0.175, 0, 0], tilt_deg: [0, 5, 0]}
```

Rotor `i` reads the motor command on port `motor_i`. There is no default vehicle, so no
mission flies unsourced numbers by accident. With `airframe` set, any of
those keys overrides one value. The airframe never switches on an optional
effect, and `body_drag_coefficient` is zero unless the mission sets it.

| `crazyflie2` | Value | Source (Förster 2015, ETH thesis) |
| --- | --- | --- |
| Mass | 0.028 kg | weighed (Table 2.2) |
| Inertia | full 3x3 matrix | measured, eq. 2.18 |
| Layout | "+", arms 0.046 m | 92 mm between opposite motors (Table 2.2) |
| c_T | 1.9005e-8 N/(rad/s)² | eqs. 3.3 and 3.4, exact at hover |
| c_Q | 0.005964552 c_T | eq. 3.5 |
| Motor speed | 0 to 2899 rad/s | top speed gives eq. 3.3's largest thrust |

Förster's body axes run along the motor arms, so this Crazyflie is a "+"
quad whose forward is an arm, 45 degrees from the real vehicle's front; that
keeps the inertia matrix as published. The thesis does not say which diagonal
pair spins clockwise; mirroring it flips only the sign of yaw. The derived
values (c_T, c_Q, the top speed) are computed from the thesis's fits in
`airframe.rs`, and its unit tests reproduce them. The thrust model is within
10.4% of the measured fit from command 8000 up and exact at hover.

| `firefly` | Value | Source (RotorS `firefly.xacro`, commit `cd813b7`) |
| --- | --- | --- |
| Mass | 1.545 kg | body 1.5, six rotors of 0.005, IMU 0.015 (see [below](#the-firefly-choices-and-why)) |
| Inertia | 0.0347563, 0.0458929, 0.0977 kg m² (diagonal) | the body's, as published |
| Layout | hexacopter, arms 0.215 m, first arm 30° left of forward; hubs 0.037 m up | `arm_length`, `rotor_offset_top` |
| Spin | ccw, cw, ccw, cw, ccw, cw, counterclockwise from front-left | each rotor's `direction` |
| c_T | 8.54858e-6 N/(rad/s)² | `motor_constant` |
| c_Q | 0.016 c_T | `moment_constant` |
| Motor speed | 0 to 838 rad/s | `max_rot_velocity` |

It hovers at 543.6 rad/s with a thrust-to-weight ratio of 2.38. These are
RotorS's model numbers for the AscTec Firefly, not a published
identification of the vehicle, so treat its results as statements about
RotorS's Firefly.

## Optional effects

These are off unless the mission sets them, like FixedWing6DOF's stall tables
and landing gear, so each can be turned on alone in an experiment. The
Crazyflie values are Förster's; the Firefly's are from its RotorS model file.

| Key | Effect (from RotorS unless noted) | Crazyflie value | Firefly value |
| --- | --- | --- | --- |
| `motor_time_constant_up_s`, `motor_time_constant_down_s` | first-order motor lag, s | 0.0647 (eq. 3.16: -T/ln(a) at 500 Hz) | 0.0125 up, 0.025 down |
| `rotor_drag_coefficient` | in-plane drag, -\|w\| x this x the hub's edgewise airspeed, kg/rad | 9.1785e-7 (eq. 4.8) | 8.06428e-5 |
| `rotor_drag_coefficient_axial` | the same along the rotor axis (Förster, not RotorS), kg/rad | 10.311e-7 (eq. 4.8) | none in RotorS; leave 0 |
| `rolling_moment_coefficient` | rolling moment, kg m/rad | not measured; leave 0 | 1e-6 |

Förster identified the lag from motor command to *thrust*; Ripple, like
RotorS, lags the *shaft speed*, and thrust goes with speed squared. The two
have the same time constant for small changes about a speed, as near hover,
but differ for large steps, so this reuse of his pole is a near-hover
approximation. His fit also did not remove radio or load cell latency, so the
real lag may be shorter. Crazyflow's independently
identified `cf2x_L250` spins up more slowly still (about 0.14 s). Treat the lag
as a parameter worth sweeping. The controller's Crazyflie gains are tuned for
Förster's lag; with a much longer one, retune them.

## What the model includes

Per rotor: thrust c_T w² along the rotor's (possibly tilted) axis at its hub,
and a reaction torque c_Q w² about that axis. With RotorS's effects on: the
motor's first-order lag (separate spin-up and spin-down constants), rotor drag
proportional to rotor speed and the hub's edgewise velocity (Martin and Salaün,
2010), its axial counterpart from Förster, and the rolling moment. The body adds
weight, an optional quadratic drag (`body_drag_coefficient`, c_D), and rigid-body rotation, integrated
with RK4 that recomputes every load, and the motor speed, at each stage.

Not modeled, each written as a named zero in `rotor.rs`: ground effect,
interference between rotors and the body, thrust change with climb or forward
speed, rotor gyroscopic and spin-up torques, wind, battery sag, and contact.
Use `GroundCollision` to remove a vehicle that reaches the ground; there is
no landing.

## The controller

`MultirotorVelocityController` is a four-loop cascade written in SI units, so
its gains are bandwidths (1/s):

1. **Velocity**, world frame: `velocity_kp` and `velocity_ki`, with the
   integral's contribution held within `velocity_integral_limit_mps2`. The
   result is limited to `max_tilt_deg` and `max_vertical_acceleration_mps2`,
   then turned into a thrust vector.
2. **Attitude**: body z along the thrust vector, heading held at the first
   belief's yaw; `attitude_kp` on the geometric attitude error.
3. **Rate**: `body_rate_kp`, with the airframe's inertia and gyroscopic term.
4. **Mixing**: the pseudo-inverse of the airframe's allocation gives each
   rotor's thrust, clamped to the motor range, then its speed.

It reads belief only, so noisy sensing reaches it like any controller. It
does not know about motor lag, and holding a position is the autonomy's job.
The default gains depend on the `airframe`, each tuned by step response:

| Gain | `crazyflie2` (and a vehicle given in full) | `firefly` |
| --- | --- | --- |
| `velocity_kp`, 1/s | 1.2 | 3.0 |
| `velocity_ki`, 1/s² | 0.36 | 0.6 |
| `attitude_kp`, 1/s | 1.7 | 5.8 |
| `body_rate_kp`, 1/s | 5.1 | 13.0 |
| At 1 m/s | 1 s rise, 20% overshoot, settled in 3.6 s | 0.4 s rise, 7% overshoot, settled in 1.0 s |

The Crazyflie's are held back by Förster's 65 ms lag, which limits how fast
the inner loops can be without derivative action. The Firefly's motors are
five times quicker, and its gains are adapted from RotorS's Lee gains (see
below). Retune them
for another vehicle.

To study model error, give the controller and the motion model different
values: for example, a heavier vehicle than the controller believes.

## How it is checked

| Check | Against | Result |
| --- | --- | --- |
| `rotors-motor` | RotorS's own motor-plugin source, compiled in Docker | 480 samples within 4e-15 N |
| `crazyflow-crazyflie` | Crazyflow's first-principles dynamics, same constants and commands | 3e-12 m over 3 s with drag off; with drag on they differ by design (Ripple's drag acts at each hub) |
| `multirotor-control` | the commands; each vehicle's rotor drag | settles; steady tilt at 1 m/s matches the drag: Crazyflie 1.456 against 1.455 degrees, Firefly 0.994 against 0.994 |
| unit tests in `airframe.rs`, `rotor.rs`, `multirotor.rs` | Förster's fits, the Firefly's hover and margin, closed-form cases | |

Matching a source is not matching a vehicle: nothing here has been compared
with recorded flights yet. The next step is replaying NanoBench's recorded
Crazyflie motor commands (172 flights with Vicon truth); see the fidelity
notes at the top of `multirotor.rs`.

## The Firefly: choices and why

The Firefly was added so the delivery study's multirotors can fly a vehicle
of a useful size rather than a point model
(`crates/delivery/missions/delivery-firefly.yaml`, the mirror of
`delivery-6dof.yaml`). Copying it raised these choices; each is recorded
here so it can be revisited.

1. **Mass: 1.545 kg, the model's own parts.** The model file sums to 1.545
   kg (body 1.5, six rotors of 0.005, the 0.015 kg IMU its base file
   mounts). RotorS's controller files (`firefly.yaml`, `parameters.h`) say
   1.56779 kg, 1.5% more, and nothing in the model accounts for the
   difference. Ripple flies what RotorS simulates.
2. **Inertia: the body's, as published.** The six rotors on their arms would
   add about 2% about x and y. RotorS's own controller uses the body's
   numbers too, so they are copied unchanged rather than recomputed.
3. **Controller gains: tuned per airframe, starting from RotorS's.** RotorS
   flies the Firefly with a Lee controller whose gains, in this controller's
   form, are velocity 3.0 /s, attitude 5.8 /s, and rate 15 /s about x and 11
   /s about y. Porting them exactly would need a rate gain per axis; instead
   the airframe picks its own defaults, with the rate gain between the two.
   Lee has no integral, so the integral gain was chosen by step response:

   | `velocity_kp`, `velocity_ki` | Overshoot at 1 m/s | Overshoot at (2, 1) m/s | Settled at 1 m/s |
   | --- | --- | --- | --- |
   | 3.0, 2.25 (the Crazyflie's ratio) | 23% | 24% | 2.1 s |
   | 3.0, 1.2 | 13% | 15% | 1.3 s |
   | **3.0, 0.6 (chosen)** | **7%** | **6%** | **1.0 s** |
   | 2.4, 0.6 | 3% | 4% | 0.7 s, but a 0.5 m/s climb takes 3.7 s |

4. **Landing: hovering on the pad.** `Multirotor` has no ground contact. A
   delivery "landing" is the vehicle holding the pad's height within the
   courier's arrival radius, as the point model does.
5. **Braking: `approach_per_s` on the courier.** The courier commanded full
   speed until one tick short of a target, which a point model follows
   exactly; the Firefly needs 18 m to stop from 12 m/s at its 4 m/s² vertical
   limit and sank 17.7 m below the depot pad. The Firefly mission sets the
   courier's `approach_per_s: 0.3`, speed in proportion to distance, which
   asks at most 3.6 m/s² of braking. With it the Fireflies stop on every pad
   (0.0 m at the depot, 40.0 m on rooftops). Left at 0, the default, the
   point-model mission is unchanged frame for frame.
6. **Step size: 50 substeps of the 0.1 s tick.** That is 500 Hz, the rate
   the control check shows converged (at 200 Hz two step responses ended
   5.5-7.9 mm from a 2000 Hz run, over its 5 mm limit). Substeps are set for
   the whole run, so the fixed-wing aircraft also integrate at 500 Hz in this
   mission; with a stand-in planner on seed 4 the two missions' scores were
   identical.
7. **The RotorS motor check needs no Firefly case.** Its `rotors-defaults`
   case already uses RotorS's default motor plugin values, which are the
   Firefly's (c_T, moment constant, both time constants, rolling moment);
   only the rotor drag differs, 8.06e-5 against 1.0e-4, and the drag is
   linear in it.
8. **Step-size convergence: the cruise is reported, not held to 5 mm.** The
   control check holds every step-response vehicle within 5 mm of its
   20-substep run (the Firefly's are within 2.2 mm). The 12 m/s cruise flies
   120 m, twelve times farther, and ends 8.8 mm away (0.007% of its path,
   shrinking threefold when the step halves), so it is reported beside the
   check rather than judged by the same absolute limit.
