# Training a single agent

The Python package's `ripple.learning` shows reinforcement learning on
Ripple: a task written as one Python autonomy class, trained with
Stable-Baselines3, then flown by Ripple from the saved network through the
same class. This chapter trains one Crazyflie to fly to a goal and hold
there. [Training a team](training-team.md) trains three agents together.
Read [Plugins in Python](python-plugins.md) first for what a Python autonomy
is.

## How to think about it

Two programs share the work:

| | Owns |
| --- | --- |
| **Ripple** | The mission, the physics, and the stepping. |
| **The task** | What the policy sees and what its action does: the `Hover` class's `observe` and `act`, a Python autonomy. |
| **The trainer** | The reward, the learning algorithm, and the training loop: `HoverEnv` (Gymnasium) and `train_hover.py`. |

While Python trains, it steps the simulation itself: it asks the task for the
agent's observation, picks an action, and runs one tick with it, during which
the same task class, flying as the agent's autonomy, takes that action. When
training ends, it saves the policy's network as an ONNX file. A replay runs
the same mission with `ripple run`, where the class feeds its own
observation to that network. The observation and the action are one class's
two methods in both cases, so the network sees in replay exactly what it saw
in training, and the trained and the flown code cannot drift apart.

## Quick start

You need [uv](https://docs.astral.sh/uv/) and Rust. From the repository root:

```sh
uv sync --extra train                           # builds the Rust module, installs PyTorch and SB3
uv run crates/python/learning/train_hover.py    # about 3 minutes; writes ripple/learning/policies/hover.onnx
uv run ripple run crates/python/learning/missions/hover.yaml --viewer
```

The package ships a trained `hover.onnx`, so the last command works without
training first. Training prints the drone's distance from the goal at the
end of 20 episodes it never trained on:

```text
distance to goal at the end, 20 new episodes: median 0.033 m, worst 0.126 m
```

## The task

`crates/python/learning/missions/hover.yaml` is an ordinary mission: one
Crazyflie with `MultirotorVelocityController` and `Multirotor`, as in
[Flying a multirotor](multirotor.md), starting at a random point a few
meters from the goal (each mission seed is a different start), and the
`Python` autonomy naming the task:

```yaml
autonomy:
  Python:
    class: ripple.learning.hover.Hover
    goal_m: [0, 0, 5]
    max_speed_mps: 2
```

| | |
| --- | --- |
| Observation | Offset to the goal (m, 3 numbers), own velocity (m/s, 3), from belief |
| Action | A velocity command: 3 numbers in [-1, 1], times `max_speed_mps` (2 m/s) |
| Reward | −log(distance + 0.1 m), every step |
| Episode | One run of the mission: 10 s, 200 decisions at 20 Hz |

The episode ends at the mission's time limit, which Gymnasium calls a
truncation, not a terminal state: the drone could have kept flying, so the
learner still values what comes after.

## The pieces

All under `crates/python`:

| File | What it does |
| --- | --- |
| `ripple/learning/hover.py` | `Hover`: `observe`, and `act`, which turns an action into a velocity command |
| `ripple/learning/__init__.py` | `Task`, the base the tasks share, whose `step` is `act(policy(observe(ctx)))`; `Trainer`, where actions come from while training |
| `src/network.rs` | `Network`: the ONNX network, run in Rust (`tract-onnx`) |
| `src/training.rs` | `Simulation`: load a mission, `observe`, `step` with actions |
| `ripple/learning/hover_env.py` | `HoverEnv`, the Gymnasium environment, with the reward |
| `learning/train_hover.py` | PPO training, a check on new episodes, and the ONNX export |

A task's policy is decided once, in `Task.initialize`: with `ctx.trainer`
present, the policy is the trainer's next action for that entity; without
it, the policy is the network read from the task's `policy_file` (the shipped
one) or the mission's `policy` key. The training `Simulation` registers the
`Python` autonomy through `PluginRegistry::register_autonomy_with`, which
builds each instance with a closure instead of `Plugin::new`, handing every
instance the trainer; `ripple run` registers it normally, so no `ctx.trainer`
exists and the network flies. Between steps, the `Simulation` gets the
environment's observation from a second instance of the task that it builds
from the mission's parameters, so `observe` must depend on `ctx` alone and
keep nothing on `self`; the `Task` docstring says so.

The replayed policy is deterministic: it takes the network's mean action, the
one SB3 uses with exploration noise off, and clamps it to [-1, 1]. A replay is
byte-identical from run to run and at any worker count, like any Ripple run,
and the Python tasks fly the shipped networks to the bit as the earlier Rust
plugins did.

## Changing the task

- **What the agent sees:** edit `Hover.observe` and `observation_size`,
  nothing else; `HoverEnv` reads the size from the class. No rebuild: the
  task is Python. A network trained on the old observation no longer loads,
  since `Network` checks the size.
- **What counts as good:** edit `HoverEnv.step`'s reward. It is Python only;
  the replay doesn't use it.
- **Where and how it flies:** edit the mission (goal, start spread, airframe,
  rates). Python trains on whatever the mission says.

## What makes this work

A few properties of Ripple let Python drive it one step at a time without
slowing the simulation down or making setup hard:

- **The simulation runs as compiled code.** Python only calls it, and the
  task's two methods are the only Python inside a step. One hover step, with
  all 25 physics substeps, takes about 36 µs, plus a few microseconds for the
  task ([The Python bridge](../development/python-bridge.md) has the cost).
- **Small crossings per step.** Python sends an action of a few numbers
  and gets an observation of a few numbers back, and the task sees a `ctx`
  of a few floats: no copy of the world, no network, no second process.
- **Ripple can be stepped from outside.** `Simulation::new` builds a fresh
  episode and `step` advances it one tick. Ripple keeps no global state, so
  many simulations can live in one Python process, and every random number is
  keyed by what it is for, so an episode depends only on its seed.
- **Building is one command.** `uv sync` compiles the Rust module through
  maturin, with nothing else to install.
- **Sharing is safe by construction.** Each simulation owns its state and
  its own `Trainer`, the one thing Python and the autonomies share. Many
  environments side by side can't corrupt each other.
- **The network runs in Rust.** `tract-onnx` loads the trained policy with
  no inference runtime to install, and gives the same answer every run.
- **One task class.** The same `observe` and `act` run in training, in
  replay, and in the test, so nothing has to be translated between them.

## How fast is it?

Training the hover policy runs at about 5,900 steps a second on a laptop
(Apple M2 Pro), so about 170 µs a step. Ripple's share is about 36 µs, with
all 25 physics substeps included; the rest is Python and PyTorch: the loop
over environments, running the network for each action, and PPO's updates.
A simulator that took no time at all would make this training only about a
quarter faster.

Simulators written in JAX (Brax, gymnax, MJX, JaxMARL) or for the GPU (Isaac
Lab) report millions of steps a second. Most of that comes from where the work
runs, not from faster physics: thousands of copies of the environment and the
policy step together on the GPU, and Python is not in the loop. Their models
are kept simple enough to run that way.

| A task that needs | Ripple from Python | A JAX or GPU simulator |
| --- | --- | --- |
| About a million steps (these demos) | minutes | seconds |
| About a hundred million (hard control, large teams) | hours to days | minutes |
| About a billion (legged locomotion, wide randomization) | impractical | hours |

Ripple is for the first row: learning a behavior on top of real vehicle
models (fixed-wing aircraft on a 6-DOF model with autopilots, checked
multirotors, sensors, estimators, and communication), in a mission you can
replay exactly and evaluate with the same models.

If you ever need more speed, there is a middle option before leaving Ripple:
one Rust call that steps many copies of the mission in parallel across the
CPU's cores, so Python is crossed once per batch rather than once per
environment. That would likely be ten times faster or more on a laptop, with
the same models. It is not built yet; build it when a real task needs it.

## Choices and why

| Choice | Options | Taken, and why |
| --- | --- | --- |
| Where training runs | Python with SB3 over PyO3; Rust with Burn | Python: students already know Gymnasium and SB3, and get every algorithm they offer. Burn would mean writing PPO. |
| Running the trained policy | ONNX Runtime; `tract-onnx`; PyTorch in the task | `tract-onnx`, exposed to the task as `Network`: pure Rust, nothing to install, and deterministic. |
| Where the task lives | Rust, with a Python window for training (how it began); Python for training with a generated Rust twin for flight; Python for both | Python for both: the point of the demo is students changing a task in Python with the trained and the flown code being one method. The cost is a Python call per agent per decision; the revisit trigger is in [The Python bridge](../development/python-bridge.md). |
| How actions get in | A global table; a new context field for every autonomy; a registry closure | The closure (`register_autonomy_with`), handing each `Python` autonomy the trainer as `ctx.trainer`: one small engine addition, and no other autonomy changes. |
| Vehicle and action | Motor speeds; attitude; velocity commands | Velocity commands to the checked Crazyflie controller: it trains in minutes. Motor-level control would be a research project, not a demo. |
| Rates | 10–100 Hz decisions | 20 Hz decisions, with 500 Hz control and motion (25 substeps), the rate the Crazyflie is checked at. |
| Reward | Below | −log(distance + 0.1 m). |
| PPO settings | SB3 defaults; tuned | Tuned: 16 environments, batches of 512, learning rate 1e-3, a 128×128 network, starting log standard deviation −0.5, two million steps. |

The reward and the PPO settings came from trials (2026-10-24, seed 0 unless
noted; median and worst distance from the goal at the end of 20 new episodes):

| Trial | Median | Worst |
| --- | --- | --- |
| −distance, SB3 defaults, 200k steps | 1.11 m | 1.73 m |
| −distance, SB3 defaults, 1M steps | 0.33 m | 0.63 m |
| −distance with less noise, γ 0.95, reward scaling, or learning rate 1e-3 (500k) | 0.29–0.52 m | 0.51–0.88 m |
| −distance − speed penalty (weights 0.1–1.0, 500k) | 0.34–0.52 m | 0.60–0.74 m |
| SAC, 60k steps (380 s) | 0.95 m | 5.07 m |
| −√distance, tuned (1M) | 0.072 m | 0.16 m |
| −log(distance + 0.1 m), SB3 defaults (1M) | 0.078 m | 0.22 m |
| −log(distance + 0.1 m), tuned (1M), seeds 0–3 | 0.032–0.22 m | 0.063–0.41 m |
| **−log(distance + 0.1 m), tuned (2M), seeds 0–1** | **0.026–0.033 m** | **0.093–0.13 m** |
| For scale: a hand-tuned PD controller | 0.037 m | 0.10 m |

With −distance the policy stopped improving about 0.3 m out, still circling
the goal at about 0.5 m/s. That reward pays the same for the last centimeter
as for the first meter, and the approach dominates the return. The logarithm
pays more the closer the drone gets. One million steps was enough for some
seeds and not others, so the script trains for two.

Training is deterministic for one seed on one machine, but sensitive: computing
the reward in float32 instead of float64 changed seed 0's result from 0.032 m
to 0.19 m. Compare settings over several seeds, not one run.

`crates/python/tests/test_learning.py` flies the shipped policy on five
more seeds and checks each ends within 0.2 m of the goal.
