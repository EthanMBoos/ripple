//! FixedWing6DOF's coefficient equations in the forward/right/down model frame.
//! GTRI SCRIMMAGE reference: FixedWing6DOF.cpp (LGPL-3.0-or-later).

use std::f64::consts::PI;

use crate::math::Vec3;

use super::{Control, FixedWingParams};

/// Coefficient names deliberately match the aircraft equations and XML keys.
#[derive(Clone)]
pub(super) struct Aerodynamics {
    c_d0: f64,
    c_d_alpha: f64,
    c_d_elevator: f64,
    c_l0: f64,
    c_l_alpha: f64,
    c_lq: f64,
    c_l_elevator: f64,
    c_y_beta: f64,
    c_y_rudder: f64,
    c_roll_beta: f64,
    c_roll_p: f64,
    c_roll_r: f64,
    c_roll_aileron: f64,
    c_roll_rudder: f64,
    c_m0: f64,
    c_m_alpha: f64,
    c_mq: f64,
    c_m_elevator: f64,
    c_n_beta: f64,
    c_np: f64,
    c_nr: f64,
    c_n_aileron: f64,
    c_n_rudder: f64,
}

pub(super) struct Airframe {
    pub density_kgpm3: f64,
    pub span_m: f64,
    pub area_m2: f64,
    pub chord_m: f64,
    pub efficiency: f64,
}

pub(super) struct Loads {
    pub force_model_n: Vec3,
    pub moment_model_nm: Vec3,
}

impl Aerodynamics {
    pub(super) fn new(params: &FixedWingParams) -> Self {
        Self {
            c_d0: params.c_d0,
            c_d_alpha: params.c_d_alpha,
            c_d_elevator: params.c_d_elevator,
            c_l0: params.c_l0,
            c_l_alpha: params.c_l_alpha,
            c_lq: params.c_lq,
            c_l_elevator: params.c_l_elevator,
            c_y_beta: params.c_y_beta,
            c_y_rudder: params.c_y_rudder,
            c_roll_beta: params.c_roll_beta,
            c_roll_p: params.c_roll_p,
            c_roll_r: params.c_roll_r,
            c_roll_aileron: params.c_roll_aileron,
            c_roll_rudder: params.c_roll_rudder,
            c_m0: params.c_m0,
            c_m_alpha: params.c_m_alpha,
            c_mq: params.c_mq,
            c_m_elevator: params.c_m_elevator,
            c_n_beta: params.c_n_beta,
            c_np: params.c_np,
            c_nr: params.c_nr,
            c_n_aileron: params.c_n_aileron,
            c_n_rudder: params.c_n_rudder,
        }
    }

    pub fn loads(
        &self,
        airframe: &Airframe,
        air_velocity_model_mps: Vec3,
        rates_model_radps: Vec3,
        control: Control,
    ) -> Loads {
        let mut airspeed_mps = air_velocity_model_mps.norm();
        if airspeed_mps.abs() < f64::EPSILON {
            airspeed_mps = 0.00001;
        }
        let alpha_rad = air_velocity_model_mps.z.atan2(air_velocity_model_mps.x);
        let beta_rad = (air_velocity_model_mps.y / airspeed_mps)
            .clamp(-1.0, 1.0)
            .asin();
        let twice_speed_mps = 2.0 * airspeed_mps;
        let span_m = airframe.span_m;
        let chord_m = airframe.chord_m;
        let aspect_ratio = span_m / chord_m;
        let pressure_area_n =
            airframe.density_kgpm3 * airspeed_mps.powi(2) * airframe.area_m2 / 2.0;
        let p_radps = rates_model_radps.x;
        let q_radps = rates_model_radps.y;
        let r_radps = rates_model_radps.z;

        let lift_coefficient = self.c_l0
            + self.c_l_alpha * alpha_rad
            + self.c_lq * q_radps * chord_m / twice_speed_mps
            + self.c_l_elevator * control.elevator_rad;
        let lift_n = lift_coefficient * pressure_area_n;
        let drag_n = (self.c_d0
            + self.c_d_alpha * alpha_rad.abs()
            + lift_coefficient * lift_coefficient / (PI * aspect_ratio * airframe.efficiency)
            + self.c_d_elevator * control.elevator_rad.abs())
            * pressure_area_n;
        let side_n =
            (self.c_y_beta * beta_rad + self.c_y_rudder * control.rudder_rad) * pressure_area_n;
        let force_model_n = Vec3::new(
            lift_n * alpha_rad.sin() - drag_n * alpha_rad.cos() - side_n * beta_rad.sin(),
            side_n * beta_rad.cos(),
            -lift_n * alpha_rad.cos() - drag_n * alpha_rad.sin(),
        );

        let roll_nm = (self.c_roll_beta * beta_rad
            + self.c_roll_p * p_radps * span_m / twice_speed_mps
            + self.c_roll_r * r_radps * span_m / twice_speed_mps
            + self.c_roll_aileron * control.aileron_rad
            + self.c_roll_rudder * control.rudder_rad)
            * pressure_area_n
            * span_m;
        let pitch_nm = (self.c_m0
            + self.c_m_alpha * alpha_rad
            + self.c_mq * q_radps * chord_m / twice_speed_mps
            + self.c_m_elevator * control.elevator_rad)
            * pressure_area_n
            * chord_m;
        let yaw_nm = (self.c_n_beta * beta_rad
            + self.c_np * p_radps * span_m / twice_speed_mps
            + self.c_nr * r_radps * span_m / twice_speed_mps
            + self.c_n_aileron * control.aileron_rad
            + self.c_n_rudder * control.rudder_rad)
            * pressure_area_n
            * span_m;
        Loads {
            force_model_n,
            moment_model_nm: Vec3::new(roll_nm, pitch_nm, yaw_nm),
        }
    }
}
