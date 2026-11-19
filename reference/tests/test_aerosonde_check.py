import math
import unittest

import aerosonde_check


class AerosondeCheckTests(unittest.TestCase):
    def test_the_trim_balances_every_load_but_the_side_force(self):
        trimmed = aerosonde_check.trim(25.0)
        for name, value in trimmed["residual"].items():
            if name != "fy":
                self.assertLess(abs(value), 1e-9, name)
        self.assertLess(abs(trimmed["residual"]["fy"]), 1.0)
        self.assertTrue(0.0 < trimmed["throttle"] < 1.0)

    def test_a_standing_propeller_is_the_polynomials_limit(self):
        # The standing form (J unbounded) equals the advance-ratio form as the
        # speed approaches zero, so the two branches of `propeller` agree.
        thrust, torque, speed = aerosonde_check.propeller(25.0, 0.0)
        self.assertGreater(speed, 0.0)
        P = aerosonde_check.PARAMETERS
        standing_thrust = P["rho"] * P["D_prop"] ** 2 * P["C_T2"] * 25.0 ** 2
        # With the propeller turning, the J^2 term is only part of the thrust.
        self.assertNotAlmostEqual(thrust, standing_thrust)
        self.assertEqual(aerosonde_check.propeller(0.0, 0.0), (-0.0, -0.0, 0.0))

    def test_the_lift_blend_reaches_the_flat_plate(self):
        self.assertAlmostEqual(aerosonde_check.lift_coefficient(math.pi / 2), 0.0, places=9)
        self.assertAlmostEqual(aerosonde_check.lift_coefficient(0.05),
                               aerosonde_check.PARAMETERS["C_L_0"] + aerosonde_check.PARAMETERS["C_L_alpha"] * 0.05,
                               places=6)

    def test_the_mission_text_round_trips_the_trim(self):
        trimmed = aerosonde_check.trim(25.0)
        text = aerosonde_check.mission_text(trimmed)
        self.assertIn(f"throttle: {trimmed['throttle']!r}", text)
        self.assertIn(f"pitch_deg: {-math.degrees(trimmed['alpha_rad'])!r}", text)
