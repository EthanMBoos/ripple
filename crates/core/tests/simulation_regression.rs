use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use ripple_core::{Mission, Params, Simulation, TraceSink};

#[derive(Debug, PartialEq, Eq)]
struct MissionOutput {
    frames: Vec<u8>,
    summary: String,
    events: String,
}

/// Collects trace bytes so a test can inspect them after the run.
struct Buffer<'a>(&'a mut Vec<u8>);
impl TraceSink for Buffer<'_> {
    fn record(&mut self, _kind: &str, _time_s: f64, value: serde_json::Value) -> Result<()> {
        serde_json::to_writer(&mut self.0, &value)?;
        self.0.push(b'\n');
        Ok(())
    }
}

fn run_mission(name: &str) -> Result<MissionOutput> {
    run_mission_with_trace(name, None)
}

fn run_mission_with_trace(name: &str, trace: Option<&mut Vec<u8>>) -> Result<MissionOutput> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mission = Mission::load(&root.join("missions").join(name), &root, &Params::new())?;
    let mut simulation = Simulation::new(
        mission.scenario,
        Arc::new(ripple_core::plugin::PluginRegistry::with_builtins()),
    )?;
    let mut trace = trace.map(Buffer);
    let mut frames = Vec::new();

    while let Some(frame) =
        simulation.step_with_trace(trace.as_mut().map(|sink| sink as &mut dyn TraceSink))?
    {
        serde_json::to_writer(&mut frames, &frame)?;
        frames.push(b'\n');
    }

    Ok(MissionOutput {
        frames,
        summary: format!("{:?}", simulation.metric_reports()),
        events: serde_json::to_string(simulation.events())?,
    })
}

#[test]
fn comparison_trace_does_not_change_mission_output() -> Result<()> {
    for mission in [
        "networks-local-global.yaml",
        "verification/aircraft-substeps-scheduled.yaml",
        "verification/noisy-contacts-bias.yaml",
        "verification/sphere-network-auction.yaml",
    ] {
        let mut trace = Vec::new();
        let traced = run_mission_with_trace(mission, Some(&mut trace))?;
        assert_eq!(run_mission(mission)?, traced, "{mission}");
        assert!(!trace.is_empty(), "{mission} wrote no trace");
    }
    Ok(())
}

#[test]
fn contacts_retain_their_mission_entity_block_id() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mission = Mission::load(
        &root.join("missions/straight-no-gui.yaml"),
        &root,
        &Params::new(),
    )?;
    let mut simulation = Simulation::new(
        mission.scenario,
        Arc::new(ripple_core::plugin::PluginRegistry::with_builtins()),
    )?;
    let initial = simulation.step()?.expect("mission emits an initial frame");
    let block_ids: Vec<_> = initial
        .entities
        .iter()
        .map(|entity| (entity.id, entity.group_index))
        .collect();
    assert_eq!(block_ids, vec![(1, 0), (2, 1)]);
    Ok(())
}
