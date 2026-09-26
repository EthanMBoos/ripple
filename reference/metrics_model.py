#!/usr/bin/env python3
"""SimpleCollisionMetrics summaries under the C++ rule and the Rust rule.

Rust intentionally scores two cases differently from C++ (docs/REFERENCE_NOTES.md):
survivors fly until the report time, and teams first spawned after the start are
scored normally. The checker accepts a summary difference only when all three hold:

  1. entity and collision counts, which neither rule changes, match C++ exactly;
  2. applying the C++ rule to the Rust run's metrics messages reproduces the C++
     summary, so the messages agree;
  3. applying the Rust rule to the same messages reproduces the Rust summary, so
     Rust's numbers follow its documented rule.

Both rules use the lifecycle and collision messages SimpleCollisionMetrics
received, from the Rust trace. C++ rule (src/plugins/metrics/SimpleCollisionMetrics):
start and end are EntityGenerated and last EntityRemoved delivery times;
EntityPresentAtEnd is never delivered; teams are fixed at the first metrics step;
the window runs from the earliest start to the latest end (or the final time if
none follows the earliest start); an entity without a later end flies to the
window's end; a team missing at the first step gets a default score with no
window and zero weights. Rust rule (simple_collision_metrics.rs): an entity
without an end flies to the report time; the window ends at the latest end,
counting survivors at the report time; every team with a generated entity is
scored normally. Standard library only.
"""

import csv
import math
from pathlib import Path
import xml.etree.ElementTree as ET

METRICS = "SimpleCollisionMetrics"
COLUMNS = ["team_id", "score", "entity_count", "flight_time", "flight_time_norm",
           "non_team_coll", "team_coll", "ground_coll"]


# include/scrimmage/plugins/metrics/SimpleCollisionMetrics/SimpleCollisionMetrics.xml
DEFAULT_WEIGHTS = {"flight_time_w": 0.0, "non_team_collisions_w": -1.0, "team_collisions_w": -1.0}


def weights(mission):
    """flight_time_w, non_team_collisions_w, team_collisions_w: plugin defaults, then mission."""
    root = ET.parse(mission).getroot()
    params = dict(DEFAULT_WEIGHTS)
    for node in root.findall("metrics"):
        if (node.text or "").strip() == METRICS:
            params.update({key: float(value) for key, value in node.attrib.items()
                           if key in DEFAULT_WEIGHTS})
    return tuple(params[key] for key in
                 ("flight_time_w", "non_team_collisions_w", "team_collisions_w"))


def predict(trace_records, start_s, final_time_s, mission_weights, rule="cpp"):
    """Rows by team under `rule` ("cpp" or "rust"), from a Rust trace's metrics
    deliveries and belief teams."""
    entity_teams = {record["id"]: record["team"] for record in trace_records
                    if record["kind"] == "belief"}
    scores = {}

    def score(entity_id):
        return scores.setdefault(entity_id, {"start": 0.0, "end": 0.0, "team_coll": 0,
                                             "non_team_coll": 0, "ground_coll": 0})

    first_step_ids = set()
    for record in trace_records:
        if record["kind"] != "delivery" or record["to"][1] != METRICS:
            continue
        ids, topic, t = record.get("ids", []), record["topic"], record["t"]
        if topic == "EntityGenerated":
            score(ids[0])["start"] = t
            if abs(t - start_s) < 1e-9:  # Teams are read at the first metrics step.
                first_step_ids.add(ids[0])
        elif topic == "EntityRemoved":
            score(ids[0])["end"] = t
            score(ids[0])["removed"] = True
        elif topic == "TeamCollision":
            for entity_id in ids:
                score(entity_id)["team_coll"] += 1
        elif topic == "NonTeamCollision":
            for entity_id in ids:
                score(entity_id)["non_team_coll"] += 1
        elif topic == "GroundCollision":
            score(ids[0])["ground_coll"] += 1

    if rule == "rust":
        return rust_rows(scores, entity_teams, final_time_s, mission_weights)
    begin = min((s["start"] for s in scores.values()), default=math.inf)
    end = max((s["end"] for s in scores.values()), default=-math.inf)
    if end <= begin:
        end = final_time_s
    window = end - begin
    flight_w, non_team_w, team_w = mission_weights

    teams = {}
    for team in {entity_teams[entity_id] for entity_id in first_step_ids}:
        teams[team] = {"weights": (flight_w, non_team_w, team_w), "window": window,
                       "count": 0, "flight": 0.0, "team_coll": 0, "non_team_coll": 0,
                       "ground_coll": 0}
    for entity_id in sorted(scores):
        entity = scores[entity_id]
        entity_end = entity["end"] if entity["end"] > entity["start"] else end
        team = teams.setdefault(entity_teams[entity_id], {
            "weights": (0.0, 0.0, 0.0), "window": 0.0, "count": 0, "flight": 0.0,
            "team_coll": 0, "non_team_coll": 0, "ground_coll": 0})
        team["count"] += 1
        team["flight"] += entity_end - entity["start"]
        for key in ("team_coll", "non_team_coll", "ground_coll"):
            team[key] += entity[key]

    rows = {}
    for team_id, team in teams.items():
        norm = divide(team["flight"], team["window"])
        flight_w, non_team_w, team_w = team["weights"]
        rows[team_id] = [norm * flight_w + team["non_team_coll"] * non_team_w
                         + team["team_coll"] * team_w, team["count"], team["flight"], norm,
                         team["non_team_coll"], team["team_coll"], team["ground_coll"]]
    return rows


def rust_rows(scores, entity_teams, final_time_s, mission_weights):
    # Rust records an end only on EntityRemoved; survivors end at the report time.
    ends = {entity_id: score["end"] if score.get("removed") else final_time_s
            for entity_id, score in scores.items()}
    begin = min((s["start"] for s in scores.values()), default=math.inf)
    last = max(ends.values(), default=-math.inf)
    window = (final_time_s if last <= begin else last) - begin
    flight_w, non_team_w, team_w = mission_weights
    teams = {}
    for entity_id in sorted(scores):
        entity = scores[entity_id]
        team = teams.setdefault(entity_teams[entity_id], {
            "count": 0, "flight": 0.0, "team_coll": 0, "non_team_coll": 0, "ground_coll": 0})
        team["count"] += 1
        team["flight"] += ends[entity_id] - entity["start"]
        for key in ("team_coll", "non_team_coll", "ground_coll"):
            team[key] += entity[key]
    rows = {}
    for team_id, team in teams.items():
        norm = divide(team["flight"], window)
        rows[team_id] = [norm * flight_w + team["non_team_coll"] * non_team_w
                         + team["team_coll"] * team_w, team["count"], team["flight"], norm,
                         team["non_team_coll"], team["team_coll"], team["ground_coll"]]
    return rows


def divide(numerator, denominator):
    if denominator != 0:
        return numerator / denominator
    return math.nan if numerator == 0 else math.copysign(math.inf, numerator)


def read_summary(path):
    with Path(path).open(newline="") as stream:
        rows = list(csv.reader(stream))
    if rows[0] != COLUMNS:
        raise ValueError(f"{path}: unexpected summary columns {rows[0]}")
    return {int(row[0]): [float(value) for value in row[1:]] for row in rows[1:]}


# Columns neither rule changes (indices after team_id).
UNAFFECTED = [COLUMNS.index(name) - 1 for name in
              ("entity_count", "non_team_coll", "team_coll", "ground_coll")]


def counts_match(cpp_summary, rust_summary):
    """Same teams, and identical entity and collision counts, under both rules."""
    cpp, rust = read_summary(cpp_summary), read_summary(rust_summary)
    return cpp.keys() == rust.keys() and all(
        cpp[team][i] == rust[team][i] for team in cpp for i in UNAFFECTED)


def matches(predicted, summary_path, tolerance=1e-6):
    """True when a prediction equals a summary.csv (six decimals, nan/inf exact)."""
    actual = read_summary(summary_path)
    if actual.keys() != predicted.keys():
        return False
    for team_id, values in actual.items():
        for left, right in zip(values, predicted[team_id]):
            if math.isnan(left) and math.isnan(right):
                continue
            if not (left == right or abs(left - right) <= tolerance):
                return False
    return True
