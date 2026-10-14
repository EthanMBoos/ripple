//! Per-plugin simulation-time update rates.
use anyhow::{Result, ensure};

#[derive(Clone, Debug)]
pub(crate) struct Rate {
    period_s: f64,
    /// Time until the plugin is next due.
    remaining_s: f64,
    /// Time since the plugin last ran.
    elapsed_s: f64,
}
impl Rate {
    pub(crate) fn new(rate_hz: f64) -> Result<Self> {
        ensure!(rate_hz >= 0.0, "loop_rate must be nonnegative");
        Ok(Self {
            period_s: if rate_hz == 0.0 { 0.0 } else { 1.0 / rate_hz },
            remaining_s: 0.0,
            elapsed_s: 0.0,
        })
    }

    /// Called once per tick (or substep) of length `dt_s`. The plugin runs on
    /// its first call and then on the call nearest each due time, so rounding
    /// in the countdown cannot make it a tick late. When it runs, returns the
    /// time since it last ran: `dt_s` on its first run and without a rate.
    pub(crate) fn due(&mut self, dt_s: f64) -> Option<f64> {
        self.elapsed_s += dt_s;
        let due = self.remaining_s < dt_s / 2.0;
        if due {
            self.remaining_s += self.period_s;
        }
        self.remaining_s -= dt_s;
        if self.period_s == 0.0 {
            self.remaining_s = 0.0;
        }
        due.then(|| std::mem::take(&mut self.elapsed_s))
    }
}

#[cfg(test)]
mod tests {
    use super::Rate;

    #[test]
    fn a_plugin_runs_once_per_period_and_receives_the_time_since_its_last_run() {
        // 5 Hz in a 10 Hz simulation: every other tick, starting on the first.
        let mut rate = Rate::new(5.0).unwrap();
        let runs: Vec<_> = (0..7).map(|_| rate.due(0.1)).collect();
        assert_eq!(
            runs,
            [Some(0.1), None, Some(0.2), None, Some(0.2), None, Some(0.2)]
        );
    }

    #[test]
    fn a_long_period_lands_on_its_tick_despite_rounding() {
        // 0.2 Hz at 10 Hz: 5 s minus fifty 0.1 s ticks is not exactly zero.
        let mut rate = Rate::new(0.2).unwrap();
        let runs: Vec<_> = (0..101)
            .filter_map(|tick| rate.due(0.1).map(|elapsed_s| (tick, elapsed_s)))
            .collect();
        assert_eq!(runs.len(), 3);
        assert_eq!((runs[0].0, runs[1].0, runs[2].0), (0, 50, 100));
        assert!((runs[1].1 - 5.0).abs() < 1e-12 && (runs[2].1 - 5.0).abs() < 1e-12);
    }

    #[test]
    fn without_a_rate_a_plugin_runs_every_call_with_that_calls_dt() {
        let mut rate = Rate::new(0.0).unwrap();
        for dt_s in [0.1, 0.02, 0.1] {
            assert_eq!(rate.due(dt_s), Some(dt_s));
        }
    }

    #[test]
    fn a_rate_above_the_simulation_rate_runs_every_tick() {
        let mut rate = Rate::new(100.0).unwrap();
        assert!((0..5).all(|_| rate.due(0.1) == Some(0.1)));
    }
}
