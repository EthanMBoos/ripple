# AircraftPIDController altitude gain after an upset

Why the default altitude derivative gain dropped from 0.01 to 0.002 (see
`docs/REFERENCE_NOTES.md`, "AircraftPIDController"). This is a tuning choice
checked on one aircraft and one upset, not a correctness fix; the timestep
change in the same commit is the fix.

`upset.xml` flies one FixedWing6DOF (the default A-4) straight at 220 m/s and
1000 m for 45 s at 0.01 s ticks, starting with a 5 degree roll and 3 degree
pitch upset. It uses the controller's default gains.

```sh
scrimmage run paper/data/aircraft-pid-upset/upset.xml --headless
```

For upstream's gain, add `altitude_pid="0.1, 0.0001, 0.01, 10"` to the
`<controller>` tag. Altitude error is the largest distance from the starting
altitude, over the whole run and after the first 5 s:

| Altitude D | Whole run | After 5 s |
|---|---|---|
| 0.01 (upstream) | 285.1 m | 285.1 m |
| 0.002 (default) | 1.41 m | 0.63 m |
| 0 | 1.51 m | 0.80 m |

Measured with the Rust port in the commit that added this folder; with no
substeps C++ and Rust fly this identically.

## Why the lower gain is better

The derivative term reacts to how fast the altitude error is changing, not
to the error itself. It is meant to act like a damper. It does not decide
where the aircraft ends up: the P and I gains do that, and they are
unchanged, so the target altitude and how it is held are the same.

At 220 m/s, D = 0.01 makes each correction about 2.2 times larger than the
motion it reacts to. Instead of damping, it amplifies: the aircraft
overcorrects one way, then further the other way, and the swings grow. At
0.002 each correction is smaller than the motion, so the swings die out.

This is not a value fitted to one result:

- D = 0 flies well too, so the result does not hinge on exactly 0.002; 0.01
  was simply too large.
- Nothing else changed. Lowering the pitch loop's D instead did not help,
  which points to the altitude D as the cause.
- It is about what upstream already flew with 5 substeps, where its timestep
  bug divided D by 5. The lower gain makes that deliberate instead of
  accidental.

It was checked on the default aircraft in this one scenario. A very
different airframe should be retuned rather than assume either value.
