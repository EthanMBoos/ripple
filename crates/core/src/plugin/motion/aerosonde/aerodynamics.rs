//! The Aerosonde's aerodynamic force and moment in the forward/right/down
//! body frame: Beard and McLain, *Small Unmanned Aircraft* (2012), chapter
//! 4, as written. Lift and drag are rotated into the body by the angle of
//! attack alone, and the side force is along the right axis as it stands;
//! the book does not rotate by sideslip.

use std::f64::consts::PI;

use crate::math::Vec3;

use super::Control;
use super::airframe::Vehicle;

/// The aerodynamic force, from the air velocity (ground velocity minus the
/// wind's) in the body axes.
pub(super) fn force(
    vehicle: &Vehicle,
    air_density_kgpm3: f64,
    air_velocity_frd_mps: Vec3,
    rates_frd_radps: Vec3,
    control: Control,
) -> Vec3 {
    // Symbols, as in the book: airspeed, angle of attack, sideslip, dynamic
    // pressure times wing area, the body rates, the surface angles, and the
    // span and chord.
    let Flow {
        v,
        alpha,
        beta,
        qbar_s,
    } = flow(vehicle, air_density_kgpm3, air_velocity_frd_mps);
    let (p, q, r) = (rates_frd_radps.x, rates_frd_radps.y, rates_frd_radps.z);
    let (delta_e, delta_a, delta_r) = (
        control.elevator_rad,
        control.aileron_rad,
        control.rudder_rad,
    );
    let (b, c) = (vehicle.wing_span_m, vehicle.mean_chord_m);
    let two_v = 2.0 * v;
    let c_lift_of_alpha = lift_coefficient(vehicle, alpha);
    // Drag against angle of attack, the book's parabolic form: the parasitic
    // C_Dp plus the induced drag of the linear lift, C_L^2 / (pi e AR).
    let c_lift_linear = vehicle.c_lift_0 + vehicle.c_lift_alpha * alpha;
    let aspect_ratio = b * b / vehicle.wing_area_m2;
    let c_drag_of_alpha = vehicle.c_drag_p
        + c_lift_linear * c_lift_linear / (PI * vehicle.oswald_efficiency * aspect_ratio);
    // F_lift and F_drag, with the pitch rate and the elevator added to each;
    // the elevator's drag is from the signed deflection, as the book has it.
    let lift_n =
        (c_lift_of_alpha + vehicle.c_lift_q * q * c / two_v + vehicle.c_lift_elevator * delta_e)
            * qbar_s;
    let drag_n =
        (c_drag_of_alpha + vehicle.c_drag_q * q * c / two_v + vehicle.c_drag_elevator * delta_e)
            * qbar_s;
    // f_y, the lateral force, with the rates and both surfaces.
    let side_n = (vehicle.c_side_0
        + vehicle.c_side_beta * beta
        + vehicle.c_side_p * p * b / two_v
        + vehicle.c_side_r * r * b / two_v
        + vehicle.c_side_aileron * delta_a
        + vehicle.c_side_rudder * delta_r)
        * qbar_s;
    // (f_x, f_z) from (-F_drag, -F_lift) rotated by alpha about the right axis.
    let (ca, sa) = (alpha.cos(), alpha.sin());
    Vec3::new(
        -drag_n * ca + lift_n * sa,
        side_n,
        -drag_n * sa - lift_n * ca,
    )
}

/// The aerodynamic moment about the center of mass.
pub(super) fn moment(
    vehicle: &Vehicle,
    air_density_kgpm3: f64,
    air_velocity_frd_mps: Vec3,
    rates_frd_radps: Vec3,
    control: Control,
) -> Vec3 {
    let Flow {
        v,
        alpha,
        beta,
        qbar_s,
    } = flow(vehicle, air_density_kgpm3, air_velocity_frd_mps);
    let (p, q, r) = (rates_frd_radps.x, rates_frd_radps.y, rates_frd_radps.z);
    let (delta_e, delta_a, delta_r) = (
        control.elevator_rad,
        control.aileron_rad,
        control.rudder_rad,
    );
    let (b, c) = (vehicle.wing_span_m, vehicle.mean_chord_m);
    let two_v = 2.0 * v;
    // l, m, n: the roll, pitch, and yaw moments.
    let roll_nm = (vehicle.c_roll_0
        + vehicle.c_roll_beta * beta
        + vehicle.c_roll_p * p * b / two_v
        + vehicle.c_roll_r * r * b / two_v
        + vehicle.c_roll_aileron * delta_a
        + vehicle.c_roll_rudder * delta_r)
        * qbar_s
        * b;
    let pitch_nm = (vehicle.c_pitch_0
        + vehicle.c_pitch_alpha * alpha
        + vehicle.c_pitch_q * q * c / two_v
        + vehicle.c_pitch_elevator * delta_e)
        * qbar_s
        * c;
    let yaw_nm = (vehicle.c_yaw_0
        + vehicle.c_yaw_beta * beta
        + vehicle.c_yaw_p * p * b / two_v
        + vehicle.c_yaw_r * r * b / two_v
        + vehicle.c_yaw_aileron * delta_a
        + vehicle.c_yaw_rudder * delta_r)
        * qbar_s
        * b;
    Vec3::new(roll_nm, pitch_nm, yaw_nm)
}

/// The lift coefficient against angle of attack: the linear lift
/// C_L0 + C_Lalpha alpha blended by the sigmoid sigma(alpha) into a flat
/// plate's 2 sign(alpha) sin^2(alpha) cos(alpha) past the stall. sigma is
/// near 0 below alpha_0 and near 1 past it, with M setting how sharply.
pub(super) fn lift_coefficient(vehicle: &Vehicle, alpha: f64) -> f64 {
    let (big_m, alpha_0) = (vehicle.stall_blend_rate, vehicle.stall_alpha_rad);
    let (below, above) = (
        (-big_m * (alpha - alpha_0)).exp(),
        (big_m * (alpha + alpha_0)).exp(),
    );
    let sigma = (1.0 + below + above) / ((1.0 + below) * (1.0 + above));
    let linear = vehicle.c_lift_0 + vehicle.c_lift_alpha * alpha;
    let flat_plate = 2.0 * alpha.signum() * alpha.sin().powi(2) * alpha.cos();
    (1.0 - sigma) * linear + sigma * flat_plate
}

struct Flow {
    v: f64,
    alpha: f64,
    beta: f64,
    qbar_s: f64,
}

/// Airspeed V_a, angle of attack alpha = atan(w_r / u_r), sideslip
/// beta = asin(v_r / V_a), and q-bar S. The book leaves V_a = 0 undefined
/// (the rates enter over 2 V_a); a tiny floor keeps a resting aircraft
/// finite, with every load then at zero.
fn flow(vehicle: &Vehicle, air_density_kgpm3: f64, air_velocity_frd_mps: Vec3) -> Flow {
    let v = air_velocity_frd_mps.norm().max(1e-5);
    Flow {
        v,
        alpha: air_velocity_frd_mps.z.atan2(air_velocity_frd_mps.x),
        beta: (air_velocity_frd_mps.y / v).clamp(-1.0, 1.0).asin(),
        qbar_s: 0.5 * air_density_kgpm3 * v * v * vehicle.wing_area_m2,
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use super::super::airframe::AerosondeAirframe;
    use super::lift_coefficient;

    #[test]
    fn lift_is_linear_in_flight_and_a_flat_plate_past_the_stall() {
        let vehicle = AerosondeAirframe::Aerosonde.vehicle().unwrap();
        let alpha = 0.05;
        let linear = vehicle.c_lift_0 + vehicle.c_lift_alpha * alpha;
        assert!((lift_coefficient(&vehicle, alpha) - linear).abs() < 1e-6);
        // Broadside to the flow, the flat plate gives 2 sin^2 cos = 0; at 60
        // degrees it gives 2 (3/4)(1/2) = 0.75, far below the linear 6.1.
        assert!(lift_coefficient(&vehicle, FRAC_PI_2).abs() < 1e-9);
        assert!((lift_coefficient(&vehicle, 60f64.to_radians()) - 0.75).abs() < 1e-9);
        // Odd in alpha where the blend is symmetric.
        assert!(
            (lift_coefficient(&vehicle, -0.6) + lift_coefficient(&vehicle, 0.6)
                - 2.0 * vehicle.c_lift_0 * (1.0 - sigma(&vehicle, 0.6)))
            .abs()
                < 1e-9
        );
    }

    fn sigma(vehicle: &super::Vehicle, alpha: f64) -> f64 {
        let (m, a0) = (vehicle.stall_blend_rate, vehicle.stall_alpha_rad);
        let (below, above) = ((-m * (alpha - a0)).exp(), (m * (alpha + a0)).exp());
        (1.0 + below + above) / ((1.0 + below) * (1.0 + above))
    }
}
