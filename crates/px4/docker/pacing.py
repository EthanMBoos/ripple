"""When the session sends its commands, and whether PX4 was ready at release.

Kept apart from session.py so it can be tested without pymavlink:
python3 -m unittest discover -s crates/px4/docker
The ArduPilot session has the first two classes (crates/ardupilot/docker).
"""

# Seconds of PX4 time between commands.
COMMAND_INTERVAL_S = 0.5

# PX4 must be armed in its mode this long before the release.
MARGIN_S = 1.0

# PX4 sits in Stabilized this long before the mode to fly, so its height
# controller, off in Stabilized, starts afresh: it resets after 1 s unused.
MODE_HOLD_S = 2.0


class CommandClock:
    """Paces commands by PX4's own clock (SYSTEM_TIME), which follows the
    simulation. Wall time would be wrong: at the SITL's speed, 0.5 s of wall
    time can be tens of simulated seconds, longer than the whole arming hold."""

    def __init__(self):
        self.last_s = None

    def due(self, px4_s):
        """True when a command is due at PX4 time `px4_s`."""
        if self.last_s is not None and px4_s - self.last_s < COMMAND_INTERVAL_S:
            return False
        self.last_s = px4_s
        return True


class ReleaseCheck:
    """Checked once, at the release time less the margin, by PX4's clock.
    After that a disarm is normal: PX4 disarms itself after landing."""

    def __init__(self, release_s):
        self.release_s = release_s
        self.done = release_s is None

    def missed(self, px4_s, ready_to_fly):
        """True once, if PX4 was not ready to fly at the deadline."""
        if self.done or px4_s < self.release_s - MARGIN_S:
            return False
        self.done = True
        return not ready_to_fly


class ModeHold:
    """PX4 must have been in `mode` for MODE_HOLD_S of its time, unbroken,
    once; after that the session moves on to the mode to fly."""

    def __init__(self, mode):
        self.mode = mode
        self.since_s = None
        self.done = False

    def held(self, px4_s, mode):
        """True from the moment PX4 has reported `mode` for MODE_HOLD_S."""
        if self.done:
            return True
        if mode != self.mode:
            self.since_s = None
            return False
        if self.since_s is None:
            self.since_s = px4_s
        self.done = px4_s - self.since_s >= MODE_HOLD_S
        return self.done
