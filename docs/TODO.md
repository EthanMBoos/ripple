# Roadmap

SCRIMMAGE-RS is at its initial release; the [paper](../paper/README.md) shows
how it agrees with C++ SCRIMMAGE, and [reference notes](REFERENCE_NOTES.md) list
the intentional differences. Keep the C++ comparison passing after any
simulation change.

## ArduPlane

Fly several ArduPlane SITL instances in lockstep with FixedWing6DOF over
ArduPilot's JSON backend. User guide:
[Flying ArduPlane](../book/src/guides/ardupilot.md).

`crates/ardupilot` has the `SitlState` sensor, the `ArduPilotBridge`
autonomy, one- and two-aircraft Rascal missions, the Docker image (`docker/`),
and `launch.py`, which starts one SITL container per aircraft, runs the
mission, and stops them. Aircraft are held at rest and catapulted at release.
One and two Rascals have flown LOITER, a looping AUTO waypoint route
(`launch.py --waypoints`), a GUIDED target (`--goto`), and a ground catapult
takeoff with real ArduPlane, with wind, and passed the pause and kill checks.
Per-aircraft routes and changing GUIDED
targets during a run are not done; add them when an experiment needs them.

## Forces at every RK4 stage (do first)

**In short.** Before this fix, results depended on the timestep: a larger dt
made them much worse (at 0.1 s the aircraft flips), and a smaller one helped
only slowly (half the dt, half the error). After it, results barely depend on
dt, so you can change it and keep the same answer. At the dt 0.01 the missions
use, flights move only a few metres; the fix matters for larger steps, long
runs, and stiff forces such as the landing gear below. It costs about 22% of
run time for FixedWing6DOF and 7% for Multirotor. That is not extra stepping:
the steps and the four RK4 stages per step are unchanged. The force
calculation used to run once per step and be reused at every stage; now it runs
at each stage, four times per step. FixedWing6DOF slows more because its
aerodynamics are most of its cost; Multirotor recomputes only weight, drag,
and the gyroscopic term.

**A worked step** (the Multirotor drag unit test): 10 m/s, drag deceleration
−0.1·v², one step of 0.1 s; the exact answer is 10 / 1.1 = 9.090909 m/s.

| Stage | Speed checked | Frozen | Every stage |
|---|---|---|---|
| 1 (start) | 10.000 | −10.00 | −10.00 |
| 2 (middle) | 9.500 | −10.00 (reused) | −9.03 |
| 3 (middle) | 9.549 | −10.00 (reused) | −9.12 |
| 4 (end) | 9.088 | −10.00 (reused) | −8.26 |

RK4 averages them with weights 1, 2, 2, 1. Frozen gives 9.000 m/s, 0.09 off,
because it keeps braking with the start-of-step drag after the vehicle has
slowed. Every stage gives 9.090912 m/s, 0.000003 off, for four drag
calculations instead of one.

**How it differs from the earlier AircraftPIDController fix.** That one was
about the controller: C++ handed the PID the whole tick instead of the substep,
so its gains changed with `motion_multiplier`, and fixing it (plus lowering the
altitude D gain) visibly changed how aircraft fly. This one is about the
motion model's physics inside each step: how accurately the aircraft moves
for a given dt. It does not change the controller, and the lower gain still
stands (part D of `rk4-loads`).

FixedWing6DOF and Multirotor copy a C++ shortcut. They compute forces and
moments once, from the state at the start of each step, then reuse them at all
four RK4 stages; only position and attitude use the stage states. So they pay
for four evaluations and get first-order accuracy: halving dt only halves the
error. The forces are also recomputed identically at each stage, and the
gyroscopic term mixes start-of-step and stage states. `paper/data/rk4-loads`
measured it in Rust:

- A glider over 60 s at dt 0.05 ends 17 m off, against 1.5e-5 m with
  stage-evaluated forces.
- A gently controlled 30 s flight differs by 2 m at dt 0.01 and 99 m at 0.05.
- At dt 0.1 the frozen version flips the aircraft.

**A bug, not a design choice.** In C++, `model(x, dxdt, t)` is what RK4 calls
at each stage, and it is handed that stage's state as `x`. In
`FixedWing6DOF.cpp` (around lines 453-560), the lines inside it split:

- **The kinematics use `x`:** `dxdt[U] = x[V]*x[R] - x[W]*x[Q] + ...` and the
  quaternion update.
- **Every force and moment uses the member `x_`, the start-of-step state:**
  `vel_rel_wind(x_[U] + ...)`, `C_LQ_ * x_[Q]`, `C_LP_ * x_[P]`, the ground
  spring's `x_[Zw]`, and the gyroscopic `pqr(x_[P], ...)`. Angle of attack
  (`alpha_`) is a member computed once in `step()`.

Freezing forces on purpose would mean computing them once in `step()`, not
four times inside the per-stage function from the wrong variable next to
lines using the right one. It reads as an `x`/`x_` slip from the original
"finishing fixed 6dof" commit (DeMarco, 2018-01-12); no comment or commit
calls it deliberate, and Multirotor repeats the pattern.

The landing gear (next section) needs this fixed: stiff, heavily damped wheel
springs are where frozen forces go unstable. Do it as its own change, before
the gear, because it is a behavior change to two ported models.

**Hypothesis.** Frozen forces act like Euler in how forces respond to the
state:

- **Oscillations gain energy**, at a rate of roughly ω²·dt/2. This hits the
  short period, the dutch roll, the phugoid, and especially fast closed
  control loops.
- **Plain decays gain damping** while λ·dt < 1, then oscillate, and blow up
  past 2.

For the default A-4D at 220 m/s and dt 0.01:

- The short period (about 5 rad/s) loses about 9% of its damping.
- A controller-stiffened mode near 10 rad/s gains about 0.5/s of false growth,
  which is no longer small.
- The Rascal at 400 Hz is barely affected.

So part of the post-upset oscillation that led to lowering
AircraftPIDController's altitude D gain (0.01 → 0.002) may have been the frozen
forces, not the gain.

**What we should see if it is right:**

1. Halving dt shrinks the error far faster after the fix.
2. The upset case is better damped at dt 0.01 with the same gains.
3. Upstream's D = 0.01 may no longer oscillate. If so, return it to 0.01 (one
   fewer deviation from C++); if it still oscillates, 0.002 stands on its own.
4. Gliding flight barely changes, since its modes are slow.

If the D = 0.01 oscillation looks the same with and without the fix, the
forces were not its cause.

**The D gain does not automatically go back.** Lowering it was the right fix
for the simulator as it behaved, and either outcome is fine:

- **D = 0.01 still oscillates with correct forces:** 0.002 was right on its
  own merits and stays.
- **D = 0.01 behaves:** the oscillation was partly the force bug, and the lower
  gain was compensating. That earlier fix still treated a real symptom. Choose
  between upstream's 0.01 (one fewer deviation from C++) and keeping 0.002 if
  it still flies well, and say why.

The estimate (about 9% lost damping on its own, more in fast loops) may or may
not explain the whole oscillation. That is why the cheap experiment comes
before any C++ edit or campaign rerun.

- [x] **2×2 experiment first (Rust only, temporary switch):** frozen against
  stage-evaluated forces, times D = 0.01 against 0.002. Use the setup that
  motivated the gain change (one FixedWing6DOF, 45 s of Straight at 1000 m,
  with the upset), at dt 0.01 and its halvings. It decides which parameters
  to touch before the C++ edit and the campaign. Done as
  `paper/data/rk4-loads` part D: D 0.01 still oscillates with correct forces
  (91 m against 285 m frozen at 0.01 s), so the forces were not its cause.
- [x] **Baseline as is:** the C++ comparison passed at `b705319`. `perf.py` and
  `benchmark.py` time only SimpleAircraft, so speed is measured in
  `rk4-loads` part C instead, against a `b705319` build.
- [x] **Fix C++ on `benchmarking-edits`** (`1c8595b91`, local, not pushed): forces, moments, and the gyroscopic
  term from each stage's state, in FixedWing6DOF and Multirotor. It becomes
  the third documented defect fixed there, after the substep order and
  AircraftPIDController; the unmodified build then differs by design, as
  with those. Add it to the allowed C++ fixes in AGENTS.md.
- [x] **The same fix in Rust,** so the comparison passes against the fixed C++.
  Change both models; fixing one would leave an inconsistency to explain.
- [x] **Measure before against after** (`rk4-loads`; C++ not timed):
  - accuracy: rerun the `rk4-loads` study on both implementations;
  - speed: `perf.py` and `benchmark.py`. Forces are now really computed four
    times per step, so the FixedWing6DOF and Multirotor motion phases slow
    down; record by how much.
- [x] **Decide the D gain** from the experiment, and document the result either
  way: 0.002 stays.
- [x] **Rerun the paper campaign** (about two hours) and update the text
  (`runs/campaign-002`): everything passes, 14 of 14 missions, 100 of 100
  generated, 8 of 10 long as before. Of the divergence write-ups only
  `generated-007` moved (now 76.4 s); `generated-000` flies no fixed model.
- [x] **Update REFERENCE_NOTES.md:** its "Frozen RK4 loads" paragraph says the
  ported models keep the approximation; it becomes "fixed on the comparison
  branch, like the substep order".

## Terrain ground contact

Ground reactions for landing, rolling, and takeoff, needed for ArduPlane off a
runway. The terrain chapter's "Why it is built this way" already chose the
design (JSBSim-style contact points). The hooks exist:

- `MotionContext::terrain` reaches motion models.
- FixedWing6DOF's flat spring is one marked block.
- `grep -rn "TODO(terrain)" crates` finds the starting points.

The C++ comparison is unaffected: the old flat spring stays for comparison
missions, and the gear is opt-in and Rust-only.

**Facts found:**

- **Sourced gear.** ArduPilot's JSBSim Rascal, where our airframe came from,
  is a taildragger:
  - two main wheels 0.43 m below the CG and slightly ahead of it;
  - a castering tail wheel;
  - spring 7,000 N/m and damping 1,460 N·s/m;
  - static, dynamic, and rolling friction 0.8, 0.5, and 0.1.
- **A runway in the data.** The McMillan terrain's origin is the airfield. A
  600 m strip about 400 m south of it, heading about 110°, has only 0.1-0.3 m
  of bumps. It is near z = -24 m, since z = 0 is 300 m above sea level.

**Steps, in order:**

- [ ] **One shared ground (core).** `Terrain` gets `ground_height(e, n)` and a
  surface normal for slopes. The off-grid height moves from TerrainCollision's
  `off_terrain_z` into the terrain, so collision and contact never disagree.
  Without terrain, the gear uses flat ground at z = 0, so runway demos work
  without a terrain mission.
- [ ] **Landing gear in FixedWing6DOF:** a new `landing_gear.rs` plus tests.
  - Contact points, each with a spring, a damper, rolling and side friction,
    and the forces and moments they produce. Opt-in through gear parameters,
    and exclusive with the old flat spring.
  - The Rascal uses JSBSim's values.
  - Gear forces are evaluated at every RK4 stage (after the section above):
    frozen, `dt × damping/mass` must stay below about 2, which holds at
    0.0025 (0.62) but fails at 0.01 (2.5).
  - The tail wheel steers with the rudder, as on real taildraggers. A freely
    castering wheel leaves ArduPlane almost no steering at taxi speed.
  - Checks: resting height and settling, a drop, a slope, friction stopping a
    rolling aircraft, and results that converge as dt shrinks.
- [ ] **Crash against touchdown in TerrainCollision.** It gains an optional
  list of structural points on the airframe (nose, tail, wingtips), checked
  along each step's path with the attitude. A structural point touching
  removes the aircraft; wheels holding it up do not. The points live in the
  interaction's parameters, not in Entity.
- [ ] **`scrimmage resolve <mission>`:** a small CLI command that prints the
  fully resolved mission as JSON.
  - The ArduPilot launcher reads ports, dt, the release time, and the terrain
    origin (its home) from it, and its Python XML reader is deleted.
  - That handles YAML terrain missions, which `launch.py` cannot read today,
    and `param_common` properly.
- [ ] **ArduPlane runway takeoff and landing.** A YAML McMillan mission with
  the Rascal starting on the runway, with no launcher: it is already still,
  so ArduPlane boots and arms cleanly.
  - Take off in TAKEOFF, then fly an AUTO route ending in `NAV_LAND`.
  - Expect `rascal.parm` tuning for the taildragger takeoff
    (`TKOFF_TDRAG_*`, ground steering): the main unknown.
  - For now, set the starting z by hand: ground height plus gear height.
  - Then fill in the ArduPlane guide's planned takeoff-and-landing section.
