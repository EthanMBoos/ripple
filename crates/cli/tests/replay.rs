use std::{fs, process::Command};

#[test]
fn missing_recording_fails_without_creating_a_run() {
    let temp = tempfile::tempdir().unwrap();
    for path in ["missing.rrd", "."] {
        let output = Command::new(env!("CARGO_BIN_EXE_scrimmage"))
            .current_dir(temp.path())
            .args(["replay", path])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot open recording"));
    }
    assert!(!temp.path().join("runs").exists());
}

#[test]
fn recording_must_be_a_file_not_a_nested_directory() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("recording.rrd")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_scrimmage"))
        .current_dir(temp.path())
        .args(["replay", "."])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("recording is not a file"));
}

#[test]
fn draw_trails_needs_the_run_files_from_a_current_recording() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("recording.rrd"), b"fixture recording").unwrap();
    let replay = || {
        Command::new(env!("CARGO_BIN_EXE_scrimmage"))
            .current_dir(temp.path())
            .args(["replay", ".", "--draw-trails"])
            .output()
            .unwrap()
    };
    let output = replay();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("needs the run's manifest.json"));

    fs::write(
        temp.path().join("manifest.json"),
        br#"{"rerun_recording": true}"#,
    )
    .unwrap();
    let output = replay();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no rerun_recording_id"));
}

#[cfg(unix)]
mod viewer_process {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

    // Stand-in executable: capture argv without opening a GUI or decoding data.
    const FAKE_VIEWER: &str = r#"#!/bin/sh
printf '%s\n' "$#" "$1" > "$SCRIMMAGE_REPLAY_TEST_LOG"
exit "${SCRIMMAGE_REPLAY_TEST_EXIT:-0}"
"#;

    fn replay_command(root: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_scrimmage"));
        command
            .current_dir(root)
            .env("PATH", root.join("bin"))
            .env("SCRIMMAGE_REPLAY_TEST_LOG", root.join("viewer-args.txt"))
            .env("SCRIMMAGE_REPLAY_TEST_EXIT", "0")
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
    fn file_and_run_directory_launch_one_literal_argument_without_changing_the_recording() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        install_fake_viewer(root);
        let run = root.join("saved run");
        fs::create_dir(&run).unwrap();
        let recording = run.join("recording.rrd");
        fs::write(&recording, b"fixture recording").unwrap();
        let expected = format!("1\n{}\n", recording.canonicalize().unwrap().display());
        for input in ["saved run", "saved run/recording.rrd"] {
            let output = replay_command(root).arg(input).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                fs::read_to_string(root.join("viewer-args.txt")).unwrap(),
                expected
            );
            assert_eq!(fs::read(&recording).unwrap(), b"fixture recording");
        }
        assert!(!root.join("runs").exists());

        // Neither CLI options nor shell syntax may be interpreted as part of the filename.
        let unusual = root.join("--literal $(touch injected).rrd");
        fs::write(&unusual, b"fixture recording").unwrap();
        let output = replay_command(root)
            .arg("--")
            .arg(unusual.file_name().unwrap())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            fs::read_to_string(root.join("viewer-args.txt")).unwrap(),
            format!("1\n{}\n", unusual.canonicalize().unwrap().display())
        );
        assert!(!root.join("injected").exists());
    }

    #[test]
    fn missing_viewer_reports_how_to_make_it_available() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("bin")).unwrap();
        fs::write(temp.path().join("recording.rrd"), b"fixture recording").unwrap();
        let output = replay_command(temp.path()).arg(".").output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("ensure rerun is on PATH"));
    }

    #[test]
    fn unsuccessful_viewer_exit_is_reported_as_failure() {
        let temp = tempfile::tempdir().unwrap();
        install_fake_viewer(temp.path());
        fs::write(temp.path().join("recording.rrd"), b"fixture recording").unwrap();
        let output = replay_command(temp.path())
            .env("SCRIMMAGE_REPLAY_TEST_EXIT", "7")
            .arg(".")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("Rerun viewer exited with"), "{error}");
        assert!(error.contains('7'), "{error}");
    }

    #[test]
    fn invalid_input_never_launches_the_viewer() {
        let temp = tempfile::tempdir().unwrap();
        install_fake_viewer(temp.path());
        let output = replay_command(temp.path())
            .arg("missing.rrd")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!temp.path().join("viewer-args.txt").exists());
    }
}
