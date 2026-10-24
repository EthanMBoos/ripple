# How models are checked

A simulator's results are only as good as its models, so each model the
paper's study uses is checked against something outside Ripple: another
implementation of the same model, or published measurements of the real
vehicle.

## What each part is checked against

- **The simulation core** is checked against SCRIMMAGE, the C++ simulator
  Ripple began as a port of. Fifteen missions and 100 generated scenarios
  are run in both, and every frame, message delivery, plugin output, and
  noise sample is compared. They agree to within 3 × 10⁻¹¹ m.
  Known SCRIMMAGE defects are fixed on both sides for the comparison. To show
  that the comparison would catch a real mistake, twelve porting bugs are
  planted one at a time; it catches all twelve.
- **Determinism**: the same missions at 1, 2, and 8 workers, with recording
  on and off, must give byte-identical output.
- **Multirotor rotors** match RotorS's motor and rotor code on 480 sampled
  states, to about 10⁻¹⁵.
- **Multirotor flight** matches Crazyflow, an independent model, on the same
  constants and motor commands: the same 3 s tumbling trajectory, to
  3 × 10⁻¹² m.
- **The Crazyflie's parameters** are its published identification: mass,
  inertia, and drag as measured, thrust within 10.4% across the measured
  range.
- **The kinematic aircraft** (SimpleAircraft with `airframe: rascal`) is
  flown through the same maneuvers as the 6-DOF Rascal: speeds, acceleration,
  climb, descent, and turn rate agree within 3%.

Checks still planned: FixedWing6DOF's stall tables against JSBSim, and
imported learned policies against their training simulator.

## Running the checks

One runner, `reference/suite.py`, runs every check by name:

```sh
python3 reference/suite.py --list                 # each check and what it passes on
python3 reference/suite.py                        # quick set, about 30 minutes
python3 reference/suite.py --release              # everything, several hours
python3 reference/suite.py --check rotors-motor   # one check
```

The SCRIMMAGE and RotorS checks build their reference in Docker. Each run
writes its logs and results to a new `runs/suiteNNN/` folder. The paper's
numbers come from one release run, kept in `paper/data/`.
[reference/README.md](https://github.com/EthanMBoos/ripple/blob/main/reference/README.md)
describes every check and its pass criterion.

A change that should not change behavior, such as a refactor, must leave
every output byte for byte unchanged; `reference/equivalence.py` compares two
builds that way.

## What agreement shows, and what it doesn't

Agreement with another implementation shows that the equations, unit
conversions, and timing came through intact: if both compute the same
numbers, a sign error or a misplaced step would have shown up. It doesn't
show the model is right about the world. Two implementations of the same
equations can be equally wrong about a real aircraft.

Agreement with a real vehicle needs measurements of that vehicle. The
Crazyflie has them; the other vehicles don't yet. So results on the aircraft
are statements about the modelled aircraft. A model is also checked only
inside its valid range: a fit to a cruising aircraft says nothing about a
stall.
