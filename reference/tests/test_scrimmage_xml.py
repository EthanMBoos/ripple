import unittest
import xml.etree.ElementTree as ET

import scrimmage_xml
from tests.fixtures import plugin, scenario


class ScrimmageXmlTests(unittest.TestCase):
    def test_every_entity_value_is_written(self):
        entity = scrimmage_xml.mission(scenario()).find("entity")
        # SCRIMMAGE's own defaults (variance 100 m²) must never apply.
        for tag, value in [("team_id", "1"), ("count", "64"), ("x", "23.0"),
                           ("variance_x", "0.0"), ("variance_y", "0.0"), ("vx", "0.0"),
                           ("heading", "0.0"), ("use_variance_all_ents", "false"),
                           ("color", "255 255 255")]:
            self.assertEqual(entity.findtext(tag), value, tag)
        self.assertIsNone(entity.find("generate_rate"))

    def test_plugins_keep_their_parameters_order_and_names(self):
        root = scrimmage_xml.mission(scenario())
        self.assertEqual([(node.text, node.get("name")) for node in root.findall("network")],
                         [("GlobalNetwork", None), ("SphereNetwork", "CommsNetwork")])
        entity = root.find("entity")
        self.assertEqual(entity.find("autonomy").get("speed"), "27")
        self.assertEqual(entity.find("controller").get("heading_pid"), "1 0.01 2 9")

    def test_noise_is_written_in_scrimmage_keys_with_zero_for_unset_fields(self):
        sensor = scrimmage_xml.mission(scenario()).find("entity/sensor")
        self.assertEqual(sensor.get("pos_noise_2"), "0 3")
        self.assertEqual(sensor.get("vel_noise_0"), "0 0")
        self.assertEqual(sensor.get("orient_noise_1"), "0 0")
        self.assertIsNone(sensor.get("position_stddev_m"))

    def test_spawning_routes_and_end_conditions(self):
        mission = scenario()
        group = mission["entities"][0]
        group["spawn"] = {"rate_hz": 0.5, "batch_size": 2, "start_s": 1.0, "time_stddev_s": 0.1}
        group["autonomy"][0]["params"]["waypoints"] = [[0, 0, 200], [100, 0, 200]]
        mission["end_conditions"]["all_dead"] = True
        root = scrimmage_xml.mission(mission)
        self.assertEqual(root.findtext("entity/generate_rate"), "0.5")
        self.assertEqual(root.findtext("entity/generate_time_variance"), "0.1")
        self.assertEqual(root.find("entity/autonomy").get("waypoints"), "0,0,200;100,0,200")
        self.assertEqual(root.findtext("end_condition"), "all_dead, time")

    def test_a_named_airframe_is_written_out_under_the_missions_own_values(self):
        mission = scenario()
        mission["entities"][0]["motion_model"] = plugin(
            "FixedWing6DOF", airframe="rascal", thrust_max=30)
        motion = scrimmage_xml.mission(mission).find("entity/motion_model")
        self.assertEqual(motion.get("mass"), "5.91")
        self.assertEqual(motion.get("delta_elevator_max"), "0.3")
        self.assertEqual(motion.get("thrust_max"), "30")
        # An airframe SCRIMMAGE's defaults already match is left as named.
        mission["entities"][0]["motion_model"] = plugin("FixedWing6DOF", airframe="a4d")
        motion = scrimmage_xml.mission(mission).find("entity/motion_model")
        self.assertEqual(motion.attrib, {"airframe": "a4d"})

    def test_structured_vehicle_values_become_scrimmage_rows(self):
        mission = scenario()
        mission["entities"][0]["motion_model"] = plugin(
            "Multirotor", inertia_kgm2=[[1, 0, 0], [0, 2, 0], [0, 0, 3]],
            rotors=[{"direction": "ccw", "position_flu_m": [0, -0.175, 0]},
                    {"direction": "cw", "position_flu_m": [0.1, 0, 0.02], "tilt_deg": [0, 5, 0]}])
        motion = scrimmage_xml.mission(mission).find("entity/motion_model")
        self.assertEqual(motion.get("inertia_matrix"), "[1 0 0] [0 2 0] [0 0 3]")
        self.assertEqual(motion.get("rotor_config"),
                         "[CCW 0 -0.175 0 0 0 0] [CW 0.1 0 0.02 0 5 0]")
        self.assertIsNone(motion.get("rotors"))

    def test_parameters_take_scrimmage_keys_sorted_by_them(self):
        mission = scenario()
        mission["entities"][0]["motion_model"] = plugin(
            "FixedWing6DOF", airframe="a4d", c_roll_p=-0.4, wind_mps=[1, 2, 0], mass_kg=9)
        motion = scrimmage_xml.mission(mission).find("entity/motion_model")
        self.assertEqual(list(motion.attrib), ["C_LP", "airframe", "mass", "wind_E", "wind_N"])
        self.assertEqual((motion.get("C_LP"), motion.get("wind_N")), ("-0.4", "2"))
        mission["entities"][0]["controller"] = [plugin("SimpleAircraftControllerPID", altitude_pid={
            "kp": 1, "ki": 0, "kd": 0.8, "integrate_within_m": 1})]
        controller = scrimmage_xml.mission(mission).find("entity/controller")
        self.assertEqual(controller.attrib, {"alt_pid": "1 0 0.8 1"})

    def test_estimators_and_terrain_are_errors(self):
        mission = scenario()
        mission["entities"][0]["localization"]["name"] = "DeadReckoning"
        with self.assertRaisesRegex(ValueError, "no localization"):
            scrimmage_xml.mission(mission)
        mission = scenario()
        mission["terrain"] = "/terrain.yaml"
        with self.assertRaisesRegex(ValueError, "terrain"):
            scrimmage_xml.mission(mission)


if __name__ == "__main__":
    unittest.main()
