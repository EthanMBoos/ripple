//! A sweep run as shards: on this machine as child processes, or on a Slurm
//! cluster as one array job. The parent plans the run folder, starts the
//! shards, shows progress, stops cleanly on Ctrl-C, resumes an unfinished
//! run, and merges the shards into one `results.jsonl` in case order.
//!
//! Shard `i` of `n` runs cases `i`, `i + n`, `i + 2n`, ... (`sweep.rs`), so
//! case `k` is the next row of shard `k mod n`: the one rule the merge needs.
//! No shard talks to another; a local process and a Slurm task are the same
//! command with the same two flags.
//!
//! On disk, `sweeps/<name>/runNNN/` holds `campaign.json` (what was run, with
//! which build, in how many shards), `logs/`, `shard-NNNN/` with each shard's
//! `sweep.json` and `results.jsonl`, and after the merge the campaign's own
//! `results.jsonl`. A `stop` file asks the shards to finish their current
//! case and exit; a `running` file holds the parent's process ID.
//!
//! Decisions, each with its reason:
//! - `--jobs` defaults to one, not the core count: a laptop's slow cores add
//!   little, a large mission takes memory per case, and a model being called
//!   is rate-limited, so all cores is a trap for the sweeps this repository
//!   runs. The help line prints the core count instead.
//! - The result is byte-identical at any job count, which is why a case's
//!   wall time goes to the shard's log and not its row.
//! - There is no retry command: running `ripple sweep` on the same file
//!   resumes its newest unfinished run, because "I had to leave" is the
//!   common reason to come back. "Unfinished" means a case has no row; an
//!   errored case is a result, and a new run reruns it (cached model
//!   replies make that cheap). This keeps a deterministic error from being
//!   rerun every time the command is typed.
//! - A resume keeps the recorded job count (the split of cases into shards
//!   was fixed at the start) and refuses when the sweep file or the binary
//!   hash changed or the recorded parent is alive: rows from two versions of
//!   a file, or two builds, in one results file is the one error nobody
//!   could see afterward.
//! - Ctrl-C once writes `stop` and lets running cases finish; twice kills.
//!   A file rather than a signal keeps the shard side trivial and works the
//!   same under Slurm. A case in flight cannot be paused.
//! - Progress is read from the rows on disk, not from the children, so the
//!   same code serves `ripple status` from another terminal and after a
//!   stop. No time-left estimate: cases of different sizes interleave.
//! - `collect` merges even when cases errored (the rows are the record) and
//!   refuses a second merge; it is hidden from the top-level help because
//!   only the Slurm path needs it, and `--slurm` prints the exact line.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, BufRead, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// `campaign.json`: everything a shard or a later command needs.
#[derive(Serialize, Deserialize)]
pub(crate) struct CampaignRecord {
    /// The sweep file, absolute.
    pub(crate) sweep: PathBuf,
    #[serde(default)]
    pub(crate) sweep_sha256: String,
    /// The executable that ran `ripple sweep`, so every shard uses one build.
    pub(crate) binary: PathBuf,
    #[serde(default)]
    pub(crate) binary_sha256: String,
    pub(crate) jobs: usize,
    #[serde(default)]
    pub(crate) total_case_count: usize,
    /// The project folder the shards run with (`--root`).
    #[serde(default)]
    pub(crate) root: PathBuf,
    #[serde(default = "default_max_steps")]
    pub(crate) max_steps: usize,
    #[serde(default)]
    pub(crate) started_unix_s: u64,
}

fn default_max_steps() -> usize {
    1_000_000
}

/// What a shard wrote in its `sweep.json`; the merge checks it belongs here.
#[derive(Deserialize)]
struct ShardRecord {
    total_case_count: usize,
    shard_index: usize,
    shard_count: usize,
}

/// The two fields of a results row the campaign reads.
#[derive(Deserialize)]
pub(crate) struct RowHead {
    pub(crate) case_id: String,
    pub(crate) error: Option<String>,
}

/// What the user asked `ripple sweep` to do, beyond the sweep itself.
pub(crate) struct CampaignRequest<'a> {
    /// The sweep file as typed, for the messages.
    pub(crate) typed: &'a Path,
    pub(crate) sweep_path: PathBuf,
    pub(crate) sweep_name: &'a str,
    pub(crate) total_case_count: usize,
    pub(crate) root: &'a Path,
    pub(crate) max_steps: usize,
    pub(crate) jobs: Option<usize>,
    pub(crate) output: Option<&'a Path>,
    pub(crate) new: bool,
    pub(crate) slurm: bool,
    pub(crate) script: Option<&'a Path>,
    pub(crate) slurm_args: &'a [String],
}

/// A run folder and how far it is.
struct RunState {
    campaign: PathBuf,
    record: CampaignRecord,
    /// Per shard: rows done, rows with an error, and whether every case ran.
    shards: Vec<ShardProgress>,
}

#[derive(Clone, Copy, Default)]
struct ShardProgress {
    done: usize,
    errors: usize,
    complete: bool,
}

/// Runs the sweep as a campaign: a new run, or the newest unfinished run of
/// the same file resumed.
pub(crate) fn run(request: &CampaignRequest<'_>) -> Result<()> {
    let binary = std::env::current_exe().context("find the ripple executable")?;
    let sweep_sha256 = sha256_file(&request.sweep_path)?;
    let binary_sha256 = sha256_file(&binary)?;
    let family = request.root.join("sweeps").join(request.sweep_name);

    // An unfinished newest run is resumed unless --new asks otherwise.
    let resumed = match request.output {
        Some(output) if output.join("campaign.json").is_file() => Some(output.to_path_buf()),
        Some(_) => None,
        None => newest_run(&family)?,
    };
    let resumed = resumed.filter(|run| !run.join("results.jsonl").exists());
    let state = match resumed {
        Some(run) if !request.new => {
            let state = read_state(&run)?;
            let done: usize = state.shards.iter().map(|shard| shard.done).sum();
            let label = run_label(&run);
            ensure!(
                state.record.sweep_sha256 == sweep_sha256,
                "{}: {label} is unfinished, but {} changed since it started; use --new",
                request.sweep_name,
                request.typed.display()
            );
            ensure!(
                state.record.binary_sha256 == binary_sha256,
                "{}: {label} is unfinished, but the ripple binary changed since it started; use --new",
                request.sweep_name
            );
            if let Some(pid) = running_pid(&run) {
                bail!(
                    "{}: {label} is already running (pid {pid}); wait for it or stop it",
                    request.sweep_name
                );
            }
            let ignored = if request.jobs.is_some_and(|jobs| jobs != state.record.jobs) {
                ", not --jobs"
            } else {
                ""
            };
            println!(
                "{}: {label} is unfinished, {done} of {} done; resuming with its {}{ignored} (--new starts another)",
                request.sweep_name,
                state.record.total_case_count,
                jobs_text(state.record.jobs)
            );
            state
        }
        resumed => {
            if let Some(run) = resumed {
                println!(
                    "{}: {} is unfinished and left as is",
                    request.sweep_name,
                    run_label(&run)
                );
            }
            let jobs = request.jobs.unwrap_or(1);
            ensure!(jobs > 0, "--jobs must be positive");
            if request.slurm {
                ensure!(
                    request.jobs.is_some(),
                    "--slurm needs --jobs: the number of Slurm tasks"
                );
            }
            let jobs = jobs.min(request.total_case_count.max(1));
            let campaign = crate::run::output::create_directory(request.output, &family)?;
            let record = CampaignRecord {
                sweep: request.sweep_path.clone(),
                sweep_sha256,
                binary,
                binary_sha256,
                jobs,
                total_case_count: request.total_case_count,
                root: request.root.to_path_buf(),
                max_steps: request.max_steps,
                started_unix_s: unix_now_s(),
            };
            fs::create_dir(campaign.join("logs"))?;
            fs::write(
                campaign.join("campaign.json"),
                serde_json::to_vec_pretty(&record)?,
            )?;
            println!(
                "{}: {} cases in {} -> {}",
                request.sweep_name,
                request.total_case_count,
                jobs_text(jobs),
                campaign.display()
            );
            RunState {
                campaign,
                record,
                shards: vec![ShardProgress::default(); jobs],
            }
        }
    };

    let pending: Vec<usize> = (0..state.record.jobs)
        .filter(|&index| !state.shards[index].complete)
        .collect();
    // A stop file left by an earlier Ctrl-C would stop the new shards at once.
    let _ = fs::remove_file(state.campaign.join("stop"));
    if request.slurm {
        return submit(request, &state, &pending);
    }
    run_local(request, &state, &pending)
}

/// Starts the pending shards as child processes, shows progress, handles
/// Ctrl-C, and merges when every shard has finished.
fn run_local(request: &CampaignRequest<'_>, state: &RunState, pending: &[usize]) -> Result<()> {
    let name = request.sweep_name;
    let campaign = &state.campaign;
    let record = &state.record;
    let started = Instant::now();
    let resuming = state.shards.iter().any(|shard| shard.done > 0);
    if resuming {
        let remaining =
            record.total_case_count - state.shards.iter().map(|shard| shard.done).sum::<usize>();
        println!("{name}: {remaining} cases to run");
    }

    fs::write(campaign.join("running"), std::process::id().to_string())?;
    let interrupts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&interrupts);
    // The handler only counts; the loop below decides what each press means.
    // A second call fails when a process already has a handler, which a
    // library caller may; the sweep then stops only by kill.
    let _ = ctrlc::set_handler(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    let mut children: Vec<(usize, Child)> = Vec::new();
    for &index in pending {
        let resume = state.shards[index].done > 0 || shard_dir(campaign, index).exists();
        let log = rotate_log(campaign, index)?;
        let mut command = shard_command(record, campaign, index, resume);
        command
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        // Their own process group: the terminal's Ctrl-C reaches only the
        // parent, which decides between a clean stop and a kill.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let child = command
            .spawn()
            .with_context(|| format!("start shard {index}"))?;
        children.push((index, child));
    }

    let terminal = io::stdout().is_terminal();
    let mut stopping = false;
    let mut killed = false;
    let mut last_line = String::new();
    let mut last_report = Instant::now() - Duration::from_secs(60);
    loop {
        let mut running = 0;
        for (_, child) in &mut children {
            if child.try_wait()?.is_none() {
                running += 1;
            }
        }
        let presses = interrupts.load(Ordering::SeqCst);
        if presses >= 2 && !killed {
            for (_, child) in &mut children {
                let _ = child.kill();
            }
            killed = true;
            finish_line(terminal, &mut last_line);
            if running > 0 {
                println!("{name}: killed {running} running cases");
            }
            break;
        }
        if presses == 1 && !stopping {
            stopping = true;
            fs::write(campaign.join("stop"), "")?;
            finish_line(terminal, &mut last_line);
            println!(
                "{name}: stopping; {running} running cases will finish, press Ctrl-C again to kill them"
            );
        }
        if last_report.elapsed() >= Duration::from_secs(2) || running == 0 {
            last_report = Instant::now();
            let progress = read_progress(record, campaign)?;
            let done: usize = progress.iter().map(|shard| shard.done).sum();
            let errors: usize = progress.iter().map(|shard| shard.errors).sum();
            let line = format!(
                "{name}: {done} of {} done, {running} running, {errors} errors, {} elapsed",
                record.total_case_count,
                elapsed_text(started.elapsed())
            );
            show_line(terminal, &mut last_line, &line);
        }
        if running == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    for (_, child) in &mut children {
        let _ = child.wait();
    }
    let _ = fs::remove_file(campaign.join("running"));
    finish_line(terminal, &mut last_line);

    let progress = read_progress(record, campaign)?;
    let done: usize = progress.iter().map(|shard| shard.done).sum();
    if stopping || killed {
        println!(
            "{name}: stopped with {done} of {} done -> {}",
            record.total_case_count,
            campaign.display()
        );
        println!(
            "{name}: finish it later with: ripple sweep {}",
            request.typed.display()
        );
        bail!("stopped before every case ran");
    }
    if done < record.total_case_count {
        println!(
            "{name}: stopped with {done} of {} done -> {}; see {}",
            record.total_case_count,
            campaign.display(),
            campaign.join("logs").display()
        );
        println!(
            "{name}: finish it later with: ripple sweep {}",
            request.typed.display()
        );
        bail!("a shard exited before every case ran");
    }
    let errors = collect(campaign)?;
    let merged = campaign.join("results.jsonl");
    if errors == 0 {
        println!(
            "{name}: {done} of {} done, 0 errors, {} -> {}",
            record.total_case_count,
            elapsed_text(started.elapsed()),
            merged.display()
        );
        return Ok(());
    }
    println!(
        "{name}: {done} of {} done, {errors} errors, {} -> {}; see {}",
        record.total_case_count,
        elapsed_text(started.elapsed()),
        merged.display(),
        campaign.join("logs").display()
    );
    bail!("{errors} cases failed; see results.jsonl");
}

/// Submits the pending shards as one Slurm array and returns at once.
fn submit(request: &CampaignRequest<'_>, state: &RunState, pending: &[usize]) -> Result<()> {
    let name = request.sweep_name;
    let script = match request.script {
        Some(script) => script.to_path_buf(),
        None => request.root.join("scripts/ripple-shard.slurm"),
    };
    ensure!(
        script.is_file(),
        "batch script {} not found; see docs for ripple sweep --slurm",
        script.display()
    );
    // A task that started and wrote nothing still left its folder, so every
    // resubmitted shard is told to resume; a shard with no folder yet is
    // unaffected by the flag.
    let resume = pending
        .iter()
        .any(|&index| shard_dir(&state.campaign, index).exists());
    let mut command = sbatch_command(
        &state.record,
        &state.campaign,
        &script,
        pending,
        request.slurm_args,
        resume,
    );
    let output = command.output().context("run sbatch")?;
    ensure!(
        output.status.success(),
        "sbatch failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let job_id = stdout
        .trim()
        .split(';')
        .next()
        .unwrap_or_default()
        .to_owned();
    println!(
        "{name}: submitted Slurm array {job_id}, {} tasks",
        pending.len()
    );
    println!(
        "{name}: when it finishes: ripple collect {}",
        state.campaign.display()
    );
    Ok(())
}

/// The shard's command: the same `ripple sweep` with the two shard flags.
fn shard_command(record: &CampaignRecord, campaign: &Path, index: usize, resume: bool) -> Command {
    let mut command = Command::new(&record.binary);
    command
        .arg("sweep")
        .arg(&record.sweep)
        .arg("--output")
        .arg(shard_dir(campaign, index))
        .arg("--shard-index")
        .arg(index.to_string())
        .arg("--shard-count")
        .arg(record.jobs.to_string())
        .args(shard_args(record, resume));
    command
}

/// The shard flags beyond the two shard numbers, for the batch script too.
fn shard_args(record: &CampaignRecord, resume: bool) -> Vec<String> {
    let mut args = vec![
        "--root".to_owned(),
        record.root.display().to_string(),
        "--max-steps".to_owned(),
        record.max_steps.to_string(),
    ];
    if resume {
        args.push("--resume".to_owned());
    }
    args
}

/// One `sbatch` call: the arguments after `--` go through untouched, then
/// the array of shard indices, the log pattern, and the environment the
/// batch script reads.
fn sbatch_command(
    record: &CampaignRecord,
    campaign: &Path,
    script: &Path,
    shards: &[usize],
    slurm_args: &[String],
    resume: bool,
) -> Command {
    let array = if shards.len() == record.jobs {
        format!("--array=0-{}", record.jobs - 1)
    } else {
        let indices: Vec<String> = shards.iter().map(ToString::to_string).collect();
        format!("--array={}", indices.join(","))
    };
    let mut command = Command::new("sbatch");
    command
        .arg("--parsable")
        .args(slurm_args)
        .arg(array)
        .arg(format!(
            "--output={}",
            campaign.join("logs").join("slurm-%A_%a.out").display()
        ))
        .arg("--export=ALL")
        .arg(script)
        .env("RIPPLE_BIN", &record.binary)
        .env("RIPPLE_SWEEP", &record.sweep)
        .env("RIPPLE_CAMPAIGN", campaign)
        .env("RIPPLE_SHARD_COUNT", record.jobs.to_string())
        .env("RIPPLE_SHARD_ARGS", shard_args(record, resume).join(" "));
    command
}

/// Merges the shards into `<campaign>/results.jsonl` in case order and
/// returns how many cases recorded an error. Refuses an incomplete campaign
/// and leaves nothing behind when it does.
pub(crate) fn collect(campaign: &Path) -> Result<usize> {
    let record = read_record(campaign)?;
    let merged = campaign.join("results.jsonl");
    ensure!(
        !merged.exists(),
        "{} is already collected",
        campaign.display()
    );
    let mut shards = Vec::new();
    for index in 0..record.jobs {
        let shard = shard_dir(campaign, index);
        ensure!(
            shard.is_dir(),
            "expected {} shards, found {index}; the array is still running or a task never started",
            record.jobs
        );
        let text = fs::read_to_string(shard.join("sweep.json"))
            .with_context(|| format!("read {}", shard.join("sweep.json").display()))?;
        let written: ShardRecord = serde_json::from_str(&text)?;
        ensure!(
            written.shard_index == index
                && written.shard_count == record.jobs
                && (record.total_case_count == 0
                    || written.total_case_count == record.total_case_count),
            "{} does not belong to this campaign",
            shard.display()
        );
        let rows = io::BufReader::new(
            fs::File::open(shard.join("results.jsonl"))
                .with_context(|| format!("open {}", shard.join("results.jsonl").display()))?,
        );
        shards.push((written.total_case_count, rows.lines()));
    }
    let total = shards.first().map_or(0, |(total, _)| *total);

    let partial = campaign.join("results.jsonl.partial");
    let mut errors = 0;
    let merge = (|| -> Result<()> {
        let mut out = io::BufWriter::new(fs::File::create(&partial)?);
        for case in 0..total {
            let (_, lines) = &mut shards[case % record.jobs];
            let Some(line) = lines.next().transpose()? else {
                bail!("case-{case:04} is missing; a shard did not finish");
            };
            let head: RowHead = serde_json::from_str(&line)
                .with_context(|| format!("case-{case:04} has a damaged row"))?;
            ensure!(
                head.case_id == format!("case-{case:04}"),
                "case-{case:04} is missing; a shard did not finish"
            );
            errors += usize::from(head.error.is_some());
            out.write_all(line.as_bytes())?;
            out.write_all(b"\n")?;
        }
        for (_, lines) in &mut shards {
            ensure!(lines.next().is_none(), "a shard has extra rows");
        }
        out.flush()?;
        Ok(())
    })();
    if let Err(error) = merge {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    fs::rename(&partial, &merged)?;
    Ok(errors)
}

/// `ripple collect`: the merge by hand, for a Slurm array that has finished.
pub(crate) fn collect_command(campaign: &Path) -> Result<()> {
    let record = read_record(campaign)?;
    let name = record.sweep.file_name().map_or_else(
        || campaign.display().to_string(),
        |file| file.to_string_lossy().replace(".sweep.yaml", ""),
    );
    let errors = collect(campaign)?;
    println!(
        "{name}: {} of {} cases from {} shards -> {} ({errors} errors)",
        record.total_case_count,
        record.total_case_count,
        record.jobs,
        campaign.join("results.jsonl").display()
    );
    ensure!(errors == 0, "{errors} cases failed; see results.jsonl");
    Ok(())
}

/// `ripple status`: how far the newest run of a sweep file (or a run folder) is.
pub(crate) fn status(target: &Path, root: &Path) -> Result<()> {
    let campaign = resolve_run(target, root)?;
    let state = read_state(&campaign)?;
    let record = &state.record;
    let name = record.sweep.file_name().map_or_else(
        || campaign.display().to_string(),
        |file| file.to_string_lossy().replace(".sweep.yaml", ""),
    );
    let done: usize = state.shards.iter().map(|shard| shard.done).sum();
    let errors: usize = state.shards.iter().map(|shard| shard.errors).sum();
    let condition = if campaign.join("results.jsonl").exists() {
        "complete"
    } else if running_pid(&campaign).is_some() {
        "running"
    } else {
        "stopped"
    };
    let elapsed = unix_now_s().saturating_sub(record.started_unix_s);
    println!(
        "{name}: {} {condition}, {done} of {} done, {errors} errors, {} since it started",
        run_label(&campaign),
        record.total_case_count,
        elapsed_text(Duration::from_secs(elapsed))
    );
    Ok(())
}

/// A run folder, or a sweep file's newest run.
fn resolve_run(target: &Path, root: &Path) -> Result<PathBuf> {
    if target.join("campaign.json").is_file() {
        return Ok(target.to_path_buf());
    }
    let file_name = target.file_name().and_then(|name| name.to_str());
    let Some(sweep_name) = file_name.and_then(|name| name.strip_suffix(".sweep.yaml")) else {
        bail!(
            "{} is neither a sweep file nor a run folder with campaign.json",
            target.display()
        );
    };
    let family = root.join("sweeps").join(sweep_name);
    newest_run(&family)?
        .with_context(|| format!("{sweep_name} has no runs under {}", family.display()))
}

/// The highest-numbered `runNNN` under a sweep's folder.
fn newest_run(family: &Path) -> Result<Option<PathBuf>> {
    if !family.is_dir() {
        return Ok(None);
    }
    let mut newest: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(family)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(number) = name
            .to_str()
            .and_then(|name| name.strip_prefix("run"))
            .and_then(|number| number.parse::<u64>().ok())
        else {
            continue;
        };
        if newest.as_ref().is_none_or(|(highest, _)| number > *highest) {
            newest = Some((number, entry.path()));
        }
    }
    Ok(newest.map(|(_, path)| path))
}

fn run_label(campaign: &Path) -> String {
    campaign.file_name().map_or_else(
        || campaign.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn read_record(campaign: &Path) -> Result<CampaignRecord> {
    let path = campaign.join("campaign.json");
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("invalid {}", path.display()))
}

fn read_state(campaign: &Path) -> Result<RunState> {
    let record = read_record(campaign)?;
    let shards = read_progress(&record, campaign)?;
    Ok(RunState {
        campaign: campaign.to_path_buf(),
        record,
        shards,
    })
}

/// Each shard's progress, from the rows on disk.
fn read_progress(record: &CampaignRecord, campaign: &Path) -> Result<Vec<ShardProgress>> {
    let mut shards = Vec::with_capacity(record.jobs);
    for index in 0..record.jobs {
        let expected: Vec<usize> = shard_cases(index, record.jobs, record.total_case_count);
        let rows = good_rows(&shard_dir(campaign, index).join("results.jsonl"), &expected)?;
        shards.push(ShardProgress {
            done: rows.len(),
            errors: rows.iter().filter(|(_, head)| head.error.is_some()).count(),
            complete: rows.len() == expected.len(),
        });
    }
    Ok(shards)
}

/// The cases shard `index` of `count` runs, in order.
pub(crate) fn shard_cases(index: usize, count: usize, total: usize) -> Vec<usize> {
    (index..total).step_by(count.max(1)).collect()
}

/// The rows of a shard's results file that are whole and in the expected
/// order, with their text; reading stops at the first row that is not (a
/// line cut short by a kill, say), which is where a resume continues.
pub(crate) fn good_rows(path: &Path, expected: &[usize]) -> Result<Vec<(String, RowHead)>> {
    let mut rows = Vec::new();
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(rows),
        Err(error) => return Err(error).with_context(|| format!("open {}", path.display())),
    };
    for (line, case) in io::BufReader::new(file).lines().zip(expected) {
        let line = line?;
        let Ok(head) = serde_json::from_str::<RowHead>(&line) else {
            break;
        };
        if head.case_id != format!("case-{case:04}") {
            break;
        }
        rows.push((line, head));
    }
    Ok(rows)
}

fn shard_dir(campaign: &Path, index: usize) -> PathBuf {
    campaign.join(format!("shard-{index:04}"))
}

/// Opens the shard's log, keeping an earlier attempt's log as `.1.log`,
/// `.2.log`, ...
fn rotate_log(campaign: &Path, index: usize) -> Result<fs::File> {
    let logs = campaign.join("logs");
    fs::create_dir_all(&logs)?;
    let log = logs.join(format!("shard-{index:04}.log"));
    if log.exists() {
        let mut attempt = 1;
        loop {
            let kept = logs.join(format!("shard-{index:04}.{attempt}.log"));
            if !kept.exists() {
                fs::rename(&log, &kept)?;
                break;
            }
            attempt += 1;
        }
    }
    fs::File::create(&log).with_context(|| format!("create {}", log.display()))
}

/// Keeps a shard's earlier results as `results.jsonl.1`, `.2`, ...
pub(crate) fn rotate_results(shard: &Path) -> Result<()> {
    let results = shard.join("results.jsonl");
    if !results.exists() {
        return Ok(());
    }
    let mut attempt = 1;
    loop {
        let kept = shard.join(format!("results.jsonl.{attempt}"));
        if !kept.exists() {
            fs::rename(&results, &kept)?;
            return Ok(());
        }
        attempt += 1;
    }
}

/// The process ID in the campaign's `running` file, if that process is alive.
fn running_pid(campaign: &Path) -> Option<u32> {
    let text = fs::read_to_string(campaign.join("running")).ok()?;
    let pid: u32 = text.trim().parse().ok()?;
    process_alive(pid).then_some(pid)
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    // Signal 0 checks for the process without sending anything.
    // SAFETY: kill with signal 0 has no effect beyond the permission check.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn unix_now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn jobs_text(jobs: usize) -> String {
    if jobs == 1 {
        "1 job".to_owned()
    } else {
        format!("{jobs} jobs")
    }
}

pub(crate) fn elapsed_text(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 90 {
        format!("{seconds} s")
    } else if seconds < 3600 {
        format!("{} min", seconds / 60)
    } else {
        format!("{} h {} min", seconds / 3600, (seconds % 3600) / 60)
    }
}

/// On a terminal the progress line is rewritten in place; elsewhere a line
/// is printed only when it changes.
fn show_line(terminal: bool, last: &mut String, line: &str) {
    if *last == line {
        return;
    }
    if terminal {
        print!(
            "\r{line}{}",
            " ".repeat(last.len().saturating_sub(line.len()))
        );
        let _ = io::stdout().flush();
    } else {
        println!("{line}");
    }
    *last = line.to_owned();
}

fn finish_line(terminal: bool, last: &mut String) {
    if terminal && !last.is_empty() {
        println!();
    }
    last.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(jobs: usize, total: usize) -> CampaignRecord {
        CampaignRecord {
            sweep: PathBuf::from("/work/test.sweep.yaml"),
            sweep_sha256: "abc".into(),
            binary: PathBuf::from("/work/ripple"),
            binary_sha256: "def".into(),
            jobs,
            total_case_count: total,
            root: PathBuf::from("/work"),
            max_steps: 1_000_000,
            started_unix_s: 0,
        }
    }

    /// Two shards of four cases: shard 0 holds cases 0 and 2, shard 1 holds 1 and 3.
    fn campaign() -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let campaign = folder.path().join("run000");
        fs::create_dir(&campaign).unwrap();
        fs::write(
            campaign.join("campaign.json"),
            serde_json::to_vec(&record(2, 4)).unwrap(),
        )
        .unwrap();
        for index in 0..2 {
            let shard = shard_dir(&campaign, index);
            fs::create_dir(&shard).unwrap();
            fs::write(
                shard.join("sweep.json"),
                format!(
                    r#"{{"name":"test","total_case_count":4,"shard_index":{index},"shard_count":2,"case_count":2}}"#
                ),
            )
            .unwrap();
            let rows: String = shard_cases(index, 2, 4)
                .into_iter()
                .map(|case| format!(r#"{{"case_id":"case-{case:04}","error":null}}"#) + "\n")
                .collect();
            fs::write(shard.join("results.jsonl"), rows).unwrap();
        }
        (folder, campaign)
    }

    fn case_ids(path: &Path) -> Vec<String> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<RowHead>(line).unwrap().case_id)
            .collect()
    }

    #[test]
    fn complete_shards_merge_in_case_order() {
        let (_folder, campaign) = campaign();
        assert_eq!(collect(&campaign).unwrap(), 0);
        assert_eq!(
            case_ids(&campaign.join("results.jsonl")),
            ["case-0000", "case-0001", "case-0002", "case-0003"]
        );
        // A second collect refuses rather than overwriting.
        assert!(collect(&campaign).is_err());
    }

    #[test]
    fn a_missing_case_is_rejected_and_leaves_no_results() {
        let (_folder, campaign) = campaign();
        fs::write(
            shard_dir(&campaign, 1).join("results.jsonl"),
            "{\"case_id\":\"case-0001\",\"error\":null}\n",
        )
        .unwrap();
        let error = collect(&campaign).unwrap_err().to_string();
        assert!(error.contains("case-0003 is missing"), "{error}");
        assert!(!campaign.join("results.jsonl").exists());
        assert!(!campaign.join("results.jsonl.partial").exists());
    }

    #[test]
    fn a_shard_from_another_campaign_is_rejected() {
        let (_folder, campaign) = campaign();
        fs::write(
            shard_dir(&campaign, 1).join("sweep.json"),
            r#"{"name":"test","total_case_count":4,"shard_index":1,"shard_count":3,"case_count":1}"#,
        )
        .unwrap();
        let error = collect(&campaign).unwrap_err().to_string();
        assert!(error.contains("does not belong"), "{error}");
    }

    #[test]
    fn errored_rows_are_merged_and_counted() {
        let (_folder, campaign) = campaign();
        fs::write(
            shard_dir(&campaign, 0).join("results.jsonl"),
            "{\"case_id\":\"case-0000\",\"error\":\"boom\"}\n{\"case_id\":\"case-0002\",\"error\":null}\n",
        )
        .unwrap();
        assert_eq!(collect(&campaign).unwrap(), 1);
    }

    #[test]
    fn progress_counts_whole_rows_in_order_and_stops_at_a_cut_line() {
        let (_folder, campaign) = campaign();
        // Shard 1 was killed while writing its second row.
        fs::write(
            shard_dir(&campaign, 1).join("results.jsonl"),
            "{\"case_id\":\"case-0001\",\"error\":null}\n{\"case_id\":\"case-00",
        )
        .unwrap();
        let progress = read_progress(&record(2, 4), &campaign).unwrap();
        assert!(progress[0].complete);
        assert_eq!(progress[1].done, 1);
        assert!(!progress[1].complete);
        // A row out of order is not counted either.
        fs::write(
            shard_dir(&campaign, 1).join("results.jsonl"),
            "{\"case_id\":\"case-0003\",\"error\":null}\n",
        )
        .unwrap();
        let progress = read_progress(&record(2, 4), &campaign).unwrap();
        assert_eq!(progress[1].done, 0);
    }

    #[test]
    fn a_shard_runs_one_shard_into_its_own_folder() {
        let campaign = Path::new("/work/sweeps/test/run000");
        let command = shard_command(&record(4, 20), campaign, 2, true);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(command.get_program(), "/work/ripple");
        assert_eq!(args[..2], ["sweep", "/work/test.sweep.yaml"]);
        assert!(args.contains(&"/work/sweeps/test/run000/shard-0002".to_owned()));
        let shard = args.iter().position(|arg| arg == "--shard-index").unwrap();
        assert_eq!(
            args[shard..shard + 4],
            ["--shard-index", "2", "--shard-count", "4"]
        );
        assert_eq!(args.last().map(String::as_str), Some("--resume"));
        assert_eq!(shard_cases(2, 4, 20), [2, 6, 10, 14, 18]);
    }

    #[test]
    fn sbatch_submits_one_array_with_the_arguments_passed_through() {
        let campaign = Path::new("/work/sweeps/test/run000");
        let script = Path::new("/work/scripts/ripple-shard.slurm");
        let all = sbatch_command(
            &record(3, 9),
            campaign,
            script,
            &[0, 1, 2],
            &["--partition=cpu".to_owned()],
            false,
        );
        let args: Vec<String> = all
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--array=0-2".to_owned()));
        assert!(args.contains(&"--partition=cpu".to_owned()));
        assert!(args.contains(&"--export=ALL".to_owned()));
        assert_eq!(args.last().map(String::as_str), script.to_str());
        let envs: Vec<(String, String)> = all
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.unwrap_or_default().to_string_lossy().into_owned(),
                )
            })
            .collect();
        assert!(envs.contains(&("RIPPLE_CAMPAIGN".to_owned(), campaign.display().to_string())));
        assert!(envs.contains(&("RIPPLE_SHARD_COUNT".to_owned(), "3".to_owned())));

        // A resume submits only the short shards, each told to resume.
        let some = sbatch_command(&record(3, 9), campaign, script, &[1], &[], true);
        let args: Vec<String> = some
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--array=1".to_owned()));
        let shard_args = some
            .get_envs()
            .find(|(key, _)| *key == "RIPPLE_SHARD_ARGS")
            .and_then(|(_, value)| value)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(shard_args.ends_with("--resume"));
    }

    #[test]
    fn the_newest_run_is_the_highest_number_and_a_live_pid_counts_as_running() {
        let folder = tempfile::tempdir().unwrap();
        let family = folder.path().join("test");
        for name in ["run000", "run002", "run001", "notes"] {
            fs::create_dir_all(family.join(name)).unwrap();
        }
        let newest = newest_run(&family).unwrap().unwrap();
        assert_eq!(newest.file_name().unwrap(), "run002");
        assert!(newest_run(&folder.path().join("none")).unwrap().is_none());

        fs::write(newest.join("running"), std::process::id().to_string()).unwrap();
        assert_eq!(running_pid(&newest), Some(std::process::id()));
        fs::write(newest.join("running"), "not a pid").unwrap();
        assert_eq!(running_pid(&newest), None);
    }

    #[test]
    fn rotated_results_keep_every_attempt() {
        let folder = tempfile::tempdir().unwrap();
        let shard = folder.path().to_path_buf();
        fs::write(shard.join("results.jsonl"), "first\n").unwrap();
        rotate_results(&shard).unwrap();
        fs::write(shard.join("results.jsonl"), "second\n").unwrap();
        rotate_results(&shard).unwrap();
        assert_eq!(
            fs::read_to_string(shard.join("results.jsonl.1")).unwrap(),
            "first\n"
        );
        assert_eq!(
            fs::read_to_string(shard.join("results.jsonl.2")).unwrap(),
            "second\n"
        );
        assert!(!shard.join("results.jsonl").exists());
    }

    #[test]
    fn elapsed_reads_in_the_unit_a_person_would_use() {
        assert_eq!(elapsed_text(Duration::from_secs(45)), "45 s");
        assert_eq!(elapsed_text(Duration::from_secs(540)), "9 min");
        assert_eq!(elapsed_text(Duration::from_secs(8_940)), "2 h 29 min");
    }
}
