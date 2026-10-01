import unittest

from release_check import ReleaseCheck


class ReleaseCheckTest(unittest.TestCase):
    def test_launch_flight_landing_and_disarm_is_not_a_miss(self):
        check = ReleaseCheck(40.0)
        # Booting, not yet armed: fine before the deadline.
        self.assertFalse(check.missed(10.0, ready_to_fly=False))
        # Armed in its mode by 39 s, then flies.
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


if __name__ == "__main__":
    unittest.main()
