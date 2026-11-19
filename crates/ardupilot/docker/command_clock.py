"""When the session sends its periodic commands (mode, arm, RC refresh).

Kept apart from session.py so it can be tested without pymavlink:
python3 -m unittest discover -s crates/ardupilot/docker
"""

# Seconds of ArduPilot time between commands. Well under ArduPilot's 3 s
# RC-override timeout (RC_OVERRIDE_TIME), so the centred sticks never lapse.
COMMAND_INTERVAL_S = 0.5


class CommandClock:
    """Paces commands by ArduPilot's own clock (SYSTEM_TIME), which follows
    the simulation. Wall time would be wrong: at SITL's speedup, 0.5 s of wall
    time can be tens of simulated seconds, longer than the whole arming hold."""

    def __init__(self):
        self.last_s = None

    def due(self, ardupilot_s):
        """True when a command is due at ArduPilot time `ardupilot_s`."""
        if self.last_s is not None and ardupilot_s - self.last_s < COMMAND_INTERVAL_S:
            return False
        self.last_s = ardupilot_s
        return True
