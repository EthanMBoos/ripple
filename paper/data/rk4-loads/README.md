# Frozen RK4 loads: how much they cost

FixedWing6DOF and Multirotor keep C++'s approximation: forces and moments are
computed once per motion step and held through all four RK4 stages. This
measures that against recomputing them from each stage's state
(`stage-loads.patch`, measurement only; the simulator is unchanged).

```sh
python3 paper/data/rk4-loads/measure.py paper/data/rk4-loads/results.json
```

The script exports the checkout, applies the patch, builds `scrimmage`, runs
the missions, and compares `frames.bin`. `results.json` records the revision
(`548b3ac` plus the uncommitted interleaved-substep change) and the patch hash.
Run on 2026-09-29, Apple M2 Pro.

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
one or two halvings (its 512-versus-1024 self-check is 2.5e-11 m and 1.4e-6 m).

## B. A controlled flight at practical steps

`fixed-wing-recovery.xml`: a gentle pitch loop (P 0.5, D 0.1) recovering from
a 20° roll and 8° pitch start in wind, 30 s, one substep. Largest difference
between frozen and stage-evaluated loads under the same controller schedule:

| Step | Position | Attitude |
|---|---|---|
| 0.01 s | 2.0 m | 0.033 rad |
| 0.05 s | 99 m | 0.27 rad |
| 0.1 s | 4.4 km | 3.13 rad (flipped) |
