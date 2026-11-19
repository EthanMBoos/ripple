//! PID control helper; angular loops use radians.

use serde::Deserialize;

use crate::math::angle_pi;

/// PID gains. The integral accumulates only while the error is within
/// `integrate_within`: error units for linear loops, degrees for angular ones.
#[derive(Clone, Copy, Debug)]
pub struct PidGains {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    pub integrate_within: f64,
}

/// Mission forms of [`PidGains`], one per loop unit, so the band's key names
/// its unit: `{kp: 1, ki: 0.1, kd: 0, integrate_within_deg: 9}`.
macro_rules! mission_gains {
    ($(#[$doc:meta])* $name:ident, $band:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub kp: f64,
            pub ki: f64,
            pub kd: f64,
            pub $band: f64,
        }

        impl From<$name> for PidGains {
            fn from(gains: $name) -> Self {
                Self { kp: gains.kp, ki: gains.ki, kd: gains.kd, integrate_within: gains.$band }
            }
        }
    };
}

mission_gains!(
    /// Gains for a loop on an angle; the band is in degrees.
    AngleGains,
    integrate_within_deg
);
mission_gains!(
    /// Gains for a loop on a length, such as altitude; the band is in metres.
    LengthGains,
    integrate_within_m
);
mission_gains!(
    /// Gains for a loop on a speed; the band is in m/s.
    SpeedGains,
    integrate_within_mps
);

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

    /// Convert `integrate_within` from degrees; updates use radians.
    pub fn angular(gains: PidGains) -> Self {
        Self::new(gains, true)
    }

    fn new(gains: PidGains, wrap_angle: bool) -> Self {
        let integral_band = if wrap_angle {
            crate::math::deg_to_rad(gains.integrate_within)
        } else {
            gains.integrate_within
        };

        Self {
            proportional_gain: gains.kp,
            integral_gain: gains.ki,
            derivative_gain: gains.kd,
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
            kp: 2.0,
            ki: 3.0,
            kd: 4.0,
            integrate_within: 10.0,
        });
        // Error 2: P=4, I=3*(2*0.5)=3, D=4*(2/0.5)=16.
        assert!((pid.step(3.0, 1.0, 0.5) - 23.0).abs() < 1e-12);
        // Error 1: P=2, I=3*1.5=4.5, D=4*((1-2)/0.5)=-8.
        assert!((pid.step(2.0, 1.0, 0.5) + 1.5).abs() < 1e-12);
    }

    #[test]
    fn leaving_the_integral_band_resets_accumulated_error() {
        let mut pid = Pid::linear(PidGains {
            kp: 0.0,
            ki: 1.0,
            kd: 0.0,
            integrate_within: 2.0,
        });
        assert!((pid.step(1.0, 0.0, 1.0) - 1.0).abs() < 1e-12);
        assert!(pid.step(3.0, 0.0, 1.0).abs() < 1e-12);
        assert!((pid.step(1.0, 0.0, 1.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn angular_loop_wraps_error_and_converts_the_integral_band_once() {
        let mut pid = Pid::angular(PidGains {
            kp: 1.0,
            ki: 1.0,
            kd: 0.0,
            integrate_within: 9.0,
        });
        assert!((pid.integral_band - 9.0_f64.to_radians()).abs() < 1e-12);
        // The shortest turn from +179 degrees to -179 degrees is +2 degrees.
        let output = pid.step((-179.0_f64).to_radians(), 179.0_f64.to_radians(), 1.0);
        assert!((output - 4.0_f64.to_radians()).abs() < 1e-12);
    }
}
