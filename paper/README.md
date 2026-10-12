# Papers

Two documents share this folder, the bibliography, the generated tables, and
the retained data. Both are the record of evidence for what they claim; there
is no separate evidence document.

- **[manuscript/main.tex](manuscript/main.tex): the academic paper.** A research
  testbed for multi-agent robotics across levels of fidelity, with an external
  baseline at each integration stage. The [roadmap](../docs/TODO.md) drives the
  work: multirotors, VTOL, vessels and waves, and conditional learned-policy
  evaluation in a shared world. Experiments ask when these capabilities change
  conclusions and when simpler models suffice. The testbed is named **Ripple**:
  small model differences can propagate through
  motion, sensing, communication, and decisions to affect a team's result.
- **[manuscript/baseline.tex](manuscript/baseline.tex): the team report.**
  The evidence that Ripple can replace C++ SCRIMMAGE: the C++
  comparison, the substep regression, workflow, cost, the estimation stage,
  the scaling work, and the comparison appendices. The paper cites it.

Both use the IEEE conference layout. Venue and titles are provisional. Results
marked **[Provisional]** do not yet come from a release run of the suite.

## Framing and scope

The central contribution is making these components work together and showing
what can be trusted at each stage. Keep three claims separate: matching the
external implementation, matching independent physical measurements, and
affecting a multi-agent result. A source comparison supports the first claim;
it does not automatically support the other two. An external autopilot is a
control-stack choice, not proof of greater physical accuracy.

Complete the roadmap as a sequence of integrations with their comparisons;
the paper records the evidence as it arrives. A changed strategy ranking is
not a gate on that work. Unchanged outcomes, useful operating limits, and the
cost of extra fidelity also matter. The existing gate on learned policies
remains: run classical strategies first and identify the decision needing them.

Keep the publication scope flexible. A systems paper can emphasize the common
architecture and several verified integrations; a vehicle-model study can
emphasize one integration and measured validation; a robotics study can
emphasize one multi-agent conclusion across fidelity choices. Select a venue
and a bounded set of completed claims when the evidence supports them. The
working manuscript can span the program without making every roadmap item a
prerequisite for every submission. Preserve SCRIMMAGE attribution and the
baseline report in each relevant account.

## Supporting material

- [Bibliography](references.bib): only sources cited in the two documents.
- [Original SCRIMMAGE paper and reading map](literature_review/README.md).
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
tectonic -X compile baseline.tex
```

`latexmk -pdf main.tex` also works with a TeX Live installation providing
`natbib` and `array`. `IEEEtran.cls` is the unchanged IEEE class,
including its LPPL notices. Generated build files are ignored. The original
paper PDF and extracted text under `literature_review/sources/` are third-party
reference material, not covered by this repository's code license.
