# Cited papers and code

Every source in [references.bib](../references.bib), downloaded for local
reading: the PDF, a Markdown copy for searching, and shallow clones of the
code the authors published. It goes in `sources/`, which git ignores, since
these are other people's papers and code.

```sh
uv run --with pymupdf4llm scripts/build_paper_sources.py            # everything
uv run --with pymupdf4llm scripts/build_paper_sources.py --key KEY  # one source
```

Rerunning skips what's already downloaded. `sources/INDEX.md` lists what came
down for each key; `sources/KEY/README.md` says where each file came from and
which commit each repository is at.

Download links and repositories live in
[source_catalog.json](source_catalog.json), one entry per BibTeX key. Adding a
reference to the paper means adding it there too; the script stops if the two
don't match. A few papers have no open PDF that a script can fetch; their
entries say so, and you can drop the PDF in by hand as `sources/KEY/paper.pdf`.

What we took from the literature, and why: [Learned vs classical: what's out
there](../../docs/LEARNING_LITERATURE.md).

## The SCRIMMAGE paper

`sources/demarco2018/` is the original SCRIMMAGE paper (14 pages). Sections
worth rereading for this work:

| Section | Why |
| --- | --- |
| 3: Design guidelines | Determinism, lockstep execution, explicit motion equations. |
| 4: Plugin architecture | The original roles and execution order, to compare with the code. |
| 5: Integrations | ROS, MOOS, and the OpenAI Gym interface. |
| 6: Predator/prey case study | Mission setup, behaviors, and the batch experiment. |
| 6.5, Table 1 | Simulation-to-wall-time ratios against agent count; historical hardware. |

Table 1's ratios are history, not a benchmark. For speed comparisons, run the
matched C++ and Rust workloads on one machine (the paper's scaling appendix
does).
