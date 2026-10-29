"""Noisy own-state measurements, in Python.

The built-in `NoisyState` sensor, number for number: Gaussian error with a
bias and a standard deviation per axis on position, velocity, and attitude,
each keyed by which reading of the tick it is, the quantity (0 position,
1 velocity, 2 attitude), and the axis. Angular velocity is reported exactly.
The attitude error is applied by quaternion products in roll, pitch, yaw
order, like the Rust sensor, so the two agree to the last bit.
"""

import math

SENSOR_PHASE_READING = 0
FIRST_READING = 1  # the first reading shares a tick with a sensor-phase one


def axis_angle(axis, angle):
    """A unit quaternion (w, x, y, z) for a rotation about one of x, y, z."""
    half = angle / 2
    sine, cosine = math.sin(half), math.cos(half)
    xyz = [0.0 * sine, 0.0 * sine, 0.0 * sine]
    xyz[axis] = 1.0 * sine
    return (cosine, *xyz)


def product(p, q):
    """The Hamilton product of two (w, x, y, z) quaternions."""
    w1, x1, y1, z1 = p
    w2, x2, y2, z2 = q
    return (
        w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2,
        w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2,
        w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2,
        w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2,
    )


class NoisyState:
    reading_kind = "state"

    def __init__(self, position_bias_m=(0, 0, 0), position_stddev_m=(0, 0, 0),
                 velocity_bias_mps=(0, 0, 0), velocity_stddev_mps=(0, 0, 0),
                 attitude_bias_rad=(0, 0, 0), attitude_stddev_rad=(0, 0, 0)):
        self.bias = (position_bias_m, velocity_bias_mps, attitude_bias_rad)
        self.stddev = (position_stddev_m, velocity_stddev_mps, attitude_stddev_rad)
        for stddevs in self.stddev:
            if any(stddev < 0 for stddev in stddevs):
                raise ValueError("standard deviations must be nonnegative")

    def initialize(self, ctx):
        return self.measure(ctx, FIRST_READING)

    def step(self, ctx):
        return self.measure(ctx, SENSOR_PHASE_READING)

    def measure(self, ctx, reading):
        truth = ctx.truth

        def error(quantity, axis):
            return ctx.random.normal((reading, quantity, axis),
                                     self.bias[quantity][axis], self.stddev[quantity][axis])

        position = tuple(truth.position[axis] + error(0, axis) for axis in range(3))
        velocity = tuple(truth.velocity[axis] + error(1, axis) for axis in range(3))
        orientation = truth.orientation
        for axis in range(3):  # roll, then pitch, then yaw, on the right
            orientation = product(orientation, axis_angle(axis, error(2, axis)))
        return {"position": position, "velocity": velocity, "orientation": orientation}
