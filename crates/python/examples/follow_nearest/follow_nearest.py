# Head toward the nearest known contact on another team, matching its
# altitude. The same plugin as crates/starter/src/follow_nearest.rs, line for
# line, so the two can be compared on the same mission.
from math import atan2, sqrt


class FollowNearest:
    ports = {"outputs": {"desired_heading": ("rad", "world"),
                         "desired_altitude": ("m", "world"),
                         "desired_speed": ("m/s", "none")}}

    def __init__(self, speed_mps=25.0):
        if speed_mps <= 0.0:
            raise ValueError("FollowNearest speed must be positive")
        self.speed_mps = speed_mps

    def step(self, ctx):
        # Input: our own estimated state and the contacts we know about.
        own = ctx.state.position

        # Calculation: find the closest opponent.
        nearest = None
        nearest_distance_m = float("inf")
        for contact in ctx.contacts:
            if contact.team_id == ctx.entity.team_id:
                continue
            dx, dy, dz = (contact.position[0] - own[0],
                          contact.position[1] - own[1],
                          contact.position[2] - own[2])
            distance_m = sqrt(dx * dx + dy * dy + dz * dz)
            if distance_m < nearest_distance_m:
                nearest_distance_m = distance_m
                nearest = contact

        # Output: head toward it, or hold heading and altitude if none remain.
        if nearest is None:
            heading_rad, altitude_m = ctx.state.heading, own[2]
        else:
            heading_rad = atan2(nearest.position[1] - own[1],
                                nearest.position[0] - own[0])
            altitude_m = nearest.position[2]
        return {"desired_heading": heading_rad,
                "desired_altitude": altitude_m,
                "desired_speed": self.speed_mps}
