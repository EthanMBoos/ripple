//! Selected options have direct checks, not a second plugin validation framework.
use crate::plugin::{
    Plugin, PluginParams,
    autonomy::{Straight, WaypointFollower},
    controller::SimpleAircraftControllerPid,
    interaction::{Boundary, GroundCollision, WaypointBroadcast},
    motion::SingleIntegrator,
};

#[test]
fn excluded_straight_spawning_and_camera_options_are_errors() {
    for key in [
        "generate_entities",
        "show_camera_images",
        "save_camera_images",
    ] {
        let error = Straight::configure(&PluginParams::yaml(&format!("{key}: true")))
            .err()
            .expect("unsupported option");
        assert!(error.to_string().contains(key));
    }
}

#[test]
fn unported_controller_and_motion_modes_are_errors() {
    for key in ["use_roll", "use_glide_slope"] {
        let params = PluginParams::yaml(&format!("{key}: true"));
        assert!(SimpleAircraftControllerPid::configure(&params).is_err());
    }
    let params = PluginParams::yaml("override_heading: true");
    assert!(SingleIntegrator::configure(&params).is_err());
}

#[test]
fn boundary_and_ground_reject_unimplemented_geometry_and_response() {
    for params in [
        "type: sphere",
        "rpy: [0, 0, 1]",
        "lengths: [10, 0, 10]",
        "show_boundary: true",
    ] {
        assert!(
            Boundary::configure(&PluginParams::yaml(params)).is_err(),
            "{params}"
        );
    }
    for params in [
        "remove_on_collision: false",
        "ground_collision_altitude: 100",
        "team: invalid",
    ] {
        assert!(
            GroundCollision::configure(&PluginParams::yaml(params)).is_err(),
            "{params}"
        );
    }
}

#[test]
fn routes_require_finite_points_positive_motion_and_a_complete_update() {
    for params in [
        "waypoints: []",
        "waypoints: [[1, 2]]",
        "waypoints: [[.nan, 0, 0]]",
        "speed: 0",
        "arrival_radius_m: -1",
    ] {
        assert!(
            WaypointFollower::configure(&PluginParams::yaml(params)).is_err(),
            "{params}"
        );
    }
    for params in ["update_at_s: 1", "update_waypoints: [[1, 2, 3]]"] {
        assert!(WaypointBroadcast::configure(&PluginParams::yaml(params)).is_err());
    }
}
