//! PID control helper; angular loops use radians.

use serde::Deserialize;

use crate::math::angle_pi;

/// Gains parsed from the C++ controller's [P, I, D, integral band] XML values.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(from = "[f64; 4]")]
pub struct PidGains {
    pub proportional: f64,
    pub integral: f64,
    pub derivative: f64,
    /// Error units for linear loops; degrees at the XML boundary for angular loops.
    pub integral_band: f64,
}

impl From<[f64; 4]> for PidGains {
    fn from([proportional, integral, derivative, integral_band]: [f64; 4]) -> Self {
        Self {
            proportional,
            integral,
            derivative,
            integral_band,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pid {
    proportional_gain: f64,
    integral_gain: f64,
    derivative_gain: f64,
    integral_band: f64,
    wrap_angle: bool,
    integral_error: f64,
    previous_error: f64,
}

impl Pid {
    pub fn linear(gains: PidGains) -> Self {
        Self::new(gains, false)
    }

    /// Convert the legacy integral band from degrees; updates use radians.
    pub fn angular(gains: PidGains) -> Self {
        Self::new(gains, true)
    }

    fn new(gains: PidGains, wrap_angle: bool) -> Self {
        let integral_band = if wrap_angle {
            gains.integral_band.to_radians()
        } else {
            gains.integral_band
        };

        Self {
            proportional_gain: gains.proportional,
            integral_gain: gains.integral,
            derivative_gain: gains.derivative,
            integral_band,
            wrap_angle,
            integral_error: 0.0,
            previous_error: 0.0,
        }
    }

    pub fn step(&mut self, setpoint: f64, measurement: f64, dt_s: f64) -> f64 {
        let mut error = setpoint - measurement;
        if self.wrap_angle {
            error = angle_pi(error);
        }

        if error.abs() > self.integral_band {
            self.integral_error = 0.0;
        } else {
            self.integral_error += error * dt_s;
        }

        let error_rate = (error - self.previous_error) / dt_s.max(1e-9);
        self.previous_error = error;

        let proportional_output = self.proportional_gain * error;
        let integral_output = self.integral_gain * self.integral_error;
        let derivative_output = self.derivative_gain * error_rate;
        proportional_output + integral_output + derivative_output
    }
}

#[cfg(test)]
mod tests {
    use super::{Pid, PidGains};

    #[test]
    fn linear_loop_applies_proportional_integral_and_derivative_terms() {
        let mut pid = Pid::linear(PidGains {
            proportional: 2.0,
            integral: 3.0,
            derivative: 4.0,
            integral_band: 10.0,
        });
        // Error 2: P=4, I=3*(2*0.5)=3, D=4*(2/0.5)=16.
        assert!((pid.step(3.0, 1.0, 0.5) - 23.0).abs() < 1e-12);
        // Error 1: P=2, I=3*1.5=4.5, D=4*((1-2)/0.5)=-8.
        assert!((pid.step(2.0, 1.0, 0.5) + 1.5).abs() < 1e-12);
    }

    #[test]
    fn leaving_the_integral_band_resets_accumulated_error() {
        let mut pid = Pid::linear(PidGains {
            proportional: 0.0,
            integral: 1.0,
            derivative: 0.0,
            integral_band: 2.0,
        });
        assert!((pid.step(1.0, 0.0, 1.0) - 1.0).abs() < 1e-12);
        assert!(pid.step(3.0, 0.0, 1.0).abs() < 1e-12);
        assert!((pid.step(1.0, 0.0, 1.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn angular_loop_wraps_error_and_converts_the_integral_band_once() {
        let mut pid = Pid::angular(PidGains {
            proportional: 1.0,
            integral: 1.0,
            derivative: 0.0,
            integral_band: 9.0,
        });
        assert!((pid.integral_band - 9.0_f64.to_radians()).abs() < 1e-12);
        // The shortest turn from +179 degrees to -179 degrees is +2 degrees.
        let output = pid.step((-179.0_f64).to_radians(), 179.0_f64.to_radians(), 1.0);
        assert!((output - 4.0_f64.to_radians()).abs() < 1e-12);
    }
}
