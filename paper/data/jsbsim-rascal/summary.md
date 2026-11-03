# FixedWing6DOF's Rascal against ArduPilot's JSBSim Rascal

JSBSim 1.3.3 [GitHub build 2002/commit 66f7fafc07fe13e7ede9db20873030fed2881713] Oct 16 2026 00:36:17 (`jsbsim==1.3.3`) flying ArduPilot's Rascal files at commit `5628b8d2305d`. Passed: the loads agree to rounding on the grid, the samples equal the fixture, and every glide in JSBSim's elevator-drag form stays within 0.2 m.

## Loads on the grid (378 samples)

The `rascal` airframe in JSBSim's elevator-drag form: largest force residual 8.54e-16 of q S, moment 3.27e-17 of q S b (the test's limit is 1.00e-12). As flown, with the elevator drag from the deflection's size: force 4.00e-02 of q S, moment 1.29e-03 of q S b.

| Term beyond the textbook equations | Largest size, of q S (forces) or q S b (moments) |
| --- | --- |
| alpha_dot | 2.94e-02 |
| constant_yaw | 7.00e-04 |
| mach | 1.00e-03 |
| reference_point | 1.24e-02 |
| sideslip_drag | 2.88e-02 |
| signed_elevator_drag | 4.00e-02 |
| wind_to_body | 2.79e-02 |

Fresh samples against the committed fixture: largest relative difference 0.00e+00.

## Glides (5 s at 0.001 s from a 20 m/s glide)

Trim: angle of attack 0.00718 rad, elevator -0.05958 rad, flight path -0.09396 rad. Mass 6.5771 kg, air density 1.19012 kg/m^3, resting acceleration 9.77936 m/s^2.

| Glide | JSBSim's form: largest position error (m) | at 5 s | attitude (rad) | As flown: largest (m) | at 5 s | attitude (rad) |
| --- | --- | --- | --- | --- | --- | --- |
| held | 2.00e-02 | 2.00e-02 | 2.96e-04 | 4.80e+00 | 4.80e+00 | 8.90e-02 |
| elevator | 3.55e-02 | 3.55e-02 | 6.25e-04 | 9.76e-01 | 9.76e-01 | 1.59e-02 |
| aileron | 2.45e-02 | 2.45e-02 | 4.80e-04 | 3.99e+00 | 3.99e+00 | 6.55e-02 |
| rudder | 1.86e-02 | 1.86e-02 | 2.53e-04 | 4.65e+00 | 4.65e+00 | 8.12e-02 |

The tolerance is 0.2 m on JSBSim's form. Sensitivities, as the largest position change over the glide (m):

| Glide | JSBSim at a tenth of the step | Ripple at a quarter | Air density of the end |
| --- | --- | --- | --- |
| held | 1.64e-04 | 7.39e-12 | 4.50e-02 |
| elevator | 2.82e-03 | 9.00e-09 | 1.51e-01 |
| aileron | 7.62e-03 | 2.82e-09 | 1.58e-01 |
| rudder | 1.39e-03 | 1.67e-11 | 5.71e-02 |
