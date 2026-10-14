# Multirotor velocity control

`missions/multirotor-velocity.yaml`, 0.01 s ticks, 5 substeps. Passed: True.

| Vehicle | Rise 10-90% (s) | Overshoot | Settles within 5% (s) | Largest tilt (deg) | Final tilt (deg) | Final error (m/s) |
| --- | --- | --- | --- | --- | --- | --- |
| crazyflie_forward | 1.02 | 20% | 3.59 | 5.9 | 1.46 | 0.0013 |
| crazyflie_climb | 1.71 | 0% | 2.34 | 0.0 | 0.00 | 0.0000 |
| crazyflie_fast | 1.03 | 20% | 3.63 | 13.0 | 3.25 | 0.0052 |
| crazyflie_upset | - | - | - | 15.0 | 0.02 | 0.0304 |

Upsets (15 degree roll, hold zero velocity): crazyflie_upset lost 0.164 m and ended 0.071 m from its start.

Crazyflie steady tilt at 1 m/s: 1.456 deg; Förster's rotor drag predicts 1.455 deg.

Largest position difference from the 20-substep run: substeps-5 2.634 mm, substeps-10 0.878 mm.
