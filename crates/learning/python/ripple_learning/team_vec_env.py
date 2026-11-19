"""Train one policy for every agent of a PettingZoo team with
Stable-Baselines3 (parameter sharing): each agent of each team copy is one
slot of an SB3 vector environment. Written here because SuperSuit's
adapter no longer works with current SB3."""

import numpy as np
from stable_baselines3.common.vec_env import VecEnv


class TeamVecEnv(VecEnv):
    """`team_count` copies of a parallel environment whose agents all act
    and finish together, as SpreadEnv's do."""

    def __init__(self, make_team, team_count, seed=0):
        self.teams = [make_team() for _ in range(team_count)]
        self.agents = self.teams[0].possible_agents
        first = self.agents[0]
        super().__init__(
            team_count * len(self.agents),
            self.teams[0].observation_space(first),
            self.teams[0].action_space(first),
        )
        self.next_seed = seed
        self.actions = None

    def _reset_team(self, team):
        observations, _ = team.reset(seed=self.next_seed)
        self.next_seed += 1
        return [observations[agent] for agent in self.agents]

    def reset(self):
        return np.array([o for team in self.teams for o in self._reset_team(team)])

    def step_async(self, actions):
        self.actions = actions

    def step_wait(self):
        observations, rewards, dones, infos = [], [], [], []
        size = len(self.agents)
        for index, team in enumerate(self.teams):
            actions = self.actions[index * size : (index + 1) * size]
            step = team.step(dict(zip(self.agents, actions)))
            team_observations, team_rewards, terminations, truncations, _ = step
            done = terminations[self.agents[0]] or truncations[self.agents[0]]
            for agent in self.agents:
                info = {}
                if done:
                    # SB3 bootstraps from the last observation of a truncated
                    # episode, then the slot starts the next episode.
                    info["terminal_observation"] = team_observations[agent]
                    info["TimeLimit.truncated"] = truncations[agent] and not terminations[agent]
                rewards.append(team_rewards[agent])
                dones.append(done)
                infos.append(info)
            observations.extend(
                self._reset_team(team) if done else [team_observations[a] for a in self.agents]
            )
        return np.array(observations), np.array(rewards, np.float32), np.array(dones), infos

    def close(self):
        pass

    def seed(self, seed=None):
        if seed is not None:
            self.next_seed = seed
        return [None] * self.num_envs

    def get_attr(self, attr_name, indices=None):
        return [None] * len(self._get_indices(indices))

    # PPO never calls these; algorithms that do (HER) need a different adapter.
    def set_attr(self, attr_name, value, indices=None):
        raise NotImplementedError("TeamVecEnv does not support set_attr")

    def env_method(self, method_name, *args, indices=None, **kwargs):
        raise NotImplementedError("TeamVecEnv does not support env_method")

    def env_is_wrapped(self, wrapper_class, indices=None):
        return [False] * len(self._get_indices(indices))
