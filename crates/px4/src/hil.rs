//! PX4's simulator MAVLink API (SimulatorMavlink.cpp in PX4): the simulator
//! sends HIL_SENSOR every tick and HIL_GPS a few times a second, and PX4
//! answers with HIL_ACTUATOR_CONTROLS. PX4's clock is the time stamp of the
//! sensor message, so it runs exactly as fast as it is fed.
//!
//! Unlike ArduPilot's JSON backend, PX4 wants sensor readings, not the true
//! state, so this module makes them from a `SitlSample`: the sample's
//! specific force and rates as the IMU, the Earth's field at the home site
//! rotated into the body as the magnetometer, the standard atmosphere at the
//! aircraft's altitude as the barometer, dynamic pressure as the pitot, and a
//! flat-Earth offset from home as the GPS. Noise is added by the bridge.
//!
//! Transport: PX4 dials out to the simulator over TCP, so the bridge listens
//! and `Link` wraps the accepted connection. A thread reads messages into a
//! channel, so the bridge can wait with a timeout and never sees a half-read
//! frame.

use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mavlink::dialects::common::{
    HIL_ACTUATOR_CONTROLS_DATA, HIL_GPS_DATA, HIL_SENSOR_DATA, HilSensorUpdatedFlags, MavMessage,
    MavModeFlag,
};
use mavlink::error::MessageReadError;
use mavlink::{MavHeader, MavlinkReader, MavlinkVersion, write_v2_msg};
use nalgebra::UnitQuaternion;
use ripple_core::Vec3;
use ripple_core::plugin::PluginRandom;
use sitl::SitlSample;

/// Where local (0, 0, 0) is on the Earth.
#[derive(Clone, Copy, Debug)]
pub struct Home {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f64,
}

/// Standard atmosphere at sea level, as PX4's Gazebo plugin uses it.
const SEA_LEVEL_PRESSURE_PA: f64 = 101325.0;
const SEA_LEVEL_TEMPERATURE_K: f64 = 288.15;
const SEA_LEVEL_DENSITY_KG_PER_M3: f64 = 1.225;
const LAPSE_RATE_K_PER_M: f64 = 0.0065;
const EARTH_RADIUS_M: f64 = 6_378_137.0;

/// What the sensors read, before noise.
#[derive(Clone, Copy, Debug)]
pub struct Readings {
    /// What the accelerometer reads: (0, 0, -g) at rest, level.
    pub specific_force_frd_mps2: Vec3,
    pub rates_frd_radps: Vec3,
    /// The Earth's field in body axes (gauss).
    pub field_frd_gauss: Vec3,
    pub pressure_pa: f64,
    pub temperature_c: f64,
    /// Dynamic pressure, what a pitot reads (Pa).
    pub differential_pressure_pa: f64,
    /// Above sea level.
    pub altitude_m: f64,
}

/// Standard deviations added to the readings, per sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct Noise {
    pub rates_radps: f64,
    pub specific_force_mps2: f64,
    pub pressure_pa: f64,
    pub field_gauss: f64,
}

/// The sensor readings for one sample.
pub fn readings(sample: &SitlSample, home: Home, field_ned_gauss: Vec3) -> Readings {
    let altitude_m = home.altitude_m - sample.position_ned_m.z;
    // Standard atmosphere: temperature falls linearly, pressure and density
    // follow it (PX4's Gazebo barometer uses the same exponents).
    let temperature_k = SEA_LEVEL_TEMPERATURE_K - LAPSE_RATE_K_PER_M * altitude_m;
    let ratio = temperature_k / SEA_LEVEL_TEMPERATURE_K;
    let pressure_pa = SEA_LEVEL_PRESSURE_PA * ratio.powf(5.256);
    let density_kg_per_m3 = SEA_LEVEL_DENSITY_KG_PER_M3 * ratio.powf(4.256);
    let differential_pressure_pa = 0.5 * density_kg_per_m3 * sample.airspeed_mps.powi(2);
    // The sample's quaternion rotates body (FRD) into NED; the field is given
    // in NED, so the inverse rotation puts it in the body.
    let [w, x, y, z] = sample.orientation_ned_from_frd;
    let ned_from_frd = UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(w, x, y, z));
    let field_frd_gauss = ned_from_frd.inverse_transform_vector(&field_ned_gauss);
    Readings {
        specific_force_frd_mps2: sample.specific_force_frd_mps2,
        rates_frd_radps: sample.gyro_frd_radps,
        field_frd_gauss,
        pressure_pa,
        temperature_c: temperature_k - 273.15,
        differential_pressure_pa,
        altitude_m,
    }
}

impl Readings {
    /// The readings with Gaussian noise, each number keyed by sensor and axis.
    pub fn with_noise(self, random: &PluginRandom, noise: Noise) -> Result<Self> {
        let vector = |name: &str, value: Vec3, stddev: f64| -> Result<Vec3> {
            let key = PluginRandom::name(name);
            Ok(Vec3::new(
                random.normal(&[key, 0], value.x, stddev)?,
                random.normal(&[key, 1], value.y, stddev)?,
                random.normal(&[key, 2], value.z, stddev)?,
            ))
        };
        Ok(Self {
            specific_force_frd_mps2: vector(
                "accelerometer",
                self.specific_force_frd_mps2,
                noise.specific_force_mps2,
            )?,
            rates_frd_radps: vector("gyro", self.rates_frd_radps, noise.rates_radps)?,
            field_frd_gauss: vector("magnetometer", self.field_frd_gauss, noise.field_gauss)?,
            pressure_pa: random.normal(
                &[PluginRandom::name("barometer")],
                self.pressure_pa,
                noise.pressure_pa,
            )?,
            ..self
        })
    }
}

/// HIL_SENSOR for one tick: every field updated. The message's MAVLink 2
/// extension fields (`id`) are not built into this crate's dialect, so they
/// are sent as zero: sensor id 0, the IMU PX4's clock and lockstep follow.
pub fn sensor_message(time_usec: u64, readings: &Readings) -> MavMessage {
    use HilSensorUpdatedFlags as Updated;
    let updated = Updated::HIL_SENSOR_UPDATED_XACC
        | Updated::HIL_SENSOR_UPDATED_YACC
        | Updated::HIL_SENSOR_UPDATED_ZACC
        | Updated::HIL_SENSOR_UPDATED_XGYRO
        | Updated::HIL_SENSOR_UPDATED_YGYRO
        | Updated::HIL_SENSOR_UPDATED_ZGYRO
        | Updated::HIL_SENSOR_UPDATED_XMAG
        | Updated::HIL_SENSOR_UPDATED_YMAG
        | Updated::HIL_SENSOR_UPDATED_ZMAG
        | Updated::HIL_SENSOR_UPDATED_ABS_PRESSURE
        | Updated::HIL_SENSOR_UPDATED_DIFF_PRESSURE
        | Updated::HIL_SENSOR_UPDATED_PRESSURE_ALT
        | Updated::HIL_SENSOR_UPDATED_TEMPERATURE;
    MavMessage::HIL_SENSOR(HIL_SENSOR_DATA {
        time_usec,
        xacc: readings.specific_force_frd_mps2.x as f32,
        yacc: readings.specific_force_frd_mps2.y as f32,
        zacc: readings.specific_force_frd_mps2.z as f32,
        xgyro: readings.rates_frd_radps.x as f32,
        ygyro: readings.rates_frd_radps.y as f32,
        zgyro: readings.rates_frd_radps.z as f32,
        xmag: readings.field_frd_gauss.x as f32,
        ymag: readings.field_frd_gauss.y as f32,
        zmag: readings.field_frd_gauss.z as f32,
        // PX4 takes pressures in hPa.
        abs_pressure: (readings.pressure_pa / 100.0) as f32,
        diff_pressure: (readings.differential_pressure_pa / 100.0) as f32,
        pressure_alt: readings.altitude_m as f32,
        temperature: readings.temperature_c as f32,
        fields_updated: updated,
    })
}

/// HIL_GPS from the sample: a 3D fix at the sample's position, placed from
/// home on a flat Earth, with the sample's velocity. The extension fields
/// (`id`, `yaw`) go as zero: GPS 0, no GPS yaw.
pub fn gps_message(time_usec: u64, sample: &SitlSample, home: Home) -> MavMessage {
    let (north_m, east_m, down_m) = (
        sample.position_ned_m.x,
        sample.position_ned_m.y,
        sample.position_ned_m.z,
    );
    let latitude_deg = home.latitude_deg + (north_m / EARTH_RADIUS_M).to_degrees();
    let longitude_deg = home.longitude_deg
        + (east_m / (EARTH_RADIUS_M * home.latitude_deg.to_radians().cos())).to_degrees();
    let velocity = sample.velocity_ned_mps;
    let ground_speed_mps = velocity.xy().norm();
    let course_deg = velocity.y.atan2(velocity.x).to_degrees().rem_euclid(360.0);
    MavMessage::HIL_GPS(HIL_GPS_DATA {
        time_usec,
        lat: (latitude_deg * 1e7).round() as i32,
        lon: (longitude_deg * 1e7).round() as i32,
        alt: ((home.altitude_m - down_m) * 1000.0).round() as i32,
        // Dilution of precision times 100, as PX4's Gazebo plugin reports.
        eph: 100,
        epv: 200,
        vel: (ground_speed_mps * 100.0).round() as u16,
        vn: (velocity.x * 100.0).round() as i16,
        ve: (velocity.y * 100.0).round() as i16,
        vd: (velocity.z * 100.0).round() as i16,
        cog: (course_deg * 100.0).round() as u16,
        fix_type: 3,
        satellites_visible: 10,
    })
}

/// One HIL_ACTUATOR_CONTROLS from PX4.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    /// PX4's clock when it sent them: the time of the sensor message answered.
    pub time_usec: u64,
    /// Outputs 1 to 16: motors 0 to 1, servos -1 to 1, all 0 while disarmed.
    pub values: [f32; 16],
    pub armed: bool,
}

/// The controls in `message`, if it is one; PX4 also sends a heartbeat and a
/// request for ground truth, which the bridge ignores.
pub fn controls(message: &MavMessage) -> Option<Controls> {
    match message {
        MavMessage::HIL_ACTUATOR_CONTROLS(HIL_ACTUATOR_CONTROLS_DATA {
            time_usec,
            controls,
            mode,
            ..
        }) => Some(Controls {
            time_usec: *time_usec,
            values: *controls,
            armed: mode.contains(MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED),
        }),
        _ => None,
    }
}

/// The connection PX4 opened to the bridge.
pub struct Link {
    stream: TcpStream,
    inbox: Receiver<MavMessage>,
    sequence: u8,
}

impl Link {
    /// Wait up to `timeout` for PX4 to connect to `listener`, which must be
    /// non-blocking.
    pub fn accept(listener: &TcpListener, timeout: Duration) -> Result<Self> {
        let deadline = Instant::now() + timeout;
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        bail!(
                            "PX4 did not connect to TCP port {} within {:.1} s",
                            listener.local_addr()?.port(),
                            timeout.as_secs_f64()
                        );
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error.into()),
            }
        };
        stream.set_nonblocking(false)?;
        stream.set_nodelay(true)?;
        let (sender, inbox) = mpsc::channel();
        let mut reader = MavlinkReader::new(stream.try_clone()?);
        thread::spawn(move || {
            loop {
                match reader.read_message::<MavMessage>(MavlinkVersion::V2) {
                    Ok((_, message)) => {
                        if sender.send(message).is_err() {
                            return;
                        }
                    }
                    // A message this crate's dialect does not know; skip it.
                    Err(MessageReadError::Parse(_)) => continue,
                    Err(MessageReadError::Io(_)) => return,
                }
            }
        });
        Ok(Self {
            stream,
            inbox,
            sequence: 0,
        })
    }

    pub fn send(&mut self, message: &MavMessage) -> Result<()> {
        let header = MavHeader {
            system_id: 1,
            component_id: 200,
            sequence: self.sequence,
        };
        self.sequence = self.sequence.wrapping_add(1);
        write_v2_msg(&mut self.stream, header, message).context("send to PX4")?;
        Ok(())
    }

    /// The next message from PX4 within `timeout`, or `None` if none came.
    pub fn receive(&mut self, timeout: Duration) -> Result<Option<MavMessage>> {
        match self.inbox.recv_timeout(timeout) {
            Ok(message) => Ok(Some(message)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => bail!("PX4 closed the connection"),
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        // Ends the reader thread's blocking read.
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SitlSample {
        SitlSample {
            position_ned_m: Vec3::new(0.0, 0.0, -100.0),
            velocity_ned_mps: Vec3::new(20.0, 0.0, 0.0),
            orientation_ned_from_frd: [1.0, 0.0, 0.0, 0.0],
            gyro_frd_radps: Vec3::zeros(),
            specific_force_frd_mps2: Vec3::new(0.0, 0.0, -9.81),
            airspeed_mps: 20.0,
        }
    }

    const MCMILLAN: Home = Home {
        latitude_deg: 35.721025,
        longitude_deg: -120.767925,
        altitude_m: 300.0,
    };

    #[test]
    fn the_barometer_reads_the_standard_atmosphere_at_altitude() {
        let readings = readings(&sample(), MCMILLAN, Vec3::zeros());
        assert!((readings.altitude_m - 400.0).abs() < 1e-9);
        // ISA at 400 m: about 96.6 kPa and 12.4 degC.
        assert!(
            (readings.pressure_pa - 96_611.0).abs() < 20.0,
            "{}",
            readings.pressure_pa
        );
        assert!((readings.temperature_c - 12.4).abs() < 0.1);
        // Dynamic pressure at 20 m/s in air of about 1.178 kg/m3.
        assert!((readings.differential_pressure_pa - 235.6).abs() < 1.0);
    }

    #[test]
    fn the_magnetometer_sees_the_field_turned_by_the_heading() {
        // Facing east (NED yaw 90 degrees), north is off the left wing: the
        // field's north part reads along -y in forward-right-down axes.
        let yaw = UnitQuaternion::from_axis_angle(&Vec3::z_axis(), 90f64.to_radians());
        let mut sample = sample();
        sample.orientation_ned_from_frd = [yaw.w, yaw.i, yaw.j, yaw.k];
        let readings = readings(&sample, MCMILLAN, Vec3::new(0.2, 0.0, 0.4));
        let field = readings.field_frd_gauss;
        assert!(
            field.x.abs() < 1e-9 && (field.y + 0.2).abs() < 1e-9,
            "{field}"
        );
        assert!((field.z - 0.4).abs() < 1e-9);
    }

    #[test]
    fn gps_places_the_aircraft_from_home_and_reports_its_course() {
        let mut sample = sample();
        sample.position_ned_m = Vec3::new(1000.0, 0.0, -100.0);
        sample.velocity_ned_mps = Vec3::new(0.0, -10.0, 0.0);
        let MavMessage::HIL_GPS(gps) = gps_message(5, &sample, MCMILLAN) else {
            panic!("not a GPS message");
        };
        // 1 km north is about 0.009 degrees of latitude.
        assert!((f64::from(gps.lat) / 1e7 - (MCMILLAN.latitude_deg + 0.00899)).abs() < 1e-5);
        assert_eq!(gps.alt, 400_000);
        // Westbound: course 270, 10 m/s.
        assert_eq!(gps.cog, 27_000);
        assert_eq!(gps.vel, 1000);
        assert_eq!(gps.ve, -1000);
        assert_eq!(gps.fix_type, 3);
    }

    #[test]
    fn noise_is_repeatable_and_leaves_the_mean_alone() -> Result<()> {
        let readings = readings(&sample(), MCMILLAN, Vec3::new(0.2, 0.0, 0.4));
        let random = PluginRandom::new(1, 7, "Px4Bridge");
        let noise = Noise {
            rates_radps: 0.01,
            specific_force_mps2: 0.1,
            pressure_pa: 1.0,
            field_gauss: 0.001,
        };
        let once = readings.with_noise(&random, noise)?;
        let again = readings.with_noise(&random, noise)?;
        assert_eq!(once.pressure_pa, again.pressure_pa);
        assert_ne!(once.pressure_pa, readings.pressure_pa);
        assert!((once.pressure_pa - readings.pressure_pa).abs() < 6.0);
        let exact = readings.with_noise(&random, Noise::default())?;
        assert_eq!(exact.pressure_pa, readings.pressure_pa);
        Ok(())
    }

    #[test]
    fn controls_carry_the_time_and_the_armed_flag() {
        let message = MavMessage::HIL_ACTUATOR_CONTROLS(HIL_ACTUATOR_CONTROLS_DATA {
            time_usec: 2500,
            flags: Default::default(),
            controls: [0.5; 16],
            mode: MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED
                | MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED,
        });
        let parsed = controls(&message).unwrap();
        assert_eq!(parsed.time_usec, 2500);
        assert!(parsed.armed);
        assert_eq!(parsed.values[4], 0.5);
        let sensors = sensor_message(0, &readings(&sample(), MCMILLAN, Vec3::zeros()));
        assert!(controls(&sensors).is_none());
    }
}
