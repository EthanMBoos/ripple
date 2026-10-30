"""Test plugins for `ctx.publish`: the value is copied when published, so
changing it afterwards changes nothing, and an integer past 64 bits is an
error at the `publish` call.
"""


class Mutating:
    ports = {"outputs": {"velocity_x": ("m/s", "world"),
                         "velocity_y": ("m/s", "world"),
                         "velocity_z": ("m/s", "world")}}

    def __init__(self, huge=False):
        self.huge = huge

    def step(self, ctx):
        value = {"count": 2**70 if self.huge else 1}
        ctx.publish("GlobalNetwork", "Count", value)
        value["count"] = -1
        return {"velocity_x": 0.0, "velocity_y": 0.0, "velocity_z": 0.0}


class Received:
    subscriptions = [("GlobalNetwork", "Count")]

    def __init__(self):
        self.counts = set()

    def step(self, ctx):
        for message in ctx.messages.get(("GlobalNetwork", "Count"), []):
            self.counts.add(message.value["count"])

    def report(self, time_s):
        return {"headers": ["smallest", "largest"],
                "teams": {1: {"score": len(self.counts),
                              "values": {"smallest": min(self.counts, default=0),
                                         "largest": max(self.counts, default=0)}}}}
