//! Position integrates commanded ENU velocity. C++ counterpart: motion/SingleIntegrator.
//! Motion phase. Legacy max_speed >= 0 sets speed magnitude; it is not a speed cap.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::math::{EulerAngles, Quaternion, Vec3};
use crate::plugin::{
    Frame, MotionContext, MotionModel, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SingleIntegratorConfig {
    /// Negative means "use the commanded speed".
    #[serde(rename = "max_speed")]
    max_speed_mps: f64,
    /// Not implemented; must stay false.
    override_heading: bool,
}

impl Default for SingleIntegratorConfig {
    fn default() -> Self {
        Self {
            max_speed_mps: -1.0,
            override_heading: false,
        }
    }
}

pub struct SingleIntegrator {
    max_speed_mps: f64,
}

impl Plugin for SingleIntegrator {
    type Config = SingleIntegratorConfig;

    fn configure(params: &PluginParams<'_>) -> Result<Self::Config> {
        let config: SingleIntegratorConfig = params.parse()?;
        ensure!(
            !config.override_heading,
            "SingleIntegrator override_heading is not implemented"
        );
        Ok(config)
    }

    fn new(config: &Self::Config) -> Self {
        Self {
            max_speed_mps: config.max_speed_mps,
        }
    }

    fn ports(_: &Self::Config) -> Ports {
        Ports::default()
            .input(Port::new("velocity_x", Unit::MetersPerSecond, Frame::World))
            .input(Port::new("velocity_y", Unit::MetersPerSecond, Frame::World))
            .input(Port::new("velocity_z", Unit::MetersPerSecond, Frame::World))
    }
}

impl MotionModel for SingleIntegrator {
    fn initialize(&mut self, context: &mut MotionContext<'_>) -> Result<()> {
        context.truth.velocity_world_mps = Vec3::zeros();
        context.truth.orientation_world_from_body = Quaternion::from_euler(EulerAngles {
            yaw_world_from_body_rad: context
                .truth
                .orientation_world_from_body
                .yaw_world_from_body_rad(),
            ..EulerAngles::default()
        });
        Ok(())
    }

    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let desired_velocity_mps = Vec3::new(
            io.read("velocity_x")?,
            io.read("velocity_y")?,
            io.read("velocity_z")?,
        );
        let speed_mps = desired_velocity_mps.norm();
        let velocity_mps = if self.max_speed_mps < 0.0 {
            desired_velocity_mps
        } else if speed_mps == 0.0 {
            Vec3::zeros()
        } else {
            desired_velocity_mps / speed_mps * self.max_speed_mps
        };
        let state = &mut context.truth;
        state.velocity_world_mps = velocity_mps;
        state.position_world_m += velocity_mps * context.time.dt_s;
        // Matches C++ for now, though we don't consider it correct: positive pitch points the
        // nose down in this frame, so a climbing agent is drawn nose-down, and yaw snaps
        // to 0 (east) whenever the velocity is zero.
        state.orientation_world_from_body = Quaternion::from_euler(EulerAngles {
            roll_world_from_body_rad: 0.0,
            pitch_world_from_body_rad: velocity_mps.z.atan2(velocity_mps.xy().norm()),
            yaw_world_from_body_rad: velocity_mps.y.atan2(velocity_mps.x),
        });
        ensure!(
            state.position_world_m.iter().all(|value| value.is_finite()),
            "SingleIntegrator produced nonfinite position"
        );
        Ok(Update::Applied)
    }
}

#[cfg(test)]
mod tests {
    use super::{SingleIntegrator, SingleIntegratorConfig};
    use crate::plugin::{
        EntityInfo, Messages, MotionContext, MotionModel, Plugin, PluginIo, StepTime,
    };
    use crate::{KinematicState, Vec3};

    #[test]
    fn velocity_integrates_position_and_legacy_speed_is_a_magnitude_not_a_cap() -> anyhow::Result<()>
    {
        for (max_speed_mps, desired, expected) in [
            (-1.0, Vec3::new(3.0, 4.0, 0.0), Vec3::new(3.0, 4.0, 0.0)),
            (10.0, Vec3::new(3.0, 4.0, 0.0), Vec3::new(6.0, 8.0, 0.0)),
            (10.0, Vec3::zeros(), Vec3::zeros()),
            (-1.0, Vec3::new(0.0, 0.0, 2.0), Vec3::new(0.0, 0.0, 2.0)),
        ] {
            let config = SingleIntegratorConfig {
                max_speed_mps,
                ..SingleIntegratorConfig::default()
            };
            let mut model = SingleIntegrator::new(&config);
            let mut io = PluginIo::new(&SingleIntegrator::ports(&config));
            io.receive(
                &[
                    ("velocity_x".into(), desired.x),
                    ("velocity_y".into(), desired.y),
                    ("velocity_z".into(), desired.z),
                ]
                .into(),
            )?;
            let mut truth = KinematicState::default();
            model.step(
                &mut MotionContext {
                    entity: EntityInfo {
                        id: 1,
                        team_id: 1,
                        sub_swarm_id: 0,
                    },
                    time: StepTime {
                        time_s: 0.0,
                        dt_s: 0.25,
                    },
                    truth: &mut truth,
                    messages: &mut Messages::default(),
                },
                &mut io,
            )?;
            assert!((truth.velocity_world_mps - expected).norm() < 1e-12);
            assert!((truth.position_world_m - expected * 0.25).norm() < 1e-12);
        }
        Ok(())
    }
}
