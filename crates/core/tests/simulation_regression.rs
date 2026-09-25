use std::path::PathBuf;

use anyhow::Result;
use scrimmage_core::{Mission, Params, Simulation, write_frame};

#[derive(Debug, PartialEq, Eq)]
struct MissionOutput {
    frames: Vec<u8>,
    summary: String,
    events: String,
}

fn run_mission(name: &str, threads: usize) -> Result<MissionOutput> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mission = Mission::load(&root.join("missions").join(name), &root, &Params::new())?;
    let mut simulation = Simulation::new(
        mission.scenario,
        &scrimmage_core::plugin::PluginRegistry::with_builtins(),
        threads,
    )?;
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

#[test]
fn mission_output_is_independent_of_worker_count() -> Result<()> {
    for mission in [
        "straight-no-gui.xml",
        "fixed-wing-6dof.xml",
        "multirotor.xml",
        "test_missions/straight_cpu.xml",
        "test_missions/straight_cpu_mul.xml",
        "test_missions/straight_cpu_threaded.xml",
        "verification/aircraft-substeps-spawning.xml",
        "noisy-state.xml",
        "noisy-contacts.xml",
        "auction-sphere.xml",
        "networks-local-global.xml",
        "waypoints-aircraft.xml",
        "waypoints-point-agents.xml",
        "verification/noisy-state-bias.xml",
    ] {
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
        &root.join("missions/straight-no-gui.xml"),
        &root,
        &Params::new(),
    )?;
    let mut simulation = Simulation::new(
        mission.scenario,
        &scrimmage_core::plugin::PluginRegistry::with_builtins(),
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
