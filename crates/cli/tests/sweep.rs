//! `scrimmage sweep`: shards must reproduce the whole sweep, row for row.
use std::{fs, path::Path, process::Command};

fn sweep(root: &Path, extra: &[&str]) -> std::process::Output {
    let sweep = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../missions/waypoints-point-agents.sweep.yaml");
    Command::new(env!("CARGO_BIN_EXE_scrimmage"))
        .arg("sweep")
        .arg(sweep)
        .arg("--root")
        .arg(root)
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn shards_together_equal_the_whole_sweep() {
    let temp = tempfile::tempdir().unwrap();
    let whole = sweep(temp.path(), &[]);
    assert!(
        whole.status.success(),
        "{}",
        String::from_utf8_lossy(&whole.stderr)
    );
    // Without --output, sweeps are numbered under <root>/sweeps/<sweep name>.
    let whole_rows = fs::read_to_string(
        temp.path()
            .join("sweeps/waypoints-point-agents/run000/results.jsonl"),
    )
    .unwrap();

    let mut shard_rows = Vec::new();
    for index in ["0", "1", "2"] {
        let output = temp.path().join(format!("shard-{index}"));
        let shard = sweep(
            temp.path(),
            &[
                "--shard-index",
                index,
                "--shard-count",
                "3",
                "--output",
                output.to_str().unwrap(),
            ],
        );
        assert!(
            shard.status.success(),
            "{}",
            String::from_utf8_lossy(&shard.stderr)
        );
        shard_rows.extend(
            fs::read_to_string(output.join("results.jsonl"))
                .unwrap()
                .lines()
                .map(str::to_owned),
        );
    }
    // Each row starts with its case ID, so sorting restores case order.
    shard_rows.sort();
    assert_eq!(whole_rows.lines().collect::<Vec<_>>(), shard_rows);
    assert_eq!(shard_rows.len(), 8);
}

#[test]
fn a_misspelled_path_fails_before_creating_output() {
    let temp = tempfile::tempdir().unwrap();
    let text = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../missions/waypoints-point-agents.sweep.yaml"),
    )
    .unwrap();
    let missions = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../missions");
    let broken = temp.path().join("broken.sweep.yaml");
    fs::write(
        &broken,
        text.replace("speed:", "sped:").replace(
            "base_scenario: waypoints-point-agents.yaml",
            &format!(
                "base_scenario: {}",
                missions.join("waypoints-point-agents.yaml").display()
            ),
        ),
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_scrimmage"))
        .arg("sweep")
        .arg(&broken)
        .arg("--root")
        .arg(temp.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("field `sped` does not exist"));
    assert!(!temp.path().join("sweeps").exists());
}
