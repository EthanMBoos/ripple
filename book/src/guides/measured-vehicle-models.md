# Adding a measured-data vehicle model

Ripple's vehicles are built to the level of the open simulators people already
trust (ArduPilot and PX4 SITL, RotorS, JSBSim), with every number traced to a
source. Going further takes measured data from a real vehicle. This page is
how to add a vehicle so that its sources are visible, its checks count as
evidence, and better data has an obvious place to go later.

The Crazyflie in `Multirotor` and the Rascal in `FixedWing6DOF` are the worked
examples. Read their `airframe.rs` files alongside this page.

## 1. Pick a source and pin it

You need equations and numbers that someone published: a paper, a thesis, a
simulator's model file. Pin the exact version (a commit, an edition and page)
and add it to the paper's reference corpus (`paper/literature_review/`), so a
local copy can be fetched and read.

- The Crazyflie's numbers come from Förster's 2015 ETH thesis, cited by
  equation number.
- The Rascal's come from ArduPilot's JSBSim `Rascal.xml`.
- The rotor effects come from RotorS's motor plugin at commit `cd813b7`.

## 2. New aircraft or new model?

If one of Ripple's models already has the right equations, add a named
airframe to its `airframe.rs`: a function that returns the vehicle's numbers,
selected in a mission with `airframe: <name>`. Missions can override any single
value. There is no default vehicle: a mission names one or gives every
number, so nobody flies made-up numbers by accident.

If the equations are new, write a motion model ([Writing
plugins](writing-plugins.md)) and give it airframes the same way.

## 3. Put each number beside its source

Copy measured values exactly as published, with a citation next to each.
When a value has to be derived (the Crazyflie's thrust coefficient comes from
two fitted curves in the thesis), compute it in code from the published
numbers rather than pasting the result, and add a unit test that reproduces
the published table or fit and says how close it gets. The Crazyflie's test
checks its thrust against Förster's fit at every motor command from 8,000 to
64,000 (within 10.4%, exact at hover).

When your numbers differ from the source, list each difference beside the
values and say why it was kept. The Rascal's profile lists six.

## 4. Write the fidelity header

Every motion model starts with a `Fidelity` block with the same five headings:

- **Source:** where the equations and numbers come from.
- **Validated against:** what the model has been checked against, and with
  which script or test. "Nothing" is an acceptable answer.
- **Valid range:** where the equations hold (attached flow, near hover, below
  some speed).
- **Not modeled:** what is left out.
- **To go further:** what data would improve it, who has that data, and what
  in the code would change.

Leads on better data go in "To go further", next to the code, not in the
roadmap.

## 5. Write left-out effects as named zeros

If a missing effect belongs in a force or moment sum, put it there as an
explicit zero with a comment saying what it would take to model:

```rust
// Ground effect raises lift and cuts induced drag within about a span of
// the ground; it needs height over span and a measured correction.
let ground_effect_model_n = Vec3::zeros();
```

Whoever adds the effect later replaces one line, and a reader sees the gap
without leaving the equation. Run `reference/equivalence.py` afterwards: the
zeros must leave every output byte-identical.

## 6. Make geometry and coefficients data

Shape a model so measured data arrives as values, not new code:

- Geometry is vectors. A multirotor rotor is an offset and a tilt, so any
  layout is a `rotor_config` row.
- Aerodynamic curves use the table-or-linear pattern: `lift_table` and
  `drag_table` replace the linear lift and drag when a mission gives them,
  read with `interpolate_clamped`. A measured curve for another coefficient
  becomes another table key using the same helper.

## 7. Keep new effects optional

An effect beyond the base model (motor lag, rotor drag, stall tables, landing
gear) is a mission key that is off unless set. A named airframe never turns
one on. Missions that don't use it must run exactly as before; prove that with
`reference/equivalence.py`.

## 8. Check it, and keep the check

Pick the strongest check the source allows:

1. **Against the source's own code**, sample by sample, when the code is
   available. `reference/rotors_motor.py` compiles RotorS's motor plugin and
   compares 480 samples.
2. **Against an independent implementation** of the same vehicle.
   `reference/crazyflow_check.py` flies the Crazyflie in Crazyflow with the
   same numbers.
3. **Against a prediction from the measurements.**
   `reference/multirotor_control.py` checks that the Crazyflie leans into its
   measured rotor drag by the predicted angle.

Turn the check into a script with a pass criterion, register it in
`reference/suite.py`, and keep its results with `--retain` so they land in
`paper/data/`. A check that only ran once in a terminal isn't evidence.

## 9. Stay inside the valid range

State the valid range in the header. Ripple has no warning channel for
plugins, so the model won't tell you when a run leaves it. Have the
experiment's evidence script record the regime it actually reached (tilt,
speed, angle of attack, motor commands) and compare that with the header.

## 10. Matching the source is not matching reality

Agreement with RotorS, JSBSim, or a thesis shows the model is the source's
model, implemented correctly. It does not show the vehicle flies that way.
Keep the two claims apart in the header, the book, and the paper. "Validated
against" names real flight data only once a check against it exists. For the
Crazyflie, that next step is replaying NanoBench's recorded flights (noted in
its header).
