//! Named, unit-checked ports that connect a plugin stack's autonomy, controller,
//! and motion model, and the entity's one map of port values they share.
//!
//! DEVNOTE: an entity keeps one `Signals` map that every plugin reads and
//! writes through its declared ports. Until 2026-11-14 each plugin owned an
//! inputs map and an outputs map, copied stage to stage before each step and
//! merged after the autonomies, as SCRIMMAGE's VariableIO connects buffers.
//! That isolated a plugin from values its upstream did not write, at two
//! copies per plugin per tick, three maps where one is enough, and phase
//! loops whose proofs sprawled. The one map gives the same values provided
//! every port name has one writing plugin per entity, which `CompiledStack`
//! checks at load. The pass-through controller SCRIMMAGE needs between a
//! velocity autonomy and SingleIntegrator is gone; the comparison scripts
//! add it on the C++ side (the book's SCRIMMAGE chapter). Speed was not the
//! reason: the delivery day timed the same once `write` overwrote in place
//! (its DEVNOTE below).
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use vstd::prelude::*;

mod proof;

verus! {

broadcast use proof::port_facts;

/// A plugin's declared ports, by name. Plugins build them with `input` and
/// `output`; `validate` rejects an empty name and a name declared twice on
/// one side.
#[derive(Clone, Default, Debug)]
#[verifier::allow(autoderive_clone_without_spec)]
pub struct Ports {
    pub(crate) inputs: BTreeMap<String, Port>,
    pub(crate) outputs: BTreeMap<String, Port>,
    /// A name declared twice on one side, for `validate` to report.
    pub(crate) duplicate: Option<String>,
}

/// A plugin's view of its entity's port values, limited to its declared
/// ports through `read` and `write`.
pub struct PluginIo<'a> {
    pub(crate) signals: &'a mut Signals,
    pub(crate) ports: &'a Ports,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Dimensionless,
    Meters,
    MetersPerSecond,
    MetersPerSecondSquared,
    Newtons,
    Radians,
    RadiansPerSecond,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    None,
    World,
    Body,
    Model,
}

/// Named rather than an enum, so a vehicle can declare configured channels
/// such as motor_0..motor_N without an engine change.
#[derive(Clone, Debug, PartialEq, Eq)]
#[verifier::allow(autoderive_clone_without_spec)]
pub struct Port {
    pub name: String,
    pub unit: Unit,
    pub frame: Frame,
}

#[verifier::external]
impl Port {
    pub fn new(name: impl Into<String>, unit: Unit, frame: Frame) -> Self {
        Self {
            name: name.into(),
            unit,
            frame,
        }
    }
}

#[verifier::external]
impl Ports {
    pub fn input(mut self, port: Port) -> Self {
        if let Some(previous) = self.inputs.insert(port.name.clone(), port) {
            self.duplicate = Some(previous.name);
        }
        self
    }
    pub fn output(mut self, port: Port) -> Self {
        if let Some(previous) = self.outputs.insert(port.name.clone(), port) {
            self.duplicate = Some(previous.name);
        }
        self
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if let Some(name) = &self.duplicate {
            bail!("port '{name}' declared twice");
        }
        ensure!(
            !self.inputs.contains_key("") && !self.outputs.contains_key(""),
            "empty port name"
        );
        Ok(())
    }
    /// Checks that every input is an output of the stage before, with the
    /// same unit and frame.
    pub(crate) fn connect(
        &self,
        upstream: &BTreeMap<String, Port>,
        plugin_name: &str,
    ) -> Result<()> {
        for (name, input) in &self.inputs {
            let output = upstream
                .get(name)
                .with_context(|| format!("{plugin_name}: missing upstream output '{name}'"))?;
            ensure!(
                input.unit == output.unit && input.frame == output.frame,
                "{plugin_name}: port '{name}' expects {:?}/{:?}, upstream provides {:?}/{:?}",
                input.unit,
                input.frame,
                output.unit,
                output.frame
            );
        }
        Ok(())
    }
    /// Whether this plugin declares `name` as an input port.
    pub(crate) fn reads(&self, name: &str) -> bool {
        self.inputs.contains_key(name)
    }
}

/// An entity's port values: one map every plugin writes its outputs to and
/// reads its inputs from, in phase order. Every declared output starts at
/// zero, and a value persists when a rate-limited plugin does not execute.
pub(crate) type Signals = BTreeMap<String, f64>;

impl PluginIo<'_> {
    pub fn read(&self, name: &str) -> (value: Result<f64>)
        ensures proof::read_port(self, name, &value)
    {
        if !self.ports.inputs.contains_key(name) {
            bail!("undeclared input '{name}'");
        }
        self.signals
            .get(name)
            .copied()
            .with_context(|| format!("input '{name}' has no value"))
    }
    pub fn write(&mut self, name: &str, value: f64) -> (outcome: Result<()>)
        ensures proof::wrote_port(old(self), final(self), name, value, &outcome)
    {
        if !value.is_finite() {
            bail!("nonfinite output '{name}'");
        }
        if !self.ports.outputs.contains_key(name) {
            bail!("undeclared output '{name}'");
        }
        // DEVNOTE: inserting an owned copy of the name on every write
        // allocated a String each time, and the 16-hub delivery day took 38 s
        // against 33 s before the one signals map, with PluginIo::write at a
        // quarter of the main thread's samples (macOS `sample`). Overwriting
        // in place takes 32 s; the insert covers a name the stack has not
        // seeded. Repeat with `/usr/bin/time ripple run
        // crates/delivery/missions/region.yaml --headless`.
        match self.signals.get_mut(name) {
            Some(slot) => *slot = value,
            None => {
                self.signals.insert(name.to_owned(), value);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Frame, PluginIo, Port, Ports, Signals, Unit};

    #[test]
    fn configured_names_connect_and_missing_channels_are_rejected() -> anyhow::Result<()> {
        let name = format!("motor_{}", 5);
        let port = Port::new(name, Unit::RadiansPerSecond, Frame::None);
        let inputs = Ports::default().input(port.clone());
        let upstream = Ports::default().output(port);
        inputs.connect(&upstream.outputs, "Multirotor")?;
        let error = inputs
            .connect(&Ports::default().outputs, "Multirotor")
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("missing upstream output 'motor_5'")
        );
        let mut signals = Signals::from([("motor_5".into(), 680.0)]);
        let io = PluginIo {
            signals: &mut signals,
            ports: &inputs,
        };
        assert_eq!(io.read("motor_5")?, 680.0);
        Ok(())
    }

    #[test]
    fn a_plugin_reads_and_writes_only_its_declared_ports() {
        let ports = Ports::default()
            .input(Port::new("speed", Unit::MetersPerSecond, Frame::World))
            .output(Port::new("throttle", Unit::Dimensionless, Frame::None));
        let mut signals = Signals::from([("speed".into(), 3.0), ("throttle".into(), 0.0)]);
        let mut io = PluginIo {
            signals: &mut signals,
            ports: &ports,
        };
        io.write("throttle", 0.5).unwrap();
        assert!(io.write("speed", 1.0).is_err());
        assert!(io.write("throttle", f64::NAN).is_err());
        assert!(io.read("throttle").is_err());
        assert_eq!(signals["throttle"], 0.5);
        assert_eq!(signals["speed"], 3.0);
    }

    #[test]
    fn a_port_declared_twice_is_rejected() {
        let twice = Ports::default()
            .input(Port::new("speed", Unit::MetersPerSecond, Frame::World))
            .input(Port::new("speed", Unit::Meters, Frame::World));
        let error = twice.validate().unwrap_err();
        assert!(error.to_string().contains("port 'speed' declared twice"));
        let once = Ports::default()
            .input(Port::new("speed", Unit::MetersPerSecond, Frame::World))
            .output(Port::new("speed", Unit::MetersPerSecond, Frame::World));
        once.validate().unwrap();
    }
}

} // verus!
