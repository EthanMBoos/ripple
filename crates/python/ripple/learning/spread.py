"""Multi-agent task: a team spreads out to cover one landmark each, MPE's
simple_spread on Ripple's point model. Every agent runs the same network.

The policy sees own velocity, the landmarks, and teammates, from belief, and
answers with a horizontal velocity command for SingleIntegrator. Book
chapter: "Training a team".
"""

from pathlib import Path

from ripple.learning import Task


class Spread(Task):
    ports = {"outputs": {"velocity_x": ("m/s", "world"),
                         "velocity_y": ("m/s", "world"),
                         "velocity_z": ("m/s", "world")}}
    action_size = 2
    policy_file = Path(__file__).with_name("policies") / "spread.onnx"

    def __init__(self, landmarks_m=((-4.0, -3.0), (4.0, -3.0), (0.0, 4.0)),
                 max_speed_mps=2.0, policy=None):
        super().__init__(policy)
        if not landmarks_m:
            raise ValueError("Spread needs at least one landmark")
        if max_speed_mps <= 0.0:
            raise ValueError("Spread max_speed_mps must be positive")
        self.landmarks_m = [tuple(landmark) for landmark in landmarks_m]
        self.max_speed_mps = max_speed_mps

    @property
    def observation_size(self):
        return 2 + 4 * len(self.landmarks_m) - 2

    def observe(self, ctx):
        """Own velocity, each landmark's offset, and each teammate's offset
        in ID order (m/s and m, east and north only). Teammates are the known
        contacts on the agent's team; there must be exactly one per landmark
        besides this agent, so a teammate the tracker has lost is an error."""
        x, y, _ = ctx.state.position
        observation = list(ctx.state.velocity[:2])
        for east_m, north_m in self.landmarks_m:
            observation += [east_m - x, north_m - y]
        teammates = sorted((contact for contact in ctx.contacts
                            if contact.team_id == ctx.entity.team_id),
                           key=lambda contact: contact.id)
        if len(teammates) + 1 != len(self.landmarks_m):
            raise ValueError(f"Spread needs one agent per landmark: {len(self.landmarks_m)} "
                             f"landmarks, {len(teammates) + 1} agents")
        for contact in teammates:
            observation += [contact.position[0] - x, contact.position[1] - y]
        return observation

    def act(self, action):
        """Each action number in [-1, 1] scales `max_speed_mps`; no climb."""
        if len(action) != self.action_size:
            raise ValueError(f"Spread needs {self.action_size} action numbers, got {len(action)}")
        return {"velocity_x": action[0] * self.max_speed_mps,
                "velocity_y": action[1] * self.max_speed_mps,
                "velocity_z": 0.0}
