"""Every message reaches every subscriber, in Python: the built-in `GlobalNetwork`.

A network answers `route(link)` for each message on its way to one
subscriber: `None` drops it, a number delivers it after that many seconds.
"""


class GlobalNetwork:
    def __init__(self, delay_s=0.0):
        if not delay_s >= 0.0:
            raise ValueError("GlobalNetwork delay_s must be finite and nonnegative")
        self.delay_s = delay_s

    def route(self, link):
        return self.delay_s
