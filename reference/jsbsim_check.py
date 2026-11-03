#!/usr/bin/env python3
"""Does FixedWing6DOF's Rascal fly like ArduPilot's JSBSim Rascal?

    python3 reference/jsbsim_check.py --output runs/jsbsim-rascal000 [--python PATH] [--update-fixture]

The `rascal` airframe is ArduPilot's JSBSim Rascal
(Tools/autotest/aircraft/Rascal, fetched here at ARDUPILOT_COMMIT and checked
against pinned hashes; nothing of ArduPilot's is committed): its numbers,
tables, and terms, as JSBSim resolves them. JSBSim itself (the `jsbsim`
package, pinned, in a uv environment as Crazyflow is) flies that file, so it
is a second evaluator of the same aircraft; reference/jsbsim/rascal.py drives
it. Not flight truth: neither model was fitted to the aircraft. One form of
the file is not in the airframe: its elevator drag falls with up-elevator
(the signed normalized deflection), a defect of the file; the mission key
`signed_elevator_drag` restores it, and the comparisons run both ways.

Part A, the loads: at a grid of airspeed, angle of attack, sideslip, body
rates, and surface angles, JSBSim's aerodynamic force and moment against
FixedWing6DOF's from the same state, in JSBSim's environment (air density,
speed of sound) and with JSBSim's rate of change of angle of attack. The Rust
test `jsbsim_rascal_samples` (crates/core/.../fixed_wing_6dof/aerodynamics.rs)
must agree to rounding at every sample in JSBSim's elevator-drag form, and
reports the residual of the airframe as flown. The helper also reads each
term beyond the textbook equations from JSBSim's own per-function values, so
the report shows what each is worth. The samples are committed as
crates/core/tests/fixtures/jsbsim_rascal_samples.json so the test runs
offline; this script checks that fresh samples equal it (`--update-fixture`
replaces it).

Part B, the glides: 5 s open loop at 1 ms from a steady glide at 20 m/s
(trimmed on JSBSim's side; see the helper), the surfaces held at the trim
("held") and with an elevator, aileron, or rudder offset from the start, the
throttle at zero on both sides. Ripple flies each with `HeldControls` in
JSBSim's environment (its air density and speed of sound at the start and the
acceleration a resting body feels in it) twice: the airframe as flown, and in
JSBSim's elevator-drag form. The largest position difference over each glide
in JSBSim's form must be within TOLERANCE_M, set before the first run from
the sensitivities that remain when the equations agree, which are sized
alongside: JSBSim at a tenth of its step (under 8 mm), Ripple at a quarter of
its (nanometers), and Ripple at the air density of the glide's end, since
JSBSim's density rises as the aircraft descends and Ripple's is constant
(up to 0.14 m); 0.2 m covers them with margin. The airframe as flown is
reported beside it.

Passes when the grid test passes and its samples equal the fixture, and every
glide in JSBSim's form is within the tolerance. Writes OUTPUT/results.json and
OUTPUT/summary.md. Needs Cargo, uv (or --python), and network access for the
ArduPilot files.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import urllib.request

from frames import read_frames
from project import CORE_PACKAGE, ROOT, build_cli, source_identity

JSBSIM = "jsbsim==1.3.3"
ARDUPILOT_COMMIT = "5628b8d2305db00fcc4a93aa2a8c386f5516ef6a"
ARDUPILOT_RASCAL = "Tools/autotest/aircraft/Rascal"
ARDUPILOT_FILES = {  # path under ARDUPILOT_RASCAL: SHA-256 at ARDUPILOT_COMMIT
    "Rascal.xml": "01e0e0962ef62c9d3a216d7214dc3355b8a8f85b4886e97b5eb558967d98f11d",
    "Engines/Zenoah_G-26A.xml": "72d265659bcab24ba7b959c98b6cd4da2b04ed134745c6201a0126b44f84d150",
    "Engines/18x8.xml": "0689018b7621e302331bb718856ffb9951f50b75f8ea53035eb7f74ebf04ed26",
}
HELPER = ROOT / "reference/jsbsim/rascal.py"
FIXTURE = ROOT / "crates/core/tests/fixtures/jsbsim_rascal_samples.json"
RUST_TEST = "plugin::motion::fixed_wing_6dof::aerodynamics::tests::jsbsim_rascal_samples"
ALTITUDE_M = 300.0
# The file's elevator-drag form, over the airframe.
JSBSIM_FORM = dict(signed_elevator_drag=True)
# The glides' pass limit on the largest position difference, from the
# sensitivities above (JSBSim's step, Ripple's step, the air density's rise).
TOLERANCE_M = 0.2

# Part A's grid: every airspeed, angle of attack (into and past the lift
# table's peak at 0.23, and below its first row), and sideslip, with each set
# of rates and surface angles.
AIRSPEEDS_MPS = [12.0, 20.0, 30.0]
ALPHAS_RAD = [-0.3, -0.1, 0.0, 0.05, 0.1, 0.2, 0.3]
BETAS_RAD = [0.0, 0.1, -0.15]
CONTROLS = [  # (p, q, r), elevator, aileron, rudder
    ((0.0, 0.0, 0.0), 0.0, 0.0, 0.0),
    ((0.5, -0.3, 0.2), 0.0, 0.0, 0.0),
    ((0.0, 0.0, 0.0), 0.1, 0.0, 0.0),
    ((0.0, 0.0, 0.0), -0.1, 0.0, 0.0),
    ((0.5, -0.3, 0.2), 0.1, 0.2, -0.15),
    ((-0.4, 0.6, -0.3), -0.2, -0.3, 0.3),
]

# Part B's glides: offsets from the trim (elevator, aileron, rudder), radians.
AIRSPEED_MPS, END_S, DT_S = 20.0, 5.0, 0.001
JSBSIM_FINE, RIPPLE_FINE = 10, 4
GLIDES = {"held": (0.0, 0.0, 0.0), "elevator": (0.05, 0.0, 0.0),
          "aileron": (0.0, 0.1, 0.0), "rudder": (0.0, 0.0, 0.1)}


def fetch_ardupilot(folder):
    """ArduPilot's Rascal files at the pinned commit, into a JSBSim root folder."""
    hashes = {}
    for name, expected in ARDUPILOT_FILES.items():
        url = (f"https://raw.githubusercontent.com/ArduPilot/ardupilot/{ARDUPILOT_COMMIT}/"
               f"{ARDUPILOT_RASCAL}/{name}")
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read()
        hashes[name] = hashlib.sha256(data).hexdigest()
        if hashes[name] != expected:
            raise ValueError(f"{url}: SHA-256 {hashes[name]}, expected {expected}")
        target = folder / "aircraft/Rascal" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    for name in ("engine", "systems"):
        (folder / name).mkdir()
    return hashes


def run_helper(helper, mode, case, folder, name):
    """Run the JSBSim helper in `mode` on `case`, keeping its log in the folder."""
    case_path, out_path, log_path = folder / f"{name}-case.json", folder / f"{name}.json", folder / f"{name}.log"
    case_path.write_text(json.dumps(case, indent=1))
    result = subprocess.run([*helper, str(HELPER), mode, str(case_path), str(out_path)],
                            capture_output=True, text=True)
    log_path.write_text(result.stdout + result.stderr)
    if result.returncode != 0:
        raise RuntimeError(f"JSBSim's {mode} failed; see {log_path}")
    return json.loads(out_path.read_text())


def grid_states():
    return [dict(airspeed_mps=airspeed, alpha_rad=alpha, beta_rad=beta, rates_radps=list(rates),
                 elevator_rad=elevator, aileron_rad=aileron, rudder_rad=rudder)
            for airspeed in AIRSPEEDS_MPS for alpha in ALPHAS_RAD for beta in BETAS_RAD
            for rates, elevator, aileron, rudder in CONTROLS]


def largest_relative_difference(left, right):
    """The largest relative difference between two JSON values of the same shape.

    Keys, list lengths, strings, booleans, and nulls must match exactly; only
    numbers may differ. A shape difference raises.
    """
    if isinstance(left, dict) and isinstance(right, dict):
        if left.keys() != right.keys():
            raise ValueError(f"keys differ: {sorted(left.keys() ^ right.keys())}")
        return max((largest_relative_difference(left[k], right[k]) for k in left), default=0.0)
    if isinstance(left, list) and isinstance(right, list):
        if len(left) != len(right):
            raise ValueError(f"list lengths differ: {len(left)} and {len(right)}")
        return max(map(largest_relative_difference, left, right), default=0.0)
    numbers = (int, float)
    if isinstance(left, numbers) and isinstance(right, numbers) and not isinstance(left, bool) \
            and not isinstance(right, bool):
        return abs(left - right) / max(abs(left), abs(right), 1e-300)
    if left != right or type(left) is not type(right):
        raise ValueError(f"values differ: {left!r} and {right!r}")
    return 0.0


def samples_text(samples):
    """The fixture, one sample per line, without the per-function values (the
    run folder keeps those in grid/jsbsim.json)."""
    rows = [{key: value for key, value in sample.items() if key != "functions"}
            for sample in samples["samples"]]
    head = json.dumps({key: value for key, value in samples.items() if key != "samples"}, indent=1)
    return (head[:-2] + ',\n "samples": [\n  ' + ",\n  ".join(json.dumps(row) for row in rows)
            + "\n ]\n}\n")


def rust_test(samples_path, folder):
    """The Rust grid test on these samples; its printed errors, or None if it failed."""
    test = subprocess.run(
        ["cargo", "test", "-p", CORE_PACKAGE, "--lib", "jsbsim_rascal_samples", "--",
         "--nocapture", "--exact", RUST_TEST],
        cwd=ROOT, capture_output=True, text=True,
        env={**os.environ, "RIPPLE_JSBSIM_SAMPLES": str(samples_path.resolve())})
    (folder / "rust-test.log").write_text(test.stdout + test.stderr)
    errors = None
    for line in test.stdout.splitlines():
        if line.startswith("JSBSIM_ERRORS "):
            errors = json.loads(line.removeprefix("JSBSIM_ERRORS "))
    return errors if test.returncode == 0 else None


def yaml_keys(keys, indent):
    pad = " " * indent
    return "".join(f"{pad}{key}: {json.dumps(value)}\n" for key, value in keys.items())


def glide_mission(name, glide, keys, dt_s):
    """A Ripple mission flying JSBSim's trimmed start with the same held surfaces.

    JSBSim's glide heads north; Ripple's `heading_deg` is from east, so 90.
    Ripple's `pitch_deg` is positive nose down (the rotation is about the
    body's left axis; see the book's coordinate-frames chapter), so JSBSim's
    theta goes in negated.
    """
    trim, controls = glide["trim"], glide["controls"]
    gamma = trim["gamma_rad"]
    velocity = [0.0, AIRSPEED_MPS * math.cos(gamma), AIRSPEED_MPS * math.sin(gamma)]
    return (f"format_version: 1\nname: {name}\nrun:\n  end_s: {END_S}\n  dt_s: {dt_s!r}\n"
            f"  seed: 1\nentities:\n  rascal:\n    position_m: [0, 0, {ALTITUDE_M!r}]\n"
            f"    velocity_mps: {json.dumps(velocity)}\n    heading_deg: 90\n"
            f"    pitch_deg: {-math.degrees(trim['theta_rad'])!r}\n"
            "    controller:\n      HeldControls:\n"
            + yaml_keys(controls, 8)
            + "    motion_model:\n      FixedWing6DOF:\n"
            + yaml_keys(keys, 8))


def run_ripple(binary, folder, name, text):
    path = folder / f"{name}.yaml"
    path.write_text(text)
    subprocess.run([binary, "run", path, "--root", ROOT, "--output", folder / name, "--headless"],
                   check=True, capture_output=True)
    return read_frames(folder / name / "run.mcap")


def multiply(a, b):
    """Quaternion product, scalar first."""
    aw, ax, ay, az = a
    bw, bx, by, bz = b
    return (aw * bw - ax * bx - ay * by - az * bz, aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw)


def ned_euler(orientation_enu_flu):
    """Roll, pitch, and yaw of Ripple's east/north/up, forward/left/up attitude
    as JSBSim gives them: in north/east/down, forward/right/down axes. The
    frame switch is `dynamics.rs`'s: a half-turn about the north-east diagonal
    and one about the nose."""
    half = math.sqrt(0.5)
    w, x, y, z = multiply(multiply((0.0, half, half, 0.0), orientation_enu_flu), (0.0, 1.0, 0.0, 0.0))
    return (math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y)),
            math.asin(max(-1.0, min(1.0, 2 * (w * y - z * x)))),
            math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z)))


def ripple_path(frames):
    path = []
    for frame in frames:
        entity = next(iter(frame.entities.values()))
        path.append(dict(time_s=frame.time_s, position_enu_m=list(entity.position),
                         velocity_enu_mps=list(entity.velocity),
                         euler_ned_rad=list(ned_euler(entity.orientation))))
    return path


def jsbsim_path(glide):
    return [dict(time_s=sample["time_s"],
                 position_enu_m=[sample["east_m"], sample["north_m"], sample["altitude_m"]],
                 velocity_enu_mps=[sample["velocity_ned_mps"][1], sample["velocity_ned_mps"][0],
                                   -sample["velocity_ned_mps"][2]],
                 euler_ned_rad=sample["euler_rad"]) for sample in glide["samples"]]


def wrap(angle_rad):
    return (angle_rad + math.pi) % (2 * math.pi) - math.pi


def compare_paths(name, left, right):
    """The largest differences between two paths at the right path's times, which
    must be every millisecond of the glide."""
    by_time = {round(sample["time_s"], 9): sample for sample in left}
    largest = dict(position_m=0.0, velocity_mps=0.0, attitude_rad=0.0)
    at = {}
    compared = 0
    for sample in right:
        other = by_time.get(round(sample["time_s"], 9))
        if other is None:
            raise ValueError(f"{name}: no sample at {sample['time_s']:.4f} s")
        values = [*sample["position_enu_m"], *sample["velocity_enu_mps"], *sample["euler_ned_rad"],
                  *other["position_enu_m"], *other["velocity_enu_mps"], *other["euler_ned_rad"]]
        if not all(math.isfinite(value) for value in values):
            raise ValueError(f"{name}: nonfinite state at {sample['time_s']} s")
        position = math.dist(sample["position_enu_m"], other["position_enu_m"])
        largest["position_m"] = max(largest["position_m"], position)
        largest["velocity_mps"] = max(largest["velocity_mps"],
                                      math.dist(sample["velocity_enu_mps"], other["velocity_enu_mps"]))
        largest["attitude_rad"] = max(largest["attitude_rad"],
                                      *(abs(wrap(a - b)) for a, b in
                                        zip(sample["euler_ned_rad"], other["euler_ned_rad"])))
        for mark in (1, 2, 3, 4, 5):
            if abs(sample["time_s"] - mark) < 1e-9:
                at[f"position_error_at_{mark}s_m"] = position
        compared += 1
    expected = round(END_S / DT_S) + 1
    if compared != expected:
        raise ValueError(f"{name}: compared {compared} samples, expected {expected}")
    return dict(samples_compared=compared, largest=largest, **at)


def glide_case(root, dt_s, record_every, offsets):
    elevator, aileron, rudder = offsets
    return dict(root=str(root), altitude_m=ALTITUDE_M, airspeed_mps=AIRSPEED_MPS, dt_s=dt_s,
                end_s=END_S, record_every=record_every, elevator_offset_rad=elevator,
                aileron_rad=aileron, rudder_rad=rudder)


def run_glide(name, offsets, root, helper, binary, output):
    folder = output / "glides" / name
    folder.mkdir(parents=True)
    jsbsim = run_helper(helper, "glide", glide_case(root, DT_S, 1, offsets), folder, "jsbsim")
    fine = run_helper(helper, "glide", glide_case(root, DT_S / JSBSIM_FINE, JSBSIM_FINE, offsets),
                      folder, "jsbsim-fine")
    vehicle = jsbsim["vehicle"]
    preset = dict(airframe="rascal", **environment(vehicle), gravity_mps2=vehicle["gravity_mps2"])
    form = {**preset, **JSBSIM_FORM}
    end_density = {**form, "air_density_kgpm3": jsbsim["air_density_end_kgpm3"]}
    paths = {}
    for variant, keys, dt_s in (("preset", preset, DT_S), ("jsbsim-form", form, DT_S),
                                ("jsbsim-form-end-density", end_density, DT_S),
                                ("jsbsim-form-fine", form, DT_S / RIPPLE_FINE)):
        frames = run_ripple(binary, folder, variant, glide_mission(f"{name}-{variant}", jsbsim, keys, dt_s))
        paths[variant] = ripple_path(frames)
    reference, reference_fine = jsbsim_path(jsbsim), jsbsim_path(fine)
    # The start must agree exactly, or the mission's attitude and velocity are
    # not JSBSim's.
    for variant in ("preset", "jsbsim-form"):
        first, theirs = paths[variant][0], reference[0]
        if (math.dist(first["position_enu_m"], theirs["position_enu_m"]) > 1e-6
                or math.dist(first["velocity_enu_mps"], theirs["velocity_enu_mps"]) > 1e-9
                or max(abs(wrap(a - b)) for a, b in
                       zip(first["euler_ned_rad"], theirs["euler_ned_rad"])) > 1e-9):
            raise ValueError(f"{name}: Ripple's start is not JSBSim's: {first} against {theirs}")
    return dict(
        trim=jsbsim["trim"], controls=jsbsim["controls"], vehicle=vehicle,
        air_density_end_kgpm3=jsbsim["air_density_end_kgpm3"],
        preset=compare_paths(f"{name} preset", paths["preset"], reference),
        jsbsim_form=compare_paths(f"{name} jsbsim-form", paths["jsbsim-form"], reference),
        sensitivities=dict(
            jsbsim_step=compare_paths(f"{name} JSBSim fine", reference_fine, reference),
            ripple_step=compare_paths(f"{name} Ripple fine", paths["jsbsim-form-fine"],
                                      paths["jsbsim-form"]),
            air_density_rise=compare_paths(f"{name} end density", paths["jsbsim-form-end-density"],
                                           paths["jsbsim-form"])))


def environment(numbers):
    """JSBSim's air at the start, as FixedWing6DOF's environment keys."""
    return dict(air_density_kgpm3=numbers["air_density_kgpm3"],
                speed_of_sound_mps=numbers["speed_of_sound_mps"])


def glides_pass(glides):
    return all(glide["jsbsim_form"]["largest"]["position_m"] <= TOLERANCE_M
               for glide in glides.values())


def number(value):
    return f"{value:.2e}"


def summary(report):
    grid, glides = report["grid"], report["glides"]
    lines = ["# FixedWing6DOF's Rascal against ArduPilot's JSBSim Rascal", "",
             f"JSBSim {report['jsbsim_version']} (`{JSBSIM}`) flying ArduPilot's Rascal files at "
             f"commit `{ARDUPILOT_COMMIT[:12]}`. {'Passed' if report['passed'] else 'FAILED'}: "
             "the loads agree to rounding on the grid, the samples equal the fixture, and every "
             f"glide in JSBSim's elevator-drag form stays within {TOLERANCE_M:g} m.", "",
             f"## Loads on the grid ({grid['samples']} samples)", ""]
    errors = grid["errors"]
    if errors:
        lines += [f"The `rascal` airframe in JSBSim's elevator-drag form: largest force residual "
                  f"{number(errors['jsbsim_form']['force'])} of q S, moment "
                  f"{number(errors['jsbsim_form']['moment'])} of q S b (the test's limit is "
                  f"{number(errors['limit'])}). As flown, with the elevator drag from the "
                  f"deflection's size: force {number(errors['preset']['force'])} of q S, moment "
                  f"{number(errors['preset']['moment'])} of q S b.", "",
                  "| Term beyond the textbook equations | Largest size, of q S (forces) or q S b (moments) |",
                  "| --- | --- |"]
        lines += [f"| {name} | {number(size)} |" for name, size in errors["terms"].items()]
    else:
        lines.append("The Rust test failed; see rust-test.log.")
    difference = grid["fixture_largest_relative_difference"]
    lines += ["", "The committed fixture was replaced by these samples." if difference is None else
              f"Fresh samples against the committed fixture: largest relative difference "
              f"{number(difference)}.", "",
              f"## Glides ({END_S:g} s at {DT_S:g} s from a {AIRSPEED_MPS:g} m/s glide)", ""]
    first = next(iter(glides.values()))
    trim = first["trim"]
    lines += [f"Trim: angle of attack {trim['alpha_rad']:.5f} rad, elevator {trim['elevator_rad']:.5f} "
              f"rad, flight path {trim['gamma_rad']:.5f} rad. Mass {first['vehicle']['mass_kg']:.4f} kg, "
              f"air density {first['vehicle']['air_density_kgpm3']:.5f} kg/m^3, resting acceleration "
              f"{first['vehicle']['gravity_mps2']:.5f} m/s^2.", "",
              "| Glide | JSBSim's form: largest position error (m) | at 5 s | attitude (rad) | "
              "As flown: largest (m) | at 5 s | attitude (rad) |",
              "| --- | --- | --- | --- | --- | --- | --- |"]
    for name, glide in glides.items():
        j, p = glide["jsbsim_form"], glide["preset"]
        lines.append(f"| {name} | {number(j['largest']['position_m'])} | "
                     f"{number(j['position_error_at_5s_m'])} | {number(j['largest']['attitude_rad'])} | "
                     f"{number(p['largest']['position_m'])} | {number(p['position_error_at_5s_m'])} | "
                     f"{number(p['largest']['attitude_rad'])} |")
    lines += ["", f"The tolerance is {TOLERANCE_M:g} m on JSBSim's form. Sensitivities, as the largest "
              "position change over the glide (m):", "",
              "| Glide | JSBSim at a tenth of the step | Ripple at a quarter | Air density of the end |",
              "| --- | --- | --- | --- |"]
    for name, glide in glides.items():
        r = glide["sensitivities"]
        lines.append(f"| {name} | {number(r['jsbsim_step']['largest']['position_m'])} | "
                     f"{number(r['ripple_step']['largest']['position_m'])} | "
                     f"{number(r['air_density_rise']['largest']['position_m'])} |")
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--python", help="an interpreter with jsbsim installed")
    parser.add_argument("--update-fixture", action="store_true")
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True)
    helper = [args.python, "-I"] if args.python else [
        "uv", "run", "--quiet", "--no-project", "--python", "3.12", "--with", JSBSIM, "python", "-I"]
    root = args.output / "ardupilot"
    hashes = fetch_ardupilot(root)
    binary = build_cli("release")

    grid_folder = args.output / "grid"
    grid_folder.mkdir()
    grid = run_helper(helper, "grid", dict(root=str(root), altitude_m=ALTITUDE_M, states=grid_states()),
                      grid_folder, "jsbsim")
    samples = dict(ardupilot_commit=ARDUPILOT_COMMIT, jsbsim=JSBSIM,
                   jsbsim_version=grid["jsbsim_version"], altitude_m=ALTITUDE_M,
                   mission_keys=dict(airframe="rascal", **environment(grid), **JSBSIM_FORM),
                   samples=grid["samples"])
    samples_path = args.output / "samples.json"
    samples_path.write_text(samples_text(samples))
    fixture_difference = None
    if args.update_fixture:
        FIXTURE.write_text(samples_path.read_text())
    else:
        fixture_difference = largest_relative_difference(json.loads(FIXTURE.read_text()),
                                                         json.loads(samples_path.read_text()))
    errors = rust_test(samples_path, grid_folder)
    grid_report = dict(samples=len(grid["samples"]), errors=errors,
                       fixture_largest_relative_difference=fixture_difference)

    glides = {name: run_glide(name, offsets, root, helper, binary, args.output)
              for name, offsets in GLIDES.items()}

    passed = (errors is not None and (fixture_difference is None or fixture_difference <= 1e-12)
              and glides_pass(glides))
    report = dict(passed=passed, jsbsim=JSBSIM, jsbsim_version=grid["jsbsim_version"],
                  ardupilot_commit=ARDUPILOT_COMMIT, ardupilot_files=hashes,
                  grid=grid_report, glides=glides, source=source_identity())
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    (args.output / "summary.md").write_text(summary(report))
    print((args.output / "summary.md").read_text())
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
