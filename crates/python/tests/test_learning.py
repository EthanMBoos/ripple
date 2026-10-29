"""The learning tasks: the shipped policies flown through the `Python`
autonomy on seeds they never trained on, and the training environments
stepping the same classes."""

import math
import re
import sys
from pathlib import Path

import numpy as np
import pytest

from test_python_plugins import CHANNELS, ROOT, read_run, run  # noqa: F401

MISSIONS = Path(__file__).resolve().parents[1] / "learning" / "missions"


def with_seed(mission, seed, path):
    text = mission.read_text()
    assert re.search(r"^  seed: 1$", text, re.M)
    path.write_text(re.sub(r"^  seed: 1$", f"  seed: {seed}", text, flags=re.M))
    return path


def final_positions(mission, seed, output, *options):
    frames = read_run(run(with_seed(mission, seed, output.with_suffix(".yaml")), output, *options)).frames
    return [entity.position for entity in frames[-1].entities.values()]


@pytest.mark.parametrize("seed", range(100, 105))
def test_the_hover_policy_holds_the_goal(tmp_path, seed):
    (position,) = final_positions(MISSIONS / "hover.yaml", seed, tmp_path / "out")
    miss_m = math.dist(position, (0.0, 0.0, 5.0))
    assert miss_m < 0.2, f"seed {seed}: {miss_m:.3f} m from the goal"


@pytest.mark.parametrize("seed", range(100, 105))
def test_the_spread_team_covers_every_landmark(tmp_path, seed):
    positions = final_positions(MISSIONS / "spread.yaml", seed, tmp_path / "out")
    for east_m, north_m in [(-4.0, -3.0), (4.0, -3.0), (0.0, 4.0)]:
        gap_m = min(math.hypot(x - east_m, y - north_m) for x, y, _ in positions)
        assert gap_m < 0.5, f"seed {seed}: landmark ({east_m}, {north_m}) is {gap_m:.3f} m from the nearest agent"


def test_the_spread_mission_is_byte_identical_across_workers(tmp_path):
    one = run(MISSIONS / "spread.yaml", tmp_path / "one", "--workers", "1").read_bytes()
    eight = run(MISSIONS / "spread.yaml", tmp_path / "eight", "--workers", "8").read_bytes()
    assert one == eight


def test_the_hover_environment_steps_the_same_task():
    from ripple.learning.hover_env import HoverEnv

    environment = HoverEnv()
    observation, info = environment.reset(seed=3)
    assert observation.shape == (6,) and "mission_seed" in info
    # Belief starts at rest, within a few meters of the goal at (0, 0, 5).
    assert np.all(observation[3:] == 0.0) and np.linalg.norm(observation[:3]) < 10.0
    for _ in range(5):
        observation, reward, terminated, truncated, _ = environment.step(np.zeros(3, np.float32))
    assert not terminated and not truncated and math.isfinite(reward)
    assert environment.simulation.time_s == pytest.approx(0.25)


def test_the_spread_environment_rewards_the_whole_team():
    from ripple.learning.spread_env import SpreadEnv

    environment = SpreadEnv()
    observations, _ = environment.reset(seed=3)
    assert sorted(observations) == ["agent_1", "agent_2", "agent_3"]
    assert all(observation.shape == (12,) for observation in observations.values())
    actions = {agent: np.zeros(2, np.float32) for agent in environment.agents}
    _, rewards, _, _, _ = environment.step(actions)
    assert len(set(rewards.values())) == 1  # coverage is shared, nobody collides at rest


def test_a_step_without_an_action_is_an_error():
    from ripple.learning.spread_env import SpreadEnv

    environment = SpreadEnv()
    environment.reset(seed=3)
    with pytest.raises(RuntimeError, match="the trainer set no action for entity"):
        environment.simulation.step({})
