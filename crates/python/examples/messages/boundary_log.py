"""A metrics plugin that records the boundary a Rust interaction published,
in Python: a Rust message arrives as the dict the run record shows for it.
"""


class BoundaryLog:
    HEADERS = ["boundaries", "center_z_m", "length_x_m", "delivered_at_s"]
    subscriptions = [("GlobalNetwork", "Boundary")]

    def __init__(self):
        self.count, self.region, self.delivered_at_s = 0, None, float("nan")

    def step(self, ctx):
        for message in ctx.messages[("GlobalNetwork", "Boundary")]:
            self.count += 1
            self.region = message.value
            self.delivered_at_s = message.delivered_at_s

    def report(self, time_s):
        values = {"boundaries": self.count,
                  "center_z_m": self.region["center_world_m"][2] if self.region else float("nan"),
                  "length_x_m": self.region["lengths_m"][0] if self.region else float("nan"),
                  "delivered_at_s": self.delivered_at_s}
        return {"headers": self.HEADERS, "teams": {0: {"score": self.count, "values": values}}}
