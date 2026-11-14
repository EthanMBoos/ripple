//! Named, unit-checked ports that connect a plugin stack's autonomy, controller,
//! and motion model.
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use vstd::prelude::*;

mod proof;

verus! {
    broadcast use proof::port_facts;
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

/// Names are extensible; adding a channel does not require an engine enum change.
/// Owned names also allow configured channels such as a vehicle's motor_0..motor_N.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    pub name: String,
    pub unit: Unit,
    pub frame: Frame,
}

impl Port {
    pub fn new(name: impl Into<String>, unit: Unit, frame: Frame) -> Self {
        Self {
            name: name.into(),
            unit,
            frame,
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct Ports {
    pub(crate) inputs: Vec<Port>,
    pub(crate) outputs: Vec<Port>,
}
impl Ports {
    pub fn input(mut self, port: Port) -> Self {
        self.inputs.push(port);
        self
    }
    pub fn output(mut self, port: Port) -> Self {
        self.outputs.push(port);
        self
    }
    pub(crate) fn validate(&self) -> Result<()> {
        for ports in [&self.inputs, &self.outputs] {
            let mut names = std::collections::BTreeSet::new();
            for port in ports {
                ensure!(
                    !port.name.is_empty() && names.insert(&port.name),
                    "empty or duplicate port {}",
                    port.name
                );
            }
        }
        Ok(())
    }
    pub(crate) fn connect(&self, upstream: &[Port], plugin_name: &str) -> Result<()> {
        for input in &self.inputs {
            let output = upstream
                .iter()
                .find(|port| port.name == input.name)
                .with_context(|| {
                    format!("{plugin_name}: missing upstream output '{}'", input.name)
                })?;
            ensure!(
                input.unit == output.unit && input.frame == output.frame,
                "{plugin_name}: port '{}' expects {:?}/{:?}, upstream provides {:?}/{:?}",
                input.name,
                input.unit,
                input.frame,
                output.unit,
                output.frame
            );
        }
        Ok(())
    }
}

pub(crate) type Signals = BTreeMap<String, f64>;

/// Inputs and outputs are distinct, including when a controller reuses a channel name.
/// Values persist when a rate-limited plugin does not execute. Plugins go through
/// `read` and `write`; the maps are public so a proof, including a future proof
/// of a plugin, can state what they hold.
pub struct PluginIo {
    pub inputs: Signals,
    pub outputs: Signals,
}
impl PluginIo {
    pub(crate) fn new(ports: &Ports) -> Self {
        Self {
            inputs: ports
                .inputs
                .iter()
                .map(|port| (port.name.clone(), 0.0))
                .collect(),
            outputs: ports
                .outputs
                .iter()
                .map(|port| (port.name.clone(), 0.0))
                .collect(),
        }
    }
    pub(crate) fn input_names(&self) -> impl Iterator<Item = &String> {
        self.inputs.keys()
    }
    pub(crate) fn input_values(&self) -> impl Iterator<Item = f64> + '_ {
        self.inputs.values().copied()
    }
    /// Whether this plugin declares `name` as an input port.
    #[verus_spec(declared => ensures declared == proof::has_port(self, name))]
    pub(crate) fn reads(&self, name: &str) -> bool {
        self.inputs.contains_key(name)
    }
    #[verus_spec(value => ensures proof::read_port(self, name, &value))]
    pub fn read(&self, name: &str) -> Result<f64> {
        self.inputs
            .get(name)
            .copied()
            .with_context(|| format!("undeclared input '{name}'"))
    }
    #[verus_spec(outcome => ensures proof::wrote_port(old(self), final(self), name, value, &outcome))]
    pub fn write(&mut self, name: &str, value: f64) -> Result<()> {
        if !value.is_finite() {
            bail!("nonfinite output '{name}'");
        }
        *self
            .outputs
            .get_mut(name)
            .with_context(|| format!("undeclared output '{name}'"))? = value;
        Ok(())
    }
    // DEVNOTE: this checks every input and then copies, in two loops, because
    // Verus 0.2026.11.02 has no contract for `iter_mut` (nor has its main
    // branch as of 2026-11-14). The outputs are the same byte for byte
    // (reference/equivalence.py), and so is the error for a missing input.
    #[verus_spec(outcome => ensures proof::received(old(self), final(self), upstream, &outcome))]
    pub(crate) fn receive(&mut self, upstream: &Signals) -> Result<()> {
        #[cfg_attr(verus_only, verus_spec(it => invariant proof::connected_so_far(&self.inputs, upstream, it.seq(), it.index())))]
        for name in self.inputs.keys() {
            if !upstream.contains_key(name) {
                bail!("missing connected input '{name}'");
            }
        }
        #[cfg_attr(verus_only, verus_spec(it => invariant proof::copied_so_far(old(self), self, upstream, it.seq(), it.index())))]
        for (name, value) in upstream.iter() {
            if let Some(input) = self.inputs.get_mut(name) {
                *input = *value;
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
        inputs.connect(&[port], "Multirotor")?;
        let error = inputs.connect(&[], "Multirotor").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("missing upstream output 'motor_5'")
        );
        let mut io = PluginIo::new(&inputs);
        io.receive(&Signals::from([("motor_5".into(), 680.0)]))?;
        assert_eq!(io.read("motor_5")?, 680.0);
        Ok(())
    }
}
