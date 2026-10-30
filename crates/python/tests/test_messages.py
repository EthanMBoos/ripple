"""Messages between Python plugins, and across the boundary to and from Rust
plugins, through the `ripple` command (examples/messages).
"""

import pytest

from test_python_plugins import CHANNELS, ROOT, failure, read_run, run

HERE = ROOT / "crates" / "python" / "examples" / "messages"
STOCK = ROOT / "missions" / "networks-local-global.yaml"


def metrics(record, plugin):
    return record.metrics[plugin]


def test_python_plugins_exchange_messages(tmp_path):
    # Three Reporter autonomies publish once a tick; the ReportCount metrics
    # plugin receives every message the same tick, since GlobalNetwork
    # delivers at once and metrics run after networks.
    record = read_run(run(HERE / "reports.yaml", tmp_path / "out"))
    steps = len(record.frames) - 1
    teams = metrics(record, "metrics/Python:0")
    assert teams[1]["reports"] == steps          # blue
    assert teams[2]["reports"] == 2 * steps      # red and green
    assert teams[1]["farthest_x_m"] == pytest.approx((steps - 1) * 0.1)
    assert (teams[1]["min_delay_s"], teams[1]["max_delay_s"]) == (0.0, 0.0)


def test_delayed_messages_cross_the_bridge_with_their_delay(tmp_path):
    # A Python network delays every message 0.3 s (three ticks), so the last
    # three reports are still in flight when the run ends.
    record = read_run(run(HERE / "reports-delayed.yaml", tmp_path / "out"))
    steps = len(record.frames) - 1
    teams = metrics(record, "metrics/Python:0")
    assert teams[1]["reports"] == steps - 3
    assert teams[2]["reports"] == 2 * (steps - 3)
    assert teams[1]["min_delay_s"] == pytest.approx(0.3)
    assert teams[1]["max_delay_s"] == pytest.approx(0.3)


def test_a_rust_message_reaches_a_python_subscriber_as_a_dict(tmp_path):
    # The Rust Boundary interaction publishes its region once, at the first
    # tick; the Python BoundaryLog metrics plugin receives it as a dict. The
    # flight is the stock mission's, since a metrics plugin changes nothing.
    logged = read_run(run(HERE / "boundary-log.yaml", tmp_path / "logged"))
    stock = read_run(run(STOCK, tmp_path / "stock"))
    log = metrics(logged, "metrics/Python:1")[0]
    assert (log["boundaries"], log["center_z_m"], log["length_x_m"]) == (1, 200, 600)
    assert log["delivered_at_s"] == 0.0
    for topic in ("/entities", "/frames", "/events"):
        assert logged.channels[topic] == stock.channels[topic], topic


def test_a_python_message_reaches_a_rust_subscriber_as_its_type(tmp_path):
    # The boundary published from Python turns the Rust Straight autonomies
    # exactly as the Rust interaction's message does.
    python = read_run(run(HERE / "boundary-from-python.yaml", tmp_path / "python"))
    stock = read_run(run(STOCK, tmp_path / "stock"))
    for topic in ("/entities", "/frames", "/events"):
        assert python.channels[topic] == stock.channels[topic], topic


@pytest.mark.parametrize("mission", ["reports.yaml", "boundary-from-python.yaml"])
def test_message_missions_are_byte_identical_across_workers(tmp_path, mission):
    one = run(HERE / mission, tmp_path / "one", "--workers", "1").read_bytes()
    eight = run(HERE / mission, tmp_path / "eight", "--workers", "8").read_bytes()
    assert one == eight


def test_a_message_of_the_wrong_shape_stops_the_run_naming_the_channel(tmp_path):
    mission = tmp_path / "bad-boundary.yaml"
    mission.write_text((HERE / "boundary-from-python.yaml").read_text().replace(
        "class: boundary.Boundary\n", "class: boundary.Boundary\n    bad_shape: true\n"))
    for name in ("boundary.py",):
        (tmp_path / name).write_text((HERE / name).read_text())
    error = failure(mission, tmp_path / "out")
    assert "GlobalNetwork/Boundary" in error and "BoundaryRegion" in error, error


def test_a_subscription_to_an_event_channel_fails_at_load(tmp_path):
    (tmp_path / "eavesdrop.py").write_text(
        "class Eavesdrop:\n"
        "    subscriptions = {('GlobalNetwork', 'TeamCollision')}\n"
        "    def step(self, ctx): pass\n"
        "    def report(self, time_s): return {'headers': [], 'teams': {}}\n")
    mission = tmp_path / "eavesdrop.yaml"
    mission.write_text((HERE / "reports.yaml").read_text().replace(
        "class: report_count.ReportCount", "class: eavesdrop.Eavesdrop"))
    for name in ("reporter.py",):
        (tmp_path / name).write_text((HERE / name).read_text())
    error = failure(mission, tmp_path / "out")
    assert "ctx.events" in error and "TeamCollision" in error, error


def test_a_published_value_is_copied_at_the_publish_call(tmp_path):
    # Mutating publishes {"count": 1} and then sets count to -1; only the 1
    # arrives (snapshot.py, beside this file).
    record = read_run(run(ROOT / "crates" / "python" / "tests" / "snapshot.yaml",
                          tmp_path / "out"))
    received = metrics(record, "metrics/Python:0")[1]
    assert (received["smallest"], received["largest"]) == (1, 1)


def test_an_integer_past_64_bits_is_an_error_at_publish(tmp_path):
    error = failure(ROOT / "crates" / "python" / "tests" / "snapshot-huge.yaml",
                    tmp_path / "out")
    assert "64 bits" in error, error
