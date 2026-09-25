use scrimmage_cli::{RunSettings, run_scenario};
use scrimmage_core::{
    EntityGroupConfig, PluginConfig, ScenarioConfig, Vec3, plugin::PluginRegistry,
};
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
                .with_params(serde_json::json!({"speed": 25}))
                .unwrap(),
        ],
        controller: vec![PluginConfig::new("SimpleAircraftControllerPID")],
        motion_model: PluginConfig::new("SimpleAircraft"),
        ..EntityGroupConfig::default()
    });
    scenario
}

#[test]
fn rust_and_file_runs_share_outputs_recording_and_manifest_settings() {
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
    autonomy: {Straight: {speed: 25}}
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
                recording,
                ..RunSettings::default()
            },
        )
        .unwrap();
        assert_eq!(output, temp.path().join("runs").join(name).join("run000"));
        let file_output = temp.path().join(format!("file-{workers}"));
        let result = Command::new(env!("CARGO_BIN_EXE_scrimmage"))
            .arg("run")
            .arg(&mission)
            .arg("--output")
            .arg(&file_output)
            .arg("--workers")
            .arg(workers.to_string())
            .arg(if recording {
                "--headless"
            } else {
                "--no-rerun"
            })
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        for file in ["frames.bin", "events.json", "summary.csv"] {
            assert_eq!(
                fs::read(output.join(file)).unwrap(),
                fs::read(file_output.join(file)).unwrap(),
                "{file}"
            );
        }
        assert_eq!(output.join("recording.rrd").exists(), recording);
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["scenario"]["run"]["seed"], 7);
        assert_eq!(manifest["worker_count"], workers);
        assert_eq!(manifest["rerun_recording"], recording);
        assert_eq!(manifest["viewer"], false);
        assert!(manifest["source"].is_null());
        let file_manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(file_output.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["scenario"], file_manifest["scenario"]);
        assert_eq!(
            file_manifest["source"],
            mission.canonicalize().unwrap().to_string_lossy().as_ref()
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
            recording: false,
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
