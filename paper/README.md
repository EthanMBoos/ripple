# Papers

The paper, its bibliography, the generated tables, and the retained data.
The paper is the record of evidence for what it claims; there is no separate
evidence document.

- **[manuscript/main.tex](manuscript/main.tex): the academic paper.** Does
  realistic sensing, communication, and vehicle dynamics change which
  approach, learned or classical, wins on standard multi-agent tasks? Three
  tasks (cooperative navigation, circle-crossing collision avoidance, flocking)
  are run up a fidelity ladder in **Ripple**, whose integrations each have an
  external baseline. The [roadmap](../docs/TODO.md) drives the work. One
  table summarizes what each component is checked against; the appendix
  covers determinism, cost, and the command that reruns every check.

It uses the IEEE conference layout. Venue and titles are provisional. Results
marked **[Provisional]** do not yet come from a release run of the suite.

## Framing and scope

A changed ranking means something only if each integration can be trusted.
Keep three claims separate: matching the
external implementation, matching independent physical measurements, and
affecting a multi-agent result. A source comparison supports the first claim;
it does not automatically support the other two. An external autopilot is a
control-stack choice, not proof of greater physical accuracy.

A changed ranking is not a gate on integration work: unchanged outcomes,
useful operating limits, and the cost of extra fidelity also matter. Credit
SCRIMMAGE as the origin of Ripple; the detail of the agreement with it lives
in [reference/README.md](../reference/README.md) and
[REFERENCE_NOTES.md](../docs/REFERENCE_NOTES.md), not the paper.

## Supporting material

- [Bibliography](references.bib): only sources the paper cites.
- [Cited papers and code](literature_review/README.md): how to download
  every cited paper and its repositories for local reading.
- [C++ comparison tools](../reference/README.md): build, tolerances, provenance.
- [Implementation roadmap](../docs/TODO.md): current plans, not paper results.

## Release evidence

Small result files under [data/](data/) back every number in the manuscript.
Each folder is one check of the [suite](../reference/README.md), named by its
check ID and written by `python3 reference/suite.py --retain`;
[data/release.json](data/release.json) records each check's purpose, reference,
pass criterion, outcome, date, source hash, and the external reference
versions. Local paths are replaced by `<checkout>` and `<run>`.

| Directory | Contents |
| --- | --- |
| `data/cpp-agreement/` | `reference_check.py` report with source/image provenance for both C++ branches; per case its `result.json` (non-interference, frame, summary, and trace comparisons), input mission, and both summaries. |
| `data/cpp-generated/` | The same for the 100 generated scenarios. |
| `data/cpp-long/` | The same for the ten long runs, plus `divergence.json`. |
| `data/cpp-noise/` | The noise mission's check and `noise_statistics.py` report. |
| `data/checker-sensitivity/` | `sensitivity.py` report: planted defects and the layers that detected them. |
| `data/rust-determinism/` | `determinism.py` report: worker, recording, trace, and lookup variants. |
| `data/rust-platform/` | `platform_check.py` report: Rust on Linux/amd64 versus the native build. |
| `data/speed-regression/` | `perf.py`'s measurement against the accepted baseline. |
| `data/speed-cpp-container/` | `benchmark.py` timings and provenance. |
| `data/speed-cpp-native/` | `native_benchmark.py` bare-metal macOS timings. |
| `data/speed-spatial/` | `spatial_benchmark.py` timings: collision checking with the scan and grids. |
| `data/speed-messages/` | `message_benchmark.py` timings and profiles: all-agent messages in C++ and Rust. |
| `data/rk4-loads/` | `rk4_loads.py` results and summary: forces at every RK4 stage against unmodified C++. |
| `data/stall-landing/` | `stall_landing.py` results, summary, and missions: the Rascal landing with and without stall tables. |
| `data/visual-qa/` | Viewer review reports. Screenshots stay in ignored `runs/`. |
| `workflow-example/` | The FollowNearest workflow example in C++ and Rust; see its README. |

Tables are generated from these files; edit the script, not its output:

```sh
python3 paper/data/make_tables.py   # writes manuscript/generated/*.tex
```

To refresh the evidence after changing code, run `python3 reference/suite.py
--release --retain` from the repository root (several hours; see
[reference/README.md](../reference/README.md)), or one check with `--check ID
--retain`. Evidence changes only when the checks pass; update the prose if
numbers moved. The workflow example (`workflow-example/run-cpp.sh`) is run
separately. Never edit numbers by hand, and keep failed cases with their
explanations.

## Build

With [Tectonic](https://tectonic-typesetting.github.io/) (`brew install
tectonic`):

```sh
cd paper/manuscript
tectonic -X compile main.tex
```

`latexmk -pdf main.tex` also works with a TeX Live installation providing
`natbib` and `array`. `IEEEtran.cls` is the unchanged IEEE class,
including its LPPL notices. Generated build files are ignored.
