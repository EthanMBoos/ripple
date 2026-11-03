import math
import unittest

import jsbsim_check


def sample(time_s, position=(0.0, 0.0, 300.0)):
    return dict(time_s=time_s, position_enu_m=list(position), velocity_enu_mps=[0.0, 20.0, 0.0],
                euler_ned_rad=[0.0, 0.0, 0.0])


class JsbsimCheckTests(unittest.TestCase):
    def test_ripple_attitude_reads_as_jsbsim_euler_angles(self):
        # Ripple's from_euler (ZYX, world from body, scalar first) with
        # heading 90 (north) and pitch_deg -5: nose up 5 degrees heading north.
        roll, pitch, yaw = 0.0, math.radians(-5.0), math.radians(90.0)
        cr, sr, cp, sp = math.cos(roll / 2), math.sin(roll / 2), math.cos(pitch / 2), math.sin(pitch / 2)
        cy, sy = math.cos(yaw / 2), math.sin(yaw / 2)
        q = (cr * cp * cy + sr * sp * sy, sr * cp * cy - cr * sp * sy,
             cr * sp * cy + sr * cp * sy, cr * cp * sy - sr * sp * cy)
        phi, theta, psi = jsbsim_check.ned_euler(q)
        self.assertAlmostEqual(phi, 0.0, places=12)
        self.assertAlmostEqual(theta, math.radians(5.0), places=12)
        self.assertAlmostEqual(psi, 0.0, places=12)

    def test_a_short_reference_cannot_pass(self):
        steps = round(jsbsim_check.END_S / jsbsim_check.DT_S)
        full = [sample(step * jsbsim_check.DT_S) for step in range(steps + 1)]
        with self.assertRaises(ValueError):
            jsbsim_check.compare_paths("short", full, full[:-1])
        with self.assertRaises(ValueError):
            jsbsim_check.compare_paths("missing", full[1:], full)
        result = jsbsim_check.compare_paths("same", full, full)
        self.assertEqual(result["samples_compared"], steps + 1)
        self.assertEqual(result["largest"]["position_m"], 0.0)

    def test_fixture_comparison_checks_shape_as_well_as_numbers(self):
        fixture = dict(jsbsim="jsbsim==1.3.3", samples=[dict(force=[1.0, 2.0], ok=True)])
        same = dict(jsbsim="jsbsim==1.3.3", samples=[dict(force=[1.0, 2.0 + 2e-13], ok=True)])
        self.assertAlmostEqual(jsbsim_check.largest_relative_difference(fixture, same), 1e-13)
        for other in (dict(fixture, jsbsim="jsbsim==1.3.4"),
                      dict(jsbsim=fixture["jsbsim"], samples=[dict(forces=[1.0, 2.0], ok=True)]),
                      dict(fixture, samples=[dict(force=[1.0], ok=True)]),
                      dict(fixture, samples=[dict(force=[1.0, 2.0], ok=1)])):
            with self.assertRaises(ValueError):
                jsbsim_check.largest_relative_difference(fixture, other)

    def test_the_glides_pass_only_within_the_tolerance(self):
        def glide(largest_m):
            return dict(jsbsim_form=dict(largest=dict(position_m=largest_m)))
        self.assertTrue(jsbsim_check.glides_pass(dict(held=glide(0.02), rudder=glide(0.19))))
        self.assertFalse(jsbsim_check.glides_pass(dict(held=glide(0.02), rudder=glide(0.21))))


if __name__ == "__main__":
    unittest.main()
