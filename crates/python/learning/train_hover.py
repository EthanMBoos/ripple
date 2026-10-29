"""Train the hover policy with PPO and save it as the task's shipped network.

    uv run crates/python/learning/train_hover.py              # about 3 minutes on a laptop
    uv run crates/python/learning/train_hover.py --steps 500000

Then fly it, from the repository root:

    uv run ripple run crates/python/learning/missions/hover.yaml --viewer
"""

import argparse
from pathlib import Path

import numpy as np
from stable_baselines3 import PPO
from stable_baselines3.common.env_util import make_vec_env

from ripple.learning.export import export_onnx
from ripple.learning.hover import Hover
from ripple.learning.hover_env import HoverEnv

POLICY = Hover.policy_file


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--steps", type=int, default=2_000_000)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--output", type=Path, default=POLICY)
    args = parser.parse_args()

    environments = make_vec_env(HoverEnv, n_envs=16, seed=args.seed)
    # Larger batches, a faster learning rate, a wider network, and less
    # starting noise than SB3's defaults: with these, two million steps hold
    # the goal to about 3 cm for every seed tried; one million steps is
    # 3-22 cm depending on the seed (book chapter "Training a single agent").
    model = PPO(
        "MlpPolicy",
        environments,
        n_steps=256,
        batch_size=512,
        learning_rate=1e-3,
        policy_kwargs=dict(net_arch=[128, 128], log_std_init=-0.5),
        seed=args.seed,
        verbose=1,
    )
    model.learn(total_timesteps=args.steps)

    # Check on episodes the policy never trained on: other mission seeds,
    # exploration noise off.
    environment = HoverEnv()
    final_distances_m = []
    for episode in range(20):
        observation, _ = environment.reset(seed=10_000 + episode)
        truncated = False
        while not truncated:
            action, _ = model.predict(observation, deterministic=True)
            observation, _, _, truncated, _ = environment.step(action)
        final_distances_m.append(np.linalg.norm(observation[:3]))
    print(f"distance to goal at the end, 20 new episodes: "
          f"median {np.median(final_distances_m):.3f} m, worst {max(final_distances_m):.3f} m")

    export_onnx(model, args.output)
    print(f"saved {args.output}")


if __name__ == "__main__":
    main()
