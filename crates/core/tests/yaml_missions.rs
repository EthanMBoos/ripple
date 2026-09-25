//! Every YAML mission beside an XML mission of the same name must describe the
//! same simulation: the same setup, and byte-identical frames, events, and summary.
use anyhow::Result;
use scrimmage_core::{Mission, Params, ScenarioConfig, Simulation, write_frame};
use std::path::{Path, PathBuf};

fn outputs(config: ScenarioConfig) -> Result<(Vec<u8>, Vec<u8>, String)> {
    let mut simulation = Simulation::new(
        config,
        &scrimmage_core::plugin::PluginRegistry::with_builtins(),
        1,
    )?;
    let mut frames = Vec::new();
    while let Some(frame) = simulation.step()? {
        write_frame(&mut frames, &frame)?;
    }
    let events = serde_json::to_vec(simulation.events())?;
    Ok((frames, events, simulation.summary_csv()))
}

fn yaml_missions(directory: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            yaml_missions(&path, found)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "yaml")
        {
            found.push(path);
        }
    }
    Ok(())
}

#[test]
fn paired_xml_and_yaml_missions_run_identically() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut missions = Vec::new();
    yaml_missions(&root.join("missions"), &mut missions)?;
    missions.sort();
    let mut compared = 0;
    for yaml in missions {
        let xml = yaml.with_extension("xml");
        if !xml.is_file() {
            continue;
        }
        let from_xml = Mission::load(&xml, &root, &Params::new())?;
        let from_yaml = Mission::load(&yaml, &root, &Params::new())?;
        let differences = from_xml.scenario.setup_differences(&from_yaml.scenario)?;
        assert!(
            differences.is_empty(),
            "{}: {differences:?}",
            yaml.display()
        );
        assert!(
            outputs(from_xml.scenario)? == outputs(from_yaml.scenario)?,
            "{}: outputs differ",
            yaml.display()
        );
        compared += 1;
    }
    assert!(
        compared >= 4,
        "expected the paired missions, found {compared}"
    );
    Ok(())
}
