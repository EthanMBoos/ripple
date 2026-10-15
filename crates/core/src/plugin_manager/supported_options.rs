//! Selected options have direct checks, not a second plugin validation framework.
use crate::plugin::{
    Plugin, PluginParams,
    autonomy::{Straight, WaypointFollower},
    controller::SimpleAircraftControllerPid,
    interaction::{Boundary, GroundCollision, WaypointBroadcast},
    motion::SingleIntegrator,
};

#[test]
fn options_ripple_does_not_have_are_unknown_keys() {
    let yaml = PluginParams::yaml;
    for error in [
        Straight::configure(&yaml("generate_entities: false")).err(),
        SimpleAircraftControllerPid::configure(&yaml("use_roll: false")).err(),
        SingleIntegrator::configure(&yaml("override_heading: false")).err(),
        Boundary::configure(&yaml("show_boundary: false")).err(),
        GroundCollision::configure(&yaml("remove_on_collision: true")).err(),
    ] {
        let error = format!("{:#}", error.expect("an unknown key is an error"));
        assert!(error.contains("unknown field"), "{error}");
    }
}

#[test]
fn boundary_and_ground_reject_bad_values() {
    assert!(Boundary::configure(&PluginParams::yaml("lengths_m: [10, 0, 10]")).is_err());
    assert!(GroundCollision::configure(&PluginParams::yaml("team: invalid")).is_err());
}

#[test]
fn routes_require_finite_points_positive_motion_and_a_complete_update() {
    for params in [
        "waypoints_m: []",
        "waypoints_m: [[1, 2]]",
        "waypoints_m: [[.nan, 0, 0]]",
        "speed_mps: 0",
        "arrival_radius_m: -1",
    ] {
        assert!(
            WaypointFollower::configure(&PluginParams::yaml(params)).is_err(),
            "{params}"
        );
    }
    for params in ["update_at_s: 1", "update_waypoints_m: [[1, 2, 3]]"] {
        assert!(WaypointBroadcast::configure(&PluginParams::yaml(params)).is_err());
    }
}
