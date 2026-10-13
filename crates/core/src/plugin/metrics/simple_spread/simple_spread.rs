//! MPE's `simple_spread` score: agents cover landmarks without colliding.
//! Rust-only. Metrics phase: score the positions after each step.
//!
//! Source: MPE2 v1.1.1 (commit `147a8e8`), `mpe2/simple_spread/simple_spread.py`
//! and `SimpleEnv._execute_world_step`. Each step every agent gets
//! `(1 - local_ratio) * team + local_ratio * own`, where `team` is minus the
//! summed distance from each landmark to its nearest agent and `own` is -1 per
//! other agent it overlaps. The episode return is the sum over steps.
//!
//! Reported for the agents' team, as separate numbers (docs/TODO.md):
//! - `return`: the benchmark return, averaged over agents;
//! - `collisions`: overlapping agent pairs, summed over steps;
//! - `occupied_landmarks`: landmarks with an agent within `occupied_radius_m`
//!   after the last step;
//! - `landmark_distance`: summed distance from each landmark to its nearest
//!   agent after the last step.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::Vec3;
use crate::plugin::{
    MetricReport, Metrics, Plugin, PluginParams, TeamMetrics, Update, WorldContext,
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SimpleSpreadConfig {
    /// The agents' team. Required.
    agent_team: Option<i32>,
    /// The landmarks' team. Required.
    landmark_team: Option<i32>,
    /// Weight of each agent's own collision penalty against the team term.
    local_ratio: f64,
    /// Every agent's radius; two agents overlap within twice this. Required.
    agent_radius_m: Option<f64>,
    /// A landmark counts as occupied with an agent this close (MPE2's 0.1).
    occupied_radius_m: f64,
}

impl Default for SimpleSpreadConfig {
    fn default() -> Self {
        Self {
            agent_team: None,
            landmark_team: None,
            local_ratio: 0.5,
            agent_radius_m: None,
            occupied_radius_m: 0.1,
        }
    }
}

pub struct SimpleSpread {
    agent_team: i32,
    landmark_team: i32,
    local_ratio: f64,
    agent_radius_m: f64,
    occupied_radius_m: f64,
    returns: BTreeMap<i32, f64>,
    collisions: usize,
    occupied_landmarks: usize,
    landmark_distance_m: f64,
}

impl Plugin for SimpleSpread {
    type Config = SimpleSpreadConfig;

    fn configure(params: &PluginParams<'_>) -> Result<SimpleSpreadConfig> {
        let config: SimpleSpreadConfig = params.parse()?;
        config.agent_team.context("SimpleSpread needs agent_team")?;
        config
            .landmark_team
            .context("SimpleSpread needs landmark_team")?;
        let radius_m = config
            .agent_radius_m
            .context("SimpleSpread needs agent_radius_m")?;
        ensure!(
            radius_m > 0.0,
            "SimpleSpread agent_radius_m must be positive"
        );
        ensure!(
            (0.0..=1.0).contains(&config.local_ratio),
            "SimpleSpread local_ratio must be between 0 and 1"
        );
        Ok(config)
    }

    fn new(config: &SimpleSpreadConfig) -> Self {
        Self {
            agent_team: config.agent_team.unwrap_or_default(),
            landmark_team: config.landmark_team.unwrap_or_default(),
            local_ratio: config.local_ratio,
            agent_radius_m: config.agent_radius_m.unwrap_or_default(),
            occupied_radius_m: config.occupied_radius_m,
            returns: BTreeMap::new(),
            collisions: 0,
            occupied_landmarks: 0,
            landmark_distance_m: 0.0,
        }
    }
}

/// Planar distance, computed as MPE's `sqrt(sum(square(a - b)))`.
fn distance_m(a: Vec3, b: Vec3) -> f64 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    (dx * dx + dy * dy).sqrt()
}

impl Metrics for SimpleSpread {
    fn step(&mut self, context: &mut WorldContext<'_>) -> Result<Update> {
        let positions_on = |team| {
            let mut positions: Vec<(i32, Vec3)> = context
                .contacts_truth
                .iter()
                .filter(|entity| entity.active && entity.team_id == team)
                .map(|entity| (entity.id, entity.truth.position_world_m))
                .collect();
            positions.sort_by_key(|(id, _)| *id);
            positions
        };
        let agents = positions_on(self.agent_team);
        let landmarks = positions_on(self.landmark_team);

        let mut team = 0.0;
        self.occupied_landmarks = 0;
        for &(_, landmark_m) in &landmarks {
            let nearest_m = agents
                .iter()
                .map(|&(_, agent_m)| distance_m(agent_m, landmark_m))
                .fold(f64::INFINITY, f64::min);
            team -= nearest_m;
            if nearest_m < self.occupied_radius_m {
                self.occupied_landmarks += 1;
            }
        }
        self.landmark_distance_m = -team;

        let overlap_m = self.agent_radius_m + self.agent_radius_m;
        for &(id, agent_m) in &agents {
            let mut own = 0.0;
            for &(other, other_m) in &agents {
                if other != id && distance_m(other_m, agent_m) < overlap_m {
                    own -= 1.0;
                    if other > id {
                        self.collisions += 1;
                    }
                }
            }
            let reward = team * (1.0 - self.local_ratio) + own * self.local_ratio;
            *self.returns.entry(id).or_insert(0.0) += reward;
        }
        Ok(Update::Applied)
    }

    fn report(&self, _time_s: f64) -> MetricReport {
        let mean_return = if self.returns.is_empty() {
            0.0
        } else {
            self.returns.values().sum::<f64>() / self.returns.len() as f64
        };
        MetricReport {
            headers: [
                "return",
                "collisions",
                "occupied_landmarks",
                "landmark_distance",
            ]
            .map(str::to_owned)
            .to_vec(),
            teams: BTreeMap::from([(
                self.agent_team,
                TeamMetrics {
                    score: mean_return,
                    values: BTreeMap::from([
                        ("return".into(), mean_return),
                        ("collisions".into(), self.collisions as f64),
                        ("occupied_landmarks".into(), self.occupied_landmarks as f64),
                        ("landmark_distance".into(), self.landmark_distance_m),
                    ]),
                },
            )]),
        }
    }
}
