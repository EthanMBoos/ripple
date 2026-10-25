# Learning demos

Reinforcement learning on Ripple, out of the box: train a policy in Python
with Stable-Baselines3, then fly it in Ripple from the saved ONNX network.

| Demo | Mission | Train | Book chapter |
| --- | --- | --- | --- |
| One Crazyflie flies to a goal and holds | `missions/hover.yaml` | `python/train_hover.py` | Training a single agent |
| Three agents cover three landmarks (MPE's simple_spread) | `missions/spread.yaml` | `python/train_spread.py` | Training a team |

```sh
cd crates/learning/python
uv sync --extra train      # builds the Rust module, installs PyTorch and SB3
uv run train_hover.py      # about 3 minutes; writes ../policies/hover.onnx
uv run train_spread.py     # about 3 minutes; writes ../policies/spread.onnx
cd ../../..
cargo run -p ripple --features learning -- run crates/learning/missions/hover.yaml --viewer
```

Trained policies ship in `policies/`, so the replay works without training.
After changing this crate's Rust code, rebuild the Python module with `uv sync
--reinstall-package ripple-learning`.

- `src/`: the `HoverPolicy` and `SpreadPolicy` autonomies, each with the
  `observe` function training and replay share, and `policy.rs`, which runs
  the ONNX network (`tract-onnx`) or takes the trainer's actions.
- `python/`: the Python module (`src/lib.rs`, PyO3), the Gymnasium and
  PettingZoo environments (`ripple_learning/`), and the training scripts.
- `tests/policies.rs`: replays the shipped policies on new seeds.

The crate is outside the default build: the `ripple` command includes it with
`--features learning`.
