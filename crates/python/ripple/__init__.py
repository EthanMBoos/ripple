"""The `ripple` command with plugins written in Python.

`main` is the console script: it hands the command line to the Rust command
(`ripple_cli::main`) with the `Python` autonomy registered, so `run`,
`sweep`, and `replay` are the same as the Rust `ripple` executable's.
"""

import sys
from pathlib import Path

from ripple._native import PROJECT_ROOT
from ripple._native import main as _main

__all__ = ["main"]


def mission_directory(argv):
    """The directory of the mission `argv` names, looked up as the engine
    does: the path as given, then `<root>/missions/<path>`."""
    root = Path(PROJECT_ROOT)
    for index, argument in enumerate(argv):
        if argument == "--root" and index + 1 < len(argv):
            root = Path(argv[index + 1])
        elif argument.startswith("--root="):
            root = Path(argument[len("--root="):])
    for argument in argv:
        if Path(argument).suffix in (".yaml", ".yml"):
            for candidate in (Path(argument), root / "missions" / argument):
                if candidate.is_file():
                    return candidate.resolve().parent
    return None


def main(argv=None):
    argv = sys.argv[1:] if argv is None else list(argv)
    # The mission's directory goes first on sys.path, so a plugin module beside
    # the mission imports with no packaging (`class: follow_nearest.FollowNearest`).
    directory = mission_directory(argv)
    if directory is not None:
        sys.path.insert(0, str(directory))
    try:
        _main(["ripple", *argv])
    except RuntimeError as error:
        sys.exit(f"ripple: {error}")
