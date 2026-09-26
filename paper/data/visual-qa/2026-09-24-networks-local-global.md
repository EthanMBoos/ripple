# Aircraft/world baseline visual QA — 2026-09-24

Recording: `runs/reference-check004/05-networks-local-global/rust-rerun/recording.rrd`.
Mission: `missions/networks-local-global.xml`. Viewer/SDK: Rerun 0.36.2.
Dedicated headless viewer at 127.0.0.1:9878, 1920x1200; MCP recording ID
`rec_7144602d37694c44bf01e0a545f1ef16`. Existing unrelated viewer was not changed.

Inspected full-resolution images:

- `world-00.png`: 0 s, three agents and readable table.
- `world-01-before-ground.png`: 0.3 s, low aircraft altitude approximately 0.6 m.
- `world-02-after-ground.png`: 0.6 s, entity 3 marker/row gone; two surviving teams.
- `world-03-turn.png`: 10 s, both aircraft have turned; map keeps full paths in view.
- `world-04-final.png`: final sample, 29.900000001 s, two survivors; separate
  legacy label 30 s. Frame 300.

Map framing, ENU labels, status table, complete paths, and marker removal passed.
Altitude plots are populated. No boundary overlay is claimed or implemented.

Visual issue: the effectively constant-speed plot autoscaling renders vertical
spikes and no readable numeric scale. This is not a full dashboard QA pass.
The numerical test and C++ comparison still pass; do not change physical output
to make the plot look better. Investigate display-range behavior separately.

C++: 301 frames, zero mismatches, max position error 4.270651487619868e-14 m.
Frames/events/summaries are byte-identical across 1/2/8 workers and recording
on/off. Ground collision/removal event time is 0.4 s.

No waypoint screenshot review or native wall-clock looping check in this batch.
Reproduction instructions and permanent results: the paper's appendix (this file is retained raw evidence).
