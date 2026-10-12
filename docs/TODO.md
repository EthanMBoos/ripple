# Roadmap

Ripple is at its initial release; the
[paper's appendix](../paper/manuscript/main.tex) records its comparison
with C++ SCRIMMAGE, [reference notes](REFERENCE_NOTES.md) list the intentional
differences, and the [checks](../reference/README.md) rerun all of it. Keep the
C++ comparison passing after any simulation change.

## Integration and evidence

Every new piece (a vehicle model, an autopilot, a sensor) needs something
outside Ripple to check it against, and a plain statement of what that check
shows:

- Compare it with its source on the same inputs, then fly it in a real
  closed-loop mission. Write down source versions, parameters, frame and unit
  conversions, timestep, errors, and known gaps.
- "Matches its source" is not "matches reality". Borrowed or estimated
  coefficients stay labeled that way even when the source comparison passes.
- With new effects turned off, old missions must run exactly as before. Write
  up deliberate fixes and keep the C++ comparison passing.
- Results and run cost go in the [paper](../paper/manuscript/main.tex), with
  the data under `paper/data/`.

A new piece doesn't have to change any ranking to be worth finishing. A
checked capability, or showing that a simpler model is good enough somewhere,
counts too.

## Learned policies

Sources and numbers: [LEARNING_LITERATURE.md](LEARNING_LITERATURE.md).

The plan: take a few well known multi-agent tasks, run learned and classical
policies on them, and see if the winner changes as we make the sim more
realistic. Realistic here means three separate knobs: noisier sensing (agents
act on what they believe, not the truth), worse communication (lost and late
messages), and real vehicle dynamics (multirotors and fixed-wing instead of
points).

Why bother: learned policies are mostly built and compared in sims with point
agents, perfect observations, and free messages (MPE, VMAS, JaxMARL). When
papers compare against classical methods, the two sides often don't get the
same information or the same tuning effort. We didn't find anyone who turns
these knobs one at a time on standard tasks and checks whether the ranking
holds.

### Tasks

Each one has published learned results and an old, trusted classical answer,
so our numbers can be checked against other people's. Each leans on one knob
in particular, but all three knobs get turned on all three tasks.

- **Cooperative navigation.** N agents spread out to cover N landmarks
  without crashing (MPE's `simple_spread`).
  - Classical: assign agents to landmarks optimally (Hungarian algorithm) and
    fly there. Decentralized version: an auction over the network.
  - Learned results are bunched together on this task, and the raw score
    also depends on where agents happened to start. So report these as
    separate numbers, never folded into one:
    - the benchmark return, exactly as the pinned implementation defines it;
    - coverage at the end: landmarks occupied, and summed distance from each
      landmark to its nearest agent;
    - path cost: total distance flown, divided by the Hungarian assignment's
      cost (straight-line distance from each agent's start to its optimally
      assigned landmark, same start). That's a lower bound on travel, not an
      optimal return: it ignores collisions and what happens along the way;
    - collisions.
  - Nobody has compared learned `simple_spread` policies to optimal
    assignment. The closest is Muthusamy et al. 2024, with noise-free
    observations, in their own environment.
  - Main knob: sensing. What happens when assignment runs on each agent's
    noisy belief, and the auction runs over a lossy network?
  - First, match one pinned implementation with point agents, transitions
    and rewards included, not just observations: MPE2's `simple_spread_v3`
    at a fixed commit, default settings (N = 3, 25 steps, `local_ratio` 0.5,
    radius 0.15, agents and landmarks in [-1, 1]², force actions with MPE's
    damping). Parity means the same exported policy gets the same returns in
    MPE2 and in Ripple from the same starts.
  - Published returns used other versions, so they're only a sanity check on
    our training. MAPPO's own `simple_spread` (in its repo) puts landmarks in
    0.8 × [-1, 1]², counts each agent as colliding with itself, and has no
    `local_ratio`; Papoudakis et al. used EPyMARL's MPE of 2021. MPE moved
    from PettingZoo to Farama's MPE2, which dropped the self-collision.
- **Circle-crossing collision avoidance.** Agents start on a circle and fly
  to the opposite side, so everyone meets in the middle.
  - Classical: ORCA (velocity obstacles; the RVO2 library), its versions for
    wheeled robots (NH-ORCA) and constant-speed aircraft, and a control
    barrier function controller.
  - Learned: a CADRL-style policy (published weights exist) and MAPPO.
  - The published "learned beats ORCA" results are softer than they look:
    the median gain was small and most of it came from the hardest cases;
    one paper had to inflate ORCA's radii to stop it colliding; another gave
    ORCA true neighbor states while the learned policy used a laser. Here
    both get the same belief.
  - Main knob: dynamics. ORCA assumes you can slow down or stop. Fixed-wing
    aircraft can't, and nobody has run this comparison on fixed-wing.
  - Run symmetric and jittered circles and pair swaps; aircraft need a
    holding pattern instead of stopping at the goal.
  - Measure success, collisions, deadlocks, extra time, and closest approach,
    at the median and in the worst cases: the 75th and 90th percentiles for
    extra time, the 25th and 10th for closest approach (small is bad).
- **Flocking with limited communication range.** Agents start with random
  velocities and have to line up and stay together without colliding,
  talking only to neighbors in range.
  - Classical: Reynolds' rules, Olfati-Saber, Vásárhelyi (the 30-drone
    flock), tuned with CMA-ES.
  - Learned: a graph network trained to imitate a centralized controller
    (Tolstaya et al. 2019), and MAPPO.
  - Tolstaya's network beat the local controller at 1 m range, where passing
    neighbor info along over several hops matters most, and lost at 4 m. The
    authors think the 4 m loss may come from how they built the network's
    inputs, so don't lean on it as a mechanism. Lost and late messages
    degrade the multi-hop info, so we expect them to change where the
    network's advantage holds.
  - Main knob: communication.
  - Fixed-wing can't hover, so they flock around a moving reference point.
  - Measure velocity alignment, how many separate groups form, closest
    approach, and messages sent.

We looked at pursuit too and dropped it: results depend on how each paper
designs the evader.

### Which vehicles

1. **Point agents first, to match published numbers.** Same setup as
   PettingZoo or VMAS. If Ripple doesn't reproduce their results here, any
   change later up the ladder could just be a bug in our setup.
2. **Multirotor (Crazyflie) is the main realistic vehicle.** Most of the
   drone work we compare against flies quadrotors, often Crazyflies (Batra,
   STGNN, and OPEN fly Crazyflies; Vásárhelyi's 30 drones are custom quads),
   so this is the easiest comparison for reviewers. Crazyflow and
   MuJoCo-Drones-Gym simulate the same vehicle, which gives a cross-check on
   our dynamics. It is built: see the book chapter "Flying a multirotor".
3. **Fixed-wing is the stress case**: `SimpleAircraft`, then `FixedWing6DOF`.
   Already built. It's where the classical assumptions break: aircraft can't
   stop, can't park on a landmark, and can't match a neighbor's velocity at
   any speed. Nobody has run these comparisons on fixed-wing. A quad can hover
   almost like a point, so rankings may hold there; fixed-wing is where we
   expect them to move.

What the policy outputs: in MPE and VMAS it's a 2-D force by default (VMAS
can take a target velocity instead, through its velocity controller). Point
agents keep the force for the runs that match published numbers. After that,
train with velocity actions: the multirotor takes the velocity as is, and for
aircraft it becomes a heading and a speed, held at or above a minimum
airspeed, so "stop here" turns into circling. Per task:
loiter over the landmark (covered if within a radius), a holding pattern at
the goal, and flocking around a moving reference.

### How we run it

- **No RL training code in this repo.** Train in VMAS, JaxMARL, or PettingZoo
  with existing libraries (BenchMARL, MAPPO), export to ONNX, run here. For
  fine-tuning in Ripple, use the bridge below.
- **Same commands, same information.** Learned and classical policies send
  the same command to the same controllers (force for point agents in the
  matching runs, velocity for the multirotor, heading, speed, altitude for
  aircraft) and read the same
  belief. Each task has one observation definition (frames, scaling, how
  neighbors are encoded, decision rate). A parity test checks it matches the
  training sim; if it silently doesn't, the whole study is garbage.
- **Train three ways:** at the benchmark's simple setting, at the realistic
  setting, and with the setting randomized during training. That tells us
  whether a learned policy lost because it was trained in the wrong place or
  because the method is fragile.
- **Fair tuning.** Classical methods get the same tuning budget (number of
  evaluations) as the learned ones, at the same setting. Also report them
  tuned at the simple setting and frozen, to match how the learned policies
  are trained.
- **Stats.** 10 training seeds. Report the interquartile mean with bootstrap
  confidence intervals (the `rliable` library). Learned and classical run on
  the exact same episodes. To say whether the ranking held, compare each
  level's ranking to the simple setting's with a rank correlation, and count
  ranking reversals. Train at
  N agents, also test at 2N and 4N.
- **The knobs.** Turn each one alone through four levels, then all together.
  Pick the noise, loss, and delay levels from real sensor and radio specs or
  ArduPilot SITL runs, not numbers we made up.

### Build order

None of this is built yet. The simulator side is ready (C++ agreement,
determinism, fixed-wing with ArduPlane, noisy sensors, lossy networks,
sweeps, a Crazyflie multirotor with a velocity controller); the learning
pipeline and the three tasks are not.

1. **Navigation with point agents, matching MPE2.** Train MAPPO in MPE2's
   `simple_spread_v3` (pinned commit) with an existing library, export to ONNX,
   run it in Ripple, and get the same returns from the same starts. Published
   returns are a sanity check on training only. Needs a point agent with MPE's
   force and damping dynamics (`SingleIntegrator` takes velocity), landmarks,
   the coverage metric, the ONNX plugin, the parity test, and the Hungarian
   baseline. This proves the whole pipeline end to end.
2. **First fidelity results on navigation.** Sensing noise and belief on the
   point agents, then network delay with the auction baseline. These are the
   first real data points for the paper.
3. **Navigation on the multirotor.** Retrain with VMAS's velocity actions.
   The multirotor's motion already matches Crazyflow (book: "Flying a
   multirotor").
4. **The training bridge**, for the "train at the realistic setting"
   condition. Measure its speed first.
5. **Collision avoidance**: ORCA family and a barrier function controller on
   point agents, then multirotor, then fixed-wing (the new part).
6. **Flocking** last.

Two good tasks are still a paper.

Pieces, by where they're needed:

- **Shared:**
  - point agents with MPE's force dynamics, and unicycle dynamics;
  - the ONNX policy autonomy plugin (Burn or `ort`) that keeps output
    byte-identical at any worker count: fixed thread count, and random
    actions drawn from the plugin's own random stream;
  - the observation parity test against MPE2 and VMAS;
  - delay in the network model;
  - the training bridge: an optional PyO3 crate, out of the default build like
    `ardupilot`, that looks like a PettingZoo parallel environment.
    `Simulation::step` returns the state from before the step, so the bridge
    needs its own step; reset rebuilds the simulation;
  - for aircraft, turning a velocity command into heading and speed held
    above minimum airspeed, and holding or moving-reference goals;
  - a tuning script for the classical baselines and the stats (`rliable`),
    both Python, outside the sim;
  - noise, loss, and delay levels taken from real sensor and radio specs or
    ArduPilot SITL. NoisyState's defaults (C++'s 5 m and 5 m/s horizontal)
    are demo values: on the Rascal they break the speed loop. Its noise is
    also fresh every tick, where real GPS error drifts slowly;
  - a standard filter (a Kalman filter on position and velocity) as an
    estimator plugin, used by learned and classical policies alike. With
    raw readings in belief (`PassThrough`), even GPS-like noise makes the
    Rascal's controller jitter (body rates about 0.15 rad/s), and detuning
    the controller makes it worse.
- **Navigation:** landmarks, a deterministic Hungarian plugin that can run on
  truth or belief, an auction over the network, the coverage-gap metric.
- **Collision avoidance:** ORCA, NH-ORCA, constant-speed ORCA, a barrier
  function controller, circle and swap sweeps, separation metrics.
- **Flocking:** Reynolds, Olfati-Saber, and Vásárhelyi plugins; neighbor
  messages that carry hop count and age; alignment and group metrics.

### What could go wrong

- One side is a strawman: an undertrained learned policy or a badly tuned
  classical one. Use published hyperparameters and weights, and the equal
  tuning budget.
- The observations don't match the training sim. The parity test is there
  for this.
- The realism levels look made up. Take them from real specs.
- Nothing flips anywhere. Report it anyway. Reproducing Tolstaya's range
  result is the check that our setup can show a flip at all.
- Closest prior work, which we have to cite and say how we differ from:
  Labiosa and Hanna (ICRA 2025), Li et al. (NeurIPS 2025), Kadian et al.
  (RA-L 2020), Muthusamy et al. (2024).
