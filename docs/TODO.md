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
  `python3 scripts/bulk_run.py local
  crates/delivery/missions/region-16-luna-api.sweep.yaml`, then the same
  for `region-64-luna-api.sweep.yaml`, on the current binary with one
  worker per run. Send back the whole output folder each makes under
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
- If time permits, try a demand-sized replanner at 4, 16, and 64 hubs to
  test whether the fixed relocation size drives the loss. Update the
  claim if it changes the ranking.
