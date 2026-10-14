//! A plugin's random numbers, keyed by what each one is for.
use anyhow::{Result, ensure};

use super::random;

/// The random numbers of one plugin instance at one tick. Each number is a
/// function of its label: the mission seed, entity ID, plugin identity, and
/// tick (supplied by the engine), then the plugin's own key, such as a
/// target's ID and an axis. A plugin asking for the same key twice in a tick
/// gets the same number, so every number needs its own key; build keys from
/// stable IDs, never from list positions. Draws don't depend on worker order,
/// on other plugins, or on how many numbers this plugin drew before.
#[derive(Clone, Copy, Debug)]
pub struct PluginRandom {
    /// The hash of the seed, entity ID, and plugin identity.
    instance: u64,
    /// `instance` with the tick absorbed: where every key starts.
    at_tick: u64,
}
impl PluginRandom {
    /// The engine gives each plugin its numbers through its context. Construct
    /// one directly only in tests.
    pub fn new(seed: u32, entity_id: i32, identity: &str) -> Self {
        let instance = random::label(&[
            u64::from(seed),
            i64::from(entity_id) as u64,
            random::name(identity),
        ]);
        Self {
            instance,
            at_tick: random::absorb(instance, 0),
        }
    }
    /// The same instance's numbers at `tick`.
    pub(crate) fn at_tick(self, tick: i64) -> Self {
        Self {
            at_tick: random::absorb(self.instance, tick as u64),
            ..self
        }
    }
    fn label(&self, key: &[u64]) -> u64 {
        key.iter()
            .fold(self.at_tick, |hash, &part| random::absorb(hash, part))
    }
    /// The numbers whose keys start with `prefix`: `random.with(&[a]).normal(&[b], ..)`
    /// is `random.normal(&[a, b], ..)`. It saves hashing a shared prefix again
    /// for every number.
    pub fn with(&self, prefix: &[u64]) -> Self {
        Self {
            at_tick: self.label(prefix),
            ..*self
        }
    }
    /// A name (a topic, say) as a key part.
    pub fn name(text: &str) -> u64 {
        random::name(text)
    }
    /// The uniform number for `key`, strictly inside (0, 1).
    pub fn uniform(&self, key: &[u64]) -> f64 {
        random::uniform(self.label(key))
    }
    /// The normal number for `key`.
    pub fn normal(&self, key: &[u64], mean: f64, stddev: f64) -> Result<f64> {
        ensure!(
            mean.is_finite() && stddev.is_finite() && stddev >= 0.0,
            "invalid normal parameters"
        );
        Ok(random::normal(self.label(key), mean, stddev))
    }
}
