use std::{fs, path::Path, process::Command};

#[test]
fn every_shipped_mission_runs_identically_with_recording_on_and_off() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    let mut directories = vec![root.join("missions")];
    let mut missions = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "xml") {
                missions.push(path);
            }
        }
    }
    missions.sort();
    assert!(!missions.is_empty());
    for (index, mission) in missions.iter().enumerate() {
        let baseline = temp.path().join(format!("{index}-plain"));
        let recorded = temp.path().join(format!("{index}-recorded"));
        for (output, mode) in [(&baseline, "--no-rerun"), (&recorded, "--headless")] {
            let result = Command::new(env!("CARGO_BIN_EXE_scrimmage"))
                .current_dir(&root)
                .arg("run")
                .arg(mission)
                .arg("--output")
                .arg(output)
                .args(["--workers", "8", mode])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}: {}",
                mission.display(),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        for file in ["frames.bin", "events.json", "summary.csv"] {
            assert_eq!(
                fs::read(baseline.join(file)).unwrap(),
                fs::read(recorded.join(file)).unwrap(),
                "{}: {file}",
                mission.display()
            );
        }
        assert!(fs::metadata(recorded.join("recording.rrd")).unwrap().len() > 0);
    }
}

#[test]
fn cli_allocates_runs_and_accepts_explicit_output_without_changing_results() {
    let temp = tempfile::tempdir().unwrap();
    let mission =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/plugin_defaults.xml");
    for output in [None, None, Some("custom/nested/run"), Some("named")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_scrimmage"));
        command
            .current_dir(temp.path())
            .arg("run")
            .arg(&mission)
            .arg("--no-rerun");
        if let Some(output) = output {
            command.args(["--output", output]);
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    for file in ["frames.bin", "events.json", "summary.csv", "manifest.json"] {
        let first = fs::read(temp.path().join("runs/run000").join(file)).unwrap();
        for directory in ["runs/run001", "custom/nested/run", "named"] {
            assert_eq!(
                first,
                fs::read(temp.path().join(directory).join(file)).unwrap()
            );
        }
    }
    let result = Command::new(env!("CARGO_BIN_EXE_scrimmage"))
        .current_dir(temp.path())
        .arg("run")
        .arg(mission)
        .args(["--no-rerun", "--output", "runs/run000"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("output must not already exist"));
}

#[test]
fn failed_runs_still_write_outputs_and_record_the_error() {
    let temp = tempfile::tempdir().unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/plugin_defaults.xml");
    let xml = fs::read_to_string(fixture).unwrap();
    let mission = temp.path().join("mission.xml");
    // A zero turning radius makes SimpleAircraft produce a nonfinite state.
    fs::write(
        &mission,
        xml.replace(r#"turning_radius="23""#, r#"turning_radius="0""#),
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_scrimmage"))
        .current_dir(temp.path())
        .arg("run")
        .arg(&mission)
        .args(["--no-rerun", "--output", "failed"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("nonfinite state"), "{error}");
    let output = temp.path().join("failed");
    for file in ["frames.bin", "events.json", "summary.csv", "manifest.json"] {
        assert!(output.join(file).exists(), "{file}");
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["termination"], "PluginFailure");
    assert!(
        manifest["error"]
            .as_str()
            .unwrap()
            .contains("nonfinite state")
    );
}

#[test]
fn invalid_missions_do_not_allocate_run_directories() {
    let temp = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_scrimmage"))
        .current_dir(temp.path())
        .args(["run", "missing-mission.xml", "--no-rerun"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!temp.path().join("runs").exists());
}

#[test]
fn unavailable_sensor_reports_its_category_before_creating_a_run() {
    let temp = tempfile::tempdir().unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/plugin_defaults.xml");
    let xml = fs::read_to_string(fixture).unwrap();
    let mission = temp.path().join("mission.xml");

    // A missing name and a real plugin name registered in the wrong category (motion).
    for name in ["RayTrace", "SimpleAircraft"] {
        fs::write(&mission, xml.replace("NoisyPosition", name)).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_scrimmage"))
            .current_dir(temp.path())
            .arg("run")
            .arg(&mission)
            .arg("--no-rerun")
            .output()
            .unwrap();
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        let expected = format!(
            "Mission requests sensor '{name}', but it isn't registered in this executable."
        );
        assert!(error.contains(&expected), "{error}");
        assert!(!temp.path().join("runs").exists());
    }
}
