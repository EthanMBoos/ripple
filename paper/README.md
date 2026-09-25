# SCRIMMAGE-RS paper

Start writing in [manuscript/main.tex](manuscript/main.tex). It follows Ripple's
IEEE conference layout and section structure, with prompts for a C++/Rust
comparison and new research capabilities. Results and contributions are still
to be established. The venue and title are provisional.

- [Bibliography](references.bib): only sources cited in the manuscript.
- [Original SCRIMMAGE paper and reading map](literature_review/README.md).
- [Current verification evidence](../docs/EVIDENCE.md) and
  [C++ comparison tools](../reference/README.md).
- [Implementation roadmap](../docs/TODO.md): current plans, not paper results.

Build with a LaTeX installation providing `latexmk`, `natbib`, and the standard
LaTeX packages:

```sh
cd paper/manuscript
latexmk -pdf main.tex
```

`IEEEtran.cls` is copied unchanged from Ripple, including its LPPL notices.
Generated manuscript files are ignored. The original paper PDF and extracted
text are retained under `literature_review/sources/` with source attribution;
they are third-party reference material, not covered by this repository's code
license. Check equations, figures, and precise claims in the PDF.

Keep benchmark scripts small and reuse `reference/` and `scripts/bulk_run.py`
where useful. Save generated experiment artifacts in ignored `runs/` directories;
link retained results and reproduction commands when adding them to the paper.
