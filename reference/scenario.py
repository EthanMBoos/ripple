"""Missions as data, for scripts that build, edit, or translate them.

Ripple records the scenario it loaded (templates expanded, overrides applied,
every default filled in) in each run's manifest.json. A script reads that
record with `recorded`, edits it as plain data, and writes it back as a YAML
mission with `as_yaml` and `write`. Missions are written as JSON, which YAML
reads, so the scripts need only the standard library. `loaded` reads the
scenario of a mission written this way without running it.
"""

import json
from pathlib import Path
import subprocess

from project import ROOT


def recorded(run):
    """The scenario a finished run loaded, from its manifest.json."""
    return json.loads((Path(run) / "manifest.json").read_text())["scenario"]


def write(path, mission):
    Path(path).write_text(json.dumps(mission, indent=1) + "\n")
    return path


def loaded(cli, mission, scratch):
    """The scenario Ripple loads from `mission`, a YAML mission written as JSON:
    a run cut to no time, with the mission's end time put back."""
    run = json.loads(Path(mission).read_text())["run"]
    subprocess.run([str(cli), "run", str(mission), "--root", str(ROOT), "--output", str(scratch),
                    "--no-rerun", f"run.end_s:={run.get('start_s', 0)}"],
                   check=True, stdout=subprocess.DEVNULL)
    scenario = recorded(scratch)
    scenario["run"]["end_s"] = run.get("end_s", 100)
    return scenario


def plugins(configs):
    """A YAML plugin slot: each plugin under its label, with its type if the label differs."""
    slot = {}
    for config in configs:
        params = dict(config["params"])
        if config["instance"] is not None:
            params["plugin"] = config["name"]
        if config["loop_rate_hz"]:
            params["loop_rate"] = config["loop_rate_hz"]
        if config.get("inputs"):
            params["inputs"] = config["inputs"]
        slot[config["instance"] or config["name"]] = params
    return slot


def as_yaml(scenario):
    """A recorded scenario as a YAML mission that loads to the same scenario."""
    entities = {}
    for group in scenario["entities"]:
        fields = {key: value for key, value in group.items() if key != "label"}
        for slot in ("autonomy", "controller", "sensors"):
            fields[slot] = plugins(group[slot])
        for slot in ("motion_model", "localization", "tracking"):
            fields[slot] = plugins([group[slot]])
        entities[group["label"]] = fields
    ends = [name for name, on in scenario["end_conditions"].items() if on]
    mission = {"format_version": 1, "name": "mission", "run": scenario["run"],
               "end_conditions": ends or ["none"], "spatial_lookup": scenario["spatial_lookup"],
               "interactions": plugins(scenario["interactions"]),
               "networks": plugins(scenario["networks"]),
               "metrics": plugins(scenario["metrics"]), "entities": entities}
    if scenario["terrain"] is not None:
        mission["terrain"] = scenario["terrain"]  # absolute, so the file's folder is irrelevant
    return mission
