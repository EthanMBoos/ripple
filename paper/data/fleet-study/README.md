# Delivery study outcomes

`results.csv` contains one row per simulated day in the reported delivery
comparisons. The `source` and `case_id` columns identify the sweep file and
case; `seed` identifies the demand stream. Use the saved campaign
configuration for the policy, manager, fleet size, and other settings;
the shipped sweep YAML may have changed since the run. Empty metric cells
mean that the run did not record that
event. The CSV contains outcomes, not model prompts or replies.

Regenerate it from the local sweep outputs with:

```sh
python3 paper/data/fleet-study/export.py > /tmp/ripple-study-results.csv &&
  cp /tmp/ripple-study-results.csv paper/data/fleet-study/results.csv
```

The source JSONL files live under ignored `sweeps/`; saved model replies
live under ignored `runs/llm-cache/`. They are kept locally for replay.

## Per-hub outcomes behind Figure 2

`hubs.csv` has one row per hub per simulated day for the seven campaigns
Figure 2 reads: the fixed policies, Claude models, and Luna at 16 and 64
hubs. `manager`, `policy`, and `model` name the arm; `requests`,
`on_time`, `late`, and `undelivered` are that hub's counts, and `gave` and
`got` count recorded giving and receiving movements of aircraft or stock,
empty when the day recorded none. `crates/delivery/loss_figure.py` reads
this CSV directly. Each model has three seeds in both roles at each size;
each fixed policy has ten seeds. The CSV retains all fixed-policy arms,
while Figure 2 uses Local rules as the baseline and plots Relocating.

Figure 2's Luna rows come from `region-16-luna-api/run003` and
`region-64-luna-api/run004`, all six cases in each. These twelve fresh API
runs completed 306,000 steps each with binary SHA-256
`91dd87b220ab78b50b0e44d2f9b7b4014447894bb144f75deca98e507ff456b4`.
Replies are in `runs/llm-cache/luna-api-fresh-Cf8lTXLx/16/` and `64/`.
Interrupted, failed, and accidentally repeated campaigns are excluded.

Refresh only Luna's hub rows, preserving the other committed evidence:

```sh
python3 paper/data/fleet-study/export.py --refresh-luna-hubs
python3 crates/delivery/loss_figure.py > paper/manuscript/figures/loss-by-hub.tex
```

With all original campaign folders available, regenerate the full CSV:

```sh
python3 paper/data/fleet-study/export.py --hubs > paper/data/fleet-study/hubs.csv
```

## Luna results used in the manuscript

Figures 2 and 3, the main-text model results, and `results.csv` use the
same twelve successful API runs: seeds 1--3 in both roles at both sizes.
All use eight hours of requests and an 8.5-hour simulation. Paths below
are relative to `sweeps/`; case IDs refer to `results.jsonl`.

| Campaign | Included cases | Role | Demand seeds |
| --- | --- | --- | --- |
| `region-16-luna-api/run003` | `case-0000`–`case-0002` | Live | 1, 2, 3 |
| `region-16-luna-api/run003` | `case-0003`–`case-0005` | Veto | 1, 2, 3 |
| `region-64-luna-api/run004` | `case-0000`–`case-0002` | Live | 1, 2, 3 |
| `region-64-luna-api/run004` | `case-0003`–`case-0005` | Veto | 1, 2, 3 |

The binary hash and reply-cache locations are recorded above. Earlier
Luna API and CLI runs are superseded in the retained CSV, not additional
samples. The interrupted and accidentally repeated campaigns are excluded.
Refresh only the Luna outcomes while preserving all other retained rows:

```sh
python3 paper/data/fleet-study/export.py --refresh-luna-results
```

Figure 3 uses `100 * (model score - Local-rules score)`, matched by region
size and seed, calculated before rounding. `hubs.csv` maps each source and
case ID to its model or policy. The plot contains 48 individual points and
16 means. Claude's third 64-hub seed comes from
`region-64-models-seed3/run000`, whose six scores are also in `results.csv`.


The appendix's model means, per-seed scores, and per-run counts use these
same 48 runs. Its 30 existing Claude count rows are retained; their shared
metrics were checked against `results.csv`. The six added Claude rows and
all twelve replacement Luna rows were checked against the successful raw
campaigns listed above. Means are calculated before rounding. The separate
CLI/API comparison table retains the earlier matched runs (three seeds at
16 hubs, two at 64); it is not part of the current 48-run comparison.

## Lender choices

`choice-agreement.csv` counts lender choices in the cached compact-report
calls. It compares each model's answer with the spare rule on the offers
that model actually saw, among newly affected hubs with multiple offers.
The count does not measure the causal effect of one different choice.
Regenerate it with:

```sh
python3 crates/delivery/lender_choices.py \
  > paper/data/fleet-study/choice-agreement.csv
```
