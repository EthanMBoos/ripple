"""Flight time and collisions per team, in Python: the built-in
`SimpleCollisionMetrics`.

A metrics plugin sees the engine's events each tick (`ctx.events`) and
answers `report(time_s)` with a score and values per team.
"""


class SimpleCollisionMetrics:
    HEADERS = ["entity_count", "flight_time_s", "flight_time_fraction",
               "non_team_collisions", "team_collisions", "ground_collisions"]

    def __init__(self, flight_time_weight=0.0, team_collision_weight=-1.0,
                 non_team_collision_weight=-1.0):
        self.flight_time_weight = flight_time_weight
        self.team_collision_weight = team_collision_weight
        self.non_team_collision_weight = non_team_collision_weight
        self.scores = {}  # by entity id

    def step(self, ctx):
        for event in ctx.events:
            for entity_id in event.entity_ids:
                score = self.scores.setdefault(entity_id, {
                    "flight_start_s": 0.0, "flight_end_s": None,
                    "same_team": 0, "opposing_team": 0, "ground": 0})
                score["team_id"] = ctx.entity_teams.get(entity_id, -1)
                if event.kind == "EntityGenerated":
                    score["flight_start_s"] = event.delivered_at_s
                elif event.kind in ("EntityRemoved", "EntityPresentAtEnd"):
                    score["flight_end_s"] = event.delivered_at_s
                elif event.kind == "TeamCollision":
                    score["same_team"] += 1
                elif event.kind == "NonTeamCollision":
                    score["opposing_team"] += 1
                elif event.kind == "GroundCollision":
                    score["ground"] += 1

    def report(self, time_s):
        scores = [self.scores[entity_id] for entity_id in sorted(self.scores)]
        first_spawn_s, last_end_s = float("inf"), float("-inf")
        for score in scores:
            first_spawn_s = min(first_spawn_s, score["flight_start_s"])
            end_s = score["flight_end_s"]
            last_end_s = max(last_end_s, time_s if end_s is None else end_s)
        effective_end_s = time_s if last_end_s <= first_spawn_s else last_end_s

        teams = {}
        for team_id in sorted({score["team_id"] for score in scores}):
            entity_count, flight_time_s = 0, 0.0
            same_team, opposing_team, ground = 0, 0, 0
            for score in scores:
                if score["team_id"] != team_id:
                    continue
                entity_count += 1
                end_s = score["flight_end_s"]
                flight_time_s += (time_s if end_s is None else end_s) - score["flight_start_s"]
                same_team += score["same_team"]
                opposing_team += score["opposing_team"]
                ground += score["ground"]
            fraction = flight_time_s / (effective_end_s - first_spawn_s)
            teams[team_id] = {
                "score": (fraction * self.flight_time_weight
                          + opposing_team * self.non_team_collision_weight
                          + same_team * self.team_collision_weight),
                "values": {"entity_count": float(entity_count),
                           "flight_time_s": flight_time_s,
                           "flight_time_fraction": fraction,
                           "non_team_collisions": float(opposing_team),
                           "team_collisions": float(same_team),
                           "ground_collisions": float(ground)}}
        return {"headers": self.HEADERS, "teams": teams}
