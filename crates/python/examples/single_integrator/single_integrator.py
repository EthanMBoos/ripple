"""A kinematic point, in Python: the built-in `SingleIntegrator` motion model.

Position integrates the commanded velocity in the world frame, at once, with
an optional speed cap. The body points along the velocity. A motion model's
`ports` has inputs only; `ctx.truth` is the vehicle's true state, and the
answer is the truth's changed fields.
"""

import math


def from_euler(roll, pitch, yaw):
    """A unit quaternion (w, x, y, z), world from body, from roll, pitch, yaw."""
    sin_roll, cos_roll = math.sin(roll * 0.5), math.cos(roll * 0.5)
    sin_pitch, cos_pitch = math.sin(pitch * 0.5), math.cos(pitch * 0.5)
    sin_yaw, cos_yaw = math.sin(yaw * 0.5), math.cos(yaw * 0.5)
    return (
        cos_roll * cos_pitch * cos_yaw + sin_roll * sin_pitch * sin_yaw,
        sin_roll * cos_pitch * cos_yaw - cos_roll * sin_pitch * sin_yaw,
        cos_roll * sin_pitch * cos_yaw + sin_roll * cos_pitch * sin_yaw,
        cos_roll * cos_pitch * sin_yaw - sin_roll * sin_pitch * cos_yaw,
    )


class SingleIntegrator:
    ports = {"inputs": {"velocity_x": ("m/s", "world"),
                        "velocity_y": ("m/s", "world"),
                        "velocity_z": ("m/s", "world")}}

    def __init__(self, max_speed_mps=-1.0):
        # Negative (the default) means no limit.
        self.max_speed_mps = max_speed_mps

    def initialize(self, ctx):
        # At rest, level, keeping the mission's heading.
        return {"velocity": (0.0, 0.0, 0.0),
                "orientation": from_euler(0.0, 0.0, ctx.truth.heading)}

    def step(self, ctx):
        vx, vy, vz = (ctx.inputs["velocity_x"], ctx.inputs["velocity_y"],
                      ctx.inputs["velocity_z"])
        speed = math.sqrt(vx * vx + vy * vy + vz * vz)
        if self.max_speed_mps >= 0.0 and speed > self.max_speed_mps:
            vx, vy, vz = (vx / speed * self.max_speed_mps, vy / speed * self.max_speed_mps,
                          vz / speed * self.max_speed_mps)
        x, y, z = ctx.truth.position
        dt = ctx.time.dt_s
        position = (x + vx * dt, y + vy * dt, z + vz * dt)
        if not all(math.isfinite(value) for value in position):
            raise ValueError("SingleIntegrator produced nonfinite position")
        pitch = math.atan2(vz, math.sqrt(vx * vx + vy * vy))
        yaw = math.atan2(vy, vx)
        return {"velocity": (vx, vy, vz), "position": position,
                "orientation": from_euler(0.0, pitch, yaw)}
