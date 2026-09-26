//! Collision scores are an event subscriber, not mutable state inside SimControl.
//!
//! C++ counterpart: src/plugins/metrics/SimpleCollisionMetrics/SimpleCollisionMetrics.cpp.
//! Metrics phase: consume delivered lifecycle/collision events; report team totals.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::Deserialize;

use crate::plugin::{
    MetricReport, Metrics, Plugin, PluginParams, TeamMetrics, Update, WorldContext,
};
use crate::{Event, EventKind};

// Subscription and consumption order must stay the same.
const EVENT_KINDS: [EventKind; 6] = [
    EventKind::EntityGenerated,
    EventKind::EntityRemoved,
    EventKind::TeamCollision,
    EventKind::NonTeamCollision,
    EventKind::GroundCollision,
    EventKind::EntityPresentAtEnd,
];

/// Score weights from the mission; `Default` supplies any key the mission leaves out.
#[derive(Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CollisionWeights {
    #[serde(rename = "flight_time_w")]
    flight_time: f64,
    #[serde(rename = "team_collisions_w")]
    same_team: f64,
    #[serde(rename = "non_team_collisions_w")]
    opposing_team: f64,
}

impl Default for CollisionWeights {
    fn default() -> Self {
        Self {
            flight_time: 0.0,
            same_team: -1.0,
            opposing_team: -1.0,
        }
    }
}

#[derive(Default)]
struct Score {
    team_id: i32,
    flight_start_s: f64,
    // None while the entity is alive; survivors fly until the report time.
    flight_end_s: Option<f64>,
    same_team_collisions: usize,
    opposing_team_collisions: usize,
    ground_collisions: usize,
}

pub struct SimpleCollisionMetrics {
    weights: CollisionWeights,
    scores: BTreeMap<i32, Score>,
}

impl Plugin for SimpleCollisionMetrics {
    type Config = CollisionWeights;

    fn configure(params: &PluginParams<'_>) -> Result<CollisionWeights> {
        params.parse()
    }

    fn new(weights: &CollisionWeights) -> Self {
        Self {
            weights: *weights,
            scores: BTreeMap::new(),
        }
    }
}

impl Metrics for SimpleCollisionMetrics {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        for kind in EVENT_KINDS {
            context
                .messages
                .subscribe::<Event>("GlobalNetwork", kind.topic())?;
        }
        Ok(())
    }

    fn step(&mut self, context: &mut WorldContext<'_>) -> Result<Update> {
        for kind in EVENT_KINDS {
            let messages = context
                .messages
                .receive::<Event>("GlobalNetwork", kind.topic())?;
            for message in messages {
                for id in &message.value.entity_ids {
                    let score = self.scores.entry(*id).or_default();
                    score.team_id = context.entity_teams.get(id).copied().unwrap_or(-1);
                    match kind {
                        EventKind::EntityGenerated => score.flight_start_s = message.delivered_at_s,
                        EventKind::EntityRemoved | EventKind::EntityPresentAtEnd => {
                            score.flight_end_s = Some(message.delivered_at_s)
                        }
                        EventKind::TeamCollision => score.same_team_collisions += 1,
                        EventKind::NonTeamCollision => score.opposing_team_collisions += 1,
                        EventKind::GroundCollision => score.ground_collisions += 1,
                    }
                }
            }
        }
        Ok(Update::Applied)
    }

    fn report(&self, time_s: f64) -> MetricReport {
        let mut report = MetricReport {
            headers: [
                "entity_count",
                "flight_time",
                "flight_time_norm",
                "non_team_coll",
                "team_coll",
                "ground_coll",
            ]
            .map(str::to_owned)
            .to_vec(),
            ..MetricReport::default()
        };
        // Intentional C++ difference: C++ never delivers EntityPresentAtEnd to metrics,
        // so survivors there are credited only until the last removal. Here an entity
        // without a recorded end flies until the report time.
        let mut first_spawn_s = f64::INFINITY;
        let mut last_end_s = f64::NEG_INFINITY;
        for score in self.scores.values() {
            first_spawn_s = first_spawn_s.min(score.flight_start_s);
            last_end_s = last_end_s.max(score.flight_end_s.unwrap_or(time_s));
        }
        let effective_end_s = if last_end_s <= first_spawn_s {
            time_s
        } else {
            last_end_s
        };

        // Intentional C++ difference: C++ lists teams at its first metrics step, so a
        // team whose entities all spawn later gets an infinite normalized flight
        // time and NaN score. Every team with a generated entity is reported here.
        let teams: BTreeSet<i32> = self.scores.values().map(|score| score.team_id).collect();
        for team_id in &teams {
            let mut entity_count = 0;
            let mut flight_time_s = 0.0;
            let mut same_team_collisions = 0;
            let mut opposing_team_collisions = 0;
            let mut ground_collisions = 0;
            for score in self.scores.values() {
                if score.team_id != *team_id {
                    continue;
                }
                entity_count += 1;
                let entity_end_s = score.flight_end_s.unwrap_or(time_s);
                flight_time_s += entity_end_s - score.flight_start_s;
                same_team_collisions += score.same_team_collisions;
                opposing_team_collisions += score.opposing_team_collisions;
                ground_collisions += score.ground_collisions;
            }

            let normalized_flight_time = flight_time_s / (effective_end_s - first_spawn_s);
            let score = normalized_flight_time * self.weights.flight_time
                + opposing_team_collisions as f64 * self.weights.opposing_team
                + same_team_collisions as f64 * self.weights.same_team;
            report.teams.insert(
                *team_id,
                TeamMetrics {
                    score,
                    values: BTreeMap::from([
                        ("entity_count".into(), entity_count as f64),
                        ("flight_time".into(), flight_time_s),
                        ("flight_time_norm".into(), normalized_flight_time),
                        ("non_team_coll".into(), opposing_team_collisions as f64),
                        ("team_coll".into(), same_team_collisions as f64),
                        ("ground_coll".into(), ground_collisions as f64),
                    ]),
                },
            );
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::{CollisionWeights, Metrics, Plugin, Score, SimpleCollisionMetrics};
    use std::collections::BTreeMap;

    #[test]
    fn team_totals_do_not_mix_entities_or_collision_weights() {
        let mut metrics = SimpleCollisionMetrics::new(&CollisionWeights {
            flight_time: 1.0,
            same_team: 2.0,
            opposing_team: 3.0,
        });
        metrics.scores = BTreeMap::from([
            (
                1,
                Score {
                    team_id: 1,
                    flight_start_s: 0.0,
                    flight_end_s: Some(2.0),
                    same_team_collisions: 1,
                    opposing_team_collisions: 0,
                    ground_collisions: 1,
                },
            ),
            (
                2,
                Score {
                    team_id: 1,
                    flight_start_s: 1.0,
                    flight_end_s: Some(3.0),
                    same_team_collisions: 0,
                    opposing_team_collisions: 1,
                    ground_collisions: 0,
                },
            ),
            (
                3,
                Score {
                    team_id: 2,
                    flight_start_s: 0.0,
                    flight_end_s: Some(1.0),
                    same_team_collisions: 0,
                    opposing_team_collisions: 0,
                    ground_collisions: 0,
                },
            ),
        ]);
        let report = metrics.report(3.0);
        let first_team = &report.teams[&1];
        let second_team = &report.teams[&2];
        assert!((first_team.values["entity_count"] - 2.0).abs() < 1e-12);
        assert!((first_team.values["flight_time"] - 4.0).abs() < 1e-12);
        assert!((first_team.score - (4.0 / 3.0 + 3.0 + 2.0)).abs() < 1e-12);
        assert!((second_team.values["entity_count"] - 1.0).abs() < 1e-12);
        assert!((second_team.values["flight_time"] - 1.0).abs() < 1e-12);
        assert!((second_team.score - 1.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn survivors_fly_until_the_report_time_after_another_entity_is_removed() {
        let mut metrics = SimpleCollisionMetrics::new(&CollisionWeights {
            flight_time: 1.0,
            same_team: 0.0,
            opposing_team: 0.0,
        });
        metrics.scores = BTreeMap::from([
            (
                1,
                Score {
                    team_id: 1,
                    flight_end_s: None,
                    ..Score::default()
                },
            ),
            (
                2,
                Score {
                    team_id: 2,
                    flight_end_s: Some(0.4),
                    ..Score::default()
                },
            ),
        ]);
        let report = metrics.report(30.0);
        assert!((report.teams[&1].values["flight_time"] - 30.0).abs() < 1e-12);
        assert!((report.teams[&1].values["flight_time_norm"] - 1.0).abs() < 1e-12);
        assert!((report.teams[&2].values["flight_time"] - 0.4).abs() < 1e-12);
    }

    #[test]
    fn a_team_that_first_spawns_late_is_still_reported() {
        let mut metrics = SimpleCollisionMetrics::new(&CollisionWeights {
            flight_time: 1.0,
            same_team: 0.0,
            opposing_team: 0.0,
        });
        metrics.scores = BTreeMap::from([
            (
                1,
                Score {
                    team_id: 1,
                    ..Score::default()
                },
            ),
            (
                2,
                Score {
                    team_id: 2,
                    flight_start_s: 1.0,
                    ..Score::default()
                },
            ),
        ]);
        let report = metrics.report(4.0);
        assert!((report.teams[&2].values["flight_time"] - 3.0).abs() < 1e-12);
        assert!((report.teams[&2].score - 0.75).abs() < 1e-12);
    }
}
