"""Ripple missions as reinforcement-learning environments.

HoverEnv is a Gymnasium environment (one agent); SpreadEnv is a PettingZoo
parallel environment (a team). Both step a Ripple simulation through the
Rust module `_native`.
"""

from ripple_learning._native import Simulation
from ripple_learning.hover_env import HoverEnv
from ripple_learning.spread_env import SpreadEnv

__all__ = ["Simulation", "HoverEnv", "SpreadEnv"]
