import unittest

from command_clock import COMMAND_INTERVAL_S, CommandClock


class CommandClockTest(unittest.TestCase):
    def test_paces_by_ardupilot_time_not_wall_time(self):
        clock = CommandClock()
        # Many loop iterations within one ArduPilot step send one command.
        self.assertEqual([clock.due(10.0) for _ in range(5)], [True, False, False, False, False])
        # However fast the simulation runs, every 0.5 s of ArduPilot time gets
        # one: 10.5, 11.0, ..., 49.5 after the first at 10.0.
        sent = [t / 100 for t in range(1000, 5000) if clock.due(t / 100)]
        self.assertEqual(len(sent), 79)
        self.assertTrue(all(b - a >= COMMAND_INTERVAL_S - 1e-9 for a, b in zip(sent, sent[1:])))

    def test_interval_keeps_rc_override_alive(self):
        # ArduPilot drops RC overrides after 3 s without a refresh.
        self.assertLess(COMMAND_INTERVAL_S, 3.0)


if __name__ == "__main__":
    unittest.main()
