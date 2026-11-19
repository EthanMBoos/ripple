# Editing this book

User guides, API reference, and developer instructions live in `book/src/`. Add
chapters to
[SUMMARY.md](../../../book/src/SUMMARY.md)
to put them in the sidebar. Keep plans, audits, and ongoing investigation notes
in the repository's [docs/
folder](../../../docs).

From the repository root:

```sh
cargo install mdbook --locked
mdbook build book
mdbook serve book --open
```

Use relative `.md` links between book chapters. For source code, missions, or
working notes outside the book, use full GitHub URLs so the links work on the
published site. Paths in shell examples are relative to the repository root.
Wrap prose at 80 columns so it reads without soft wrapping; tables, code
blocks, and long URLs are exempt.

The [book
workflow](../../../.github/workflows/book.yml)
builds and publishes changes on `main` to GitHub Pages. The repository's Pages
source setting must be **GitHub Actions**. Generated HTML lives in the ignored
`book/book/` directory.

## The demo gallery

The [Demos](../demos.md) page and the chapters it links show each demo's
video, captured from the Rerun viewer, with a link to the Rerun recording it
was captured from. Each demo is one file in `book/demos/` naming a mission
to run (or a saved run to replay), the Cargo features the run needs, the
preset layout (`flight`, `belief`, or `fleet`), the interval and frame
stride to keep, the camera, the entities to feature with trails and labels
(`labels: false` keeps the trails and drops the map labels), the captions,
and, where the scene needs them, a window of the terrain to draw at full
resolution, a runway to draw on the ground, and highlights that ring a
drawn mark for a while. The runway demos start a SITL container, so
they need Docker and the images the [ArduPilot](../guides/ardupilot.md) and
[PX4](../guides/px4.md) chapters build. The plan behind it is
[docs/demos-plan.md](../../../docs/demos-plan.md).

```sh
python3 reference/demos.py                  # every demo
python3 reference/demos.py terrain-flight   # one demo
python3 reference/demos.py --no-video       # recordings only
```

The script runs `ripple run --demo` (or `ripple replay --demo` for a saved
run), copies the recording to `book/src/demos/<name>/recording.rrd`, then
captures `demo.mp4` from the headless Rerun viewer frame by frame, at 1080p
with the viewer zoomed so its text reads and its top bar hidden; the video
needs `rerun` and `ffmpeg` on the PATH. Both files are committed, like the
retained evidence in `paper/data/`, and the book workflow never runs the
simulator. A recording over 40 MB is refused: shorten the interval, raise
`every`, or lower `terrain_samples`. Motion is as smooth as the kept frames,
since the viewer does not interpolate, so keep `1 / (dt_s * every * speed)`
at 30 or more where a vehicle moves on screen.

To add a demo, write its file in `book/demos/`, run the script for it, add
an `embed.html` beside its recording (copy an existing one and change the
name and title), and include that file from the chapter that owns the demo
and from the Demos page; `book/theme/demos.js` resolves the embed's paths
from either page.
