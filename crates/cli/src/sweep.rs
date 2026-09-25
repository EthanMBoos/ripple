//! `scrimmage sweep`: run one YAML mission across seeds and parameter values,
//! whole or as one shard, following Ripple's sweep (`../ripple/crates/ripple/src/sweep.rs`).
//!
//! Implemented, the minimum that sharding needs:
//! - the `*.sweep.yaml` file: `name`, `base_scenario`, `seeds`,
//!   `parameter_combinations` (`cartesian` or `zip`), and dotted-path `parameters`;
//! - Ripple's case numbering, so `case-0007` means the same seed and values
//!   whether the sweep runs whole or in shards;
//! - `--shard-index i --shard-count n`, which runs cases i, i+n, i+2n, ...;
//! - one JSON row per case in `results.jsonl`, and `sweep.json` with the case
//!   counts that `scripts/bulk_run.py collect` checks before merging shards.
//!
//! Not implemented (Ripple has them): per-case run folders and recordings,
//! `--case-id` replay, digests, input archiving and hashes, and CSV/Markdown
//! summaries. To look at one case, rerun it with `scrimmage run <base> <path>:=<value>`;
//! runs are deterministic, so it is the same run.
use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use scrimmage_core::{
    Mission, Params, ScenarioConfig, Simulation, TerminationReason, plugin::PluginRegistry,
};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Args)]
pub(crate) struct SweepOptions {
    sweep: PathBuf,
    /// Output directory; defaults to the next <root>/sweeps/<sweep name>/run000, run001, ...
    #[arg(long)]
    output: Option<PathBuf>,
    /// Project folder (missions/, runs/, sweeps/); defaults to this repository.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Zero-based shard to run; cases index, index + count, index + 2 * count, ...
    #[arg(long, requires = "shard_count")]
    shard_index: Option<usize>,
    /// Number of shards the sweep is divided into.
    #[arg(long, requires = "shard_index")]
    shard_count: Option<usize>,
    /// Per-case step limit, as for `scrimmage run`.
    #[arg(long, default_value_t = 1_000_000)]
    max_steps: usize,
}

/// The sweep file, field for field Ripple's `SweepSpec`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SweepFile {
    name: String,
    /// A YAML mission, relative to the sweep file.
    base_scenario: PathBuf,
    /// Each case uses one; empty means the base mission's own `run.seed`.
    #[serde(default)]
    seeds: Vec<u32>,
    parameter_combinations: Combinations,
    /// Dotted path (as in a command-line override) to the values it takes.
    parameters: BTreeMap<String, Vec<Value>>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Combinations {
    /// Every combination of values.
    Cartesian,
    /// The i-th value of every path together; lists must have equal lengths.
    Zip,
}

/// Written once per output directory; `bulk_run.py collect` uses these counts
/// to check that the shards cover every case exactly once.
#[derive(Serialize)]
struct SweepRecord<'a> {
    name: &'a str,
    total_case_count: usize,
    shard_index: usize,
    shard_count: usize,
    case_count: usize,
}

/// One line of `results.jsonl`.
#[derive(Serialize)]
struct CaseRow {
    case_id: String,
    seed: u32,
    parameters: BTreeMap<String, Value>,
    /// The case's error; the other results are then empty.
    error: Option<String>,
    steps: Option<usize>,
    termination: Option<TerminationReason>,
    /// Post-run results only: each metrics plugin's per-team `score` and declared
    /// columns, the same values `summary.csv` holds (a missing value is 0 there too).
    /// In-run detail is deliberately not kept; rerun one case to inspect it.
    metrics: BTreeMap<String, BTreeMap<i32, BTreeMap<String, f64>>>,
}

pub(crate) fn sweep(
    options: SweepOptions,
    registry: &PluginRegistry,
    project_root: &Path,
) -> Result<()> {
    let root = options.root.as_deref().unwrap_or(project_root);
    let file_name = options
        .sweep
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    // The suffix tells a sweep apart from a mission; the name without it names
    // the output folder: straight_speed.sweep.yaml -> sweeps/straight_speed/.
    let Some(sweep_name) = file_name.strip_suffix(".sweep.yaml") else {
        bail!(
            "sweep file `{}` must end in .sweep.yaml",
            options.sweep.display()
        );
    };
    // Like a mission, a sweep file given by name is also looked for in the
    // project's missions/ folder.
    let sweep_path = if options.sweep.is_file() {
        options.sweep.clone()
    } else {
        root.join("missions").join(&options.sweep)
    };
    let text = fs::read_to_string(&sweep_path)
        .with_context(|| format!("read {}", options.sweep.display()))?;
    let spec: SweepFile = serde_yaml_ng::from_str(&text)
        .with_context(|| format!("invalid sweep {}", sweep_path.display()))?;
    validate(&spec)?;
    let base = sweep_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&spec.base_scenario);
    ensure!(
        base.extension()
            .is_some_and(|extension| extension == "yaml"),
        "base_scenario must be a YAML mission; XML missions cannot be swept"
    );

    // Ripple's numbering: case i uses seed i % seeds and value combination
    // i / seeds, so every seed runs with each combination before the next one.
    let seed_count = spec.seeds.len().max(1);
    let total_case_count = combination_count(&spec) * seed_count;
    let (shard_index, shard_count) = match (options.shard_index, options.shard_count) {
        (Some(index), Some(count)) if count > 0 && index < count => (index, count),
        (None, None) => (0, 1),
        _ => bail!("shard index must be less than a positive shard count"),
    };
    let cases: Vec<usize> = (shard_index..total_case_count)
        .step_by(shard_count)
        .collect();

    // Check file syntax and override paths before creating output. Scenario
    // validation happens per case, so a bad configuration is recorded in its row.
    load_case(&spec, &base, root, registry, 0)
        .context("the first case does not load; check the sweep paths")?;

    let output = crate::run::output::create_directory(
        options.output.as_deref(),
        &root.join("sweeps").join(sweep_name),
    )?;
    let record = SweepRecord {
        name: &spec.name,
        total_case_count,
        shard_index,
        shard_count,
        case_count: cases.len(),
    };
    fs::write(
        output.join("sweep.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;

    let mut rows = fs::File::create_new(output.join("results.jsonl"))?;
    let mut errors = 0;
    for index in cases {
        let row = run_case(&spec, &base, root, registry, index, options.max_steps);
        errors += usize::from(row.error.is_some());
        serde_json::to_writer(&mut rows, &row)?;
        rows.write_all(b"\n")?;
        // Flush each finished case, so a stopped shard keeps its completed rows.
        rows.flush()?;
    }

    println!(
        "{}: {} of {} cases, {} errors; output {}",
        spec.name,
        record.case_count,
        total_case_count,
        errors,
        output.display()
    );
    ensure!(errors == 0, "{errors} cases failed; see results.jsonl");
    Ok(())
}

fn validate(spec: &SweepFile) -> Result<()> {
    ensure!(!spec.name.trim().is_empty(), "sweep name must not be empty");
    for (path, values) in &spec.parameters {
        ensure!(
            path != "run.seed",
            "run.seed is set by the sweep's `seeds`, not `parameters`"
        );
        ensure!(!values.is_empty(), "sweep parameter `{path}` has no values");
    }
    if matches!(spec.parameter_combinations, Combinations::Zip) {
        let lengths: Vec<usize> = spec.parameters.values().map(Vec::len).collect();
        ensure!(
            lengths.windows(2).all(|pair| pair[0] == pair[1]),
            "zipped sweep parameters must have the same number of values"
        );
    }
    Ok(())
}

fn combination_count(spec: &SweepFile) -> usize {
    match spec.parameter_combinations {
        Combinations::Zip => spec.parameters.values().next().map_or(1, Vec::len),
        Combinations::Cartesian => spec.parameters.values().map(Vec::len).product(),
    }
}

/// The values of combination `index`. Cartesian order matches Ripple: paths in
/// alphabetical order, the last path changing fastest.
fn parameters_at(spec: &SweepFile, index: usize) -> BTreeMap<String, Value> {
    if matches!(spec.parameter_combinations, Combinations::Zip) {
        return spec
            .parameters
            .iter()
            .map(|(path, values)| (path.clone(), values[index].clone()))
            .collect();
    }
    let mut remaining = index;
    let mut selected = BTreeMap::new();
    for (path, values) in spec.parameters.iter().rev() {
        selected.insert(path.clone(), values[remaining % values.len()].clone());
        remaining /= values.len();
    }
    selected
}

/// Case `index`'s seed (if the sweep sets one) and parameter values.
fn case_inputs(spec: &SweepFile, index: usize) -> (Option<u32>, BTreeMap<String, Value>) {
    let seed_count = spec.seeds.len().max(1);
    let seed = spec.seeds.get(index % seed_count).copied();
    (seed, parameters_at(spec, index / seed_count))
}

/// Loads the base mission with this case's values, applied exactly like
/// command-line overrides (after template expansion, every path must exist).
fn load_case(
    spec: &SweepFile,
    base: &Path,
    root: &Path,
    registry: &PluginRegistry,
    index: usize,
) -> Result<ScenarioConfig> {
    let (seed, parameters) = case_inputs(spec, index);
    let mut overrides = Params::new();
    for (path, value) in parameters {
        overrides.insert(path, serde_yaml_ng::to_string(&value)?);
    }
    if let Some(seed) = seed {
        overrides.insert("run.seed".into(), seed.to_string());
    }
    Ok(Mission::load_with_registry(base, root, &overrides, registry)?.scenario)
}

fn run_case(
    spec: &SweepFile,
    base: &Path,
    root: &Path,
    registry: &PluginRegistry,
    index: usize,
    max_steps: usize,
) -> CaseRow {
    let (seed, parameters) = case_inputs(spec, index);
    let mut row = CaseRow {
        case_id: format!("case-{index:04}"),
        seed: seed.unwrap_or_default(),
        parameters,
        error: None,
        steps: None,
        termination: None,
        metrics: BTreeMap::new(),
    };
    let result = (|| -> Result<()> {
        let config = load_case(spec, base, root, registry, index)?;
        row.seed = config.run.seed;
        // One worker per case: Slurm gives each shard one CPU.
        let mut simulation = Simulation::new(config, registry, 1)?;
        while simulation.step()?.is_some() {
            ensure!(
                simulation.step_count() <= max_steps,
                "run exceeded {max_steps} steps"
            );
        }
        row.steps = Some(simulation.step_count());
        row.termination = simulation.termination();
        for (name, report) in simulation.metric_reports() {
            let mut teams = BTreeMap::new();
            for (team, metrics) in &report.teams {
                let mut values = BTreeMap::from([("score".to_owned(), metrics.score)]);
                for header in &report.headers {
                    let value = metrics.values.get(header).copied().unwrap_or(0.0);
                    values.insert(header.clone(), value);
                }
                teams.insert(*team, values);
            }
            row.metrics.insert(name, teams);
        }
        Ok(())
    })();
    if let Err(error) = result {
        row.error = Some(format!("{error:#}"));
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(combinations: Combinations, seeds: Vec<u32>) -> SweepFile {
        SweepFile {
            name: "test".into(),
            base_scenario: "base.yaml".into(),
            seeds,
            parameter_combinations: combinations,
            parameters: BTreeMap::from([
                ("a".into(), vec![Value::from(1), Value::from(2)]),
                ("b".into(), vec![Value::from(10), Value::from(20)]),
            ]),
        }
    }

    #[test]
    fn case_numbering_matches_ripple() {
        // Cartesian: the last path changes fastest.
        let cartesian = spec(Combinations::Cartesian, vec![]);
        assert_eq!(combination_count(&cartesian), 4);
        let values: Vec<_> = (0..4)
            .map(|index| {
                let parameters = parameters_at(&cartesian, index);
                (parameters["a"].as_i64(), parameters["b"].as_i64())
            })
            .collect();
        assert_eq!(
            values,
            [(1, 10), (1, 20), (2, 10), (2, 20)].map(|(a, b)| (Some(a), Some(b)))
        );

        // Zip pairs the i-th values; seeds cycle fastest across cases.
        let zipped = spec(Combinations::Zip, vec![7, 8]);
        assert_eq!(combination_count(&zipped), 2);
        let (seed, parameters) = case_inputs(&zipped, 3);
        assert_eq!(seed, Some(8));
        assert_eq!(parameters["a"].as_i64(), Some(2));
        assert_eq!(parameters["b"].as_i64(), Some(20));
    }

    #[test]
    fn invalid_sweeps_are_rejected() {
        let mut seeded = spec(Combinations::Cartesian, vec![]);
        seeded
            .parameters
            .insert("run.seed".into(), vec![Value::from(1)]);
        assert!(validate(&seeded).is_err());

        let mut uneven = spec(Combinations::Zip, vec![]);
        uneven.parameters.insert("c".into(), vec![Value::from(1)]);
        assert!(validate(&uneven).is_err());
    }
}
