//! MPE's soft push between overlapping agents, for MpeParticle. Rust-only.
//!
//! Interaction phase. MPE computes the push from the positions at the start of
//! a step and adds it to that step's force. A motion model sees only its own
//! entity, so this does it in two parts: each tick it computes the push from
//! the positions it remembered last tick (the start of this tick's step) and
//! adds push * dt to velocity after motion has run. The first call, during
//! setup, only remembers the starting positions.
//!
//! Source: MPE2 v1.1.1 (commit `147a8e8`), `World.get_collision_force` in
//! `mpe2/_mpe_utils/core.py`: force = contact_force * delta / dist *
//! softplus(-(dist - 2 radius) / margin) * margin, equal and opposite.
//! MPE adds the push into the force before multiplying by dt; adding it
//! afterwards rounds differently, so velocities can differ from MPE's in the
//! last bit on steps with contact. Away from contact the push is still
//! nonzero (softplus never reaches zero) but far too small to change a
//! velocity. Unit mass, as MpeParticle.
//!
//! Limitation: sensors run before interactions, so on a step with contact a
//! sensor's velocity reading misses that step's push (position is unaffected;
//! the push moves it from the next step). The default perfect belief is taken
//! after interactions and is right. A zero-noise NoisyState would differ from
//! perfect belief on those steps. If that ever matters, move the push into
//! motion: compute it here for the next step and hand it to MpeParticle.

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
    /// Positions at the start of the current step, by entity id; None before
    /// setup.
    previous_m: Option<BTreeMap<i32, Vec3>>,
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
            previous_m: None,
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
        let current_m: BTreeMap<i32, Vec3> = context
            .entities
            .iter()
            .filter(|entity| entity.is_active() && self.teams.contains(&entity.team_id()))
            .map(|entity| (entity.id(), entity.truth().position_world_m))
            .collect();
        let Some(previous_m) = self.previous_m.replace(current_m) else {
            return Ok(Update::Applied);
        };

        // Pairs in id order, as MPE loops over its agents.
        let mut push_n: BTreeMap<i32, Vec3> = BTreeMap::new();
        let positions: Vec<(i32, Vec3)> = previous_m.into_iter().collect();
        for (index, &(a, a_m)) in positions.iter().enumerate() {
            for &(b, b_m) in &positions[index + 1..] {
                let force_n = self.push_n(a_m, b_m);
                *push_n.entry(a).or_insert_with(Vec3::zeros) += force_n;
                *push_n.entry(b).or_insert_with(Vec3::zeros) -= force_n;
            }
        }
        let dt_s = context.time.dt_s;
        for entity in context.entities.iter_mut() {
            if let Some(force_n) = push_n.get(&entity.id()) {
                entity.truth_mut().velocity_world_mps += force_n * dt_s;
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
