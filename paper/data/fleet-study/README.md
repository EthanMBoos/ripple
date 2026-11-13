# Delivery study outcomes

`results.csv` contains one row per simulated day in the reported delivery
comparisons. The `source` and `case_id` columns identify the sweep file and
case; `seed` identifies the demand stream. The corresponding sweep YAML in
`crates/delivery/missions/` defines the policy, manager, fleet size, and
other settings. Empty metric cells mean that the run did not record that
event. The CSV contains outcomes, not model prompts or replies.

Regenerate it from the local sweep outputs with:

```sh
python3 paper/data/fleet-study/export.py \
  > paper/data/fleet-study/results.csv
```

The source JSONL files live under ignored `sweeps/`; saved model replies
live under ignored `runs/llm-cache/`. They are kept locally for replay.

`choice-agreement.csv` counts lender choices in the cached compact-report
calls. It compares each model's answer with the spare rule on the offers
that model actually saw, among newly affected hubs with multiple offers.
The count does not measure the causal effect of one different choice.
Regenerate it with:

```sh
python3 crates/delivery/lender_choices.py \
  > paper/data/fleet-study/choice-agreement.csv
```
