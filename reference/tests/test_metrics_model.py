import math
import unittest

import metrics_model as model


def delivery(t, topic, ids):
    return {"kind": "delivery", "t": t, "topic": topic, "ids": ids,
            "from": [None, "SimControl"], "to": [None, "SimpleCollisionMetrics"]}


def belief(entity_id, team):
    return {"kind": "belief", "t": 0.0, "id": entity_id, "team": team}


class CppMetricsModelTests(unittest.TestCase):
    def test_survivors_are_credited_until_the_last_removal(self):
        records = [belief(1, 1), belief(2, 2), delivery(0.0, "EntityGenerated", [1]),
                   delivery(0.0, "EntityGenerated", [2]), delivery(0.4, "EntityRemoved", [2])]
        rows = model.predict(records, 0.0, 29.9, (1.0, -1.0, -1.0))
        self.assertAlmostEqual(rows[1][2], 0.4)  # Not 29.9: the window ends at 0.4.
        self.assertAlmostEqual(rows[2][2], 0.4)

    def test_team_first_generated_later_gets_default_nan_score(self):
        records = [belief(1, 1), belief(2, 2), delivery(0.0, "EntityGenerated", [1]),
                   delivery(1.0, "EntityGenerated", [2])]
        rows = model.predict(records, 0.0, 3.0, (0.0, -1.0, -1.0))
        self.assertEqual(rows[1][3], 1.0)
        self.assertTrue(math.isnan(rows[2][0]) and math.isinf(rows[2][3]))

    def test_entity_removed_before_the_first_tick_counts_as_a_survivor(self):
        records = [belief(1, 1), belief(2, 1), delivery(0.0, "EntityGenerated", [1]),
                   delivery(0.0, "EntityGenerated", [2]), delivery(0.0, "EntityRemoved", [1]),
                   delivery(0.0, "TeamCollision", [1, 2])]
        rows = model.predict(records, 0.0, 5.0, (0.0, -1.0, -1.0))
        self.assertAlmostEqual(rows[1][2], 10.0)  # End equals start, so both fly 5 s.
        self.assertEqual(rows[1][0], -2.0)


    def test_corrupted_rust_summaries_are_not_explained(self):
        import tempfile
        from pathlib import Path
        records = [belief(1, 1), belief(2, 2), delivery(0.0, "EntityGenerated", [1]),
                   delivery(0.0, "EntityGenerated", [2]), delivery(0.4, "EntityRemoved", [2]),
                   delivery(0.4, "NonTeamCollision", [1, 2])]
        weights = (0.0, -1.0, -1.0)

        def csv(rows):
            lines = [",".join(model.COLUMNS)]
            lines += [",".join([str(team)] + [f"{value:.6f}" for value in values])
                      for team, values in sorted(rows.items())]
            return "\n".join(lines) + "\n"

        rust = model.predict(records, 0.0, 29.9, weights, rule="rust")
        cpp = model.predict(records, 0.0, 29.9, weights)
        self.assertNotEqual(rust, cpp)  # survivor flight time differs
        with tempfile.TemporaryDirectory() as directory:
            cpp_path, rust_path = Path(directory) / "cpp.csv", Path(directory) / "rust.csv"
            cpp_path.write_text(csv(cpp))
            rust_path.write_text(csv(rust))
            self.assertTrue(model.counts_match(cpp_path, rust_path))
            self.assertTrue(model.matches(rust, rust_path) and model.matches(cpp, cpp_path))
            rust[1][0] = 123456789.0  # a wrong score
            rust_path.write_text(csv(rust))
            self.assertFalse(model.matches(model.predict(records, 0.0, 29.9, weights, "rust"),
                                           rust_path))
            rust[1][1] = 987654321.0  # a wrong entity count
            rust_path.write_text(csv(rust))
            self.assertFalse(model.counts_match(cpp_path, rust_path))


if __name__ == "__main__":
    unittest.main()
