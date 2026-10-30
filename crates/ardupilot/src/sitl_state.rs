//! The state ArduPilot SITL needs, sampled from truth after motion.
//!
//! Sensor phase: read truth, convert to ArduPilot's frames, publish a
//! `SitlSample` on LocalNetwork for `ArduPilotBridge` to send next tick.
//! Truth is sent on purpose: SITL makes its own noisy IMU, GPS, baro and compass
//! from it, and its EKF is the aircraft's belief.
//!
//! It must run every tick (no `loop_rate_hz`): the acceleration divides one
//! tick's velocity change by one tick. A skipped tick is an error.
//!
//! Frames: Ripple uses ENU world and forward-left-up (FLU) body axes;
//! ArduPilot uses NED world and forward-right-down (FRD) body axes.

use anyhow::{Result, ensure};
use nalgebra::{Matrix3, Rotation3, UnitQuaternion};
use ripple_core::plugin::{Plugin, PluginParams, Sensor, SensorContext, Update};
use ripple_core::{KinematicState, Vec3};
use serde::{Deserialize, Serialize};

/// LocalNetwork topic carrying one `SitlSample` per tick.
pub const SITL_STATE_TOPIC: &str = "SitlState";

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SitlStateConfig {
    /// Must match the motion model's gravity (FixedWing6DOF's default is 9.81).
    gravity_mps2: f64,
    /// Must match FixedWing6DOF's `wind_mps`, which the sensor cannot read.
    wind_mps: [f64; 3],
}

impl Default for SitlStateConfig {
    fn default() -> Self {
        Self {
            gravity_mps2: 9.81,
            wind_mps: [0.0; 3],
        }
    }
}

/// One state sample in ArduPilot's frames and units.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SitlSample {
    pub position_ned_m: Vec3,
    pub velocity_ned_mps: Vec3,
    /// Rotation from FRD body axes to NED, scalar first (w, x, y, z).
    pub orientation_ned_from_frd: [f64; 4],
    pub gyro_frd_radps: Vec3,
    /// Specific force (what an accelerometer reads): (0, 0, -g) at rest, level.
    pub specific_force_frd_mps2: Vec3,
    /// True airspeed, the length of the air-relative velocity.
    pub airspeed_mps: f64,
}

pub struct SitlState {
    gravity_mps2: f64,
    wind_world_mps: Vec3,
    /// World velocity one tick ago (the initial velocity before the first
    /// sample), for the acceleration estimate.
    previous_velocity_world_mps: Vec3,
    /// Time of the previous sample, to check that no tick was skipped.
    previous_time_s: Option<f64>,
}

impl Plugin for SitlState {
    type Config = SitlStateConfig;

    fn configure(params: &PluginParams<'_>) -> Result<SitlStateConfig> {
        let config: SitlStateConfig = params.parse()?;
        ensure!(
            config.gravity_mps2 > 0.0,
            "SitlState gravity_mps2 must be positive"
        );
        Ok(config)
    }

    fn new(config: &SitlStateConfig) -> Self {
        Self {
            gravity_mps2: config.gravity_mps2,
            wind_world_mps: Vec3::from(config.wind_mps),
            previous_velocity_world_mps: Vec3::zeros(),
            previous_time_s: None,
        }
    }
}

impl Sensor for SitlState {
    fn initialize(&mut self, context: &mut SensorContext<'_>) -> Result<()> {
        // Startup truth is the state before the first motion step.
        self.previous_velocity_world_mps = context.truth.velocity_world_mps;
        Ok(())
    }

    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update> {
        if let Some(previous_time_s) = self.previous_time_s {
            let elapsed_s = context.time.time_s - previous_time_s;
            ensure!(
                (elapsed_s - context.time.dt_s).abs() <= 1e-9 * context.time.dt_s.max(1.0),
                "SitlState must run every tick (remove its loop_rate_hz)"
            );
        }
        self.previous_time_s = Some(context.time.time_s);

        // Acceleration over the tick just integrated, from the change in world velocity.
        let velocity_world_mps = context.truth.velocity_world_mps;
        let acceleration_world_mps2 =
            (velocity_world_mps - self.previous_velocity_world_mps) / context.time.dt_s;
        self.previous_velocity_world_mps = velocity_world_mps;

        let sample = sample(
            context.truth,
            acceleration_world_mps2,
            self.wind_world_mps,
            self.gravity_mps2,
        );
        context
            .messages
            .publish("LocalNetwork", SITL_STATE_TOPIC, sample)?;
        Ok(Update::Applied)
    }
}

/// Convert a truth state and its world acceleration to ArduPilot's frames.
fn sample(
    truth: &KinematicState,
    acceleration_world_mps2: Vec3,
    wind_world_mps: Vec3,
    gravity_mps2: f64,
) -> SitlSample {
    let attitude = truth.orientation_world_from_body;
    // Speed through the air, as FixedWing6DOF flies it.
    let airspeed_mps = (truth.velocity_world_mps - wind_world_mps).norm();

    // Specific force is acceleration minus gravity; gravity points down (-z in ENU).
    let gravity_world_mps2 = Vec3::new(0.0, 0.0, -gravity_mps2);
    let specific_force_world_mps2 = acceleration_world_mps2 - gravity_world_mps2;
    let specific_force_flu_mps2 = attitude.rotate_world_to_body(specific_force_world_mps2);
    let gyro_flu_radps = attitude.rotate_world_to_body(truth.angular_velocity_world_radps);

    // FRD body axes written in NED: forward stays forward, right is -left,
    // down is -up. These are the columns of the rotation from FRD to NED.
    let forward_ned = enu_to_ned(attitude.rotate_body_to_world(Vec3::x()));
    let right_ned = enu_to_ned(-attitude.rotate_body_to_world(Vec3::y()));
    let down_ned = enu_to_ned(-attitude.rotate_body_to_world(Vec3::z()));
    let rotation =
        Rotation3::from_matrix(&Matrix3::from_columns(&[forward_ned, right_ned, down_ned]));
    let orientation = UnitQuaternion::from_rotation_matrix(&rotation);

    SitlSample {
        position_ned_m: enu_to_ned(truth.position_world_m),
        velocity_ned_mps: enu_to_ned(truth.velocity_world_mps),
        orientation_ned_from_frd: [orientation.w, orientation.i, orientation.j, orientation.k],
        gyro_frd_radps: flu_to_frd(gyro_flu_radps),
        specific_force_frd_mps2: flu_to_frd(specific_force_flu_mps2),
        airspeed_mps,
    }
}

/// East-north-up to north-east-down.
fn enu_to_ned(enu: Vec3) -> Vec3 {
    Vec3::new(enu.y, enu.x, -enu.z)
}

/// Forward-left-up to forward-right-down.
fn flu_to_frd(flu: Vec3) -> Vec3 {
    Vec3::new(flu.x, -flu.y, -flu.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ripple_core::{EulerAngles, Quaternion};

    fn state(roll_deg: f64, pitch_deg: f64, yaw_deg: f64) -> KinematicState {
        KinematicState {
            orientation_world_from_body: Quaternion::from_euler(EulerAngles {
                roll_world_from_body_rad: roll_deg.to_radians(),
                pitch_world_from_body_rad: pitch_deg.to_radians(),
                yaw_world_from_body_rad: yaw_deg.to_radians(),
            }),
            ..KinematicState::default()
        }
    }

    fn close(actual: Vec3, expected: Vec3) {
        assert!(
            (actual - expected).norm() < 1e-9,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn level_at_rest_reads_minus_g_down() {
        let sample = sample(&state(0.0, 0.0, 0.0), Vec3::zeros(), Vec3::zeros(), 9.81);
        close(sample.specific_force_frd_mps2, Vec3::new(0.0, 0.0, -9.81));
    }

    #[test]
    fn enu_east_heading_is_ned_yaw_90_degrees() {
        // Ripple yaw 0 faces east (ENU +x); ArduPilot measures yaw from north.
        let truth = KinematicState {
            position_world_m: Vec3::new(10.0, 20.0, 30.0),
            velocity_world_mps: Vec3::new(20.0, 0.0, 0.0),
            ..state(0.0, 0.0, 0.0)
        };
        let sample = sample(&truth, Vec3::zeros(), Vec3::zeros(), 9.81);
        close(sample.position_ned_m, Vec3::new(20.0, 10.0, -30.0));
        close(sample.velocity_ned_mps, Vec3::new(0.0, 20.0, 0.0));
        let (_, _, yaw) = roll_pitch_yaw(&sample);
        assert!((yaw - 90f64.to_radians()).abs() < 1e-9, "yaw {yaw}");
    }

    fn roll_pitch_yaw(sample: &SitlSample) -> (f64, f64, f64) {
        let [w, x, y, z] = sample.orientation_ned_from_frd;
        UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(w, x, y, z)).euler_angles()
    }

    #[test]
    fn roll_keeps_its_sign_and_tilts_gravity_into_the_right_wing() {
        // FLU roll about forward lifts the left wing: right wing down, which is
        // also ArduPilot's positive roll.
        let sample = sample(&state(20.0, 0.0, 0.0), Vec3::zeros(), Vec3::zeros(), 9.81);
        let (roll, pitch, _) = roll_pitch_yaw(&sample);
        assert!((roll - 20f64.to_radians()).abs() < 1e-9, "roll {roll}");
        assert!(pitch.abs() < 1e-9);
        // The accelerometer reads "up" (+g); with the right wing down, part of
        // up lies along -right.
        let angle = 20f64.to_radians();
        close(
            sample.specific_force_frd_mps2,
            Vec3::new(0.0, -9.81 * angle.sin(), -9.81 * angle.cos()),
        );
    }

    #[test]
    fn pitch_changes_sign() {
        // FLU pitch about the left axis puts the nose down; ArduPilot's pitch is
        // nose up, so +10 degrees here is -10 there.
        let sample = sample(&state(0.0, 10.0, 0.0), Vec3::zeros(), Vec3::zeros(), 9.81);
        let (_, pitch, _) = roll_pitch_yaw(&sample);
        assert!((pitch + 10f64.to_radians()).abs() < 1e-9, "pitch {pitch}");
    }

    #[test]
    fn airspeed_is_speed_through_the_air() {
        let truth = KinematicState {
            velocity_world_mps: Vec3::new(20.0, 0.0, 0.0),
            ..state(0.0, 0.0, 0.0)
        };
        let calm = sample(&truth, Vec3::zeros(), Vec3::zeros(), 9.81);
        assert!((calm.airspeed_mps - 20.0).abs() < 1e-12);
        // An east wind of +5 on an eastbound aircraft is a tailwind.
        let windy = sample(&truth, Vec3::zeros(), Vec3::new(5.0, 0.0, 0.0), 9.81);
        assert!((windy.airspeed_mps - 15.0).abs() < 1e-12);
    }

    #[test]
    fn body_rates_follow_the_frame_change() {
        // Level, facing east, yawing left (counterclockwise from above) in ENU.
        let truth = KinematicState {
            angular_velocity_world_radps: Vec3::new(0.0, 0.0, 0.5),
            ..state(0.0, 0.0, 0.0)
        };
        let sample = sample(&truth, Vec3::zeros(), Vec3::zeros(), 9.81);
        // FRD yaw rate is positive turning right, so a left turn is negative.
        close(sample.gyro_frd_radps, Vec3::new(0.0, 0.0, -0.5));
    }
}
