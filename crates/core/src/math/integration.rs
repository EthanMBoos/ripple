//! Fourth-order Runge-Kutta with the legacy arithmetic order.

/// Advance coordinates by `dt_s`; derivatives must use the same coordinate order.
pub fn rk4<const N: usize>(
    coordinates: &mut [f64; N],
    dt_s: f64,
    derivative: impl Fn(&[f64; N]) -> [f64; N],
) {
    rk4_timed(coordinates, dt_s, |_, stage| derivative(stage));
}

/// RK4 whose derivative also receives the stage's time since the start of
/// the step (0, dt/2, dt/2, dt), for inputs that change within a step in a
/// known way, such as a lagging motor.
pub fn rk4_timed<const N: usize>(
    coordinates: &mut [f64; N],
    dt_s: f64,
    derivative: impl Fn(f64, &[f64; N]) -> [f64; N],
) {
    let k1 = derivative(0.0, coordinates);
    let k2 = derivative(
        0.5 * dt_s,
        &std::array::from_fn(|i| coordinates[i] + dt_s * 0.5 * k1[i]),
    );
    let k3 = derivative(
        0.5 * dt_s,
        &std::array::from_fn(|i| coordinates[i] + dt_s * 0.5 * k2[i]),
    );
    let k4 = derivative(
        dt_s,
        &std::array::from_fn(|i| coordinates[i] + dt_s * k3[i]),
    );

    // Boost odeint's scale_sum5 uses this summation order.
    for i in 0..N {
        coordinates[i] = coordinates[i]
            + (dt_s / 6.0) * k1[i]
            + (dt_s / 3.0) * k2[i]
            + (dt_s / 3.0) * k3[i]
            + (dt_s / 6.0) * k4[i];
    }
}
