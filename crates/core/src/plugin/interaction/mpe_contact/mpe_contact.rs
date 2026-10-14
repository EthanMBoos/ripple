//! MPE's soft push between overlapping agents, for MpeParticle. Rust-only.
//!
//! Interaction phase. MPE computes the push from the positions at the start of
//! a step and adds it to that step's force. Each interaction phase runs after
//! motion, at the start of the next step, so this computes the push from the
//! current positions and applies it as a force (`Entity::apply_force`) that
//! MpeParticle adds to its commanded force in the next motion step. Truth is
//! not changed here, so a sensor's reading includes every push.
//!
//! Source: MPE2 v1.1.1 (commit `147a8e8`), `World.get_collision_force` in
//! `mpe2/_mpe_utils/core.py`: force = contact_force * delta / dist *
//! softplus(-(dist - 2 radius) / margin) * margin, equal and opposite. Pushes
//! are applied pair by pair in id order, as MPE adds them to its forces.
//! Away from contact the push is still nonzero (softplus never reaches zero)
//! but far too small to change a velocity. Unit mass, as MpeParticle.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::Vec3;
use crate::plugin::{Interaction, InteractionContext, Plugin, PluginParams, Update};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MpeContactConfig {
    /// Teams whose entities push each other (MPE's `collide`). Required.
    teams: Vec<i32>,
    /// Every agent's radius (MPE's `size`; `simple_spread` uses 0.15). Required.
    radius_m: Option<f64>,
    /// MPE's `World.contact_force`.
    contact_force: f64,
    /// MPE's `World.contact_margin`: how soft the contact is.
    contact_margin_m: f64,
}

impl Default for MpeContactConfig {
    fn default() -> Self {
        Self {
            teams: Vec::new(),
            radius_m: None,
            contact_force: 100.0,
            contact_margin_m: 1e-3,
        }
    }
}

pub struct MpeContact {
    teams: Vec<i32>,
    radius_m: f64,
    contact_force: f64,
    contact_margin_m: f64,
}

impl Plugin for MpeContact {
    type Config = MpeContactConfig;

    fn configure(params: &PluginParams<'_>) -> Result<MpeContactConfig> {
        let config: MpeContactConfig = params.parse()?;
        ensure!(!config.teams.is_empty(), "MpeContact needs teams");
        let radius_m = config.radius_m.context("MpeContact needs radius_m")?;
        ensure!(radius_m > 0.0, "MpeContact radius_m must be positive");
        ensure!(
            config.contact_margin_m > 0.0,
            "MpeContact contact_margin_m must be positive"
        );
        Ok(config)
    }

    fn new(config: &MpeContactConfig) -> Self {
        Self {
            teams: config.teams.clone(),
            radius_m: config.radius_m.unwrap_or_default(),
            contact_force: config.contact_force,
            contact_margin_m: config.contact_margin_m,
        }
    }
}

impl MpeContact {
    /// The push on `a` from `b`; `b` gets the opposite.
    fn push_n(&self, a_m: Vec3, b_m: Vec3) -> Vec3 {
        let delta_m = Vec3::new(a_m.x - b_m.x, a_m.y - b_m.y, 0.0);
        let dist_m = (delta_m.x * delta_m.x + delta_m.y * delta_m.y).sqrt();
        let k = self.contact_margin_m;
        let penetration_m = log_add_exp_zero(-(dist_m - 2.0 * self.radius_m) / k) * k;
        // Grouped as MPE's numpy expression: ((force * delta) / dist) * penetration.
        delta_m * self.contact_force / dist_m * penetration_m
    }
}

/// log(1 + e^z), computed as numpy's `logaddexp(0, z)` is.
fn log_add_exp_zero(z: f64) -> f64 {
    if z == 0.0 {
        std::f64::consts::LN_2
    } else if z > 0.0 {
        z + (-z).exp().ln_1p()
    } else {
        z.exp().ln_1p()
    }
}

impl Interaction for MpeContact {
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        // Entity index by id, so pairs go in id order, as MPE loops over its agents.
        let agents: BTreeMap<i32, usize> = context
            .entities
            .iter()
            .enumerate()
            .filter(|(_, entity)| entity.is_active() && self.teams.contains(&entity.team_id()))
            .map(|(index, entity)| (entity.id(), index))
            .collect();
        let agents: Vec<usize> = agents.into_values().collect();
        for (pair, &a) in agents.iter().enumerate() {
            for &b in &agents[pair + 1..] {
                let entities = &mut *context.entities;
                let force_n = self.push_n(
                    entities[a].truth().position_world_m,
                    entities[b].truth().position_world_m,
                );
                entities[a].apply_force(force_n);
                entities[b].apply_force(-force_n);
            }
        }
        Ok(Update::Applied)
    }
}

#[cfg(test)]
mod tests {
    use super::{MpeContact, MpeContactConfig, Plugin, log_add_exp_zero};
    use crate::Vec3;

    #[test]
    fn the_push_is_negligible_apart_and_repels_when_overlapping() {
        let contact = MpeContact::new(&MpeContactConfig {
            teams: vec![1],
            radius_m: Some(0.15),
            ..MpeContactConfig::default()
        });
        let apart = contact.push_n(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.0));
        // 0.2 m clear: softplus leaves about exp(-200) of a push.
        assert!(apart.norm() < 1e-80, "{apart:?}");
        let overlapping = contact.push_n(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.2, 0.0, 0.0));
        // 0.1 m of overlap: about contact_force * 0.1 m, away from the other agent.
        assert!((overlapping.x + 10.0).abs() < 1e-3, "{overlapping:?}");
        assert_eq!(overlapping.y, 0.0);
    }

    #[test]
    fn softplus_matches_its_definition() {
        for z in [-700.0, -3.0, -1e-9, 0.0, 1e-9, 3.0, 700.0] {
            let expected = if z > 30.0 {
                z
            } else {
                (1.0 + f64::exp(z)).ln()
            };
            assert!((log_add_exp_zero(z) - expected).abs() <= 1e-12 * expected.max(1.0));
        }
    }
}
