"""An autonomy that flies straight and reports its believed position every
tick, in Python: a message a Python metrics plugin (`report_count.py`) counts.

`ctx.publish(network, topic, value)` queues a message that is sent when the
call returns; the value is plain data, here a dict.
"""


class Reporter:
    ports = {"outputs": {"velocity_x": ("m/s", "world"),
                         "velocity_y": ("m/s", "world"),
                         "velocity_z": ("m/s", "world")}}

    def __init__(self, velocity_mps=(1.0, 0.0, 0.0), topic="Report"):
        self.velocity_mps = tuple(velocity_mps)
        self.topic = topic

    def step(self, ctx):
        ctx.publish("GlobalNetwork", self.topic,
                    {"tick": ctx.time.tick, "position": list(ctx.state.position)})
        vx, vy, vz = self.velocity_mps
        return {"velocity_x": vx, "velocity_y": vy, "velocity_z": vz}
