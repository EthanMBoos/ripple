"""The `Python` plugins, end to end through the `ripple` command.

Each run is its own process (`python -m ripple`), as a user's is, so a plugin
module imported for one mission cannot be reused by the next. Runs are written
under pytest's temporary directory. `reference/run.py` reads the records;
`cargo build --release` provides the Rust executable for the two-commands
check (maturin builds this package's module in release too, and the debug and
release builds do not write identical frames).
"""

import shutil
import subprocess
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
EXAMPLE = HERE.parent / "examples" / "follow_nearest" / "follow-nearest.yaml"
RUST_MISSION = ROOT / "crates" / "starter" / "missions" / "follow-nearest.yaml"
CHANNELS = ("/entities", "/frames", "/events", "/summary")

sys.path.insert(0, str(ROOT / "reference"))
from run import read_run  # noqa: E402


def ripple(*arguments):
    return subprocess.run([sys.executable, "-m", "ripple", *map(str, arguments)],
                          capture_output=True, text=True)


def run(mission, output, *options):
    result = ripple("run", mission, "--headless", "--output", output, *options)
    assert result.returncode == 0, result.stderr
    return output / "run.mcap"


def failure(mission, output):
    result = ripple("run", mission, "--headless", "--output", output)
    assert result.returncode != 0, result.stdout
    return result.stderr


def test_python_follow_nearest_matches_the_rust_plugin(tmp_path):
    python = read_run(run(EXAMPLE, tmp_path / "python"))
    rust = read_run(run(RUST_MISSION, tmp_path / "rust"))
    for topic in CHANNELS:
        assert python.channels[topic] == rust.channels[topic], topic
    # The records themselves differ only in the scenario, which names the plugin.
    assert python.scenario != rust.scenario


def test_python_mission_is_byte_identical_across_workers(tmp_path):
    one = run(EXAMPLE, tmp_path / "one", "--workers", "1").read_bytes()
    eight = run(EXAMPLE, tmp_path / "eight", "--workers", "8").read_bytes()
    assert one == eight


def test_numpy_generator_seeded_by_ctx_random_replays(tmp_path):
    mission = HERE / "random-walk.yaml"
    one = run(mission, tmp_path / "one", "--workers", "1").read_bytes()
    eight = run(mission, tmp_path / "eight", "--workers", "8").read_bytes()
    again = run(mission, tmp_path / "again", "--workers", "1").read_bytes()
    assert one == eight == again
    frames = read_run(tmp_path / "one").frames
    assert len(frames) > 100


def test_rust_only_mission_matches_the_rust_executable(tmp_path):
    cargo = shutil.which("cargo")
    if cargo is None:
        pytest.skip("cargo is not installed")
    subprocess.run([cargo, "build", "--release", "-p", "ripple", "--bin", "ripple"],
                   cwd=ROOT, check=True, capture_output=True)
    executable = ROOT / "target" / "release" / "ripple"
    subprocess.run([executable, "run", str(RUST_MISSION), "--headless",
                    "--output", str(tmp_path / "rust")], check=True, capture_output=True)
    python = run(RUST_MISSION, tmp_path / "python").read_bytes()
    assert python == (tmp_path / "rust" / "run.mcap").read_bytes()


def mission_with(text, replacements, path):
    for old, new in replacements:
        assert old in text, old
        text = text.replace(old, new)
    path.write_text(text)
    shutil.copy(EXAMPLE.parent / "follow_nearest.py", path.parent)
    return path


@pytest.mark.parametrize("replacement, message", [
    (("class: follow_nearest.FollowNearest", "class: nowhere.Missing"),
     "No module named 'nowhere'"),
    (("speed_mps: 30", "speed_mps: 30\n        turbo: 1"),
     "unexpected keyword argument 'turbo'"),
    (("speed_mps: 30", "speed_mps: -1"), "speed must be positive"),
])
def test_load_and_construction_errors_name_the_plugin(tmp_path, replacement, message):
    mission = mission_with(EXAMPLE.read_text(), [replacement], tmp_path / "bad.yaml")
    error = failure(mission, tmp_path / "out")
    assert message in error
    assert "Python plugin 'follow_nearest.FollowNearest'" in error or "nowhere.Missing" in error


def test_a_bad_port_fails_at_load(tmp_path):
    plugin = EXAMPLE.parent.joinpath("follow_nearest.py").read_text()
    plugin = plugin.replace('"desired_speed": ("m/s", "none")', '"desired_speed": ("knots", "none")')
    (tmp_path / "follow_nearest.py").write_text(plugin)
    mission = tmp_path / "bad-port.yaml"
    mission.write_text(EXAMPLE.read_text())
    assert "unknown unit 'knots'" in failure(mission, tmp_path / "out")


def test_a_mission_under_the_root_missions_folder_finds_its_plugin(tmp_path):
    missions = tmp_path / "missions" / "chase"
    missions.mkdir(parents=True)
    shutil.copy(EXAMPLE, missions)
    shutil.copy(EXAMPLE.parent / "follow_nearest.py", missions)
    run("chase/follow-nearest.yaml", tmp_path / "out", "--root", tmp_path)


PID = HERE.parent / "examples" / "pid_controller" / "follow-nearest.yaml"


def test_python_pid_controller_matches_the_rust_controller(tmp_path):
    python = read_run(run(PID, tmp_path / "python"))
    rust = read_run(run(RUST_MISSION, tmp_path / "rust"))
    for topic in CHANNELS:
        assert python.channels[topic] == rust.channels[topic], topic


def test_python_controller_mission_is_byte_identical_across_workers(tmp_path):
    one = run(PID, tmp_path / "one", "--workers", "1").read_bytes()
    eight = run(PID, tmp_path / "eight", "--workers", "8").read_bytes()
    assert one == eight


def test_an_autonomy_declaring_inputs_fails_at_load(tmp_path):
    plugin = EXAMPLE.parent.joinpath("follow_nearest.py").read_text()
    plugin = plugin.replace('ports = {"outputs"', 'ports = {"inputs": {"x": ("m", "world")}, "outputs"')
    (tmp_path / "follow_nearest.py").write_text(plugin)
    mission = tmp_path / "bad-inputs.yaml"
    mission.write_text(EXAMPLE.read_text())
    assert "an autonomy's `ports` has no `inputs`" in failure(mission, tmp_path / "out")


ESTIMATORS = HERE.parent / "examples"
DEAD_RECKONING = ESTIMATORS / "dead_reckoning" / "dead-reckoning.yaml"
NOISY_STATE = ESTIMATORS / "noisy_state" / "noisy-state.yaml"
PASS_THROUGH = ESTIMATORS / "pass_through" / "follow-nearest.yaml"


def rust_noisy_state(path):
    """The sensor mission with the built-in sensor under the same instance name.

    A sensor's random numbers are keyed by its instance name, and a sensor
    written without a `plugin:` line (as in `missions/noisy-state.yaml`) is
    named `NoisyState:0`, so the twin names the built-in explicitly.
    """
    text = NOISY_STATE.read_text()
    old = "plugin: Python\n        class: noisy_state.NoisyState"
    assert old in text
    path.write_text(text.replace(old, "plugin: NoisyState"))
    return path


def test_python_dead_reckoning_matches_the_rust_localization(tmp_path):
    python = read_run(run(DEAD_RECKONING, tmp_path / "python"))
    rust = read_run(run(ROOT / "crates" / "starter" / "missions" / "dead-reckoning.yaml",
                        tmp_path / "rust"))
    for topic in CHANNELS:
        assert python.channels[topic] == rust.channels[topic], topic


def test_python_noisy_state_draws_the_rust_sensors_numbers(tmp_path):
    python = read_run(run(NOISY_STATE, tmp_path / "python"))
    rust = read_run(run(rust_noisy_state(tmp_path / "rust-noisy-state.yaml"), tmp_path / "rust"))
    for topic in CHANNELS:
        assert python.channels[topic] == rust.channels[topic], topic


def test_python_pass_through_estimators_match_the_defaults(tmp_path):
    python = read_run(run(PASS_THROUGH, tmp_path / "python"))
    rust = read_run(run(RUST_MISSION, tmp_path / "rust"))
    for topic in CHANNELS:
        assert python.channels[topic] == rust.channels[topic], topic


@pytest.mark.parametrize("mission", [DEAD_RECKONING, NOISY_STATE, PASS_THROUGH],
                         ids=["localization", "sensor", "estimators"])
def test_python_sensor_and_estimator_missions_are_byte_identical_across_workers(tmp_path,
                                                                                 mission):
    one = run(mission, tmp_path / "one", "--workers", "1").read_bytes()
    eight = run(mission, tmp_path / "eight", "--workers", "8").read_bytes()
    assert one == eight


def test_a_bad_reading_kind_fails_at_load(tmp_path):
    plugin = NOISY_STATE.with_name("noisy_state.py").read_text()
    (tmp_path / "noisy_state.py").write_text(plugin.replace('reading_kind = "state"',
                                                            'reading_kind = "pose"'))
    mission = tmp_path / "noisy-state.yaml"
    mission.write_text(NOISY_STATE.read_text())
    error = failure(mission, tmp_path / "out")
    assert "unknown reading kind 'pose'" in error and "noisy_state.NoisyState" in error


MOTION = ESTIMATORS / "single_integrator" / "waypoints-point-agents.yaml"
WORLD = ESTIMATORS / "world" / "follow-nearest.yaml"


def test_python_single_integrator_matches_the_rust_motion_model(tmp_path):
    python = read_run(run(MOTION, tmp_path / "python"))
    rust = read_run(run(ROOT / "missions" / "waypoints-point-agents.yaml", tmp_path / "rust"))
    for topic in CHANNELS:
        assert python.channels[topic] == rust.channels[topic], topic


def test_python_world_plugins_match_the_rust_interaction_network_and_metrics(tmp_path):
    python = read_run(run(WORLD, tmp_path / "python"))
    rust = read_run(run(RUST_MISSION, tmp_path / "rust"))
    for topic in ("/entities", "/frames", "/events"):
        assert python.channels[topic] == rust.channels[topic], topic
    # The summary names the metrics plugin, so compare what it reported.
    assert list(python.metrics.values()) == list(rust.metrics.values())


@pytest.mark.parametrize("mission", [MOTION, WORLD], ids=["motion", "world"])
def test_python_motion_and_world_missions_are_byte_identical_across_workers(tmp_path, mission):
    one = run(mission, tmp_path / "one", "--workers", "1").read_bytes()
    eight = run(mission, tmp_path / "eight", "--workers", "8").read_bytes()
    assert one == eight


def test_a_motion_model_declaring_outputs_fails_at_load(tmp_path):
    plugin = MOTION.with_name("single_integrator.py").read_text()
    plugin = plugin.replace('ports = {"inputs":',
                            'ports = {"outputs": {"thrust": ("N", "body")},\n             "inputs":')
    (tmp_path / "single_integrator.py").write_text(plugin)
    mission = tmp_path / "waypoints.yaml"
    mission.write_text(MOTION.read_text())
    assert "a motion model's `ports` has no `outputs`" in failure(mission, tmp_path / "out")
