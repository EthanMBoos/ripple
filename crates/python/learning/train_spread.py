"""Train the spread team with PPO and save the policy where spread.yaml reads
it. Every agent shares one network (parameter sharing): TeamVecEnv turns
copies of the PettingZoo team into a batch of single-agent environments, one
per agent.

    uv run crates/python/learning/train_spread.py              # about 3 minutes on a laptop
    uv run crates/python/learning/train_spread.py --steps 300000

Then fly it, from the repository root:

    uv run ripple run crates/python/learning/missions/spread.yaml --viewer
"""

import argparse
from pathlib import Path

import numpy as np
from stable_baselines3 import PPO

from ripple.learning.export import export_onnx
from ripple.learning.spread import Spread
from ripple.learning.spread_env import SpreadEnv
from ripple.learning.team_vec_env import TeamVecEnv

POLICY = Spread.policy_file


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--steps", type=int, default=1_000_000)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--output", type=Path, default=POLICY)
    args = parser.parse_args()

    environments = TeamVecEnv(SpreadEnv, team_count=8, seed=args.seed)
    # SB3's defaults. Across seeds, 0-10% of new episodes end with two agents
    # on one landmark (book chapter "Training a team"); seed 0 had none in 50.
    model = PPO("MlpPolicy", environments, n_steps=256, seed=args.seed, verbose=1)
    model.learn(total_timesteps=args.steps)

    # Check on episodes the team never trained on: other mission seeds,
    # exploration noise off. Coverage: each landmark's nearest agent.
    environment = SpreadEnv()
    worst_gaps_m = []
    for episode in range(20):
        observations, _ = environment.reset(seed=10_000 + episode)
        while environment.agents:
            actions = {
                agent: model.predict(observation, deterministic=True)[0]
                for agent, observation in observations.items()
            }
            observations, _, _, _, _ = environment.step(actions)
        end = 2 + 2 * environment.landmark_count
        distances = np.array([
            np.linalg.norm(observation[2:end].reshape(-1, 2), axis=1)
            for observation in observations.values()
        ])
        worst_gaps_m.append(distances.min(axis=0).max())
    print(f"farthest landmark from its nearest agent at the end, 20 new episodes: "
          f"median {np.median(worst_gaps_m):.3f} m, worst {max(worst_gaps_m):.3f} m")

    export_onnx(model, args.output)
    print(f"saved {args.output}")


if __name__ == "__main__":
    main()
