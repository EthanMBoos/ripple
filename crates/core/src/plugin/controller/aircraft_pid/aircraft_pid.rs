//! Nested heading/altitude and roll/pitch PID loops for FixedWing6DOF.
//!
//! Derived from SCRIMMAGE: controller/AircraftPIDController (GTRI, LGPL-3.0-or-later).
//! Controller phase: read desired commands and belief, output throttle and surfaces.

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::common::{Pid, PidGains};
use crate::plugin::motion::FixedWingAirframe;
use crate::plugin::{
    AgentContext, Controller, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

/// Mission parameters. `airframe` loads the gains and attitude limits tuned
/// for that aircraft, which any key below overrides; without it the mission
/// gives every gain and limit. Each PID is `P, I, D, integral band`.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AircraftPidParams {
    /// `a4d` or `rascal`, as for FixedWing6DOF.
    airframe: Option<FixedWingAirframe>,
    #[serde(rename = "heading_pid")]
    heading: Option<PidGains>,
    #[serde(rename = "altitude_pid")]
    altitude: Option<PidGains>,
    #[serde(rename = "speed_pid")]
    speed: Option<PidGains>,
    #[serde(rename = "pitch_pid")]
    pitch: Option<PidGains>,
    #[serde(rename = "roll_pid")]
    roll: Option<PidGains>,
    #[serde(rename = "max_pitch")]
    max_pitch_deg: Option<f64>,
    #[serde(rename = "max_roll")]
    max_roll_deg: Option<f64>,
    use_roll_control: bool,
}

#[derive(Clone)]
pub struct AircraftPidConfig {
    heading: PidGains,
    altitude: PidGains,
    speed: PidGains,
    pitch: PidGains,
    roll: PidGains,
    max_pitch_deg: f64,
    max_roll_deg: f64,
    use_roll_control: bool,
}

/// The gains and limits each aircraft flies with. `use_roll_control` is a
/// mission choice, not part of a profile.
fn airframe_settings(airframe: FixedWingAirframe) -> AircraftPidConfig {
    // Set with the A-4D; its altitude derivative gain is 0.002, where 0.01
    // let a pitch upset grow into an altitude and heading oscillation at
    // 220 m/s (docs/REFERENCE_NOTES.md).
    let a4d = AircraftPidConfig {
        heading: PidGains::from([1.0, 0.2, 1.0, 45.0]),
        altitude: PidGains::from([0.1, 0.0001, 0.002, 10.0]),
        speed: PidGains::from([1.0, 0.1, 0.03, 10.0]),
        pitch: PidGains::from([5.0, 0.01, 0.01, 9.0]),
        roll: PidGains::from([0.3, 0.1, 0.001, 9.0]),
        max_pitch_deg: 45.0,
        max_roll_deg: 60.0,
        use_roll_control: false,
    };
    match airframe {
        FixedWingAirframe::A4d => a4d,
        // DEVNOTE: the A-4D's gains with ArduPlane's default attitude limits
        // (`ROLL_LIMIT_DEG` 45, `PITCH_MAX` 20 in ArduPlane/config.h; its
        // pitch-down limit is 25, and this controller's is symmetric).
        // Measured: the Rascal (`airframe: rascal`) at 20 m/s, in
        // `reference/rascal_control.py` (evidence in paper/data/rascal-control).
        // Altitude D from 0 to 0.02 changes the altitude error after 5/3 and
        // 60/30 degree upsets by under 1 mm at 0.01, 0.005, and 0.0025 s steps,
        // so the A-4D's 0.002 is kept. Under the A-4D's 45/60 limits a 20 m
        // altitude step pitched the Rascal to 48 degrees, past what a 20 m/s RC
        // plane could fly; under 20/45 it settles in 3.4 s at 23 degrees with no
        // overshoot, and heading and speed steps are unchanged. Rerun that
        // script after changing these settings.
        FixedWingAirframe::Rascal => AircraftPidConfig {
            max_pitch_deg: 20.0,
            max_roll_deg: 45.0,
            ..a4d
        },
    }
}

pub struct AircraftPidController {
    heading: Pid,
    altitude: Pid,
    speed: Pid,
    pitch: Pid,
    roll: Pid,
    max_pitch_rad: f64,
    max_roll_rad: f64,
    use_roll_control: bool,
}

impl Plugin for AircraftPidController {
    type Config = AircraftPidConfig;

    fn configure(params: &PluginParams<'_>) -> Result<AircraftPidConfig> {
        let params: AircraftPidParams = params.parse()?;
        let profile = params.airframe.map(airframe_settings);
        let missing = |key: &str| {
            format!(
                "AircraftPIDController: without `airframe`, the mission must give every \
                 gain and limit; `{key}` is missing"
            )
        };
        let config = AircraftPidConfig {
            heading: params
                .heading
                .or(profile.as_ref().map(|p| p.heading))
                .with_context(|| missing("heading_pid"))?,
            altitude: params
                .altitude
                .or(profile.as_ref().map(|p| p.altitude))
                .with_context(|| missing("altitude_pid"))?,
            speed: params
                .speed
                .or(profile.as_ref().map(|p| p.speed))
                .with_context(|| missing("speed_pid"))?,
            pitch: params
                .pitch
                .or(profile.as_ref().map(|p| p.pitch))
                .with_context(|| missing("pitch_pid"))?,
            roll: params
                .roll
                .or(profile.as_ref().map(|p| p.roll))
                .with_context(|| missing("roll_pid"))?,
            max_pitch_deg: params
                .max_pitch_deg
                .or(profile.as_ref().map(|p| p.max_pitch_deg))
                .with_context(|| missing("max_pitch"))?,
            max_roll_deg: params
                .max_roll_deg
                .or(profile.as_ref().map(|p| p.max_roll_deg))
                .with_context(|| missing("max_roll"))?,
            use_roll_control: params.use_roll_control,
        };
        ensure!(
            config.max_pitch_deg >= 0.0 && config.max_roll_deg >= 0.0,
            "AircraftPIDController attitude limits must be nonnegative"
        );
        Ok(config)
    }

    fn new(config: &AircraftPidConfig) -> Self {
        Self {
            heading: Pid::angular(config.heading),
            altitude: Pid::linear(config.altitude),
            speed: Pid::linear(config.speed),
            pitch: Pid::angular(config.pitch),
            roll: Pid::angular(config.roll),
            max_pitch_rad: crate::math::deg_to_rad(config.max_pitch_deg),
            max_roll_rad: crate::math::deg_to_rad(config.max_roll_deg),
            use_roll_control: config.use_roll_control,
        }
    }

    fn ports(config: &AircraftPidConfig) -> Ports {
        let steering = if config.use_roll_control {
            "desired_roll"
        } else {
            "desired_heading"
        };
        Ports::default()
            .input(Port::new(steering, Unit::Radians, Frame::World))
            .input(Port::new("desired_altitude", Unit::Meters, Frame::World))
            .input(Port::new(
                "desired_speed",
                Unit::MetersPerSecond,
                Frame::None,
            ))
            .output(Port::new("throttle", Unit::Dimensionless, Frame::None))
            .output(Port::new("elevator", Unit::Radians, Frame::Model))
            .output(Port::new("aileron", Unit::Radians, Frame::Model))
            .output(Port::new("rudder", Unit::Radians, Frame::Model))
    }
}

impl Controller for AircraftPidController {
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let state = context.state;
        let attitude = state.orientation_world_from_body;
        // Use the supplied substep interval, not the full mission tick, so the
        // tuning doesn't change with motion_multiplier.
        let dt_s = context.time.dt_s;
        let desired_pitch_rad =
            -self
                .altitude
                .step(io.read("desired_altitude")?, state.position_world_m.z, dt_s);
        let desired_pitch_rad = desired_pitch_rad.clamp(-self.max_pitch_rad, self.max_pitch_rad);
        let elevator_rad = self.pitch.step(
            desired_pitch_rad,
            attitude.pitch_world_from_body_rad(),
            dt_s,
        );

        let desired_roll_rad = if self.use_roll_control {
            io.read("desired_roll")?
        } else {
            -self.heading.step(
                io.read("desired_heading")?,
                attitude.yaw_world_from_body_rad(),
                dt_s,
            )
        };
        let desired_roll_rad = desired_roll_rad.clamp(-self.max_roll_rad, self.max_roll_rad);
        let aileron_rad =
            self.roll
                .step(desired_roll_rad, attitude.roll_world_from_body_rad(), dt_s);
        let throttle = self.speed.step(
            io.read("desired_speed")?,
            state.velocity_world_mps.norm(),
            dt_s,
        );

        io.write("elevator", elevator_rad.clamp(-1.0, 1.0))?;
        io.write("aileron", aileron_rad.clamp(-1.0, 1.0))?;
        io.write("throttle", throttle.clamp(-1.0, 1.0))?;
        io.write("rudder", 0.0)?;
        Ok(Update::Applied)
    }
}

#[cfg(test)]
mod tests {
    use super::AircraftPidController;
    use crate::plugin::{
        AgentContext, Controller, EntityInfo, KnownContacts, Messages, Plugin, PluginIo,
        PluginParams, PluginRandom, StepTime,
    };
    use crate::{KinematicState, Params};

    #[test]
    fn pid_integral_and_derivative_use_the_supplied_substep_interval() -> anyhow::Result<()> {
        // Five 0.02-second updates cover one 0.1-second mission tick.
        // With constant speed error 1, I grows by 0.02 each update; D is
        // 0.01 / 0.02 = 0.5 on the first update and zero thereafter.
        let params = Params::from([
            ("airframe".into(), "a4d".into()),
            ("speed_pid".into(), "0,1,0.01,10".into()),
        ]);
        let config = AircraftPidController::configure(&PluginParams::text(&params))?;
        let mut controller = AircraftPidController::new(&config);
        let mut io = PluginIo::new(&AircraftPidController::ports(&config));
        io.receive(
            &[
                ("desired_heading".into(), 0.0),
                ("desired_altitude".into(), 0.0),
                ("desired_speed".into(), 1.0),
            ]
            .into(),
        )?;
        let state = KinematicState::default();
        let mut messages = Messages::default();
        for (index, expected_throttle) in [0.52, 0.04, 0.06, 0.08, 0.10].into_iter().enumerate() {
            controller.step(
                &mut AgentContext {
                    entity: EntityInfo {
                        id: 1,
                        team_id: 1,
                        sub_swarm_id: 0,
                    },
                    time: StepTime {
                        time_s: index as f64 * 0.02,
                        dt_s: 0.02,
                        tick: index as i64,
                    },
                    state: &state,
                    contacts: KnownContacts::new(&[], 1),
                    messages: &mut messages,
                    random: PluginRandom::new(1, 1, "test"),
                },
                &mut io,
            )?;
            assert!((io.outputs["throttle"] - expected_throttle).abs() < 1e-12);
        }
        Ok(())
    }

    #[test]
    fn the_rascal_flies_the_a4d_gains_with_arduplane_limits() -> anyhow::Result<()> {
        let configure = |airframe: &str| {
            AircraftPidController::configure(&PluginParams::text(&Params::from([(
                "airframe".into(),
                airframe.into(),
            )])))
        };
        let a4d = configure("a4d")?;
        let rascal = configure("rascal")?;
        assert_eq!((a4d.max_pitch_deg, a4d.max_roll_deg), (45.0, 60.0));
        assert_eq!((rascal.max_pitch_deg, rascal.max_roll_deg), (20.0, 45.0));
        assert_eq!(rascal.altitude.derivative, a4d.altitude.derivative);
        let error = AircraftPidController::configure(&PluginParams::text(&Params::new()))
            .err()
            .expect("no airframe and no gains");
        assert!(error.to_string().contains("without `airframe`"), "{error}");
        Ok(())
    }
}
