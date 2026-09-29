# Interleaved substeps: output and cost

Evidence for the paper's "A Regression in the Reference" section.

- `equivalence.json`: `paper/data/message-routing/equivalence.py` with the
  `scrimmage` build of `548b3ac` (controllers then motion, all substeps) as old
  and the interleaved build as new. It runs every mission and perf workload at
  1, 2, and 8 workers with traces. 31 of 35 are byte-identical; the four that
  differ are the substep workloads: the two
  `verification/aircraft-substeps-*.xml` missions and perf's `substeps-64` and
  `substeps-512`.
- `timings.json`: `paper/data/estimation-cost/alternate.py` with the benchmark
  runners of the same two builds, alternating run by run (a warm-up pair and 5
  timed pairs). On one worker, no workload changed consistently: every workload
  had pairs in both directions. On eight workers, the substep workloads were
  slower in every pair: `substeps-64` by 1.3-4.0% and `substeps-512` by
  3.8-9.5%. The other workloads changed in both directions.

The C++ comparison against the fixed `benchmarking-edits` branch is in
`../cpp-comparison/`, from `runs/campaign-006`.
