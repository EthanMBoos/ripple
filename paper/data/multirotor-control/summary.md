# Multirotor velocity control

`missions/multirotor-velocity.yaml`, 0.01 s ticks, 5 substeps. Passed: True.

| Vehicle | Rise 10-90% (s) | Overshoot | Settles within 5% (s) | Largest tilt (deg) | Final tilt (deg) | Final error (m/s) |
| --- | --- | --- | --- | --- | --- | --- |
| crazyflie_forward | 1.02 | 20% | 3.59 | 5.9 | 1.46 | 0.0013 |
| crazyflie_climb | 1.71 | 0% | 2.34 | 0.0 | 0.00 | 0.0000 |
| crazyflie_fast | 1.03 | 20% | 3.63 | 13.0 | 3.25 | 0.0052 |
| crazyflie_upset | - | - | - | 15.0 | 0.02 | 0.0304 |
| firefly_forward | 0.38 | 7% | 0.97 | 14.9 | 0.99 | 0.0009 |
| firefly_climb | 0.58 | 5% | 0.74 | 0.0 | 0.00 | 0.0055 |
| firefly_fast | 0.57 | 6% | 1.33 | 20.4 | 2.22 | 0.0132 |
| firefly_upset | - | - | - | 15.0 | 0.00 | 0.0047 |
| firefly_cruise | 4.26 | -0% | 5.20 | 20.4 | 11.55 | 0.1004 |

Upsets (15 degree roll, hold zero velocity): crazyflie_upset lost 0.164 m and ended 0.071 m from its start; firefly_upset lost 0.023 m and ended 0.019 m from its start.

Steady tilt at 1 m/s against the rotor drag's prediction: crazyflie 1.456 deg, predicted 1.455 deg; firefly 0.994 deg, predicted 0.994 deg.

Largest position difference from the 20-substep run, step responses: substeps-5 2.634 mm, substeps-10 0.878 mm; the 12 m/s cruise (reported, not checked): substeps-5 8.812 mm, substeps-10 2.930 mm.
