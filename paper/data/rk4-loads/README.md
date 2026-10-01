# RK4 loads: frozen against every stage

C++ FixedWing6DOF and Multirotor computed forces and moments from the
start-of-step state at all four RK4 stages, so the step was first order. Both
implementations now compute them from each stage's state (`benchmarking-edits`
commit `1c8595b91` in C++; see `docs/REFERENCE_NOTES.md`). This measures what
that changed, in Rust: `b705319`, the last revision with frozen loads, against
the fixed checkout.

```sh
python3 paper/data/rk4-loads/measure.py paper/data/rk4-loads/results.json
```

The script builds both revisions, runs the missions, and compares
`frames.bin`. `results.json` records both revisions. Run on 2026-10-01, Apple
M2 Pro.

## A. Integration accuracy, constant controls

Largest position error over the run, against stage-evaluated loads at 1024
substeps. With constant controls the substep count changes only the
integration step.

| Substeps | Multirotor (`missions/multirotor.xml`, 4 s, dt 0.01 s) frozen / stage | Fixed-wing glider (`fixed-wing-open-loop.xml`, 60 s, dt 0.05 s) frozen / stage |
|---|---|---|
| 1 | 6.1e-2 / 5.1e-10 m | 16.9 / 1.5e-5 m |
| 2 | 3.1e-2 / 3.5e-11 m | 8.4 / 4.8e-6 m |
| 4 | 1.5e-2 / 2.5e-11 m | 4.2 / 2.7e-6 m |
| 8 | 7.7e-3 / 2.5e-11 m | 2.1 / 2.7e-6 m |
| 16 | 3.8e-3 / 2.5e-11 m | 1.05 / 2.7e-6 m |

Frozen loads are first order: the error halves exactly with each halving of
the step. Stage-evaluated loads reach the reference's own resolution within
one or two halvings (its 512-versus-1024 self-check is 2.5e-11 m and 1.4e-6 m),
so this shows a far smaller error, not a measured fourth-order rate.

## B. A controlled flight at practical steps

`fixed-wing-recovery.xml`: a gentle pitch loop (P 0.5, D 0.1) recovering from
a 20° roll and 8° pitch start in wind, 30 s, one substep. Largest difference
between frozen and stage-evaluated loads:

| Step | Position | Attitude |
|---|---|---|
| 0.01 s | 5.0 m | 0.038 rad |
| 0.05 s | 81 m | 0.23 rad |
| 0.1 s | 2.8 km | 3.14 rad (flipped) |

## C. Speed

Whole `scrimmage run` wall time at one worker, including writing frames: a
warm-up, then the median (range) of five runs.

| 128 agents | Frozen | Every stage | Change |
|---|---|---|---|
| FixedWing6DOF with AircraftPIDController, 30 s at 0.01 s | 0.381 s (0.380-0.394) | 0.463 s (0.461-0.466) | +22% |
| Multirotor hovering, 4 s at 0.001 s | 0.446 s (0.444-0.450) | 0.476 s (0.463-0.478) | +7% |

The forces are now really computed four times per step. FixedWing6DOF's
aerodynamics are most of its motion cost; Multirotor recomputes only weight,
drag, and the gyroscopic term (thrust depends only on the motor commands). The
motion phase alone slows by more than these whole-run figures. C++ was not
timed: no `perf.py` or `benchmark.py` workload uses either model.

## D. The altitude gain

Whether frozen loads caused the oscillation that lowered AircraftPIDController's
default altitude D gain from 0.01 to 0.002 (`paper/data/aircraft-pid-upset/`).
`upset.xml` (the default A-4, 45 s at 1000 m after a 5° roll and 3° pitch
upset) at the step that motivated the change and two halvings. Largest
altitude error over the run (after the first 5 s):

| Altitude D | Step | Frozen | Every stage |
|---|---|---|---|
| 0.01 | 0.01 s | 285 m | 91 m |
| 0.01 | 0.005 s | 13 m | 10 m (4.9 m) |
| 0.01 | 0.0025 s | 68 m | 20 m |
| 0.002 | 0.01 s | 1.41 m (0.63 m) | 1.39 m (0.64 m) |
| 0.002 | 0.005 s | 1.38 m (0.61 m) | 1.37 m (0.62 m) |
| 0.002 | 0.0025 s | 1.36 m (0.61 m) | 1.36 m (0.61 m) |
| 0 | 0.01 s | 1.51 m (0.79 m) | 1.48 m (0.72 m) |

With correct loads, D 0.01 still oscillates at every step, by tens of metres
and without settling as the step shrinks. The frozen loads made it about three
times worse at 0.01 s but did not cause it, so the default stays 0.002. That
gain barely notices the fix (1.41 m against 1.39 m).
