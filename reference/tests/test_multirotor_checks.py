from types import SimpleNamespace
import unittest

import crazyflow_check
import multirotor_control


def frame(time_s, velocity, position=(0.0, 0.0, 10.0)):
    entity = SimpleNamespace(position=list(position), velocity=list(velocity),
                             orientation=[1.0, 0.0, 0.0, 0.0], angular_velocity=[0.0, 0.0, 0.0])
    return SimpleNamespace(time_s=time_s, entities={1: entity})


class MultirotorControlTests(unittest.TestCase):
    def test_sideways_drift_does_not_count_as_settled(self):
        # Commanded (1, 0, 0) but flying (1, 5, 0): on target along the
        # command, 5 m/s off sideways.
        frames = [frame(0.01 * step, (1.0, 5.0, 0.0)) for step in range(1001)]
        result = multirotor_control.analyze(frames, ["crazyflie_forward"])["crazyflie_forward"]
        self.assertFalse(result["settled"])

    def test_tracking_the_command_settles(self):
        frames = [frame(0.01 * step, (min(1.0, 0.01 * step), 0.0, 0.0)) for step in range(1001)]
        result = multirotor_control.analyze(frames, ["crazyflie_forward"])["crazyflie_forward"]
        self.assertTrue(result["settled"])


class CrazyflowCheckTests(unittest.TestCase):
    def test_an_empty_reference_cannot_pass(self):
        frames = [frame(crazyflow_check.DT_S * (step + 1), (0.0, 0.0, 0.0)) for step in range(3001)]
        with self.assertRaises(ValueError):
            crazyflow_check.compare("no-drag", frames, [])


if __name__ == "__main__":
    unittest.main()
