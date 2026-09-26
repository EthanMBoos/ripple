import unittest

import traces


def belief(t, entity_id, x=0.0):
    return {"kind": "belief", "t": t, "id": entity_id, "p": [x, 0, 0], "v": [0, 0, 0],
            "q": [1, 0, 0, 0], "w": [0, 0, 0]}


def delivery(t, sender, receiver, topic="EntityGenerated", ids=None):
    record = {"kind": "delivery", "t": t, "network": "GlobalNetwork", "topic": topic,
              "from": sender, "to": receiver}
    if ids is not None:
        record["ids"] = ids
    return record


def output(t, entity_id, port, value):
    return {"kind": "output", "t": t, "id": entity_id, "plugin": "Straight",
            "port": port, "value": value}


class TraceComparisonTests(unittest.TestCase):
    def base(self):
        return [belief(0.0, 1), belief(0.0, 2),
                delivery(0.0, [0, "SimControl"], [0, "Metrics"], ids=[1]),
                delivery(0.0, [0, "SimControl"], [0, "Metrics"], ids=[2]),
                output(0.0, 1, "desired_speed", 20.0)]

    def test_identical_traces_pass_and_world_parents_normalize(self):
        rust = self.base()
        for record in rust:
            for key in ("from", "to"):
                if key in record:
                    record[key] = [None, record[key][1]]  # Rust has no world parent.
        result = traces.compare_traces(self.base(), rust)
        self.assertTrue(result.passed, result.first_mismatches)
        self.assertEqual((result.deliveries, result.beliefs, result.outputs), (2, 2, 1))

    def test_delivery_order_content_and_missing_groups_fail(self):
        swapped = self.base()
        swapped[2], swapped[3] = swapped[3], swapped[2]
        result = traces.compare_traces(self.base(), swapped)
        self.assertFalse(result.passed)
        self.assertIn("order only", result.first_mismatches[0])

        wrong_payload = self.base()
        wrong_payload[3]["ids"] = [3]
        self.assertIn("content", traces.compare_traces(self.base(), wrong_payload)
                      .first_mismatches[0])

        missing = self.base()[:2] + self.base()[4:]
        result = traces.compare_traces(self.base(), missing)
        self.assertEqual(result.mismatch_count, 1)
        self.assertIn("only in reference", result.first_mismatches[0])

    def test_value_tolerances_and_missing_outputs(self):
        moved = self.base()
        moved[0] = belief(0.0, 1, x=0.5)
        moved[4] = output(0.0, 1, "desired_speed", 20.0 + 1e-3)
        moved.append(output(0.0, 1, "desired_heading", 0.0))
        result = traces.compare_traces(self.base(), moved)
        self.assertEqual(result.max_belief_position_error_m, 0.5)
        self.assertAlmostEqual(result.max_output_error, 1e-3)
        self.assertEqual(result.max_output_error_port, "Straight.desired_speed")
        self.assertEqual(result.mismatch_count, 3)

    def test_published_payloads_compare_ids_values_and_covariance(self):
        def publication(x=0.0, cov=1.0, ids=(2,)):
            states = [{"id": i, "p": [x, 0, 0], "v": [0, 0, 0], "q": [1, 0, 0, 0],
                       "w": [0, 0, 0], "cov": [cov, 0, 0, 0, cov, 0, 0, 0, cov]} for i in ids]
            return {"kind": "publication", "t": 0.0, "topic": "ContactsWithCovariances",
                    "from": [1, "NoisyContacts"], "states": states}
        reference = self.base() + [publication()]
        self.assertTrue(traces.compare_traces(reference, self.base() + [publication()]).passed)
        result = traces.compare_traces(reference, self.base() + [publication(cov=5.0)])
        self.assertEqual(result.max_payload_covariance_error, 4.0)
        self.assertFalse(result.passed)
        self.assertIn("contact IDs differ", traces.compare_traces(
            reference, self.base() + [publication(ids=(3,))]).first_mismatches[0])

    def test_nonfinite_strings_decode_and_matching_nan_outputs_pass(self):
        self.assertTrue(traces.decode_numbers({"value": "NaN"})["value"] != 0)
        reference = self.base()[:4] + [output(0.0, 1, "desired_speed", float("nan"))]
        candidate = self.base()[:4] + [output(0.0, 1, "desired_speed", float("nan"))]
        self.assertTrue(traces.compare_traces(reference, candidate).passed)
        candidate[4] = output(0.0, 1, "desired_speed", 20.0)
        self.assertFalse(traces.compare_traces(reference, candidate).passed)

    def test_invalid_records_are_errors(self):
        with self.assertRaises(ValueError):
            traces.compare_traces([{"kind": "mystery", "t": 0}], [])
        # Two same-named plugins are compared in chain order, not rejected.
        pair = [output(0.0, 1, "x", 1.0), output(0.0, 1, "x", 2.0)]
        self.assertTrue(traces.compare_traces(pair, list(pair)).passed)
        self.assertFalse(traces.compare_traces(pair, pair[::-1]).passed)


if __name__ == "__main__":
    unittest.main()
