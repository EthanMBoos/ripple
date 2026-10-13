from types import SimpleNamespace
import unittest

import mpe_parity


def episode(steps, velocity):
    """Three agents standing still, apart, except for the recorded velocity."""
    agents = [[-0.5, 0.0], [0.0, 0.0], [0.5, 0.0]]
    return dict(seed=0, agents=agents, positions=[agents] * steps,
                velocities=[[velocity] * 3] * steps, rewards=[[0.0] * 3] * steps)


def frames(count):
    return [SimpleNamespace(entities={index + 1: SimpleNamespace(position=[x, 0.0, 0.0],
                                                                 velocity=[0.0, 0.0, 0.0])
                                      for index, x in enumerate([-0.5, 0.0, 0.5])})
            for _ in range(count)]


class MpeParityTests(unittest.TestCase):
    def test_matching_states_are_bit_identical(self):
        result = mpe_parity.compare(episode(mpe_parity.STEPS, [0.0, 0.0]),
                                    frames(mpe_parity.STEPS + 1), 0.0)
        self.assertTrue(result["bit_identical"])
        self.assertFalse(result["contact"])

    def test_a_last_bit_difference_is_reported(self):
        result = mpe_parity.compare(episode(mpe_parity.STEPS, [1e-17, 0.0]),
                                    frames(mpe_parity.STEPS + 1), 0.0)
        self.assertFalse(result["bit_identical"])
        self.assertEqual(result["largest_state_error"], 1e-17)

    def test_a_short_run_cannot_pass(self):
        with self.assertRaises(ValueError):
            mpe_parity.compare(episode(mpe_parity.STEPS, [0.0, 0.0]), frames(mpe_parity.STEPS), 0.0)


if __name__ == "__main__":
    unittest.main()
