//! Per-plugin simulation-time update rates.
use anyhow::{Result, ensure};

#[derive(Clone, Debug)]
pub(crate) struct Rate {
    period_s: f64,
    remaining_s: f64,
}
impl Rate {
    pub(crate) fn new(rate_hz: f64) -> Result<Self> {
        ensure!(rate_hz >= 0.0, "loop_rate must be nonnegative");
        Ok(Self {
            period_s: if rate_hz == 0.0 { 0.0 } else { 1.0 / rate_hz },
            remaining_s: 0.0,
        })
    }
    pub(crate) fn due(&mut self, dt_s: f64) -> bool {
        self.remaining_s -= dt_s;
        if self.remaining_s <= 0.0 {
            self.remaining_s = if self.period_s == 0.0 {
                -1.0
            } else {
                self.remaining_s + self.period_s
            };
            true
        } else {
            false
        }
    }
}
