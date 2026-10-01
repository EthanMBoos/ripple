"""Whether ArduPlane was ready when a catapult launch released the aircraft.

Kept apart from session.py so it can be tested without pymavlink:
python3 -m unittest discover -s crates/ardupilot/docker
"""

# ArduPlane must be armed in its mode this long before the release.
MARGIN_S = 1.0


class ReleaseCheck:
    """Checked once, at the release time less the margin, by ArduPilot's clock.
    After that a disarm is normal: ArduPlane disarms itself after landing."""

    def __init__(self, release_s):
        self.release_s = release_s
        self.done = release_s is None

    def missed(self, ardupilot_s, ready_to_fly):
        """True once, if ArduPlane was not ready to fly at the deadline."""
        if self.done or ardupilot_s < self.release_s - MARGIN_S:
            return False
        self.done = True
        return not ready_to_fly
