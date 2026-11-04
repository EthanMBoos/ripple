# Training a single agent

The optional `learning` crate shows reinforcement learning on Ripple: a policy
trained in Python with Stable-Baselines3, then flown by Ripple from the saved
network. This chapter trains one Crazyflie to fly to a goal and hold there.
[Training a team](training-team.md) trains three agents together.

## How to think about it

Two programs share the work:

| | Owns |
| --- | --- |
| **Ripple** | The mission, the physics, and what the policy sees: the `HoverPolicy` autonomy's `observe` function. |
| **Python** | The reward, the learning algorithm, and the training loop: `HoverEnv` (Gymnasium) and `train_hover.py`. |

While Python trains, it steps the simulation itself: it asks Ripple for the
agent's observation, picks an action, and runs one tick with it. When training
ends, it saves the policy's network as an ONNX file. A replay runs the same
mission in `ripple`, where `HoverPolicy` feeds its own observation to that
network. The observation comes from one Rust function in both cases, so the
network sees in replay exactly what it saw in training.

## Quick start

You need [uv](https://docs.astral.sh/uv/) and Rust. From the repository root:

```sh
cd crates/learning/python
uv sync --extra train     # builds the Rust module, installs PyTorch and SB3
uv run train_hover.py     # about 3 minutes; writes ../policies/hover.onnx
cd ../../..
cargo run -p ripple --features learning -- run crates/learning/missions/hover.yaml --viewer
```

The repository ships a trained `hover.onnx`, so the last command works
without training first. Training prints the drone's distance from the goal at
the end of 20 episodes it never trained on:

```text
distance to goal at the end, 20 new episodes: median 0.033 m, worst 0.126 m
```

After changing Rust code in the learning crate, rebuild the module with `uv
sync --reinstall-package ripple-learning`.

## The task

`crates/learning/missions/hover.yaml` is an ordinary mission: one Crazyflie
with `MultirotorVelocityController` and `Multirotor`, as in [Flying a
multirotor](multirotor.md), starting at a random point a few meters from the
goal (each mission seed is a different start).

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

| File | What it does |
| --- | --- |
| `crates/learning/src/hover.rs` | `HoverPolicy`: `observe`, and the step that turns an action into a velocity command |
| `crates/learning/src/policy.rs` | Where actions come from: the ONNX network (`tract-onnx`), or the Python trainer |
| `crates/learning/python/src/lib.rs` | The Python module: load a mission, `observe`, `step` with actions |
| `crates/learning/python/ripple_learning/hover_env.py` | `HoverEnv`, the Gymnasium environment, with the reward |
| `crates/learning/python/train_hover.py` | PPO training, a check on new episodes, and the ONNX export |

Python sends actions in through `PluginRegistry::register_autonomy_with`,
which builds each autonomy with a closure instead of `Autonomy::new`. The
Python module registers `HoverPolicy` that way, handing every instance the
table the trainer writes actions to (`learning::register_for_training`). The
stock `ripple` command registers it normally, so it reads its `policy` file.

The replayed policy is deterministic: it takes the network's mean action, the
one SB3 uses with exploration noise off, and clamps it to [-1, 1]. A replay is
byte-identical from run to run, like any Ripple run.

## Changing the task

- **What the agent sees:** edit `HoverConfig::observe` and the observation
  size (`OBSERVATION_SIZE` and `HoverEnv`'s `observation_space`). Python picks
  up the change after the rebuild above. A network trained on the old
  observation no longer loads: `HoverPolicy` checks the size.
- **What counts as good:** edit `HoverEnv.step`'s reward. It is Python only;
  the replay doesn't use it.
- **Where and how it flies:** edit the mission (goal, start spread, airframe,
  rates). Python trains on whatever the mission says.

## Training speed and settings

Python sends a small action to Ripple and receives a small observation at each
step. Ripple runs the 25 control and physics substeps in compiled code. On an
Apple M2 Pro, training reached about 5,900 steps/s (170 µs per step); the
simulation used about 36 µs of that step. Python, policy inference, and PPO
accounted for the rest. The trained network runs in Ripple through
`tract-onnx`, using the same Rust observation function as training.

The shipped policy uses velocity commands at 20 Hz, with control and motion
at 500 Hz. PPO trains on 16 environments with batches of 512, learning rate
1e-3, a 128×128 network, and initial log standard deviation −0.5. It trains
for two million steps. The trials below used seed 0 unless noted. Each result
is the median and worst distance from the goal over 20 new episodes.

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
the goal at about 0.5 m/s. The logarithmic reward places more weight on the
last part of the approach. One million steps worked for some seeds but not
others, so the script trains for two million.

Training is deterministic for one seed on one machine, but sensitive: computing
the reward in float32 instead of float64 changed seed 0's result from 0.032 m
to 0.19 m. Compare settings over several seeds, not one run.

`crates/learning/tests/policies.rs` replays the shipped policy in Ripple on
five more seeds and checks each ends within 0.2 m of the goal.
