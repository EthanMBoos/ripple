# Learning demos

Reinforcement learning on Ripple, out of the box: a task is one Python
autonomy class (`ripple.learning.hover.Hover`, `ripple.learning.spread.Spread`,
in `../ripple/learning/`), trained in Python with Stable-Baselines3 and flown
by Ripple from the saved ONNX network through the same class.

| Demo | Mission | Train | Book chapter |
| --- | --- | --- | --- |
| One Crazyflie flies to a goal and holds | `missions/hover.yaml` | `train_hover.py` | Training a single agent |
| Three agents cover three landmarks (MPE's simple_spread) | `missions/spread.yaml` | `train_spread.py` | Training a team |

From the repository root:

```sh
uv sync --extra train                             # builds the Rust module, installs PyTorch and SB3
uv run crates/python/learning/train_hover.py      # about 3 minutes; writes ripple/learning/policies/hover.onnx
uv run crates/python/learning/train_spread.py     # about 3 minutes; writes ripple/learning/policies/spread.onnx
uv run ripple run crates/python/learning/missions/hover.yaml --viewer
```

Trained policies ship in `../ripple/learning/policies/`, so flying works
without training. A task's `policy` mission key names another network.

- `../ripple/learning/`: `Task` and `Trainer` (`__init__.py`), the two tasks,
  the Gymnasium and PettingZoo environments (`hover_env.py`,
  `spread_env.py`, `team_vec_env.py`), and the ONNX export.
- `../src/network.rs`: the network, run in Rust by `tract-onnx`;
  `../src/training.rs`: the simulation Python steps while training.
- `../tests/test_learning.py`: flies the shipped policies on new seeds and
  steps the environments.
