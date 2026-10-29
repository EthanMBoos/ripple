"""Collisions between vehicles, in Python: the built-in `SimpleCollision`.

Two vehicles closer than `collision_range_m` both lose their health, and a
collision event names them. An interaction sees every entity's truth and
asks `ctx.nearby` which are close; it answers with effects by entity id.
"""


class SimpleCollision:
    # The Rust plugin also takes startup_collision_range_m,
    # startup_collisions_only, and startup_altitude_only, which govern
    # `collision_exists`, the engine's question of whether a new vehicle may
    # appear at a point. The bridge does not ask a Python interaction that
    # question: it runs in the generate phase, before the interaction's own
    # step, and answering it from Python would add a call per candidate
    # position per spawn for a check the stock plugin already does. The three
    # are left out rather than accepted and ignored, so a mission that sets
    # one fails at construction instead of running with no collision checks.
    # The shipped missions spawn vehicles apart, so the omission costs them
    # nothing; a mission that needs the startup check uses the Rust plugin.
    def __init__(self, collision_range_m=2.0, enable_team_collisions=True,
                 enable_non_team_collisions=True):
        self.collision_range_m = collision_range_m
        self.enable_team_collisions = enable_team_collisions
        self.enable_non_team_collisions = enable_non_team_collisions

    def step(self, ctx):
        health = {entity.index: entity.health for entity in ctx.entities}
        effects = {"health": {}, "events": []}
        for first_index, second_index in ctx.nearby.pairs_within(self.collision_range_m):
            if health[first_index] <= 0 or health[second_index] <= 0:
                continue
            first, second = ctx.entities[first_index], ctx.entities[second_index]
            same_team = first.team_id == second.team_id
            enabled = (self.enable_team_collisions if same_team
                       else self.enable_non_team_collisions)
            if not enabled:
                continue
            health[first_index] = health[second_index] = 0
            effects["health"][first.id] = 0
            effects["health"][second.id] = 0
            kind = "TeamCollision" if same_team else "NonTeamCollision"
            effects["events"].append((kind, [first.id, second.id]))
        return effects
