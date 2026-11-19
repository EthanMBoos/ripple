import unittest

from pacing import COMMAND_INTERVAL_S, MODE_HOLD_S, CommandClock, ModeHold, ReleaseCheck


class CommandClockTest(unittest.TestCase):
    def test_paces_by_px4_time_not_wall_time(self):
        clock = CommandClock()
        # Many loop iterations within one PX4 step send one command.
        self.assertEqual([clock.due(10.0) for _ in range(5)], [True, False, False, False, False])
        # However fast the simulation runs, every 0.5 s of PX4 time gets one.
        sent = [t / 100 for t in range(1000, 5000) if clock.due(t / 100)]
        self.assertEqual(len(sent), 79)
        self.assertTrue(all(b - a >= COMMAND_INTERVAL_S - 1e-9 for a, b in zip(sent, sent[1:])))


class ReleaseCheckTest(unittest.TestCase):
    def test_launch_flight_landing_and_disarm_is_not_a_miss(self):
        check = ReleaseCheck(40.0)
        self.assertFalse(check.missed(10.0, ready_to_fly=False))
        self.assertFalse(check.missed(39.0, ready_to_fly=True))
        self.assertFalse(check.missed(100.0, ready_to_fly=True))
        # Lands and disarms itself: not a missed release.
        self.assertFalse(check.missed(180.0, ready_to_fly=False))

    def test_not_armed_by_the_deadline_is_a_miss_reported_once(self):
        check = ReleaseCheck(40.0)
        self.assertTrue(check.missed(39.0, ready_to_fly=False))
        self.assertFalse(check.missed(39.5, ready_to_fly=False))

    def test_no_release_never_misses(self):
        check = ReleaseCheck(None)
        self.assertFalse(check.missed(1000.0, ready_to_fly=False))


class ModeHoldTest(unittest.TestCase):
    def test_held_only_after_an_unbroken_stay_in_the_mode(self):
        hold = ModeHold("STABILIZED")
        self.assertFalse(hold.held(2.5, "LOITER"))
        self.assertFalse(hold.held(3.0, "STABILIZED"))
        self.assertFalse(hold.held(3.0 + MODE_HOLD_S / 2, "STABILIZED"))
        self.assertTrue(hold.held(3.0 + MODE_HOLD_S, "STABILIZED"))
        # Held once is held for good: the next mode does not start it over.
        self.assertTrue(hold.held(6.0, "TAKEOFF"))

    def test_leaving_the_mode_early_starts_the_stay_over(self):
        hold = ModeHold("STABILIZED")
        self.assertFalse(hold.held(3.0, "STABILIZED"))
        self.assertFalse(hold.held(3.5, "LOITER"))
        self.assertFalse(hold.held(4.0, "STABILIZED"))
        self.assertFalse(hold.held(3.0 + MODE_HOLD_S, "STABILIZED"))
        self.assertTrue(hold.held(4.0 + MODE_HOLD_S, "STABILIZED"))


if __name__ == "__main__":
    unittest.main()
