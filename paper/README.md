# SCRIMMAGE-RS paper

The manuscript is the record of verification evidence for this project: its
Validation, Results, Discussion, and appendix sections state what has been
established, how, and with which limits. There is no separate evidence document.
Start writing in [manuscript/main.tex](manuscript/main.tex); it follows Ripple's
IEEE conference layout. Venue and title are provisional. Results marked
**[Provisional]** come from the working tree and are replaced by the frozen
campaign in the [roadmap](../docs/TODO.md).

- [Bibliography](references.bib): only sources cited in the manuscript.
- [Original SCRIMMAGE paper and reading map](literature_review/README.md).
- [C++ comparison tools](../reference/README.md): build, tolerances, provenance.
- [Implementation roadmap](../docs/TODO.md): current plans, not paper results.

## Retained data

Small result files under [data/](data/) back every number in the manuscript.
Local paths in them are replaced by `<workspace>` and similar placeholders.

| Directory | Contents |
| --- | --- |
| `data/cpp-comparison/` | `reference_check.py` report with source/image provenance for both C++ branches; per-mission `result.json` (non-interference, frame, summary, and trace comparisons) and both summaries. |
| `data/generated/` | The same for the generated scenarios, with their mission files. |
| `data/generated-long/` | The same for the long runs, plus `divergence.json`. |
| `data/sensitivity/` | `sensitivity.py` report: planted defects and the layers that detected them. |
| `data/noise/` | The noise mission's check and `noise_statistics.py` report. |
| `data/platform/` | `platform_check.py` report: Rust on Linux/amd64 versus the native build. |
| `data/benchmark/` | `benchmark.py` timings and provenance. |
| `data/refactor-equivalence/` | Byte-equality records for behavior-preserving changes. |
| `data/visual-qa/` | Viewer review reports. Screenshots stay in ignored `runs/`. |
| `rq2/` | The FollowNearest workflow example in C++ and Rust; see its README. |

Tables are generated from these files; edit the script, not its output:

```sh
python3 paper/data/make_tables.py   # writes manuscript/generated/*.tex
```

To refresh every result after changing code, run
`python3 reference/campaign.py` from the repository root (about two hours; see
[reference/README.md](../reference/README.md)). It reruns the C++ comparisons,
replaces their folders, and regenerates the tables; update the prose if numbers
moved. The benchmark (`reference/benchmark.py`), the workflow example
(`rq2/run-cpp.sh`), and the test suites are run separately.
Never edit numbers by hand, and keep failed cases with their explanations.

## Build

With [Tectonic](https://tectonic-typesetting.github.io/) (`brew install tectonic`):

```sh
cd paper/manuscript
tectonic -X compile main.tex
```

`latexmk -pdf main.tex` also works with a TeX Live installation providing
`natbib` and `array`. `IEEEtran.cls` is copied unchanged from Ripple, including
its LPPL notices. Generated build files are ignored. The original paper PDF and
extracted text under `literature_review/sources/` are third-party reference
material, not covered by this repository's code license.
