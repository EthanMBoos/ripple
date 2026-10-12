# Flying a multirotor

`Multirotor` is a rigid-body quadrotor model with optional motor and rotor
effects ported from RotorS and a built-in Crazyflie 2.0 measured by Förster.
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
        velocity: [1, 0, 0]
    controller:
      MultirotorVelocityController:
        airframe: crazyflie2
    motion_model:
      Multirotor:
        airframe: crazyflie2
        motor_time_constant_up: 0.0647
        motor_time_constant_down: 0.0647
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

A mission either names the built-in measured vehicle with `airframe:
crazyflie2`, or gives every vehicle number itself: `mass`, `inertia_matrix`,
`c_T`, `c_Q`, `omega_min`, `omega_max`, and `rotor_config`
(`gravity_magnitude` defaults to 9.81). There is no default vehicle, so no
mission flies unsourced numbers by accident. With `airframe` set, any of
those keys overrides one value. The airframe never switches on an optional
effect, and body drag `c_D` is zero unless the mission sets it.

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

## Optional effects

These are off unless the mission sets them, like FixedWing6DOF's stall tables
and landing gear, so each can be turned on alone in an experiment. The
Crazyflie values are Förster's.

| Key | Effect (from RotorS unless noted) | Crazyflie value |
| --- | --- | --- |
| `motor_time_constant_up`, `motor_time_constant_down` | first-order motor lag, s | 0.0647 (eq. 3.16: -T/ln(a) at 500 Hz) |
| `rotor_drag_coefficient` | in-plane drag, -\|w\| x this x the hub's edgewise airspeed, kg/rad | 9.1785e-7 (eq. 4.8) |
| `rotor_drag_coefficient_axial` | the same along the rotor axis (Förster, not RotorS), kg/rad | 10.311e-7 (eq. 4.8) |
| `rolling_moment_coefficient` | rolling moment, kg m/rad | not measured; leave 0 |

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
weight, an optional quadratic drag (`c_D`), and rigid-body rotation, integrated
with RK4 that recomputes every load, and the motor speed, at each stage.

Not modeled, each written as a named zero in `rotor.rs`: ground effect,
interference between rotors and the body, thrust change with climb or forward
speed, rotor gyroscopic and spin-up torques, wind, battery sag, and contact.
Use `GroundCollision` to remove a vehicle that reaches the ground; there is
no landing.

## The controller

`MultirotorVelocityController` is a four-loop cascade written in SI units, so
its gains are bandwidths (1/s):

1. **Velocity**, world frame: `velocity_p` and `velocity_i`, with the
   integral's contribution held within `velocity_integral_limit` (m/s²). The
   result is limited to `max_tilt` (degrees) and
   `max_vertical_acceleration` (m/s²), then turned into a thrust vector.
2. **Attitude**: body z along the thrust vector, heading held at the first
   belief's yaw; `attitude_p` on the geometric attitude error.
3. **Rate**: `rate_p`, with the airframe's inertia and gyroscopic term.
4. **Mixing**: the pseudo-inverse of the airframe's allocation gives each
   rotor's thrust, clamped to the motor range, then its speed.

It reads belief only, so noisy sensing reaches it like any controller. It
does not know about motor lag, and holding a position is the autonomy's job.
The default gains are tuned for the Crazyflie with Förster's 65 ms lag, which
limits how fast the inner loops can be without derivative action: about 1 s
rise, 20% overshoot, settled in 4 s at 1 m/s. Retune them for another
vehicle.

To study model error, give the controller and the motion model different
values: for example, a heavier vehicle than the controller believes.

## How it is checked

| Check | Against | Result |
| --- | --- | --- |
| `rotors-motor` | RotorS's own motor-plugin source, compiled in Docker | 480 samples within 4e-15 N |
| `crazyflow-crazyflie` | Crazyflow's first-principles dynamics, same constants and commands | 3e-12 m over 3 s with drag off; with drag on they differ by design (Ripple's drag acts at each hub) |
| `multirotor-control` | the commands; Förster's drag | settles; the Crazyflie's steady tilt at 1 m/s is Förster's (1.456 against 1.455 degrees) |
| unit tests in `airframe.rs`, `rotor.rs`, `multirotor.rs` | Förster's fits, closed-form cases | |

Matching a source is not matching a vehicle: nothing here has been compared
with recorded flights yet. The next step is replaying NanoBench's recorded
Crazyflie motor commands (172 flights with Vicon truth); see the fidelity
notes at the top of `multirotor.rs`.
