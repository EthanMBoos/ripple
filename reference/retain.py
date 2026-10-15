"""Copy the small result files of a check into the release evidence (paper/data).

suite.py calls these when it retains a passing check. Recordings, frames, and
traces stay in the ignored run folder; only reports, summaries, and inputs are
kept. Local paths are replaced by <checkout> and <run>, so the evidence does not
depend on where the checkout or the runs folder was. Standard library only.
"""

from pathlib import Path
import shutil

from project import ROOT
from run import read_run


def sanitize(path, run):
    """Replace the absolute run and checkout paths in a text file."""
    text = path.read_text()
    for absolute, placeholder in ((Path(run).resolve(), "<run>"), (ROOT, "<checkout>"),
                                  (ROOT.resolve(), "<checkout>")):
        text = text.replace(str(absolute), placeholder)
    path.write_text(text)


def copy_files(run, output, names):
    """Copy the named files or folders of a run into output, sanitizing text files."""
    output.mkdir(parents=True, exist_ok=True)
    for name in names:
        source = Path(run) / name
        target = output / Path(name).name
        if source.is_dir():
            shutil.copytree(source, target)
            for path in target.rglob("*"):
                if path.is_file() and path.suffix in (".json", ".md", ".txt", ".yaml", ".xml"):
                    sanitize(path, run)
        else:
            shutil.copyfile(source, target)
            if target.suffix in (".json", ".md", ".txt", ".yaml", ".xml"):
                sanitize(target, run)


def copy_reference_check(check, output, extra=()):
    """report.json and, per case, its result, both summaries, and its input missions."""
    output.mkdir(parents=True)
    copy_files(check, output, ["report.json"])
    for result in sorted(Path(check).glob("*/result.json")):
        case = output / result.parent.name
        case.mkdir()
        shutil.copyfile(result, case / "result.json")
        sanitize(case / "result.json", check)
        # The mission both ran: Ripple's YAML and the XML written from it for C++.
        for name in ("input.yaml", "input.xml"):
            if (result.parent / "input" / name).is_file():
                shutil.copyfile(result.parent / "input" / name, case / name)
        summaries = list((result.parent / "cpp/logs").glob("*/summary.csv"))
        if summaries:
            shutil.copyfile(summaries[0], case / "cpp-summary.csv")
            (case / "rust-summary.csv").write_text(read_run(result.parent / "rust-1").summary)
    for path in extra:
        copy_files(Path(path).parent, output, [Path(path).name])
        sanitize(output / Path(path).name, check)
