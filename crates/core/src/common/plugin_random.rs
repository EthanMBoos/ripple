//! Stable, per-plugin random streams, independent of worker order.
use anyhow::{Result, ensure};

/// Per-instance SplitMix64 stream, keyed by mission seed, entity ID, and plugin identity.
/// Separate streams keep draws independent of worker order and of which other
/// plugins draw.
pub struct PluginRandom {
    state: u64,
}
impl PluginRandom {
    /// The engine gives each sensor, autonomy, controller, and network its own stream
    /// through its context. Construct one directly only in tests.
    pub fn new(seed: u32, entity_id: i32, identity: &str) -> Self {
        let mut state = 0xcbf29ce484222325_u64;
        for byte in seed
            .to_le_bytes()
            .into_iter()
            .chain(entity_id.to_le_bytes())
            .chain(identity.bytes())
        {
            state = (state ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        Self { state }
    }
    fn uniform_open(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut bits = self.state;
        bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d049bb133111eb);
        bits ^= bits >> 31;
        // Strictly inside (0, 1), avoiding log(0) in Box-Muller.
        ((bits >> 12) as f64 + 0.5) / 4_503_599_627_370_496.0
    }
    /// Uniform sample strictly inside (0, 1).
    pub fn uniform(&mut self) -> f64 {
        self.uniform_open()
    }
    pub fn normal(&mut self, mean: f64, stddev: f64) -> Result<f64> {
        ensure!(
            mean.is_finite() && stddev.is_finite() && stddev >= 0.0,
            "invalid normal parameters"
        );
        let radius = (-2.0 * self.uniform_open().ln()).sqrt();
        let angle_rad = std::f64::consts::TAU * self.uniform_open();
        Ok(mean + stddev * radius * angle_rad.cos())
    }
}
