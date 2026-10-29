# A test plugin: each tick, a heading change drawn from a numpy generator
# seeded by the engine's keyed numbers, so the walk replays and is the same at
# any worker count. Flies straight at a fixed altitude and speed otherwise.
import numpy as np


class RandomWalk:
    ports = {"outputs": {"desired_heading": ("rad", "world"),
                         "desired_altitude": ("m", "world"),
                         "desired_speed": ("m/s", "none")}}

    def __init__(self, speed_mps=20.0, turn_stddev_rad=0.3):
        self.speed_mps = speed_mps
        self.turn_stddev_rad = turn_stddev_rad

    def step(self, ctx):
        rng = np.random.default_rng(ctx.random.seed("turn"))
        turn = rng.normal(0.0, self.turn_stddev_rad)
        return {"desired_heading": ctx.state.heading + turn,
                "desired_altitude": ctx.state.position[2],
                "desired_speed": self.speed_mps}
