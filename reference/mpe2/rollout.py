"""Run seeded random episodes of MPE2's simple_spread_v3 and record them.

    python rollout.py SEEDS OUTPUT.json

Run by reference/mpe_parity.py in an environment with MPE2 installed. For
each seed: reset with that seed, then act with continuous actions drawn
uniformly from [0, 1]^5 by numpy's generator seeded the same way. Records
the agents' and landmarks' starting positions, the force each agent's action
became (MPE's `agent.action.u`, after the 5x sensitivity), and after every
step each agent's position, velocity, observation, and reward.
"""
import json
import sys

import numpy as np
from mpe2 import simple_spread_v3


def episode(seed):
    env = simple_spread_v3.parallel_env(continuous_actions=True)
    env.reset(seed=seed)
    world = env.unwrapped.world
    observations = {a.name: env.unwrapped.observe(a.name) for a in world.agents}
    rng = np.random.default_rng(seed)
    record = dict(
        seed=seed,
        agents=[a.state.p_pos.tolist() for a in world.agents],
        landmarks=[lm.state.p_pos.tolist() for lm in world.landmarks],
        observations=[[observations[a.name].tolist() for a in world.agents]],
        forces=[], positions=[], velocities=[], rewards=[])
    while env.agents:
        actions = {name: rng.uniform(0, 1, 5).astype(np.float32) for name in env.agents}
        observations, rewards, _, _, _ = env.step(actions)
        record["forces"].append([a.action.u.tolist() for a in world.agents])
        record["positions"].append([a.state.p_pos.tolist() for a in world.agents])
        record["velocities"].append([a.state.p_vel.tolist() for a in world.agents])
        record["observations"].append([observations[a.name].tolist() for a in world.agents])
        record["rewards"].append([rewards[a.name] for a in world.agents])
    return record


def main():
    seeds = [int(seed) for seed in sys.argv[1].split(",")]
    with open(sys.argv[2], "w") as out:
        json.dump([episode(seed) for seed in seeds], out)


if __name__ == "__main__":
    main()
