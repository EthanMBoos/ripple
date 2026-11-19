//! The Aerosonde's motor and propeller: Beard and McLain's addendum on
//! propeller and motor models (the model `mavsim_public`'s parameter file
//! names, "see addendum by McLain"), replacing the printed book's simple
//! propeller. The throttle sets the motor's voltage, the steady propeller
//! speed balances the motor's torque against the propeller's, and the
//! thrust and torque follow from fitted polynomials in advance ratio.

use std::f64::consts::PI;

use super::airframe::Vehicle;

pub(super) struct Propeller {
    /// Along the nose.
    pub thrust_n: f64,
    /// The torque the motor drives the propeller with; the airframe feels
    /// its reaction, the opposite way about the nose.
    pub torque_nm: f64,
}

/// The steady propeller's speed Omega_p at this airspeed and throttle: the
/// positive root of the quadratic that balances the motor's torque against
/// the propeller's, a Omega^2 + b Omega + c = 0.
pub(super) fn speed_radps(
    vehicle: &Vehicle,
    air_density_kgpm3: f64,
    airspeed_mps: f64,
    throttle: f64,
) -> f64 {
    // Symbols, as in the addendum: air density, propeller diameter, airspeed,
    // the motor's constants (KV, KQ, resistance R, no-load current i_0), and
    // the torque polynomial's coefficients.
    let (rho, d, v_a) = (
        air_density_kgpm3,
        vehicle.propeller_diameter_m,
        airspeed_mps,
    );
    let (kv, kq, big_r, i_0) = (
        vehicle.motor_speed_constant_vspr,
        vehicle.motor_torque_constant_nmpa,
        vehicle.motor_resistance_ohm,
        vehicle.no_load_current_a,
    );
    let (c_q2, c_q1, c_q0) = (vehicle.c_torque_2, vehicle.c_torque_1, vehicle.c_torque_0);
    // The throttle sets the voltage at the motor.
    let v_in = vehicle.max_voltage_v * throttle;
    let a = rho * d.powi(5) * c_q0 / (2.0 * PI).powi(2);
    let b = rho * d.powi(4) * c_q1 * v_a / (2.0 * PI) + kq * kv / big_r;
    let c = rho * d.powi(3) * c_q2 * v_a * v_a - kq * v_in / big_r + kq * i_0;
    // At rest with no throttle the no-load current leaves the root slightly
    // negative; the propeller then stands still.
    ((-b + (b * b - 4.0 * a * c).sqrt()) / (2.0 * a)).max(0.0)
}

/// The steady propeller's thrust and torque at this airspeed and throttle.
pub(super) fn loads(
    vehicle: &Vehicle,
    air_density_kgpm3: f64,
    airspeed_mps: f64,
    throttle: f64,
) -> Propeller {
    // Symbols, as in the addendum: air density, propeller diameter, airspeed,
    // and the propeller's speed.
    let (rho, d, v_a) = (
        air_density_kgpm3,
        vehicle.propeller_diameter_m,
        airspeed_mps,
    );
    let omega_p = speed_radps(vehicle, air_density_kgpm3, airspeed_mps, throttle);
    // Thrust rho n^2 D^4 C_T(J) and torque rho n^2 D^5 C_Q(J), with
    // n = Omega_p / 2 pi and the advance ratio J = 2 pi V_a / (Omega_p D),
    // multiplied out so that a standing propeller (J unbounded) is defined:
    // n J = V_a / D.
    let n = omega_p / (2.0 * PI);
    let n_j = v_a / d;
    let thrust_n = rho
        * d.powi(4)
        * (vehicle.c_thrust_2 * n_j * n_j
            + vehicle.c_thrust_1 * n_j * n
            + vehicle.c_thrust_0 * n * n);
    let torque_nm = rho
        * d.powi(5)
        * (vehicle.c_torque_2 * n_j * n_j
            + vehicle.c_torque_1 * n_j * n
            + vehicle.c_torque_0 * n * n);
    Propeller {
        thrust_n,
        torque_nm,
    }
}

#[cfg(test)]
mod tests {
    use super::super::airframe::AerosondeAirframe;
    use super::{loads, speed_radps};

    #[test]
    fn full_throttle_pulls_and_a_cut_motor_at_cruise_drags() {
        let vehicle = AerosondeAirframe::Aerosonde.vehicle().unwrap();
        let rho = 1.2682;
        let standing = loads(&vehicle, rho, 0.0, 1.0);
        assert!(standing.thrust_n > 20.0 && standing.torque_nm > 0.0);
        let cruise = loads(&vehicle, rho, 25.0, 1.0);
        assert!(cruise.thrust_n > 0.0 && cruise.thrust_n < standing.thrust_n);
        assert!(speed_radps(&vehicle, rho, 25.0, 1.0) > speed_radps(&vehicle, rho, 0.0, 1.0));
        // The windmilling propeller at cruise with no throttle: a drag.
        let cut = loads(&vehicle, rho, 25.0, 0.0);
        assert!(cut.thrust_n < 0.0 && speed_radps(&vehicle, rho, 25.0, 0.0) > 0.0);
        // At rest with no throttle it stands still, with no thrust or torque.
        let rest = loads(&vehicle, rho, 0.0, 0.0);
        assert_eq!(speed_radps(&vehicle, rho, 0.0, 0.0), 0.0);
        assert_eq!(rest.thrust_n, 0.0);
        assert_eq!(rest.torque_nm, 0.0);
    }
}
