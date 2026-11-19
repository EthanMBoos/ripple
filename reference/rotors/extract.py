#!/usr/bin/env python3
"""Copy RotorS's motor-plugin code verbatim for harness.cpp.

    python3 extract.py ROTORS_CHECKOUT OUTPUT_DIR

Writes rotors_filter.inc (FirstOrderFilter from common.h) and
rotors_methods.inc (GazeboMotorModel::NormalizeAngle and
::UpdateForcesAndMoments from gazebo_motor_model.cpp). Fails if a piece is
missing, so a changed upstream file cannot pass silently.
"""
from pathlib import Path
import sys


def block(lines, first, what):
    """Lines from `first` through the next line that is exactly a closing brace."""
    end = first
    while lines[end].rstrip() not in ("}", "};"):
        end += 1
        if end == len(lines):
            sys.exit(f"no end for {what}")
    return lines[first:end + 1]


def find(lines, text):
    for index, line in enumerate(lines):
        if line.startswith(text):
            return index
    sys.exit(f"missing: {text}")


def main():
    checkout, output = Path(sys.argv[1]), Path(sys.argv[2])
    plugins = checkout / "rotors_gazebo_plugins"
    common = (plugins / "include/rotors_gazebo_plugins/common.h").read_text().splitlines()
    start = find(common, "class FirstOrderFilter")
    # The template line directly above the class belongs to it.
    assert common[start - 1].startswith("template"), common[start - 1]
    (output / "rotors_filter.inc").write_text(
        "\n".join(common[start - 1:start] + block(common, start, "FirstOrderFilter")) + "\n")
    motor = (plugins / "src/gazebo_motor_model.cpp").read_text().splitlines()
    methods = []
    for signature in ("double GazeboMotorModel::NormalizeAngle",
                      "void GazeboMotorModel::UpdateForcesAndMoments"):
        methods += block(motor, find(motor, signature), signature) + [""]
    (output / "rotors_methods.inc").write_text("\n".join(methods) + "\n")


if __name__ == "__main__":
    main()
