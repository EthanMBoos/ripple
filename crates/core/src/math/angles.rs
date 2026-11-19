//! Angle wrapping with the reference positive-pi boundary.
use std::f64::consts::{PI, TAU};

pub(crate) fn angle_2pi(angle_rad: f64) -> f64 {
    let wrapped_rad = angle_rad % TAU;
    if wrapped_rad < 0.0 {
        wrapped_rad + TAU
    } else {
        wrapped_rad
    }
}

pub(crate) fn angle_pi(angle_rad: f64) -> f64 {
    let wrapped_rad = angle_2pi(angle_rad);
    if wrapped_rad > PI {
        wrapped_rad - TAU
    } else {
        wrapped_rad
    }
}
