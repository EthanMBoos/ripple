//! One delivery vehicle: wait at its hub, run the skill it is assigned
//! (`skill.rs`), return, and reload; or fly to another hub when told to
//! relocate. Autonomy phase: read assignments and relocations, advance the
//! running skill or its own phase, write aircraft and ENU velocity commands,
//! and report status.
//!
//! The courier owns its lifecycle (idle, flying out through an optional
//! waypoint, running a skill, returning through the waypoint, relocating,
//! reloading, lost); each skill owns what happens at the site. A multirotor
//! waits on the hub pad.
//!
//! Flight time more than `hub_radius_m` from its hub uses endurance, which a
//! reload refills; a vehicle that runs out away from a hub is lost, with its
//! package. An idle fixed-wing courier is taken to be recovered and on the
//! ground: SimpleAircraft can't stop, so it is drawn loitering at
//! `loiter_radius_m`, which a mission keeps inside `hub_radius_m`.
//!
//! Status goes out on every change of phase or hub, and otherwise every
//! `report_interval_s`, staggered by vehicle, so 4,096 couriers do not send
//! 4,096 messages a tick.

use anyhow::{Context, Result, ensure};
use ripple_core::Vec3;
use ripple_core::plugin::{
    AgentContext, Autonomy, Frame, PluginIo, PluginParams, Port, Ports, Unit,
};
use serde::Deserialize;

use crate::messages::{
    ASSIGNMENT_TOPIC, Assignment, DELIVERED_TOPIC, Delivered, Phase, RELOCATE_TOPIC, Relocate,
    STATUS_TOPIC, SkillName, Status, VehicleCard, VehicleClass,
};
use crate::region::horizontal_m;
use crate::skill::{self, Settings, Skill};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CourierConfig {
    /// `fixed_wing` or `multirotor`. Required.
    class: Option<VehicleClass>,
    /// Its hub, numbered from 1, and where that hub is. A recipe leaves
    /// them 0 and the Dispatcher's spawn request gives them.
    hub: u32,
    hub_world_m: Vec3,
    cruise_speed_mps: f64,
    endurance_s: f64,
    cruise_altitude_m: f64,
    /// Within this of its hub, flight time is free and reloading can start.
    hub_radius_m: f64,
    /// How close a skill must get to the site (see each skill).
    arrival_radius_m: f64,
    loiter_radius_m: f64,
    /// A fixed-wing within this of a site for `CIRCLING_S` without
    /// delivering is circling it: it runs out to a point this far ahead and
    /// turns onto a straight run in (`circling`); 0 never does.
    run_in_m: f64,
    dwell_s: f64,
    reload_s: f64,
    /// Slow toward a target at this rate, speed = rate x distance up to
    /// cruise, so a vehicle with real dynamics can stop on it. 0 commands
    /// cruise until one tick short, as point models follow exactly.
    approach_per_s: f64,
    report_interval_s: f64,
}

/// How long a fixed-wing may stay near a site without delivering before it
/// is taken to be circling it: a full turn at 25 m/s on a 110 m circle takes
/// under 30 s, so a pass that misses lines up again well inside a minute.
const CIRCLING_S: f64 = 60.0;

impl Default for CourierConfig {
    fn default() -> Self {
        Self {
            class: None,
            hub: 0,
            hub_world_m: Vec3::zeros(),
            cruise_speed_mps: 12.0,
            endurance_s: 1500.0,
            cruise_altitude_m: 100.0,
            hub_radius_m: 200.0,
            arrival_radius_m: 3.0,
            loiter_radius_m: 300.0,
            run_in_m: 0.0,
            dwell_s: 30.0,
            reload_s: 120.0,
            approach_per_s: 0.0,
            report_interval_s: 10.0,
        }
    }
}

pub struct Courier {
    card: VehicleCard,
    hub: u32,
    hub_world_m: Vec3,
    cruise_altitude_m: f64,
    hub_radius_m: f64,
    arrival_radius_m: f64,
    loiter_radius_m: f64,
    run_in_m: f64,
    dwell_s: f64,
    reload_s: f64,
    approach_per_s: f64,
    report_interval_s: f64,
    /// When the aircraft came within `run_in_m` of its site, while it is.
    near_site_since_s: Option<f64>,
    /// The fixed point it runs out to from a site it circled, while it does.
    /// A point that moved with the aircraft would sit dead ahead, so the
    /// heading controller, which turns heading error into roll rate, would
    /// hold the bank and the circle.
    run_out_world_m: Option<Vec3>,
    /// Its own phase; while a skill runs, the skill's.
    phase: Phase,
    running: Option<Box<dyn Skill>>,
    /// The waypoint of the current trip, and whether the outbound leg has
    /// passed it; the return passes it again.
    via_world_m: Option<Vec3>,
    passed_via: bool,
    /// The request it carries, until it reloads.
    carrying: Option<i32>,
    skill_name: Option<SkillName>,
    /// When the current reload ends.
    phase_ends_s: f64,
    /// Where it ran out of endurance, if it did.
    lost_at_world_m: Vec3,
    endurance_left_s: f64,
    last_reported: Option<(Phase, u32)>,
}

impl Autonomy for Courier {
    type Config = CourierConfig;

    fn configure(params: &PluginParams) -> Result<CourierConfig> {
        let config: CourierConfig = params.parse()?;
        config
            .class
            .context("Courier needs class: fixed_wing or multirotor")?;
        ensure!(
            config.cruise_speed_mps > 0.0 && config.endurance_s > 0.0,
            "Courier cruise speed and endurance must be positive"
        );
        ensure!(
            config.arrival_radius_m > 0.0 && config.hub_radius_m > 0.0,
            "Courier arrival and hub radii must be positive"
        );
        ensure!(
            config.approach_per_s >= 0.0 && config.report_interval_s > 0.0,
            "Courier approach_per_s must be nonnegative and report_interval_s positive"
        );
        Ok(config)
    }

    fn new(config: &CourierConfig) -> Self {
        Self {
            card: VehicleCard {
                class: config.class.unwrap_or(VehicleClass::Multirotor),
                cruise_speed_mps: config.cruise_speed_mps,
                endurance_s: config.endurance_s,
            },
            hub: config.hub,
            hub_world_m: config.hub_world_m,
            cruise_altitude_m: config.cruise_altitude_m,
            hub_radius_m: config.hub_radius_m,
            arrival_radius_m: config.arrival_radius_m,
            loiter_radius_m: config.loiter_radius_m,
            run_in_m: config.run_in_m,
            near_site_since_s: None,
            run_out_world_m: None,
            dwell_s: config.dwell_s,
            reload_s: config.reload_s,
            approach_per_s: config.approach_per_s,
            report_interval_s: config.report_interval_s,
            phase: Phase::Idle,
            running: None,
            via_world_m: None,
            passed_via: false,
            carrying: None,
            skill_name: None,
            phase_ends_s: 0.0,
            lost_at_world_m: Vec3::zeros(),
            endurance_left_s: config.endurance_s,
            last_reported: None,
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

    fn initialize(&mut self, context: &mut AgentContext) -> Result<()> {
        ensure!(
            self.hub >= 1,
            "Courier {} has no hub: give the group `hub`, or build it from a Dispatcher's `fleet`",
            context.entity.id
        );
        context
            .messages
            .subscribe::<Assignment>("GlobalNetwork", ASSIGNMENT_TOPIC)?;
        context
            .messages
            .subscribe::<Relocate>("GlobalNetwork", RELOCATE_TOPIC)
    }

    /// One tick: take a new order, advance the running skill or the
    /// courier's own phase, fly toward the target, and report.
    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let now_s = context.time.time_s;
        let position_world_m = context.state.position_world_m;
        let id = context.entity.id;
        self.take_orders(context, id)?;
        let velocity_world_mps = context.state.velocity_world_mps;
        self.advance(context, id, now_s, position_world_m, velocity_world_mps)?;
        self.fly(io, position_world_m, context.time.dt_s)?;
        self.report(context, id, position_world_m)
    }
}

impl Courier {
    /// A new assignment or relocation, taken only while idle.
    fn take_orders(&mut self, context: &mut AgentContext, id: i32) -> Result<()> {
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
                self.via_world_m = message.value.via_world_m;
                self.passed_via = false;
                self.near_site_since_s = None;
                self.run_out_world_m = None;
                self.carrying = Some(message.value.request_id);
                self.skill_name = Some(message.value.skill);
            }
        }
        for message in context
            .messages
            .receive::<Relocate>("GlobalNetwork", RELOCATE_TOPIC)?
        {
            if message.value.vehicle_id == id && self.phase == Phase::Idle {
                self.hub = message.value.hub;
                self.hub_world_m = message.value.hub_world_m;
                self.phase = Phase::Relocating;
            }
        }
        Ok(())
    }

    /// Advances the running skill, or the courier's own phase, and spends
    /// endurance away from the hub; a vehicle that runs out is lost.
    fn advance(
        &mut self,
        context: &mut AgentContext,
        id: i32,
        now_s: f64,
        position_world_m: Vec3,
        velocity_world_mps: Vec3,
    ) -> Result<()> {
        let fixed_wing = self.card.class == VehicleClass::FixedWing;
        if fixed_wing
            && self.run_in_m > 0.0
            && let Some(site_world_m) = self.running.as_ref().map(|r| r.target().0)
        {
            let distance_m = horizontal_m(position_world_m, site_world_m);
            match self.run_out_world_m {
                // Out: aim at the site again, with a fresh minute to reach it.
                Some(point)
                    if distance_m >= self.run_in_m || self.reached(position_world_m, point) =>
                {
                    self.run_out_world_m = None;
                    self.near_site_since_s = None;
                }
                Some(_) => {}
                None => {
                    if self.circling(now_s, distance_m) {
                        self.run_out_world_m =
                            Some(self.run_out_point(position_world_m, velocity_world_mps));
                    }
                }
            }
        }
        let at_hub = |position: Vec3, hub: Vec3, arrival_m: f64| {
            horizontal_m(position, hub) <= self.hub_radius_m
                && (fixed_wing || (position - hub).norm() <= arrival_m)
        };
        if let Some(running) = &mut self.running {
            // Outbound through the waypoint first; the skill waits until then.
            let before_via = self.via_world_m.is_some_and(|via| {
                !self.passed_via
                    && horizontal_m(position_world_m, via) > self.arrival_radius_m.max(20.0)
            });
            if before_via {
                self.phase = Phase::Outbound;
            } else {
                self.passed_via = true;
                if let Some(request_id) = running.advance(now_s, position_world_m) {
                    context.messages.publish(
                        "GlobalNetwork",
                        DELIVERED_TOPIC,
                        Delivered {
                            vehicle_id: id,
                            request_id,
                            skill: self.skill_name.unwrap_or(SkillName::Drop),
                            time_s: now_s,
                        },
                    )?;
                }
                if running.done() {
                    self.running = None;
                    self.near_site_since_s = None;
                    self.run_out_world_m = None;
                    self.phase = Phase::Returning;
                    // Return through the waypoint too.
                    self.passed_via = self.via_world_m.is_none();
                } else {
                    self.phase = running.phase();
                }
            }
        } else {
            match self.phase {
                Phase::Returning if !self.passed_via => {
                    if let Some(via) = self.via_world_m
                        && horizontal_m(position_world_m, via) <= self.arrival_radius_m.max(20.0)
                    {
                        self.passed_via = true;
                    }
                }
                Phase::Returning | Phase::Relocating
                    if at_hub(position_world_m, self.hub_world_m, self.arrival_radius_m) =>
                {
                    self.carrying = None;
                    self.skill_name = None;
                    self.via_world_m = None;
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
            && horizontal_m(position_world_m, self.hub_world_m) > self.hub_radius_m
        {
            self.endurance_left_s = (self.endurance_left_s - context.time.dt_s).max(0.0);
            if self.endurance_left_s == 0.0 {
                self.phase = Phase::Lost;
                self.running = None;
                self.carrying = None;
                self.lost_at_world_m = position_world_m;
            }
        }
        Ok(())
    }

    /// Flies toward the running skill's target, or the phase's. A multirotor
    /// descends onto the hub pad, and onto a site when the skill says so.
    fn fly(&self, io: &mut PluginIo, position_world_m: Vec3, dt_s: f64) -> Result<()> {
        let fixed_wing = self.card.class == VehicleClass::FixedWing;
        let (target_world_m, descend) = match (&self.running, self.phase) {
            (_, Phase::Lost) => (
                Vec3::new(self.lost_at_world_m.x, self.lost_at_world_m.y, 0.0),
                !fixed_wing,
            ),
            (Some(_), _) if !self.passed_via => {
                (self.via_world_m.unwrap_or(self.hub_world_m), false)
            }
            // The skill's target, or the run-out point from a site it circled.
            (Some(running), _) => match self.run_out_world_m {
                Some(point_world_m) => (point_world_m, false),
                None => {
                    let (target_world_m, descend) = running.target();
                    (target_world_m, !fixed_wing && descend)
                }
            },
            (None, Phase::Returning) if !self.passed_via => {
                (self.via_world_m.unwrap_or(self.hub_world_m), false)
            }
            (None, Phase::Returning | Phase::Relocating) => (self.hub_world_m, !fixed_wing),
            (None, _) if fixed_wing => (self.loiter_point(position_world_m), false),
            (None, _) => (self.hub_world_m, true),
        };
        let (heading_rad, mut altitude_m, velocity_world_mps) =
            self.command(position_world_m, target_world_m, descend, dt_s);
        if self.phase == Phase::Lost {
            altitude_m = 0.0;
        }
        io.write("desired_heading", heading_rad)?;
        io.write("desired_altitude", altitude_m)?;
        io.write("desired_speed", self.card.cruise_speed_mps)?;
        io.write("velocity_x", velocity_world_mps.x)?;
        io.write("velocity_y", velocity_world_mps.y)?;
        io.write("velocity_z", velocity_world_mps.z)?;
        Ok(())
    }

    /// Reports status on a change, and on the vehicle's own heartbeat tick.
    fn report(
        &mut self,
        context: &mut AgentContext,
        id: i32,
        position_world_m: Vec3,
    ) -> Result<()> {
        let report_ticks = (self.report_interval_s / context.time.dt_s)
            .round()
            .max(1.0) as i64;
        let heartbeat = (context.time.tick + i64::from(id)).rem_euclid(report_ticks) == 0;
        let changed = self.last_reported != Some((self.phase, self.hub));
        if changed || heartbeat {
            context.messages.publish(
                "GlobalNetwork",
                STATUS_TOPIC,
                Status {
                    vehicle_id: id,
                    hub: self.hub,
                    card: self.card,
                    phase: self.phase,
                    position_world_m,
                    endurance_left_s: self.endurance_left_s,
                    request_id: self.carrying,
                },
            )?;
            self.last_reported = Some((self.phase, self.hub));
        }
        Ok(())
    }

    /// Whether a fixed-wing has been within `run_in_m` of its site for
    /// `CIRCLING_S` without delivering. Aimed straight at a site inside its
    /// turning circle, an aircraft circles the site without ever passing
    /// over it: a lent flight to a clinic a few hundred meters from its own
    /// hub, taken up from the loiter, circled it until it ran dry. A pass
    /// that misses the site turns back and lines up within a turn, so a
    /// minute nearby without a release means it cannot; it then runs out to
    /// the fixed point `run_in_m` ahead (`run_out_point`) and aims at the
    /// site again from there. Flights that never linger are untouched.
    fn circling(&mut self, now_s: f64, distance_m: f64) -> bool {
        if distance_m >= self.run_in_m {
            self.near_site_since_s = None;
            return false;
        }
        let since_s = *self.near_site_since_s.get_or_insert(now_s);
        now_s - since_s > CIRCLING_S
    }

    /// Whether the aircraft has come within the arrival radius of a point.
    fn reached(&self, position_world_m: Vec3, point_world_m: Vec3) -> bool {
        horizontal_m(position_world_m, point_world_m) <= self.arrival_radius_m.max(20.0)
    }

    /// The point `run_in_m` straight ahead.
    fn run_out_point(&self, position_world_m: Vec3, velocity_world_mps: Vec3) -> Vec3 {
        let speed_mps = velocity_world_mps.x.hypot(velocity_world_mps.y);
        if speed_mps < 1e-6 {
            return position_world_m;
        }
        let heading = Vec3::new(velocity_world_mps.x, velocity_world_mps.y, 0.0) / speed_mps;
        position_world_m + heading * self.run_in_m
    }

    /// A point a little ahead on a counterclockwise circle around the hub.
    fn loiter_point(&self, position_world_m: Vec3) -> Vec3 {
        let offset_m = position_world_m - self.hub_world_m;
        let angle_rad = offset_m.y.atan2(offset_m.x) + 0.5;
        self.hub_world_m
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_wing_near_a_site_for_a_minute_runs_out_and_back() {
        let config = CourierConfig {
            class: Some(VehicleClass::FixedWing),
            hub: 1,
            arrival_radius_m: 75.0,
            run_in_m: 500.0,
            ..CourierConfig::default()
        };
        let mut courier = Courier::new(&config);
        // Near the site: a pass takes under a minute, then it is circling.
        assert!(!courier.circling(0.0, 100.0));
        assert!(!courier.circling(59.0, 120.0));
        assert!(courier.circling(61.0, 110.0));
        // Out past run_in_m the clock resets; coming back starts it again.
        assert!(!courier.circling(70.0, 500.0));
        assert!(!courier.circling(100.0, 100.0));
        assert!(!courier.circling(150.0, 100.0));
        assert!(courier.circling(161.0, 100.0));
        let east = Vec3::new(25.0, 0.0, 0.0);
        let point = courier.run_out_point(Vec3::zeros(), east);
        assert_eq!(point, Vec3::new(500.0, 0.0, 0.0));
        // The run-out ends within the arrival radius of that fixed point.
        assert!(!courier.reached(Vec3::new(400.0, 0.0, 120.0), point));
        assert!(courier.reached(Vec3::new(450.0, 20.0, 120.0), point));
        // Off, nothing is ever circling.
        let mut plain = Courier::new(&CourierConfig {
            run_in_m: 0.0,
            ..config
        });
        assert!(!plain.circling(0.0, 100.0));
        assert!(!plain.circling(1000.0, 100.0));
    }
}
