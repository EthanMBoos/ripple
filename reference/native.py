"""The native macOS C++ build that the bare-metal benchmarks time against.

native_build.sh builds C++ SCRIMMAGE's comparison branch into runs/native-cpp
(or NATIVE_CPP); `ensure_build` runs it once when the build is missing.
"""

import os
from pathlib import Path
import platform
import subprocess

from project import ROOT, source_identity

NATIVE = Path(os.environ.get("NATIVE_CPP", ROOT / "runs/native-cpp"))
CPP = NATIVE / "build/bin/scrimmage.app/Contents/MacOS/scrimmage"
CPP_ENV = {**os.environ,
           "SCRIMMAGE_PLUGIN_PATH": f"{NATIVE}/build/plugin_libs:{NATIVE}/source/include/scrimmage/plugins",
           "SCRIMMAGE_DATA_PATH": f"{NATIVE}/source/data",
           "SCRIMMAGE_CONFIG_PATH": f"{NATIVE}/source/config",
           "DYLD_LIBRARY_PATH": f"{NATIVE}/build/lib:{NATIVE}/build/plugin_libs"}


def ensure_build(log=None):
    """Build the native C++ executable if it is missing; macOS only."""
    if platform.system() != "Darwin":
        raise RuntimeError("the native C++ build is macOS only (reference/native_build.sh)")
    if not CPP.is_file():
        subprocess.run(["sh", ROOT / "reference/native_build.sh"], cwd=ROOT, check=True,
                       stdout=log, stderr=subprocess.STDOUT if log else None)
    return CPP


def provenance():
    """What was timed: the C++ commit and compiler, the Rust sources and compiler."""
    return {"cpp_branch": "benchmarking-edits",
            "cpp_commit": (NATIVE / "commit.txt").read_text().strip(),
            "cpp_build": "reference/native_build.sh; " + subprocess.check_output(
                ["/usr/bin/clang++", "--version"], text=True).splitlines()[0],
            "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
            "source_sha256": source_identity()["source_sha256"],
            "platform": platform.platform()}
