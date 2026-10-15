use ripple_core::record;
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
            } else if path
                .extension()
                .is_some_and(|extension| extension == "yaml")
                && !path.to_string_lossy().ends_with(".sweep.yaml")
                && !path.to_string_lossy().ends_with(".terrain.yaml")
            {
                missions.push(path);
            }
        }
    }
    missions.sort();
    assert!(!missions.is_empty());
    for (index, mission) in missions.iter().enumerate() {
        let baseline = temp.path().join(format!("{index}-plain"));
        let recorded = temp.path().join(format!("{index}-recorded"));
        for (output, mode) in [(&baseline, "--headless"), (&recorded, "--export-rrd")] {
            let result = Command::new(env!("CARGO_BIN_EXE_ripple"))
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
        assert_eq!(
            fs::read(baseline.join("run.mcap")).unwrap(),
            fs::read(recorded.join("run.mcap")).unwrap(),
            "{}",
            mission.display()
        );
        assert!(!baseline.join("recording.rrd").exists());
        assert!(fs::metadata(recorded.join("recording.rrd")).unwrap().len() > 0);
    }
}

#[test]
fn cli_allocates_runs_and_accepts_explicit_output_without_changing_results() {
    let temp = tempfile::tempdir().unwrap();
    let mission =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/plugin_defaults.yaml");
    for output in [None, None, Some("custom/nested/run"), Some("named")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ripple"));
        command
            .current_dir(temp.path())
            .arg("run")
            .arg(&mission)
            .arg("--headless")
            .arg("--root")
            .arg(temp.path());
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
    for file in ["run.mcap"] {
        // Automatic runs are grouped by mission file name under <root>/runs.
        let first = fs::read(temp.path().join("runs/plugin_defaults/run000").join(file)).unwrap();
        for directory in ["runs/plugin_defaults/run001", "custom/nested/run", "named"] {
            assert_eq!(
                first,
                fs::read(temp.path().join(directory).join(file)).unwrap()
            );
        }
    }
    let result = Command::new(env!("CARGO_BIN_EXE_ripple"))
        .current_dir(temp.path())
        .arg("run")
        .arg(mission)
        .args(["--headless", "--output", "runs/plugin_defaults/run000"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("output must not already exist"));
}

#[test]
fn failed_runs_still_write_outputs_and_record_the_error() {
    let temp = tempfile::tempdir().unwrap();
    let mission = temp.path().join("mission.yaml");
    // A point moving at 1.7e308 m/s leaves the range of f64 within a few steps.
    fs::write(
        &mission,
        "format_version: 1
name: overflow
run: {end_s: 3, dt_s: 0.1, seed: 1}
metrics:
  SimpleCollisionMetrics:
entities:
  point:
    team: 1
    autonomy:
      ConstantVelocity: {velocity_mps: [1.7e308, 0, 0]}
    controller:
      SingleIntegratorControllerSimple:
    motion_model:
      SingleIntegrator:
",
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ripple"))
        .current_dir(temp.path())
        .arg("run")
        .arg(&mission)
        .args(["--headless", "--output", "failed"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("nonfinite position"), "{error}");
    let output = temp.path().join("failed");
    assert!(output.join("run.mcap").exists());
    let result = record::read_result(&output.join("run.mcap")).unwrap();
    assert_eq!(result["termination"], "PluginFailure");
    assert!(result["error"].contains("nonfinite position"));
    assert!(
        !record::read_frames(&output.join("run.mcap"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn invalid_missions_do_not_allocate_run_directories() {
    let temp = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ripple"))
        .current_dir(temp.path())
        .args(["run", "missing-mission.yaml", "--headless", "--root"])
        .arg(temp.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!temp.path().join("runs").exists());
}

#[test]
fn unavailable_sensor_reports_its_category_before_creating_a_run() {
    let temp = tempfile::tempdir().unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/plugin_defaults.yaml");
    let text = fs::read_to_string(fixture).unwrap();
    let mission = temp.path().join("mission.yaml");

    // A missing name and a real plugin name registered in the wrong category (motion).
    for name in ["RayTrace", "SimpleAircraft"] {
        fs::write(&mission, text.replace("NoisyPosition", name)).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_ripple"))
            .current_dir(temp.path())
            .arg("run")
            .arg(&mission)
            .arg("--headless")
            .arg("--root")
            .arg(temp.path())
            .output()
            .unwrap();
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        let expected = format!("unregistered Sensor plugin '{name}'");
        assert!(error.contains(&expected), "{error}");
        assert!(!temp.path().join("runs").exists());
    }
}
