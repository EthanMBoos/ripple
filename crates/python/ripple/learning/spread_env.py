"""PettingZoo parallel environment for crates/python/learning/missions/spread.yaml:
a team of point agents spreads out to cover one landmark each (MPE's
simple_spread)."""

from pathlib import Path

import gymnasium as gym
import numpy as np
from pettingzoo import ParallelEnv

from ripple.learning import Simulation

MISSION = Path(__file__).resolve().parents[2] / "learning" / "missions" / "spread.yaml"


class SpreadEnv(ParallelEnv):
    """Each agent observes its velocity, every landmark's offset, and every
    teammate's offset (the Spread task's own `observe`). Action: a
    horizontal velocity command, each number in [-1, 1] times the mission's
    max_speed_mps. Reward, as in MPE: minus the sum over landmarks of the
    nearest agent's distance, shared by the team, and minus one for each
    teammate closer than `collision_distance_m`, for that agent alone.
    """

    metadata = {"name": "ripple_spread_v0"}
    render_mode = None

    def __init__(self, mission=MISSION, collision_distance_m=1.0):
        self.mission = str(mission)
        self.collision_distance_m = collision_distance_m
        self.random = np.random.default_rng()
        self.simulation = None
        # One look at the mission for its agents and sizes; not kept, so the
        # environment can be copied before its first reset.
        probe = Simulation(self.mission, 0)
        self.ids = {f"agent_{id}": id for id in probe.agents}
        self.possible_agents = list(self.ids)
        self.agents = []
        first = probe.agents[0]
        observation_size = len(probe.observe(first))
        action_size = probe.action_size(first)
        self.landmark_count = (observation_size + 2) // 4
        self._observation_space = gym.spaces.Box(
            -np.inf, np.inf, (observation_size,), np.float32
        )
        self._action_space = gym.spaces.Box(-1.0, 1.0, (action_size,), np.float32)

    def observation_space(self, agent):
        return self._observation_space

    def action_space(self, agent):
        return self._action_space

    def reset(self, seed=None, options=None):
        if seed is not None:
            self.random = np.random.default_rng(seed)
        mission_seed = int(self.random.integers(2**31))
        self.simulation = Simulation(self.mission, mission_seed)
        self.agents = list(self.possible_agents)
        infos = {agent: {"mission_seed": mission_seed} for agent in self.agents}
        return self._observe(), infos

    def step(self, actions):
        running = self.simulation.step(
            {self.ids[agent]: np.asarray(action).tolist() for agent, action in actions.items()}
        )
        observations = self._observe()
        rewards = self._rewards(observations)
        terminations = {agent: False for agent in self.agents}
        # The run ending at its time limit is a truncation (see HoverEnv).
        truncations = {agent: not running for agent in self.agents}
        infos = {agent: {} for agent in self.agents}
        if not running:
            self.agents = []
        return observations, rewards, terminations, truncations, infos

    def _observe(self):
        return {
            agent: np.asarray(self.simulation.observe(self.ids[agent]), np.float32)
            for agent in self.agents
        }

    def _rewards(self, observations):
        # Observation layout: velocity (2), landmark offsets (2 each), then
        # teammate offsets (2 each).
        landmarks_end = 2 + 2 * self.landmark_count
        landmark_distances = np.array([
            np.linalg.norm(observation[2:landmarks_end].reshape(-1, 2), axis=1)
            for observation in observations.values()
        ])
        coverage = -landmark_distances.min(axis=0).sum()
        rewards = {}
        for agent, observation in observations.items():
            teammates = observation[landmarks_end:].reshape(-1, 2)
            collisions = np.sum(np.linalg.norm(teammates, axis=1) < self.collision_distance_m)
            rewards[agent] = float(coverage - collisions)
        return rewards
