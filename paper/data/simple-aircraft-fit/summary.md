# SimpleAircraft's `rascal` limits

FixedWing6DOF's Rascal under AircraftPIDController, and SimpleAircraft with `airframe: rascal` under SimpleAircraftControllerPID. Passed: True.

| Limit | Rascal (FixedWing6DOF) | SimpleAircraft | Agrees |
| --- | --- | --- | --- |
| min_speed_mps | 11.00 | 11.00 | yes |
| max_speed_mps | 35.33 | 35.30 | yes |
| max_acceleration_mps2 | 2.32 | 2.30 | yes |
| max_deceleration_mps2 | 1.31 | 1.30 | yes |
| max_climb_rate_mps | 4.02 | 4.00 | yes |
| max_descent_rate_mps | 10.05 | 10.10 | yes |
| turn_rate_degps | 30.24 | 29.61 | yes |
| effective_bank_deg | 46.49 | 46.49 | yes |
| max_roll_rate_degps | 33.39 | 33.40 | yes |
| max_pitch_rate_degps | 50.07 | 50.10 | yes |
| held_bank_deg | 48.96 | 46.50 | reported |
| held_climb_pitch_deg | 19.08 | 11.54 | reported |

Level flight at each speed asked (last 20 s of 120 s):

| Asked (m/s) | Rascal speed (m/s) | Rascal sink (m/s) |
| --- | --- | --- |
| 7 | 10.15 | 1.810 |
| 8 | 10.15 | 1.810 |
| 9 | 10.15 | 1.810 |
| 10 | 10.15 | 1.810 |
| 11 | 11.00 | -0.002 |
| 12 | 12.00 | -0.002 |
| 13 | 13.00 | -0.002 |
| 14 | 14.00 | -0.001 |
