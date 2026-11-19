use super::*;

#[test]
fn automatic_runs_create_parents_and_start_at_zero() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let runs = temp.path().join("new/runs");
    assert_eq!(create_directory(None, &runs)?, runs.join("run000"));
    assert_eq!(create_directory(None, &runs)?, runs.join("run001"));
    Ok(())
}

#[test]
fn numbering_continues_after_existing_runs_and_grows_past_three_digits() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let runs = temp.path();
    fs::create_dir(runs.join("run005"))?;
    fs::create_dir(runs.join("named-experiment"))?;
    fs::write(runs.join("run006"), "do not overwrite")?;
    assert_eq!(create_directory(None, runs)?, runs.join("run007"));
    assert_eq!(fs::read_to_string(runs.join("run006"))?, "do not overwrite");
    fs::create_dir(runs.join("run999"))?;
    assert_eq!(create_directory(None, runs)?, runs.join("run1000"));
    Ok(())
}

#[test]
fn explicit_output_creates_parents_but_never_reuses_a_directory() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("nested/custom-run");
    let runs = temp.path().join("unused");
    assert_eq!(create_directory(Some(&output), &runs)?, output);
    fs::write(output.join("keep.txt"), "previous result")?;
    assert!(create_directory(Some(&output), &runs).is_err());
    assert_eq!(
        fs::read_to_string(output.join("keep.txt"))?,
        "previous result"
    );
    assert!(!runs.exists());
    Ok(())
}

#[test]
fn simultaneous_runs_claim_distinct_directories() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let runs = temp.path().join("runs");
    let barrier = std::sync::Barrier::new(8);
    let mut outputs = std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..8 {
            workers.push(scope.spawn(|| {
                barrier.wait();
                create_directory(None, &runs)
            }));
        }
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Result<Vec<_>>>()
    })?;
    outputs.sort();
    let expected: Vec<_> = (0..8)
        .map(|number| runs.join(format!("run{number:03}")))
        .collect();
    assert_eq!(outputs, expected);
    Ok(())
}

#[test]
fn a_file_blocking_the_runs_directory_is_reported() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let runs = temp.path().join("runs");
    fs::write(&runs, "keep")?;
    assert!(create_directory(None, &runs).is_err());
    assert_eq!(fs::read_to_string(runs)?, "keep");
    Ok(())
}

#[test]
fn exhausted_run_numbers_are_reported_without_wrapping() -> Result<()> {
    let temp = tempfile::tempdir()?;
    fs::create_dir(temp.path().join(format!("run{}", u64::MAX)))?;
    assert!(create_directory(None, temp.path()).is_err());
    assert!(!temp.path().join("run000").exists());
    Ok(())
}
