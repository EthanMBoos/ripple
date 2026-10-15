use std::{fs, path::Path, process::Command};

fn make_run(root: &Path) {
    let mission =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/plugin_defaults.yaml");
    let result = Command::new(env!("CARGO_BIN_EXE_ripple"))
        .current_dir(root)
        .args([
            "run",
            mission.to_str().unwrap(),
            "--headless",
            "--output",
            "saved run",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn missing_or_invalid_record_never_creates_a_run() {
    let temp = tempfile::tempdir().unwrap();
    for path in ["missing.mcap", "."] {
        let output = Command::new(env!("CARGO_BIN_EXE_ripple"))
            .current_dir(temp.path())
            .args(["replay", path])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot open record"));
    }
    fs::write(temp.path().join("broken.mcap"), b"not MCAP").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ripple"))
        .current_dir(temp.path())
        .args(["replay", "broken.mcap"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!temp.path().join("runs").exists());
}

#[cfg(unix)]
mod viewer_process {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const FAKE_VIEWER: &str = r#"#!/bin/sh
test -s "$1" || exit 8
printf '%s\n' "$#" "$1" > "$RIPPLE_REPLAY_TEST_LOG"
exit "${RIPPLE_REPLAY_TEST_EXIT:-0}"
"#;

    fn replay_command(root: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ripple"));
        command
            .current_dir(root)
            .env("PATH", root.join("bin"))
            .env("RIPPLE_REPLAY_TEST_LOG", root.join("viewer-args.txt"))
            .env("RIPPLE_REPLAY_TEST_EXIT", "0")
            .arg("replay");
        command
    }

    fn install_fake_viewer(root: &Path) {
        fs::create_dir(root.join("bin")).unwrap();
        let executable = root.join("bin/rerun");
        fs::write(&executable, FAKE_VIEWER).unwrap();
        fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn replay_rebuilds_a_view_from_directory_or_file_and_draws_trails() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        make_run(root);
        install_fake_viewer(root);
        let record = root.join("saved run/run.mcap");
        let original = fs::read(&record).unwrap();
        for input in ["saved run", "saved run/run.mcap"] {
            let output = replay_command(root)
                .arg(input)
                .arg("--draw-trails")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let log = fs::read_to_string(root.join("viewer-args.txt")).unwrap();
            assert_eq!(log.lines().next(), Some("1"));
            assert!(log.lines().nth(1).unwrap().ends_with("replay.rrd"));
            assert_eq!(fs::read(&record).unwrap(), original);
        }

        let unusual = root.join("--literal $(touch injected).mcap");
        fs::copy(&record, &unusual).unwrap();
        let output = replay_command(root)
            .arg("--")
            .arg(unusual.file_name().unwrap())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!root.join("injected").exists());
    }

    #[test]
    fn terrain_replay_uses_the_original_unchanged_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../missions");
        let inputs = root.join("terrain");
        fs::create_dir(&inputs).unwrap();
        for (source, target) in [
            ("mcmillan.terrain.yaml", "header.yaml"),
            ("mcmillan.f32", "mcmillan.f32"),
            ("mcmillan.png", "mcmillan.png"),
        ] {
            fs::copy(project.join("terrain").join(source), inputs.join(target)).unwrap();
        }
        let mission = fs::read_to_string(project.join("terrain-mcmillan.yaml"))
            .unwrap()
            .replace("end_s: 150", "end_s: 0.1")
            .replace(
                "terrain: terrain/mcmillan.terrain.yaml",
                "terrain: terrain/header.yaml",
            );
        fs::write(root.join("terrain-test.yaml"), mission).unwrap();
        let run = Command::new(env!("CARGO_BIN_EXE_ripple"))
            .current_dir(root)
            .args([
                "run",
                "terrain-test.yaml",
                "--headless",
                "--output",
                "saved",
            ])
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        install_fake_viewer(root);
        let replay = replay_command(root).arg("saved").output().unwrap();
        assert!(
            replay.status.success(),
            "{}",
            String::from_utf8_lossy(&replay.stderr)
        );

        fs::write(inputs.join("mcmillan.png"), b"changed image").unwrap();
        let replay = replay_command(root).arg("saved").output().unwrap();
        assert!(!replay.status.success());
        assert!(
            String::from_utf8_lossy(&replay.stderr)
                .contains("terrain files changed since this run")
        );

        fs::remove_dir_all(inputs).unwrap();
        let replay = replay_command(root).arg("saved").output().unwrap();
        assert!(!replay.status.success());
        assert!(String::from_utf8_lossy(&replay.stderr).contains("read terrain"));
    }

    #[test]
    fn missing_viewer_and_unsuccessful_exit_are_reported() {
        let temp = tempfile::tempdir().unwrap();
        make_run(temp.path());
        fs::create_dir(temp.path().join("bin")).unwrap();
        let output = replay_command(temp.path())
            .arg("saved run")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("ensure rerun is on PATH"));

        fs::remove_dir(temp.path().join("bin")).unwrap();
        install_fake_viewer(temp.path());
        let output = replay_command(temp.path())
            .env("RIPPLE_REPLAY_TEST_EXIT", "7")
            .arg("saved run")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("exited with exit status: 7"));
    }
}
