use ripple_cli::{RunSettings, run_scenario};
use ripple_core::record;
use ripple_core::{EntityGroupConfig, PluginConfig, ScenarioConfig, Vec3, plugin::PluginRegistry};
use std::{fs, process::Command};

fn scenario() -> ScenarioConfig {
    let mut scenario = ScenarioConfig::default();
    scenario.run.end_s = 0.3;
    scenario.run.seed = 7;
    scenario.networks = vec![
        PluginConfig::new("LocalNetwork"),
        PluginConfig::new("GlobalNetwork"),
    ];
    scenario.entities.push(EntityGroupConfig {
        label: "plane".into(),
        team: 1,
        position_m: Vec3::new(0.0, 0.0, 100.0),
        autonomy: vec![
            PluginConfig::new("Straight")
                .with_params(serde_json::json!({"speed_mps": 25}))
                .unwrap(),
        ],
        controller: vec![PluginConfig::new("SimpleAircraftControllerPID")],
        motion_model: PluginConfig::new("SimpleAircraft"),
        ..EntityGroupConfig::default()
    });
    scenario
}

#[test]
fn rust_and_file_runs_share_record_and_export_settings() {
    let temp = tempfile::tempdir().unwrap();
    let mission = temp.path().join("plane.yaml");
    fs::write(
        &mission,
        r#"format_version: 1
name: plane
run: {end_s: 0.3, seed: 7}
networks: {LocalNetwork: null, GlobalNetwork: null}
entities:
  plane:
    team: 1
    position_m: [0, 0, 100]
    autonomy: {Straight: {speed_mps: 25}}
    controller: {SimpleAircraftControllerPID: null}
    motion_model: {SimpleAircraft: null}
"#,
    )
    .unwrap();
    let registry = PluginRegistry::with_builtins();
    for (workers, recording) in [(1, false), (8, true)] {
        let name = format!("rust-{workers}");
        let output = run_scenario(
            scenario(),
            &registry,
            RunSettings {
                root: temp.path().to_owned(),
                name: name.clone(),
                workers,
                export_rrd: recording,
                ..RunSettings::default()
            },
        )
        .unwrap();
        assert_eq!(output, temp.path().join("runs").join(name).join("run000"));
        let file_output = temp.path().join(format!("file-{workers}"));
        let result = Command::new(env!("CARGO_BIN_EXE_ripple"))
            .arg("run")
            .arg(&mission)
            .arg("--output")
            .arg(&file_output)
            .arg("--workers")
            .arg(workers.to_string())
            .arg("--headless")
            .args(if recording {
                vec!["--export-rrd"]
            } else {
                vec![]
            })
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            fs::read(output.join("run.mcap")).unwrap(),
            fs::read(file_output.join("run.mcap")).unwrap()
        );
        assert_eq!(output.join("recording.rrd").exists(), recording);
        let saved = record::read_scenario(&output.join("run.mcap")).unwrap();
        assert_eq!(saved["run"]["seed"], 7);
        assert!(
            !record::read_result(&output.join("run.mcap"))
                .unwrap()
                .contains_key("error")
        );
    }
}

#[test]
fn invalid_rust_setup_does_not_create_outputs() {
    let temp = tempfile::tempdir().unwrap();
    let registry = PluginRegistry::with_builtins();
    for invalid in 0..3 {
        let mut scenario = scenario();
        let mut settings = RunSettings {
            root: temp.path().to_owned(),
            export_rrd: false,
            ..RunSettings::default()
        };
        match invalid {
            0 => scenario.run.dt_s = 0.0,
            1 => scenario.entities[0].motion_model = PluginConfig::new("MissingMotion"),
            _ => settings.workers = 0,
        }
        assert!(run_scenario(scenario, &registry, settings).is_err());
        assert!(!temp.path().join("runs").exists());
    }
}

#[test]
fn repeated_group_labels_keep_distinct_recorded_group_indices() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = scenario();
    let mut second = config.entities[0].clone();
    second.position_m = Vec3::new(100.0, 0.0, 100.0);
    config.entities.push(second);
    let output = run_scenario(
        config,
        &PluginRegistry::with_builtins(),
        RunSettings {
            root: temp.path().to_owned(),
            ..RunSettings::default()
        },
    )
    .unwrap();
    let frames = record::read_frames(&output.join("run.mcap")).unwrap();
    let groups: Vec<_> = frames[0]
        .entities
        .iter()
        .map(|entity| entity.sub_swarm_id)
        .collect();
    assert_eq!(groups, vec![0, 1]);
}
