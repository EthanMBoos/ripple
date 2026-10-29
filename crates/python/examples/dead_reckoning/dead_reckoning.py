"""Dead reckoning with occasional position fixes, in Python.

The starter crate's `DeadReckoning` localization, line for line: predict
position from the commanded velocity on each substep, replace it on a fix.
Attitude is not estimated.
"""


class DeadReckoning:
    # One input, the role `fix`, a full-state reading; only its position is used.
    inputs = {"fix": "state"}

    def __init__(self, speed_gain=1.0):
        if speed_gain < 0:
            raise ValueError("DeadReckoning speed_gain must be nonnegative")
        self.speed_gain = speed_gain

    def initialize(self, ctx):
        # Start from the first fix, at rest; the other fields keep their defaults.
        fix = ctx.inputs["fix"]
        if fix is None:
            raise RuntimeError("DeadReckoning needs a position fix at startup")
        return {"position": fix.value.position}

    def step(self, ctx):
        # Predict: move by the velocity the motion model was told to fly, on each substep.
        x, y, z = ctx.state.position
        velocity = ctx.state.velocity
        for command in ctx.commands:
            vx = command.values["velocity_x"] * self.speed_gain
            vy = command.values["velocity_y"] * self.speed_gain
            vz = command.values["velocity_z"] * self.speed_gain
            velocity = (vx, vy, vz)
            dt = command.time.dt_s
            x += vx * dt
            y += vy * dt
            z += vz * dt
        # Correct: a new fix replaces the predicted position.
        fix = ctx.inputs["fix"]
        if fix is not None and fix.fresh:
            x, y, z = fix.value.position
        return {"position": (x, y, z), "velocity": velocity}
