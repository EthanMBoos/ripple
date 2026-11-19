//! Fourth-order Runge-Kutta with the legacy arithmetic order.

/// A state RK4 can advance: `self + scale * rate`, field by field. `Rate`
/// holds each field's time derivative under the name of what it is (an
/// acceleration for a velocity), so the derivative's result reads as the
/// physics.
pub trait Integrable: Copy {
    type Rate: Copy;
    fn plus_scaled(self, scale: f64, rate: Self::Rate) -> Self;
}

impl<const N: usize> Integrable for [f64; N] {
    type Rate = Self;
    fn plus_scaled(self, scale: f64, rate: Self) -> Self {
        std::array::from_fn(|i| self[i] + scale * rate[i])
    }
}

/// One step of `dt_s`.
pub fn rk4<S: Integrable>(state: S, dt_s: f64, derivative: impl Fn(S) -> S::Rate) -> S {
    rk4_timed(state, dt_s, |_, stage| derivative(stage))
}

/// RK4 whose derivative also receives the stage's time since the start of
/// the step (0, dt/2, dt/2, dt), for inputs that change within a step in a
/// known way, such as a lagging motor.
pub fn rk4_timed<S: Integrable>(state: S, dt_s: f64, derivative: impl Fn(f64, S) -> S::Rate) -> S {
    let k1 = derivative(0.0, state);
    let k2 = derivative(0.5 * dt_s, state.plus_scaled(dt_s * 0.5, k1));
    let k3 = derivative(0.5 * dt_s, state.plus_scaled(dt_s * 0.5, k2));
    let k4 = derivative(dt_s, state.plus_scaled(dt_s, k3));
    // Boost odeint's scale_sum5 uses this summation order.
    state
        .plus_scaled(dt_s / 6.0, k1)
        .plus_scaled(dt_s / 3.0, k2)
        .plus_scaled(dt_s / 3.0, k3)
        .plus_scaled(dt_s / 6.0, k4)
}
