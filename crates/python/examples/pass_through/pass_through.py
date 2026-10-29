"""The default estimators, in Python: copy the latest reading into belief.

`PassThroughState` fills the localization slot from a state reading, and
`PassThroughContacts` the tracking slot from a contacts reading, like the
built-in `PassThrough` pair. With a perfect sensor, belief equals truth at
the next decision; between a slow sensor's readings, the last one stays.
"""


class PassThroughState:
    inputs = {"state": "state"}

    def initialize(self, ctx):
        # The initial belief must come from a reading; there is no fallback to truth.
        reading = ctx.inputs["state"]
        if reading is None:
            raise RuntimeError("the state sensor gave no reading at startup")
        return reading.value

    def step(self, ctx):
        reading = ctx.inputs["state"]
        if reading is not None and reading.fresh:
            return reading.value
        return None


class PassThroughContacts:
    inputs = {"contacts": "contacts"}

    def initialize(self, ctx):
        # No reading yet means no known contacts.
        return self.step(ctx)

    def step(self, ctx):
        reading = ctx.inputs["contacts"]
        if reading is not None and reading.fresh:
            return reading.value
        return None
