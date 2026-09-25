# Original SCRIMMAGE paper

- [Local PDF](sources/demarco_scrimmage/paper.pdf)
- [Searchable text](sources/demarco_scrimmage/paper.txt)
- [Author-hosted source](https://www.kevindemarco.com/assets/pdf/scrimmage.pdf)
- [Upstream recommended citation](https://github.com/gtri/scrimmage#citation),
  used for `demarco2018` in [references.bib](../references.bib).

Downloaded 2026-09-25. The PDF has 14 pages. SHA-256:
`4886f704a8b808b8044615ce1dca552de8a357cfc1f83a966b46cf6a66203b23`.

Regenerate the text from the repository root with Poppler:

```sh
pdftotext -layout paper/literature_review/sources/demarco_scrimmage/paper.pdf \
  paper/literature_review/sources/demarco_scrimmage/paper.txt
```

Page breaks and layout are preserved for review. Figures and equations still
need the PDF; extraction is not a replacement for inspecting them.

## Reading map

| Section | What to revisit for this paper |
| --- | --- |
| 1-2: Introduction and related work | Original motivation and simulator comparisons; update the literature separately. |
| 3: Design guidelines | Determinism, lockstep execution, and explicit motion equations. |
| 4: Plugin architecture | Original roles and execution diagram; compare with source-reviewed current behavior. |
| 5: Integrations | ROS, MOOS, and OpenAI Gym context for choosing a new integration experiment. |
| 6: Predator/prey case study | Mission setup, models, behaviors, and statistical experiment. |
| 6.5, Table 1 (PDF page 12) | Simulation/wall-time ratios versus agent count and motion model; historical hardware and single-thread settings are stated here. |
| 7: Conclusion | Original claims and proposed future learning work. |

Table 1 motivates a new scaling experiment. Its published ratios are historical
context; rerun matched C++ and Rust workloads on the same machine for a direct
performance comparison. The paper and the current `Ubuntu-24.04` C++ branch
represent different points in the project's history.
