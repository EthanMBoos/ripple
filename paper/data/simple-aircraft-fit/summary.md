# SimpleAircraft's `rascal` limits

FixedWing6DOF's Rascal under AircraftPIDController, and SimpleAircraft with `airframe: rascal` under SimpleAircraftControllerPID. Passed: True.

| Limit | Rascal (FixedWing6DOF) | SimpleAircraft | Agrees |
| --- | --- | --- | --- |
| min_speed_mps | 9.01 | 9.00 | yes |
| max_speed_mps | 38.13 | 38.10 | yes |
| max_acceleration_mps2 | 2.78 | 2.80 | yes |
| max_deceleration_mps2 | 1.17 | 1.20 | yes |
| max_climb_rate_mps | 5.77 | 5.80 | yes |
| max_descent_rate_mps | 9.75 | 9.70 | yes |
| turn_rate_degps | 26.34 | 26.20 | yes |
| effective_bank_deg | 42.93 | 42.99 | yes |
| max_roll_rate_degps | 31.17 | 31.00 | yes |
| max_pitch_rate_degps | 51.63 | 52.00 | yes |
| held_bank_deg | 47.12 | 43.00 | reported |
| held_climb_pitch_deg | 19.97 | 16.86 | reported |

Level flight at each speed asked (last 20 s of 120 s):

| Asked (m/s) | Rascal speed (m/s) | Rascal sink (m/s) |
| --- | --- | --- |
| 7 | 7.54 | 0.665 |
| 8 | 7.54 | 0.665 |
| 9 | 9.01 | -0.002 |
| 10 | 10.00 | -0.002 |
| 11 | 11.00 | -0.002 |
| 12 | 12.00 | -0.001 |
| 13 | 13.00 | -0.001 |
| 14 | 14.00 | -0.001 |
