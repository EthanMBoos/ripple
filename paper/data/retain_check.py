#!/usr/bin/env python3
"""Copy the small result files of a reference check into paper/data.

    python3 paper/data/retain_check.py runs/reference-check007 paper/data/cpp-comparison-DATE

Keeps report.json and, per case, result.json (non-interference, frame, summary,
and trace comparisons), plus both summaries and any noise or sensitivity report
given with --extra. Recordings, frames, and traces stay in the ignored check
directory. Local paths are replaced by <check>, <runs>, and <workspace>.
Standard library only.
"""

import argparse
from pathlib import Path
import shutil


def sanitize(path, check):
    text = path.read_text()
    text = text.replace(str(check.resolve()), "<check>")
    text = text.replace(str(check.resolve().parent), "<runs>")  # e.g. generated missions
    text = text.replace(str(Path(__file__).resolve().parents[3]), "<workspace>")
    path.write_text(text)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("check", type=Path)
    parser.add_argument("output", type=Path, help="new directory under paper/data")
    parser.add_argument("--extra", type=Path, action="append", default=[],
                        help="another JSON report to keep at the top level")
    parser.add_argument("--missions", type=Path, help="also keep every *.xml in this directory")
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    kept = [args.output / "report.json"]
    shutil.copyfile(args.check / "report.json", kept[0])
    for result in sorted(args.check.glob("*/result.json")):
        case = args.output / result.parent.name
        case.mkdir()
        shutil.copyfile(result, case / "result.json")
        kept.append(case / "result.json")
        summaries = list((result.parent / "cpp/logs").glob("*/summary.csv"))
        if summaries:
            shutil.copyfile(summaries[0], case / "cpp-summary.csv")
            shutil.copyfile(result.parent / "rust-1/summary.csv", case / "rust-summary.csv")
    for extra in args.extra:
        shutil.copyfile(extra, args.output / extra.name)
        kept.append(args.output / extra.name)
    if args.missions:
        for mission in sorted(args.missions.glob("*.xml")):
            shutil.copyfile(mission, args.output / mission.name)
    for path in kept:
        sanitize(path, args.check)


if __name__ == "__main__":
    main()
