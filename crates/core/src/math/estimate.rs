//! A kinematic state known with uncertainty: a Gaussian over states, as
//! estimators and the tracking and estimation literature represent a belief
//! or a measurement. Truth stays a bare `KinematicState`; this type says that
//! a value is an estimate, and how good.
use super::{KinematicState, Matrix3, Vec3};
use nalgebra::SMatrix;
use serde::{Deserialize, Serialize};

/// The covariance of a state's error in the tangent space at its mean: a
/// 12x12 over position (world frame, m), attitude as a rotation vector (rad),
/// velocity (world frame, m/s), and angular velocity (world frame, rad/s), in
/// that order. The attitude block is over a small rotation applied on the
/// body side, `R = R_mean * Exp(delta_theta)`, which is how NoisyState
/// perturbs attitude and how GTSAM's `Rot3::retract` does; a rotation is
/// never given four variances for its quaternion components. A zero
/// covariance means an exact value, so the engine's perfect measurement
/// reports zero and no sentinel for "unknown" exists; a quantity an
/// estimator does not estimate carries an infinite variance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StateCovariance(pub SMatrix<f64, 12, 12>);

impl Default for StateCovariance {
    fn default() -> Self {
        Self::zero()
    }
}

impl StateCovariance {
    /// Where each quantity's three rows and columns start. The order is the
    /// ROS pose covariance (position, then rotation) extended with the two
    /// rates.
    pub const POSITION: usize = 0;
    pub const ATTITUDE: usize = 3;
    pub const VELOCITY: usize = 6;
    pub const ANGULAR_VELOCITY: usize = 9;

    /// An exact value: the perfect measurement's covariance.
    pub fn zero() -> Self {
        Self(SMatrix::zeros())
    }

    /// Independent errors with these standard deviations per axis, so the
    /// matrix is diagonal: the noise a stock sensor injects. Attitude
    /// sigmas are rotations about the body x, y, and z axes.
    pub fn from_sigmas(
        position_m: [f64; 3],
        attitude_rad: [f64; 3],
        velocity_mps: [f64; 3],
        angular_velocity_radps: [f64; 3],
    ) -> Self {
        let mut matrix = SMatrix::zeros();
        for (start, sigmas) in [
            (Self::POSITION, position_m),
            (Self::ATTITUDE, attitude_rad),
            (Self::VELOCITY, velocity_mps),
            (Self::ANGULAR_VELOCITY, angular_velocity_radps),
        ] {
            for (axis, sigma) in sigmas.into_iter().enumerate() {
                matrix[(start + axis, start + axis)] = sigma * sigma;
            }
        }
        Self(matrix)
    }

    fn block(&self, start: usize) -> Matrix3 {
        self.0.fixed_view::<3, 3>(start, start).into_owned()
    }
    fn set_block(&mut self, start: usize, block: &Matrix3) {
        self.0.fixed_view_mut::<3, 3>(start, start).copy_from(block);
    }

    /// The position error's 3x3, in m².
    pub fn position(&self) -> Matrix3 {
        self.block(Self::POSITION)
    }
    /// The attitude error's 3x3 over the body-side rotation vector, in rad².
    pub fn attitude(&self) -> Matrix3 {
        self.block(Self::ATTITUDE)
    }
    /// The velocity error's 3x3, in (m/s)².
    pub fn velocity(&self) -> Matrix3 {
        self.block(Self::VELOCITY)
    }
    /// The angular velocity error's 3x3, in (rad/s)².
    pub fn angular_velocity(&self) -> Matrix3 {
        self.block(Self::ANGULAR_VELOCITY)
    }
    pub fn set_position(&mut self, block: &Matrix3) {
        self.set_block(Self::POSITION, block);
    }
    pub fn set_attitude(&mut self, block: &Matrix3) {
        self.set_block(Self::ATTITUDE, block);
    }
    pub fn set_velocity(&mut self, block: &Matrix3) {
        self.set_block(Self::VELOCITY, block);
    }
    pub fn set_angular_velocity(&mut self, block: &Matrix3) {
        self.set_block(Self::ANGULAR_VELOCITY, block);
    }

    /// The standard deviation of each of the twelve error components.
    pub fn sigmas(&self) -> [f64; 12] {
        let mut sigmas = [0.0; 12];
        for (index, sigma) in sigmas.iter_mut().enumerate() {
            *sigma = self.0[(index, index)].sqrt();
        }
        sigmas
    }
}

/// A world-frame position measurement with its noise covariance: what a
/// position sensor (a GPS-like fix) reports. A filter that corrects with it
/// reads the covariance as the measurement noise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionMeasurement {
    pub position_world_m: Vec3,
    pub covariance_m2: Matrix3,
}

/// A state and the covariance of its error: a belief, or a state
/// measurement. The mean is a `KinematicState` so the motion models and
/// their `advance` work on it unchanged.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct GaussianState {
    pub mean: KinematicState,
    pub covariance: StateCovariance,
}

impl GaussianState {
    /// A state known exactly: the engine's perfect measurement.
    pub fn perfect(mean: KinematicState) -> Self {
        Self {
            mean,
            covariance: StateCovariance::zero(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmas_fill_the_diagonal_of_each_block_in_order() {
        let covariance = StateCovariance::from_sigmas(
            [1.0, 2.0, 3.0],
            [0.1, 0.2, 0.3],
            [4.0, 5.0, 6.0],
            [0.4, 0.5, 0.6],
        );
        assert_eq!(covariance.position().diagonal(), Vec3::new(1.0, 4.0, 9.0));
        assert!((covariance.attitude()[(2, 2)] - 0.09).abs() < 1e-15);
        assert_eq!(covariance.velocity()[(0, 0)], 16.0);
        assert!((covariance.angular_velocity()[(1, 1)] - 0.25).abs() < 1e-15);
        // Off-diagonal entries stay zero: the errors are independent.
        assert_eq!(covariance.0[(0, 6)], 0.0);
        let sigmas = covariance.sigmas();
        assert_eq!(sigmas[StateCovariance::POSITION + 2], 3.0);
        assert_eq!(sigmas[StateCovariance::VELOCITY + 1], 5.0);
    }

    #[test]
    fn a_block_can_be_replaced_without_touching_the_others() {
        let mut covariance = StateCovariance::from_sigmas([1.0; 3], [0.0; 3], [2.0; 3], [0.0; 3]);
        covariance.set_position(&(Matrix3::identity() * 7.0));
        assert_eq!(covariance.position(), Matrix3::identity() * 7.0);
        assert_eq!(covariance.velocity(), Matrix3::identity() * 4.0);
    }

    #[test]
    fn the_perfect_measurement_has_zero_covariance() {
        let state = GaussianState::perfect(KinematicState {
            position_world_m: Vec3::new(1.0, 2.0, 3.0),
            ..KinematicState::default()
        });
        assert_eq!(state.covariance, StateCovariance::zero());
        assert_eq!(state.mean.position_world_m, Vec3::new(1.0, 2.0, 3.0));
    }
}
