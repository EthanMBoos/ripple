use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use ripple_core::{Mission, Params, Simulation, write_frame};

#[derive(Debug, PartialEq, Eq)]
struct MissionOutput {
    frames: Vec<u8>,
    summary: String,
    events: String,
}

/// Collects trace bytes so a test can inspect them after the run.
#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);
impl Write for SharedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn run_mission(name: &str, threads: usize) -> Result<MissionOutput> {
    run_mission_with_trace(name, threads, None)
}

fn run_mission_with_trace(
    name: &str,
    threads: usize,
    trace: Option<SharedBuffer>,
) -> Result<MissionOutput> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mission = Mission::load(&root.join("missions").join(name), &root, &Params::new())?;
    let mut simulation = Simulation::new(
        mission.scenario,
        &ripple_core::plugin::PluginRegistry::with_builtins(),
        threads,
    )?;
    if let Some(trace) = trace {
        simulation.enable_trace(Box::new(trace));
    }
    let mut frames = Vec::new();

    while let Some(frame) = simulation.step()? {
        write_frame(&mut frames, &frame)?;
    }

    Ok(MissionOutput {
        frames,
        summary: simulation.summary_csv(),
        events: serde_json::to_string(simulation.events())?,
    })
}

/// Every mission under `missions/` (not sweeps or terrain headers), as paths
/// relative to that folder.
fn missions() -> Result<Vec<String>> {
    fn collect(directory: &std::path::Path, found: &mut Vec<PathBuf>) -> Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                collect(&path, found)?;
            } else if path
                .extension()
                .is_some_and(|extension| extension == "yaml")
                && !path.to_string_lossy().ends_with(".sweep.yaml")
                && !path.to_string_lossy().ends_with(".terrain.yaml")
            {
                found.push(path);
            }
        }
        Ok(())
    }
    let missions = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../missions");
    let mut found = Vec::new();
    collect(&missions, &mut found)?;
    let mut names: Vec<String> = found
        .iter()
        .map(|path| {
            path.strip_prefix(&missions)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    Ok(names)
}

#[test]
fn comparison_trace_does_not_change_mission_output() -> Result<()> {
    for mission in [
        "networks-local-global.yaml",
        "verification/aircraft-substeps-scheduled.yaml",
        "verification/noisy-contacts-bias.yaml",
        "verification/sphere-network-auction.yaml",
    ] {
        let trace = SharedBuffer::default();
        let traced = run_mission_with_trace(mission, 2, Some(trace.clone()))?;
        assert_eq!(run_mission(mission, 2)?, traced, "{mission}");
        assert!(
            !trace.0.lock().unwrap().is_empty(),
            "{mission} wrote no trace"
        );
    }
    Ok(())
}

#[test]
fn mission_output_is_independent_of_worker_count() -> Result<()> {
    for mission in missions()? {
        let mission = mission.as_str();
        let serial = run_mission(mission, 1)?;
        for threads in [2, 8] {
            assert_eq!(
                serial,
                run_mission(mission, threads)?,
                "{mission}, {threads} workers"
            );
        }
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
        &ripple_core::plugin::PluginRegistry::with_builtins(),
        2,
    )?;
    let initial = simulation.step()?.expect("mission emits an initial frame");
    let block_ids: Vec<_> = initial
        .entities
        .iter()
        .map(|entity| (entity.id, entity.sub_swarm_id))
        .collect();
    assert_eq!(block_ids, vec![(1, 0), (2, 1)]);
    Ok(())
}
