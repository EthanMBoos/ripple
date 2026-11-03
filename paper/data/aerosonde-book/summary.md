# The Aerosonde against the book's equations

Passed: True

Numbers from mavsim_public 10363e0e, `mavsim_python/parameters/aerosonde_parameters.py` (SHA-256 edeb72d99766...).

## Held states

432 samples; the Rust test's largest errors acceleration 3.5e-15, angular_acceleration 1.7e-15, force 1.6e-15, moment 4.5e-17, propeller_speed 4.4e-17, thrust 4.2e-16, torque 1.1e-15, velocity_ned 8.1e-16; fixture difference 0.0.

## Longitudinal trim for level flight at 25 m/s

alpha 2.848 deg, elevator -7.100 deg, throttle 0.7735 (thrust 10.330 N, propeller 514.4 rad/s, torque 0.6340 N m), ailerons 0.3393 deg, rudder -0.0541 deg; lateral residual: side force 0.0577 N.

Mission equals the trim: True.

## The flight, 10 s open loop

- trim: altitude drift 2.5e-05 m (limit 0.001), airspeed drift 6.1e-06 m/s (limit 0.001), lateral 0.3130 m, roll 0.0379 deg, pitch 0.0000 deg, yaw 0.1686 deg
- at a quarter step: position differs by 2.83e-12 m, velocity by 1.39e-11 m/s
