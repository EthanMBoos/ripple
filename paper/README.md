# Paper and evidence

- **[manuscript/main.tex](manuscript/main.tex): the working paper**. A
  language-model fleet manager over a classical dispatcher
  over-helps: it relocates vehicles and stock when the best policy is local
  rules and lent flights, and the cost of relocating grows with the region.
  The study code is `crates/delivery`; what is left before submission is
  in [docs/TODO.md](../docs/TODO.md).
  Build it with
  `tectonic -o <folder> main.tex` in `manuscript/`. The earlier manuscript,
  a different paper on catching planner mistakes, is on the `llm-fleet`
  branch.
- [appendix/main.tex](appendix/main.tex), built as
  [appendix.pdf](appendix/appendix.pdf): the paper's appendix as its own
  document, since RA-L counts an appendix toward its eight pages and the
  paper cites this folder by URL instead. The frozen system prompt, the
  event texts, one complete model call and a veto prompt with their
  replies, and the per-run counts for the 40 model runs, from the text
  files beside it. Rebuild the PDF after editing it.
- [references.bib](references.bib): the reading list, one entry per paper
  in [docs/LITERATURE.md](../docs/LITERATURE.md), with local copies built as
  described in [literature_review/README.md](literature_review/README.md).
- [data/](data/): the retained results of the
  [reference suite](../reference/README.md) and the FollowNearest workflow
  example, below.

## Release evidence

Small result files under [data/](data/) back each check's claims.
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
| `data/cpp-noise/` | The noise mission's check (samples compared one for one) and `noise_statistics.py` report. |
| `data/checker-sensitivity/` | `sensitivity.py` report: planted defects and the layers that detected them. |
| `data/rust-determinism/` | `determinism.py` report: worker, recording, trace, and lookup variants. |
| `data/rust-platform/` | `platform_check.py` report: Rust on Linux/amd64 versus the native build. |
| `data/speed-regression/` | `perf.py`'s measurement against the accepted baseline. |
| `data/speed-cpp-container/` | `benchmark.py` timings and provenance. |
| `data/speed-cpp-native/` | `native_benchmark.py` bare-metal macOS timings. |
| `data/speed-spatial/` | `spatial_benchmark.py` timings: collision checking with the scan and grids. |
| `data/speed-messages/` | `message_benchmark.py` timings and profiles: all-agent messages in C++ and Rust. |
| `data/simple-aircraft-fit/` | `simple_aircraft_fit.py` results and summary: SimpleAircraft's `rascal` limits against FixedWing6DOF's Rascal. |
| `data/rk4-loads/` | `rk4_loads.py` results and summary: forces at every RK4 stage against unmodified C++. |
| `data/stall-landing/` | `stall_landing.py` results, summary, and missions: the Rascal landing with and without stall tables. |
| `data/visual-qa/` | Viewer review reports. Screenshots stay in ignored `runs/`. |
| `workflow-example/` | The FollowNearest workflow example in C++ and Rust; see its README. |

[data/SUMMARY.md](data/SUMMARY.md) is the readable summary, generated from
these files; edit the script, not its output:

```sh
python3 paper/data/summarize.py   # writes data/SUMMARY.md
```

To refresh the evidence after changing code, run `python3 reference/suite.py
--release --retain` from the repository root (several hours; see
[reference/README.md](../reference/README.md)), or one check with `--check ID
--retain`. Evidence changes only when the checks pass; update the prose if
numbers moved. The workflow example (`workflow-example/run-cpp.sh`) is run
separately. Never edit numbers by hand, and keep failed cases with their
explanations.
