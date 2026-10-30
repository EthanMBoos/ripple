"""The flight boundary, published from Python: the built-in `Boundary`
interaction, whose one message the Rust `Straight` autonomy reads.

A message a Rust plugin subscribes to arrives as its type when the value has
the shape the run record shows for it: `BoundaryRegion` is
`{"center_world_m": [x, y, z], "lengths_m": [x, y, z]}`. A value of another
shape stops the run in the network phase, naming the channel.
"""


class Boundary:
    def __init__(self, center_m=(0.0, 0.0, 0.0), lengths_m=(1.0, 1.0, 1.0),
                 network="GlobalNetwork", bad_shape=False):
        if not all(length > 0.0 for length in lengths_m):
            raise ValueError("Boundary lengths must be positive")
        self.region = {"center_world_m": list(center_m), "lengths_m": list(lengths_m)}
        if bad_shape:   # for the test of what a wrong shape does
            self.region = {"center_world_m": "nowhere"}
        self.network = network
        self.published = False

    def step(self, ctx):
        # Published once, as the Rust plugin does.
        if not self.published:
            ctx.publish(self.network, "Boundary", self.region)
            self.published = True
