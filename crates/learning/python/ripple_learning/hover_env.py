"""Gymnasium environment for crates/learning/missions/hover.yaml: one
Crazyflie flies to a goal and holds there."""

from pathlib import Path

import gymnasium as gym
import numpy as np

from ripple_learning._native import Simulation

MISSION = Path(__file__).resolve().parents[2] / "missions" / "hover.yaml"


class HoverEnv(gym.Env):
    """Observation: the offset to the goal (m) and own velocity (m/s), from
    HoverPolicy's own `observe` in Rust. Action: a velocity command, each
    number in [-1, 1] times the mission's max_speed_mps. Reward, every step:
    -log(distance + 0.1 m), which keeps paying for the last few centimeters;
    minus the plain distance stopped 0.3 m out, circling (see the book
    chapter "Training a single agent"). An episode is one run of the mission;
    each reset draws a new mission seed, so the drone starts somewhere else.
    """

    def __init__(self, mission=MISSION):
        self.mission = str(mission)
        self.simulation = None
        self.observation_space = gym.spaces.Box(-np.inf, np.inf, (6,), np.float32)
        self.action_space = gym.spaces.Box(-1.0, 1.0, (3,), np.float32)

    def reset(self, seed=None, options=None):
        super().reset(seed=seed)
        mission_seed = int(self.np_random.integers(2**31))
        self.simulation = Simulation(self.mission, mission_seed)
        (self.agent,) = self.simulation.agents
        return self._observe(), {"mission_seed": mission_seed}

    def step(self, action):
        running = self.simulation.step({self.agent: np.asarray(action).tolist()})
        observation = self._observe()
        distance_m = float(np.linalg.norm(observation[:3]))
        reward = -np.log(distance_m + 0.1)
        # The run ending at its time limit is a truncation, not a terminal
        # state: the drone could have kept flying.
        return observation, reward, False, not running, {}

    def _observe(self):
        return np.asarray(self.simulation.observe(self.agent), np.float32)
