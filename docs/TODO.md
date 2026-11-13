# Submission TODO

- Review the final manuscript and PDF against the saved results. Check the
  figures, citations, page count, and anonymous text.
- If the simulator changes, refresh the retained evidence with
  `python3 reference/suite.py --release --retain`.
- Commit and push the final manuscript, study code, and outcome data.
- Once the paper is final, decide whether to share an anonymous code mirror.
  If sharing, create it once, inspect the public view and both tracked PDFs,
  then add its link to the submission.
- Rerun Luna through the OpenAI API and keep everything (Ezra). Run
  `ripple sweep crates/delivery/missions/region-16-luna-api.sweep.yaml`,
  then the same for `region-64-luna-api.sweep.yaml`, on the current
  release binary (`cargo build --release`), one job each, so the API is
  called one day at a time. Send back the whole run folder each makes under
  `sweeps/` (its `results.jsonl` holds the score, moves, lends, and the
  per-hub columns) and the new reply files under `runs/llm-cache/`, so
  the runs replay here without calls. Then add Luna to Figure 2 through
  `crates/delivery/loss_figure.py`, replace its rounded day marks in
  Figure 3 with the sweep scores, refresh its row in `counts_table.py`
  and the appendix, recheck the Luna sentences in the results, and drop
  the sentence under Figure 2 that says it is omitted. Use seeds 1 and 2
  at 64 hubs so Luna matches the Claude models; seed 3 stays out of the
  figures.
- Submit to RA-L in early December.

## Possible additions

- Make a demo video from the title-figure recording.
