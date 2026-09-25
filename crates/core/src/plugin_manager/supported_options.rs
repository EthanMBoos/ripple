//! Selected options have direct checks, not a second plugin validation framework.
use crate::{
    Params,
    plugin::{
        Plugin, PluginParams,
        autonomy::{Straight, WaypointFollower},
        controller::SimpleAircraftControllerPid,
        interaction::{Boundary, GroundCollision, WaypointBroadcast},
        motion::SingleIntegrator,
    },
};

#[test]
fn excluded_straight_spawning_and_camera_options_are_errors() {
    for key in [
        "generate_entities",
        "show_camera_images",
        "save_camera_images",
    ] {
        let params = Params::from([(key.into(), "true".into())]);
        let error = Straight::configure(&PluginParams::new(&params))
            .err()
            .expect("unsupported option");
        assert!(error.to_string().contains(key));
    }
}

#[test]
fn unported_controller_and_motion_modes_are_errors() {
    for key in ["use_roll", "use_glide_slope"] {
        let params = Params::from([(key.into(), "true".into())]);
        assert!(SimpleAircraftControllerPid::configure(&PluginParams::new(&params)).is_err());
    }
    let params = Params::from([("override_heading".into(), "true".into())]);
    assert!(SingleIntegrator::configure(&PluginParams::new(&params)).is_err());
}

#[test]
fn boundary_and_ground_reject_unimplemented_geometry_and_response() {
    for (key, value) in [
        ("type", "sphere"),
        ("rpy", "0,0,1"),
        ("lengths", "10,0,10"),
        ("show_boundary", "true"),
    ] {
        let params = Params::from([(key.into(), value.into())]);
        assert!(
            Boundary::configure(&PluginParams::new(&params)).is_err(),
            "{key}"
        );
    }
    for (key, value) in [
        ("remove_on_collision", "false"),
        ("ground_collision_altitude", "100"),
        ("team", "invalid"),
    ] {
        let params = Params::from([(key.into(), value.into())]);
        assert!(
            GroundCollision::configure(&PluginParams::new(&params)).is_err(),
            "{key}"
        );
    }
}

#[test]
fn routes_require_finite_points_positive_motion_and_a_complete_update() {
    for (key, value) in [
        ("waypoints", ""),
        ("waypoints", "1,2"),
        ("waypoints", "NaN,0,0"),
        ("speed", "0"),
        ("arrival_radius_m", "-1"),
    ] {
        let params = Params::from([(key.into(), value.into())]);
        assert!(
            WaypointFollower::configure(&PluginParams::new(&params)).is_err(),
            "{key}={value}"
        );
    }
    for (key, value) in [("update_at_s", "1"), ("update_waypoints", "1,2,3")] {
        let params = Params::from([(key.into(), value.into())]);
        assert!(WaypointBroadcast::configure(&PluginParams::new(&params)).is_err());
    }
}
