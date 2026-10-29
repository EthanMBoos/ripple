"""Single-agent task: a multirotor flies to a goal and holds there.

The policy sees the offset to the goal and own velocity, from belief, and
answers with a velocity command for MultirotorVelocityController. Book
chapter: "Training a single agent".
"""

from pathlib import Path

from ripple.learning import Task


class Hover(Task):
    ports = {"outputs": {"velocity_x": ("m/s", "world"),
                         "velocity_y": ("m/s", "world"),
                         "velocity_z": ("m/s", "world")}}
    observation_size = 6
    action_size = 3
    policy_file = Path(__file__).with_name("policies") / "hover.onnx"

    def __init__(self, goal_m=(0.0, 0.0, 5.0), max_speed_mps=2.0, policy=None):
        super().__init__(policy)
        if max_speed_mps <= 0.0:
            raise ValueError("Hover max_speed_mps must be positive")
        self.goal_m = tuple(goal_m)
        self.max_speed_mps = max_speed_mps

    def observe(self, ctx):
        """The offset to the goal (m), then own velocity (m/s)."""
        x, y, z = ctx.state.position
        goal_x, goal_y, goal_z = self.goal_m
        return [goal_x - x, goal_y - y, goal_z - z, *ctx.state.velocity]

    def act(self, action):
        """Each action number in [-1, 1] scales `max_speed_mps`."""
        if len(action) != self.action_size:
            raise ValueError(f"Hover needs {self.action_size} action numbers, got {len(action)}")
        return {"velocity_x": action[0] * self.max_speed_mps,
                "velocity_y": action[1] * self.max_speed_mps,
                "velocity_z": action[2] * self.max_speed_mps}
