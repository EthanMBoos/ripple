//! One delivery vehicle: wait at the depot, run the skill it is assigned
//! (`skill.rs`), return, and reload. Autonomy phase: read assignments, advance
//! the running skill or its own phase, write aircraft and ENU velocity
//! commands, and report status.
//!
//! The courier owns its lifecycle (idle, running a skill, returning,
//! reloading, lost); each skill owns how it flies a delivery. A multirotor
//! waits on the depot pad.
//!
//! Flight time more than `depot_radius_m` from the depot uses endurance, which
//! a reload refills; a vehicle that runs out away from the depot is lost,
//! with its package. An idle fixed-wing courier is taken to be recovered and
//! on the ground: SimpleAircraft can't stop, so it is drawn loitering at
//! `loiter_radius_m`, which a mission keeps inside `depot_radius_m`.

use anyhow::{Context, Result, ensure};
use ripple_core::Vec3;
use ripple_core::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};
use serde::Deserialize;

use crate::messages::{
    ASSIGNMENT_TOPIC, Assignment, DELIVERED_TOPIC, Delivered, Phase, STATUS_TOPIC, Status,
    VehicleCard, VehicleKind,
};
use crate::skill::{self, Settings, Skill};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CourierConfig {
    /// `fixed_wing` or `multirotor`. Required.
    kind: Option<VehicleKind>,
    cruise_speed_mps: f64,
    /// Reported to the dispatcher; the motion model enforces the real limits.
    min_speed_mps: f64,
    turn_radius_m: f64,
    endurance_s: f64,
    cruise_altitude_m: f64,
    depot_m: [f64; 3],
    /// Within this of the depot, flight time is free and reloading can start.
    depot_radius_m: f64,
    /// How close a skill must get to the site (see each skill).
    arrival_radius_m: f64,
    loiter_radius_m: f64,
    dwell_s: f64,
    reload_s: f64,
    /// Slow toward a target at this rate, speed = rate x distance up to
    /// cruise, so a vehicle with real dynamics can stop on it. 0 commands
    /// cruise until one tick short, as point models follow exactly.
    approach_per_s: f64,
}

impl Default for CourierConfig {
    fn default() -> Self {
        Self {
            kind: None,
            cruise_speed_mps: 12.0,
            min_speed_mps: 0.0,
            turn_radius_m: 0.0,
            endurance_s: 1500.0,
            cruise_altitude_m: 100.0,
            depot_m: [0.0; 3],
            depot_radius_m: 200.0,
            arrival_radius_m: 3.0,
            loiter_radius_m: 300.0,
            dwell_s: 30.0,
            reload_s: 120.0,
            approach_per_s: 0.0,
        }
    }
}

pub struct Courier {
    card: VehicleCard,
    cruise_altitude_m: f64,
    depot_world_m: Vec3,
    depot_radius_m: f64,
    arrival_radius_m: f64,
    loiter_radius_m: f64,
    dwell_s: f64,
    reload_s: f64,
    approach_per_s: f64,
    /// Its own phase; while a skill runs, the skill's.
    phase: Phase,
    running: Option<Box<dyn Skill>>,
    /// The request it carries, until it reloads.
    carrying: Option<i32>,
    /// When the current reload ends.
    phase_ends_s: f64,
    /// Where it ran out of endurance, if it did.
    lost_at_world_m: Vec3,
    endurance_left_s: f64,
}

impl Plugin for Courier {
    type Config = CourierConfig;

    fn configure(params: &PluginParams<'_>) -> Result<CourierConfig> {
        let config: CourierConfig = params.parse()?;
        config
            .kind
            .context("Courier needs kind: fixed_wing or multirotor")?;
        ensure!(
            config.cruise_speed_mps > 0.0 && config.endurance_s > 0.0,
            "Courier cruise speed and endurance must be positive"
        );
        ensure!(
            config.arrival_radius_m > 0.0 && config.depot_radius_m > 0.0,
            "Courier arrival and depot radii must be positive"
        );
        ensure!(
            config.approach_per_s >= 0.0,
            "Courier approach_per_s must be nonnegative"
        );
        Ok(config)
    }

    fn new(config: &CourierConfig) -> Self {
        Self {
            card: VehicleCard {
                kind: config.kind.unwrap_or(VehicleKind::Multirotor),
                cruise_speed_mps: config.cruise_speed_mps,
                min_speed_mps: config.min_speed_mps,
                turn_radius_m: config.turn_radius_m,
                endurance_s: config.endurance_s,
            },
            cruise_altitude_m: config.cruise_altitude_m,
            depot_world_m: Vec3::from(config.depot_m),
            depot_radius_m: config.depot_radius_m,
            arrival_radius_m: config.arrival_radius_m,
            loiter_radius_m: config.loiter_radius_m,
            dwell_s: config.dwell_s,
            reload_s: config.reload_s,
            approach_per_s: config.approach_per_s,
            phase: Phase::Idle,
            running: None,
            carrying: None,
            phase_ends_s: 0.0,
            lost_at_world_m: Vec3::zeros(),
            endurance_left_s: config.endurance_s,
        }
    }

    fn ports(_: &CourierConfig) -> Ports {
        Ports::default()
            .output(Port::new("desired_heading", Unit::Radians, Frame::World))
            .output(Port::new("desired_altitude", Unit::Meters, Frame::World))
            .output(Port::new(
                "desired_speed",
                Unit::MetersPerSecond,
                Frame::None,
            ))
            .output(Port::new("velocity_x", Unit::MetersPerSecond, Frame::World))
            .output(Port::new("velocity_y", Unit::MetersPerSecond, Frame::World))
            .output(Port::new("velocity_z", Unit::MetersPerSecond, Frame::World))
    }
}

/// Distance in the horizontal plane.
fn horizontal_m(a: Vec3, b: Vec3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

impl Autonomy for Courier {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        context
            .messages
            .subscribe::<Assignment>("GlobalNetwork", ASSIGNMENT_TOPIC)
    }

    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let now_s = context.time.time_s;
        let position_world_m = context.state.position_world_m;
        let id = context.entity.id;

        // Input: a new order, taken only while idle.
        for message in context
            .messages
            .receive::<Assignment>("GlobalNetwork", ASSIGNMENT_TOPIC)?
        {
            if message.value.vehicle_id == id && self.phase == Phase::Idle {
                let settings = Settings {
                    arrival_radius_m: self.arrival_radius_m,
                    dwell_s: self.dwell_s,
                };
                let running = skill::start(&message.value, settings);
                self.phase = running.phase();
                self.running = Some(running);
                self.carrying = Some(message.value.request_id);
            }
        }

        // Calculation: advance the running skill, or the courier's own phase.
        let fixed_wing = self.card.kind == VehicleKind::FixedWing;
        if let Some(running) = &mut self.running {
            if let Some(request_id) = running.advance(now_s, position_world_m) {
                context.messages.publish(
                    "GlobalNetwork",
                    DELIVERED_TOPIC,
                    Delivered {
                        vehicle_id: id,
                        request_id,
                        time_s: now_s,
                    },
                )?;
            }
            if running.done() {
                self.running = None;
                self.phase = Phase::Returning;
            } else {
                self.phase = running.phase();
            }
        } else {
            match self.phase {
                Phase::Returning
                    if horizontal_m(position_world_m, self.depot_world_m)
                        <= self.depot_radius_m
                        && (fixed_wing
                            || (position_world_m - self.depot_world_m).norm()
                                <= self.arrival_radius_m) =>
                {
                    self.carrying = None;
                    self.phase = Phase::Reloading;
                    self.phase_ends_s = now_s + self.reload_s;
                }
                Phase::Reloading if now_s >= self.phase_ends_s => {
                    self.phase = Phase::Idle;
                    self.endurance_left_s = self.card.endurance_s;
                }
                _ => {}
            }
        }
        if self.phase != Phase::Lost
            && horizontal_m(position_world_m, self.depot_world_m) > self.depot_radius_m
        {
            self.endurance_left_s = (self.endurance_left_s - context.time.dt_s).max(0.0);
            if self.endurance_left_s == 0.0 {
                self.phase = Phase::Lost;
                self.running = None;
                self.carrying = None;
                self.lost_at_world_m = position_world_m;
            }
        }

        // Output: fly toward the running skill's target, or the phase's.
        // A multirotor descends onto the depot pad, and onto a site when the
        // skill says so.
        let (target_world_m, descend) = match (&self.running, self.phase) {
            (_, Phase::Lost) => (
                Vec3::new(self.lost_at_world_m.x, self.lost_at_world_m.y, 0.0),
                !fixed_wing,
            ),
            (Some(running), _) => {
                let (target_world_m, descend) = running.target();
                (target_world_m, !fixed_wing && descend)
            }
            (None, Phase::Returning) => (self.depot_world_m, !fixed_wing),
            (None, _) if fixed_wing => (self.loiter_point(position_world_m), false),
            (None, _) => (self.depot_world_m, true),
        };
        let (heading_rad, mut altitude_m, velocity_world_mps) =
            self.command(position_world_m, target_world_m, descend, context.time.dt_s);
        if self.phase == Phase::Lost {
            altitude_m = 0.0;
        }
        io.write("desired_heading", heading_rad)?;
        io.write("desired_altitude", altitude_m)?;
        io.write("desired_speed", self.card.cruise_speed_mps)?;
        io.write("velocity_x", velocity_world_mps.x)?;
        io.write("velocity_y", velocity_world_mps.y)?;
        io.write("velocity_z", velocity_world_mps.z)?;

        context.messages.publish(
            "GlobalNetwork",
            STATUS_TOPIC,
            Status {
                vehicle_id: id,
                card: self.card,
                phase: self.phase,
                position_world_m,
                endurance_left_s: self.endurance_left_s,
                request_id: self.carrying,
            },
        )?;
        Ok(Update::Applied)
    }
}

impl Courier {
    /// A point a little ahead on a counterclockwise circle around the depot.
    fn loiter_point(&self, position_world_m: Vec3) -> Vec3 {
        let offset_m = position_world_m - self.depot_world_m;
        let angle_rad = offset_m.y.atan2(offset_m.x) + 0.5;
        self.depot_world_m
            + Vec3::new(
                self.loiter_radius_m * angle_rad.cos(),
                self.loiter_radius_m * angle_rad.sin(),
                0.0,
            )
    }

    /// Heading and altitude for the aircraft controller, and a velocity for
    /// the point model. Away from the target a vehicle flies at cruise
    /// altitude; with `descend` it drops to the target's height once over it.
    fn command(
        &self,
        position_world_m: Vec3,
        target_world_m: Vec3,
        descend: bool,
        dt_s: f64,
    ) -> (f64, f64, Vec3) {
        let horizontal_distance_m = horizontal_m(position_world_m, target_world_m);
        let over_target = horizontal_distance_m <= self.arrival_radius_m.max(20.0);
        let altitude_m = if descend && over_target {
            target_world_m.z
        } else {
            self.cruise_altitude_m
        };
        let aim_world_m = Vec3::new(target_world_m.x, target_world_m.y, altitude_m);
        let displacement_m = aim_world_m - position_world_m;
        let heading_rad = displacement_m.y.atan2(displacement_m.x);
        let distance_m = displacement_m.norm();
        let velocity_world_mps = if distance_m < 1e-9 {
            Vec3::zeros()
        } else {
            // Slow down to land on the point instead of overshooting it.
            let stopping_speed_mps = if self.approach_per_s > 0.0 {
                distance_m * self.approach_per_s
            } else {
                distance_m / dt_s
            };
            let speed_mps = self.card.cruise_speed_mps.min(stopping_speed_mps);
            displacement_m / distance_m * speed_mps
        };
        (heading_rad, altitude_m, velocity_world_mps)
    }
}
