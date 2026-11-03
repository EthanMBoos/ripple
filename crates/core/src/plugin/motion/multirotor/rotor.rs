//! One rotor's force and moment on the body, and its motor's lag.
//!
//! Thrust and reaction torque go with rotor speed squared, along each rotor's
//! axis.
//! Motor lag, rotor drag, and the rolling moment are optional ports of RotorS's
//! motor plugin (ETH Zurich ASL, Apache-2.0: `GazeboMotorModel::
//! UpdateForcesAndMoments` and `FirstOrderFilter` in rotors_gazebo_plugins, at
//! commit cd813b7a8c37). RotorS takes rotor drag and the rolling moment from
//! Martin and Salaün (ICRA 2010): a spinning rotor moving edgewise is pushed
//! back in proportion to its speed, which sets a quad's velocity dynamics near
//! hover. All three are off (zero) unless a mission sets them, and then the
//! model computes exactly what it did before they were added.
//!
//! `reference/rotors_motor.py` runs RotorS's own source on sampled states and
//! these functions must reproduce it sample by sample (`tests` below).

use crate::math::Vec3;

use super::airframe::Rotor;

/// RotorS's per-rotor aerodynamic coefficients, plus the axial drag term of
/// Förster's model. Zero turns an effect off.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RotorAerodynamics {
    /// RotorS `rotorDragCoefficient` (kg/rad): in-plane drag force
    /// -|w| * this * hub velocity across the rotor axis.
    pub drag_coefficient: f64,
    /// Not in RotorS: Förster's K_aero along the rotor axis (kg/rad), drag
    /// -|w| * this * hub velocity along the axis. Zero matches RotorS.
    pub axial_drag_coefficient: f64,
    /// RotorS `rollingMomentCoefficient` (kg m/rad): moment
    /// -|w| * this * hub velocity across the rotor axis.
    pub rolling_moment_coefficient: f64,
}

impl RotorAerodynamics {
    fn is_off(self) -> bool {
        self.drag_coefficient == 0.0
            && self.axial_drag_coefficient == 0.0
            && self.rolling_moment_coefficient == 0.0
    }
}

/// RotorS's motor: the shaft speed approaches its command as a first-order
/// lag, with separate spin-up and spin-down time constants. Zero is no lag.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MotorLag {
    /// RotorS `timeConstantUp`.
    pub time_constant_up_s: f64,
    /// RotorS `timeConstantDown`.
    pub time_constant_down_s: f64,
}

impl MotorLag {
    /// Speed `elapsed_s` after the start of a step whose command is held.
    /// At the end of a step this is RotorS's `FirstOrderFilter::updateFilter`
    /// with the same arithmetic; within it, the exact solution, so RK4
    /// stages see the speed the rotor has then.
    pub fn speed_radps(self, start_radps: f64, command_radps: f64, elapsed_s: f64) -> f64 {
        // RotorS picks the constant by comparing command with the last speed.
        let time_constant_s = if command_radps > start_radps {
            self.time_constant_up_s
        } else {
            self.time_constant_down_s
        };
        if time_constant_s == 0.0 {
            return command_radps;
        }
        let alpha = (-elapsed_s / time_constant_s).exp();
        alpha * start_radps + (1.0 - alpha) * command_radps
    }
}

/// A rotor's load on the body, with the moment about the center of mass.
pub(super) struct RotorLoads {
    pub force_body_n: Vec3,
    pub moment_body_nm: Vec3,
}

/// Loads from one rotor turning at `speed_radps`, on a body moving at
/// `velocity_body_mps` and rotating at `rates_body_radps` (both relative to
/// still air).
pub(super) fn rotor_loads(
    rotor: &Rotor,
    thrust_coefficient: f64,
    torque_coefficient: f64,
    aerodynamics: RotorAerodynamics,
    speed_radps: f64,
    velocity_body_mps: Vec3,
    rates_body_radps: Vec3,
) -> RotorLoads {
    // Symbols, as in RotorS: the rotor's speed, axis, and offset from the
    // center of mass, its thrust and torque coefficients, and the body's
    // velocity and rates.
    let (omega_i, e_i, r_i) = (speed_radps, rotor.thrust_axis_body, rotor.offset_body_m);
    let (c_t, c_q) = (thrust_coefficient, torque_coefficient);
    let (v, omega) = (velocity_body_mps, rates_body_radps);
    let omega_i_sq = omega_i * omega_i;

    // Thrust c_T omega_i^2 along the rotor axis, applied at the hub, and the
    // reaction to the air's drag on the blades, c_Q omega_i^2 about the axis
    // (RotorS's "drag torque", moment constant c_Q / c_T).
    let thrust_n = e_i * (c_t * omega_i_sq);
    let reaction_torque_nm = e_i * (rotor.direction.reaction_sign() * c_q * omega_i_sq);
    let mut loads = RotorLoads {
        force_body_n: thrust_n,
        moment_body_nm: r_i.cross(&thrust_n) + reaction_torque_nm,
    };

    // Not modeled here: thrust change with inflow (climb or forward speed),
    // which would scale the thrust and needs a measured thrust curve against
    // airspeed, and wind. Ground effect, downwash, and the rotor's
    // gyroscopic and spin-up torques are named zeros in multirotor.rs.
    if aerodynamics.is_off() {
        return loads;
    }

    // The hub's velocity v + omega x r_i: RotorS reads the rotor link's own
    // velocity, which includes the body's rotation.
    let hub_velocity = v + omega.cross(&r_i);
    let axial_speed = hub_velocity.dot(&e_i);
    let edgewise_velocity = hub_velocity - e_i * axial_speed;
    let abs_omega_i = omega_i.abs();

    // Rotor drag -|omega_i| c_D across the axis (and Forster's axial term
    // along it). RotorS adds it at the rotor link, so it also pitches and
    // rolls the body (r_i x F).
    let rotor_drag_n = edgewise_velocity * (-abs_omega_i * aerodynamics.drag_coefficient)
        + e_i * (-abs_omega_i * aerodynamics.axial_drag_coefficient * axial_speed);
    let rolling_moment_nm =
        edgewise_velocity * (-abs_omega_i * aerodynamics.rolling_moment_coefficient);
    loads.force_body_n += rotor_drag_n;
    loads.moment_body_nm += r_i.cross(&rotor_drag_n) + rolling_moment_nm;
    loads
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::super::airframe::{Direction, Rotor};
    use super::{MotorLag, RotorAerodynamics, rotor_loads};
    use crate::math::{Quaternion, Vec3};

    fn vector(value: &Value) -> Vec3 {
        Vec3::new(
            value[0].as_f64().unwrap(),
            value[1].as_f64().unwrap(),
            value[2].as_f64().unwrap(),
        )
    }

    fn quaternion(value: &Value) -> Quaternion {
        Quaternion {
            w: value[0].as_f64().unwrap(),
            x: value[1].as_f64().unwrap(),
            y: value[2].as_f64().unwrap(),
            z: value[3].as_f64().unwrap(),
        }
    }

    /// Every sample of RotorS's motor plugin, run by reference/rotors_motor.py
    /// on prescribed states, reproduced from the same inputs: the loads from
    /// the speed RotorS used, and the filtered speed it set for the next step.
    /// Prints the largest errors for that script.
    #[test]
    fn rotors_motor_samples() {
        let text = match std::env::var("RIPPLE_ROTORS_SAMPLES") {
            Ok(path) => std::fs::read_to_string(path).expect("samples file"),
            Err(_) => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/rotors_motor_samples.json"
            ))
            .to_string(),
        };
        let fixture: Value = serde_json::from_str(&text).expect("samples JSON");
        let mut largest_force_n: f64 = 0.0;
        let mut largest_moment_nm: f64 = 0.0;
        let mut largest_speed_radps: f64 = 0.0;
        let mut count = 0;
        for (name, case) in fixture["cases"].as_object().unwrap() {
            let attitude = quaternion(&case["rotor_quaternion"]);
            let rotor = Rotor {
                port: "motor_0".into(),
                direction: match case["direction"].as_str().unwrap() {
                    "CW" => Direction::Clockwise,
                    _ => Direction::Counterclockwise,
                },
                offset_body_m: vector(&case["offset_body_m"]),
                thrust_axis_body: attitude.rotate_body_to_world(Vec3::new(0.0, 0.0, 1.0)),
            };
            let thrust_coefficient = case["thrust_coefficient"].as_f64().unwrap();
            let torque_coefficient = case["torque_coefficient"].as_f64().unwrap();
            let aerodynamics = RotorAerodynamics {
                drag_coefficient: case["rotor_drag_coefficient"].as_f64().unwrap(),
                axial_drag_coefficient: 0.0,
                rolling_moment_coefficient: case["rolling_moment_coefficient"].as_f64().unwrap(),
            };
            let lag = MotorLag {
                time_constant_up_s: case["time_constant_up_s"].as_f64().unwrap(),
                time_constant_down_s: case["time_constant_down_s"].as_f64().unwrap(),
            };
            for sample in case["samples"].as_array().unwrap() {
                let vehicle = quaternion(&sample["attitude"]);
                let speed_radps = sample["speed_used_radps"].as_f64().unwrap();
                let loads = rotor_loads(
                    &rotor,
                    thrust_coefficient,
                    torque_coefficient,
                    aerodynamics,
                    speed_radps,
                    vehicle.rotate_world_to_body(vector(&sample["velocity_world_mps"])),
                    vehicle.rotate_world_to_body(vector(&sample["angular_velocity_world_radps"])),
                );
                let force_error_n = (loads.force_body_n - vector(&sample["force_body_n"])).norm();
                let moment_error_nm =
                    (loads.moment_body_nm - vector(&sample["moment_body_nm"])).norm();
                let speed_after_radps = lag.speed_radps(
                    speed_radps,
                    sample["command_radps"].as_f64().unwrap(),
                    sample["dt_s"].as_f64().unwrap(),
                );
                let speed_error_radps =
                    (speed_after_radps - sample["speed_after_radps"].as_f64().unwrap()).abs();
                // Relative to the sample's own size: RotorS stores the shaft
                // speed divided by its slowdown factor (10), which rounds.
                let thrust_n = thrust_coefficient * speed_radps * speed_radps;
                let arm_m = rotor.offset_body_m.norm();
                assert!(
                    force_error_n <= 1e-12 * thrust_n,
                    "{name}: force error {force_error_n}"
                );
                assert!(
                    moment_error_nm <= 1e-12 * thrust_n * (1.0 + arm_m),
                    "{name}: moment error {moment_error_nm}"
                );
                assert!(
                    speed_error_radps <= 1e-12 * speed_radps,
                    "{name}: speed error {speed_error_radps}"
                );
                largest_force_n = largest_force_n.max(force_error_n);
                largest_moment_nm = largest_moment_nm.max(moment_error_nm);
                largest_speed_radps = largest_speed_radps.max(speed_error_radps);
                count += 1;
            }
        }
        assert!(count > 0);
        println!(
            "ROTORS_ERRORS {{\"samples\": {count}, \"force_n\": {largest_force_n:e}, \
             \"moment_nm\": {largest_moment_nm:e}, \"speed_radps\": {largest_speed_radps:e}}}"
        );
    }
}
