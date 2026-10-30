# Training a team

This chapter trains three agents to spread out and cover three landmarks,
MPE's `simple_spread` task rebuilt on Ripple's point model. Every agent runs
the same network. Read [Training a single agent](training-single-agent.md)
first: the setup, the pieces, and how a trained network gets back into Ripple
are the same.

## Quick start

From the repository root, after the setup in
[Training a single agent](training-single-agent.md#quick-start):

```sh
python crates/python/learning/train_spread.py    # about 3 minutes; writes ripple/learning/policies/spread.onnx
ripple run crates/python/learning/missions/spread.yaml --viewer
```

The package ships a trained `spread.onnx`. Training prints how far the
least-covered landmark is from its nearest agent at the end of 20 new
episodes:

```text
farthest landmark from its nearest agent at the end, 20 new episodes: median 0.181 m, worst 0.235 m
```

The viewer shows the agents, not the landmarks; they are at (−4, −3), (4, −3),
and (0, 4).

## The task

`crates/python/learning/missions/spread.yaml` has one entity group with
`count: 3`, `SingleIntegrator` motion, and the `Python` autonomy running the
`Spread` task (`class: ripple.learning.spread.Spread`). Each mission seed
scatters the agents differently, a few meters around the origin.

| | |
| --- | --- |
| Observation, per agent | Own velocity (2), each landmark's offset (2 each), each teammate's offset in ID order (2 each): 12 numbers, east and north, from belief |
| Action, per agent | A horizontal velocity command: 2 numbers in [-1, 1], times `max_speed_mps` (2 m/s) |
| Reward, per agent | MPE's: minus the sum over landmarks of the nearest agent's distance, shared by the team, minus 1 for each teammate within 1 m |
| Episode | One run of the mission: 15 s, 150 decisions at 10 Hz |

Teammates are the contacts on the agent's own team that its tracker knows
(`ctx.contacts`), so with a noisy contact sensor the policy sees noisy
teammates, in training and in replay. The policy needs every teammate: one the
tracker has lost, such as one out of a sensor's range, stops the run with an
error.

## The pieces

All under `crates/python`:

| File | What it does |
| --- | --- |
| `ripple/learning/spread.py` | `Spread`: `observe` and `act` |
| `ripple/learning/spread_env.py` | `SpreadEnv`, a PettingZoo parallel environment, with the reward |
| `ripple/learning/team_vec_env.py` | `TeamVecEnv`: turns copies of the team into SB3's vector environment, one slot per agent |
| `learning/train_spread.py` | PPO training, a check on new episodes, and the ONNX export |

One network for every agent is called parameter sharing. SB3 trains one agent
at a time, so `TeamVecEnv` presents each agent of each team copy as a separate
environment; every agent's experience trains the same network. The agents
still differ in what they see, which is how they come to pick different
landmarks.

## Changing the task

- **More agents:** add landmarks (`landmarks_m`) and raise `count` to match;
  `Spread` requires one agent per landmark. The observation grows by 4
  numbers per agent, so train a new network.
- **Noisy sensing:** give the agents a `NoisyContacts` sensor in the mission,
  with a range that covers the area. The policy sees what the tracker
  believes. Messages play no part: the policy reads teammates from its
  tracker, not from a network.
- **A different team reward:** edit `SpreadEnv._rewards`. It is computed from
  the observations, so it needs nothing else from the simulation.

## Choices and why

| Choice | Options | Taken, and why |
| --- | --- | --- |
| Multi-agent training | SuperSuit's SB3 adapter; a multi-agent library (MAPPO); our own adapter | Our own short `TeamVecEnv`: SuperSuit's adapter fails with SB3 2.9 (it passes `seed` to an object without one), and a multi-agent library is one more thing to learn. |
| Vehicle | Multirotors; the point model | `SingleIntegrator`: the lesson is coordination, and it trains in minutes. Swap the motion model in the mission to try a vehicle. |
| Reward | MPE's; the log reward that helped the hover | MPE's, so results compare with the benchmark. The log version did no better on the seeds that failed (below). |
| PPO settings | SB3 defaults; the hover's tuned settings | Defaults, one million steps. |

Trials (2026-10-24): episodes, out of 50 new ones, that end with a landmark
more than 1 m from every agent, which happens when two agents settle on the
same landmark.

| Trial | Seed 0 | Seed 1 | Seed 2 | Seed 3 |
| --- | --- | --- | --- | --- |
| MPE reward, defaults, 1M steps | **0** | 3 | 5 | 1 |
| MPE reward, hover's tuned settings | | 0 | 30 | |
| Log reward, defaults | | 3 | 0 | |
| Log reward, tuned | | 2 | 3 | |

At 500k steps, seeds 0 and 3 already covered every landmark in 20 episodes;
seeds 1 and 2 did not. No change fixed every seed: settling on the same
landmark is the coordination problem `simple_spread` is known for, and
methods that train a critic on the whole team's state (MAPPO) target it. The
shipped policy is seed 0's.

`crates/python/tests/test_learning.py` flies it on five more seeds and
checks every landmark ends within 0.5 m of an agent.
