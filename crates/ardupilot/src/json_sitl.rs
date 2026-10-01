//! ArduPilot's JSON SITL wire format (libraries/SITL/SIM_JSON.cpp).
//!
//! SITL sends a little-endian binary servo packet; the simulator answers each
//! new frame with one line of JSON. The exchange rules are in ardupilot_bridge.rs.

use anyhow::{Result, bail};
use serde_json::json;

use crate::sitl_state::SitlSample;

/// Magic number of the 16-channel servo packet. The 32-channel packet
/// (`SERVO_32_ENABLE=1`, magic 29569) is not supported.
const MAGIC_16: u16 = 18458;
const PACKET_16_BYTES: usize = 2 + 2 + 4 + 16 * 2;

/// One servo frame from SITL.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServoFrame {
    /// SITL's current physics-rate estimate; advisory only.
    pub frame_rate_hz: u16,
    pub frame_count: u32,
    /// Servo outputs in microseconds; index 0 is SERVO1.
    pub pwm_us: [u16; 16],
}

/// Parse one datagram as a 16-channel servo packet.
pub fn parse_servo_packet(bytes: &[u8]) -> Result<ServoFrame> {
    if bytes.len() != PACKET_16_BYTES {
        bail!(
            "servo packet is {} bytes, expected {PACKET_16_BYTES} (16 channels; SERVO_32_ENABLE is not supported)",
            bytes.len()
        );
    }
    let u16_at = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    let magic = u16_at(0);
    if magic != MAGIC_16 {
        bail!("servo packet magic is {magic}, expected {MAGIC_16}");
    }
    let mut pwm_us = [0; 16];
    for (channel, pwm) in pwm_us.iter_mut().enumerate() {
        *pwm = u16_at(8 + 2 * channel);
    }
    Ok(ServoFrame {
        frame_rate_hz: u16_at(2),
        frame_count: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        pwm_us,
    })
}

/// The state reply for one frame. SITL needs the newline before and after.
pub fn state_reply(timestamp_s: f64, sample: &SitlSample) -> String {
    let vector = |v: &scrimmage_core::Vec3| [v.x, v.y, v.z];
    let reply = json!({
        "timestamp": timestamp_s,
        "imu": {
            "gyro": vector(&sample.gyro_frd_radps),
            "accel_body": vector(&sample.specific_force_frd_mps2),
        },
        "position": vector(&sample.position_ned_m),
        "velocity": vector(&sample.velocity_ned_mps),
        "quaternion": sample.orientation_ned_from_frd,
        "airspeed": sample.airspeed_mps,
    });
    format!("\n{reply}\n")
}

/// Build a servo packet; used by tests that stand in for SITL.
#[cfg(test)]
pub fn servo_packet(frame_count: u32, pwm_us: [u16; 16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(PACKET_16_BYTES);
    bytes.extend_from_slice(&MAGIC_16.to_le_bytes());
    bytes.extend_from_slice(&400u16.to_le_bytes());
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    for pwm in pwm_us {
        bytes.extend_from_slice(&pwm.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrimmage_core::Vec3;

    #[test]
    fn servo_packet_round_trips() -> Result<()> {
        let mut pwm_us = [1500; 16];
        pwm_us[2] = 1100;
        let frame = parse_servo_packet(&servo_packet(42, pwm_us))?;
        assert_eq!(frame.frame_count, 42);
        assert_eq!(frame.frame_rate_hz, 400);
        assert_eq!(frame.pwm_us, pwm_us);
        Ok(())
    }

    #[test]
    fn wrong_size_or_magic_is_rejected() {
        assert!(parse_servo_packet(&[0; 10]).is_err());
        let mut bytes = servo_packet(0, [1500; 16]);
        bytes[0] = 0;
        assert!(parse_servo_packet(&bytes).is_err());
    }

    #[test]
    fn reply_is_one_json_line_between_newlines() -> Result<()> {
        let sample = SitlSample {
            position_ned_m: Vec3::new(1.0, 2.0, -3.0),
            velocity_ned_mps: Vec3::new(0.0, 20.0, 0.0),
            orientation_ned_from_frd: [1.0, 0.0, 0.0, 0.0],
            gyro_frd_radps: Vec3::zeros(),
            specific_force_frd_mps2: Vec3::new(0.0, 0.0, -9.81),
            airspeed_mps: 20.0,
        };
        let reply = state_reply(0.25, &sample);
        assert!(reply.starts_with('\n') && reply.ends_with('\n'));
        let value: serde_json::Value = serde_json::from_str(reply.trim())?;
        assert_eq!(value["timestamp"], 0.25);
        assert_eq!(value["imu"]["accel_body"][2], -9.81);
        assert_eq!(value["position"][2], -3.0);
        assert_eq!(value["quaternion"][0], 1.0);
        assert_eq!(value["airspeed"], 20.0);
        Ok(())
    }
}
