"""A metrics plugin that counts the reports the `Reporter` autonomies send,
in Python.

`subscriptions` names the channels, as `(network, topic)` pairs; each step,
`ctx.messages[(network, topic)]` lists what arrived since the last step, each
with `sender`, `sent_at_s`, `delivered_at_s`, and `value`.
"""


class ReportCount:
    HEADERS = ["reports", "farthest_x_m", "min_delay_s", "max_delay_s"]
    subscriptions = [("GlobalNetwork", "Report")]

    def __init__(self):
        self.reports = {}  # by team id: [count, farthest x]
        self.min_delay_s, self.max_delay_s = float("inf"), float("-inf")

    def step(self, ctx):
        for message in ctx.messages[("GlobalNetwork", "Report")]:
            team_id = ctx.entity_teams[message.sender.entity_id]
            count = self.reports.setdefault(team_id, [0, float("-inf")])
            count[0] += 1
            count[1] = max(count[1], message.value["position"][0])
            delay_s = message.delivered_at_s - message.sent_at_s
            self.min_delay_s = min(self.min_delay_s, delay_s)
            self.max_delay_s = max(self.max_delay_s, delay_s)

    def report(self, time_s):
        return {"headers": self.HEADERS,
                "teams": {team_id: {"score": count,
                                    "values": {"reports": count, "farthest_x_m": farthest,
                                               "min_delay_s": self.min_delay_s,
                                               "max_delay_s": self.max_delay_s}}
                          for team_id, (count, farthest) in sorted(self.reports.items())}}
