//! The mission's plugin list for one entity group, checked and wired once at
//! load. `CompiledStack::compile` reads the group's configuration, connects
//! the control-chain ports from autonomy through controllers to motion, names
//! each sensor, and chooses each estimator's input source. Nothing here runs
//! during the tick; `PluginStack` is built from this at each spawn.
use crate::plugin::estimator::Source;
use crate::plugin::*;
use crate::{plugin::CompiledPlugin, scenario::EntityGroupConfig};
use anyhow::{Context, Result, bail, ensure};
use std::collections::{BTreeMap, BTreeSet};

/// The name missions use for the engine's perfect measurement.
pub(crate) const PERFECT: &str = "perfect";

/// Chooses each estimator input's source: the mission's choice, else the one
/// sensor of that kind, else the perfect measurement. Several candidates is an
/// error rather than a silent pick.
fn select_sources(
    role: &str,
    config: &crate::scenario::PluginConfig,
    inputs: &[Input],
    sensors: &[CompiledPlugin<dyn Sensor>],
) -> Result<Vec<(String, Source)>> {
    for named in config.inputs.keys() {
        ensure!(
            inputs.iter().any(|input| &input.role == named),
            "{role} '{}' has no input '{named}'; its inputs are: {}",
            config.name,
            inputs
                .iter()
                .map(|input| input.role.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let perfect_allowed = |input: &Input| input.kind != ReadingKind::Other;
    let mut sources = Vec::new();
    for input in inputs {
        let source = if let Some(sensor) = config.inputs.get(&input.role) {
            if sensor == PERFECT {
                ensure!(
                    perfect_allowed(input),
                    "{role} input '{}' needs a sensor, not the perfect measurement",
                    input.role
                );
                Source::Perfect(input.kind)
            } else {
                let kind = sensors
                    .iter()
                    .find(|candidate| &candidate.instance_name == sensor)
                    .map(|candidate| candidate.reading.kind)
                    .with_context(|| {
                        format!(
                            "{role} input '{}' names sensor '{sensor}', but the entity's sensors are: {}",
                            input.role,
                            sensors.iter().map(|candidate| candidate.instance_name.as_str()).collect::<Vec<_>>().join(", ")
                        )
                    })?;
                ensure!(
                    input.kind == ReadingKind::Other || kind == input.kind,
                    "{role} input '{}' needs {:?} readings, but sensor '{sensor}' gives {kind:?}",
                    input.role,
                    input.kind
                );
                Source::Sensor(sensor.clone())
            }
        } else if perfect_allowed(input) {
            let candidates: Vec<_> = sensors
                .iter()
                .filter(|candidate| candidate.reading.kind == input.kind)
                .collect();
            match candidates.as_slice() {
                [] => Source::Perfect(input.kind),
                [candidate] => Source::Sensor(candidate.instance_name.clone()),
                _ => bail!(
                    "{role} input '{}' could use any of the sensors {}; name one in the mission's `inputs`",
                    input.role,
                    candidates
                        .iter()
                        .map(|candidate| candidate.instance_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        } else {
            bail!(
                "{role} input '{}' needs a sensor named in the mission's `inputs`",
                input.role
            );
        };
        sources.push((input.role.clone(), source));
    }
    Ok(sources)
}

fn source_names(sources: &[(String, Source)]) -> BTreeMap<String, String> {
    sources
        .iter()
        .map(|(role, source)| {
            let name = match source {
                Source::Perfect(_) => PERFECT.to_owned(),
                Source::Sensor(sensor) => sensor.clone(),
            };
            (role.clone(), name)
        })
        .collect()
}

/// One entity group's plugins as the mission names them, compiled and wired
/// but not yet running. One of these exists per group for the whole run;
/// every entity spawned from the group is instantiated from it.
pub(crate) struct CompiledStack {
    pub(super) autonomies: Vec<CompiledPlugin<dyn Autonomy>>,
    pub(super) controllers: Vec<CompiledPlugin<dyn Controller>>,
    pub(super) motion: CompiledPlugin<dyn MotionModel>,
    pub(super) sensors: Vec<CompiledPlugin<dyn Sensor>>,
    pub(super) localization: CompiledPlugin<dyn Localization>,
    pub(super) localization_sources: Vec<(String, Source)>,
    pub(super) tracking: CompiledPlugin<dyn Tracking>,
    pub(super) tracking_sources: Vec<(String, Source)>,
}
impl CompiledStack {
    pub fn compile(config: &EntityGroupConfig, registry: &PluginRegistry) -> Result<Self> {
        let autonomies = config
            .autonomy
            .iter()
            .map(|plugin| registry.autonomies.compile(plugin))
            .collect::<Result<Vec<_>>>()?;
        let controllers = config
            .controller
            .iter()
            .map(|plugin| registry.controllers.compile(plugin))
            .collect::<Result<Vec<_>>>()?;
        let motion = registry.motion.compile(&config.motion_model)?;
        let mut sensors = config
            .sensors
            .iter()
            .map(|plugin| registry.sensors.compile(plugin))
            .collect::<Result<Vec<_>>>()?;

        let mut upstream: Vec<Port> = Vec::new();
        for autonomy in &autonomies {
            autonomy.ports.connect(&[], &autonomy.name)?;
            for output in &autonomy.ports.outputs {
                if let Some(previous) = upstream.iter().find(|port| port.name == output.name) {
                    ensure!(
                        previous == output,
                        "autonomies disagree on port '{}'",
                        output.name
                    );
                } else {
                    upstream.push(output.clone());
                }
            }
        }
        for controller in &controllers {
            controller.ports.connect(&upstream, &controller.name)?;
            upstream = controller.ports.outputs.clone();
        }
        motion.ports.connect(&upstream, &motion.name)?;
        for sensor in &sensors {
            ensure!(
                sensor.ports.inputs.is_empty() && sensor.ports.outputs.is_empty(),
                "sensor '{}' produces readings, not control-chain ports",
                sensor.name
            );
        }
        // A sensor's instance name: the mission's, else its plugin name and
        // how many of that plugin came before it.
        let mut occurrences = BTreeMap::new();
        let mut names = BTreeSet::new();
        for (source, sensor) in config.sensors.iter().zip(&mut sensors) {
            let count = occurrences.entry(&source.name).or_insert(0);
            sensor.instance_name = source
                .instance
                .clone()
                .unwrap_or_else(|| format!("{}:{}", source.name, count));
            *count += 1;
            ensure!(
                names.insert(sensor.instance_name.clone()),
                "duplicate sensor instance '{}'",
                sensor.instance_name
            );
        }
        ensure!(
            !sensors.iter().any(|sensor| sensor.instance_name == PERFECT),
            "'{PERFECT}' is reserved for the perfect measurement; rename the sensor"
        );
        // Estimators run every tick: fresh-reading flags and recorded commands
        // cover one tick, so a skipped tick would lose them. An estimator that
        // wants to do expensive work less often decides that in its own `step`.
        for (role, estimator) in [
            ("localization", &config.localization),
            ("tracking", &config.tracking),
        ] {
            ensure!(
                estimator.loop_rate_hz == 0.0,
                "{role} '{}' runs every tick and takes no loop_rate_hz",
                estimator.name
            );
        }
        let localization = registry.localization.compile(&config.localization)?;
        let localization_sources = select_sources(
            "localization",
            &config.localization,
            &localization.inputs,
            &sensors,
        )?;
        let tracking = registry.tracking.compile(&config.tracking)?;
        let tracking_sources =
            select_sources("tracking", &config.tracking, &tracking.inputs, &sensors)?;
        Ok(Self {
            autonomies,
            controllers,
            motion,
            sensors,
            localization,
            localization_sources,
            tracking,
            tracking_sources,
        })
    }

    /// Each estimator's chosen sources, for the resolved configuration.
    pub fn estimator_inputs(&self) -> [BTreeMap<String, String>; 2] {
        [
            source_names(&self.localization_sources),
            source_names(&self.tracking_sources),
        ]
    }
}
