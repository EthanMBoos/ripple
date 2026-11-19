//! A test driver for SimpleAircraft: writes a constant throttle, roll rate,
//! and pitch rate, as HeldControls does for the aerodynamic aircraft.
use anyhow::Result;
use ripple_core::plugin::*;
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeldRatesConfig {
    throttle: f64,
    roll_rate_radps: f64,
    pitch_rate_radps: f64,
}

pub struct HeldRates {
    throttle: f64,
    roll_rate_radps: f64,
    pitch_rate_radps: f64,
}

impl Controller for HeldRates {
    type Config = HeldRatesConfig;

    fn new(config: &HeldRatesConfig) -> Self {
        Self {
            throttle: config.throttle,
            roll_rate_radps: config.roll_rate_radps,
            pitch_rate_radps: config.pitch_rate_radps,
        }
    }

    fn ports(_config: &HeldRatesConfig) -> Ports {
        Ports::default()
            .output(Port::new("throttle", Unit::Dimensionless, Frame::None))
            .output(Port::new("roll_rate", Unit::RadiansPerSecond, Frame::Model))
            .output(Port::new(
                "pitch_rate",
                Unit::RadiansPerSecond,
                Frame::Model,
            ))
    }

    fn step(&mut self, _: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        io.write("throttle", self.throttle)?;
        io.write("roll_rate", self.roll_rate_radps)?;
        io.write("pitch_rate", self.pitch_rate_radps)?;
        Ok(())
    }
}
