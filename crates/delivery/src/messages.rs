//! The messages between the dispatcher, the couriers, and the score, all on
//! GlobalNetwork.
use ripple_core::Vec3;
use serde::{Deserialize, Serialize};

use crate::order::{Order, Outcome};

/// Each courier's state: on every change, and every `report_interval_s`.
pub const STATUS_TOPIC: &str = "DeliveryStatus";
/// The dispatcher's orders to fly a request; each courier keeps only its own.
pub const ASSIGNMENT_TOPIC: &str = "DeliveryAssignment";
/// The dispatcher's order to move to another hub.
pub const RELOCATE_TOPIC: &str = "DeliveryRelocate";
/// A package reached its site.
pub const DELIVERED_TOPIC: &str = "Delivered";
/// A request opened.
pub const REQUEST_TOPIC: &str = "DeliveryRequest";
/// A request passed its deadline unassigned, with why.
pub const MISSED_TOPIC: &str = "DeliveryMissed";
/// An event of the day, when it starts.
pub const EVENT_TOPIC: &str = "DeliveryEvent";
/// Every order a fleet manager gave and what became of it.
pub const ORDER_TOPIC: &str = "FleetManagerOrder";
/// Every call to the fleet manager: its size and what it produced.
pub const CALL_TOPIC: &str = "FleetManagerCall";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VehicleClass {
    FixedWing,
    Multirotor,
}

impl VehicleClass {
    pub fn as_str(self) -> &'static str {
        match self {
            VehicleClass::FixedWing => "fixed-wing",
            VehicleClass::Multirotor => "multirotor",
        }
    }
}

/// What a vehicle can do, as the dispatcher is told it.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct VehicleCard {
    pub class: VehicleClass,
    pub cruise_speed_mps: f64,
    /// Flight time on a full battery or tank.
    pub endurance_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Outbound,
    /// A multirotor on the ground at the site.
    Landed,
    Returning,
    /// Flying to another hub, which becomes its own.
    Relocating,
    Reloading,
    /// Ran out of endurance away from a hub: down, with its package, and out
    /// of service for the rest of the run.
    Lost,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Outbound => "outbound",
            Phase::Landed => "landed",
            Phase::Returning => "returning",
            Phase::Relocating => "relocating",
            Phase::Reloading => "reloading",
            Phase::Lost => "lost",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Status {
    pub vehicle_id: i32,
    /// The hub it belongs to, numbered from 1.
    pub hub: u32,
    pub card: VehicleCard,
    pub phase: Phase,
    pub position_world_m: Vec3,
    pub endurance_left_s: f64,
    /// The request it is carrying, if any.
    pub request_id: Option<i32>,
}

impl Status {
    /// Free to take an order.
    pub fn free(&self) -> bool {
        self.phase == Phase::Idle && self.request_id.is_none()
    }
}

/// The name of a skill a call asks for; each is described in `skill/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillName {
    Drop,
    Land,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Assignment {
    pub vehicle_id: i32,
    pub request_id: i32,
    /// The hub it launches from.
    pub hub: u32,
    pub class: VehicleClass,
    pub skill: SkillName,
    /// Where to deliver: the site, or its ground pad for a closed rooftop.
    pub site_world_m: Vec3,
    /// A waypoint to pass on the way out and back, around a closed area.
    pub via_world_m: Option<Vec3>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Relocate {
    pub vehicle_id: i32,
    pub hub: u32,
    pub hub_world_m: Vec3,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Delivered {
    pub vehicle_id: i32,
    pub request_id: i32,
    pub skill: SkillName,
    pub time_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Routine,
    Urgent,
    Trauma,
}

impl Urgency {
    pub fn as_str(self) -> &'static str {
        match self {
            Urgency::Routine => "routine",
            Urgency::Urgent => "urgent",
            Urgency::Trauma => "trauma",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    /// Blood for a clinic or hospital, scored by its deadline.
    Delivery,
    /// Stock for another hub, from a `transfer` order; not scored as a
    /// delivery.
    Transfer,
}

#[derive(Clone, Debug, Serialize)]
pub struct Request {
    pub id: i32,
    pub kind: RequestKind,
    /// The hub that serves the site unless an order says otherwise; for a
    /// transfer, the hub the stock leaves.
    pub hub: u32,
    /// The site code, such as `3A` or `3H1`; for a transfer, `hub 5`.
    pub site: String,
    pub site_world_m: Vec3,
    /// Rooftops take only a landing.
    pub rooftop: bool,
    /// The ground pad 300 m from a rooftop, used when the rooftop is closed.
    pub pad_world_m: Vec3,
    pub item: String,
    pub units: u32,
    pub urgency: Urgency,
    pub opened_at_s: f64,
    pub deadline_s: f64,
    /// For a transfer, the hub that receives the stock.
    pub to_hub: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MissReason {
    /// No eligible hub had the item.
    NoStock,
    /// Every eligible hub was out or grounded for the class it needed.
    Grounded,
    /// No idle vehicle with the right skill.
    NoVehicle,
    /// Every idle vehicle was too far.
    OutOfRange,
    /// The site lies inside an area flights must avoid.
    Closed,
}

impl MissReason {
    pub fn as_str(self) -> &'static str {
        match self {
            MissReason::NoStock => "no_stock",
            MissReason::Grounded => "grounded",
            MissReason::NoVehicle => "no_vehicle",
            MissReason::OutOfRange => "out_of_range",
            MissReason::Closed => "closed",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Missed {
    pub request_id: i32,
    pub reason: MissReason,
    pub time_s: f64,
}

/// Who gave an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerKind {
    /// The dispatcher alone; events are ignored.
    None,
    /// Hand-written orders at set times, from the mission.
    Scripted,
    /// Fixed rules reading the events as structured data.
    Replanner,
    /// Rules written once as data (`policy.rs`), by hand or by a model.
    Policy,
    /// The language model.
    Model,
    /// The replanner plans and the language model drops what it objects to.
    Veto,
}

impl ManagerKind {
    /// Calls a language model.
    pub fn uses_model(self) -> bool {
        matches!(self, ManagerKind::Model | ManagerKind::Veto)
    }
}

/// One order and what became of it.
#[derive(Clone, Debug, Serialize)]
pub struct OrderRecord {
    pub time_s: f64,
    pub source: ManagerKind,
    /// From the manager above the clusters.
    pub top: bool,
    /// Given on a call with no new event and nothing wrong in view.
    pub quiet: bool,
    pub order: Order,
    pub outcome: Outcome,
    /// The hub pairs a relocation or stock flight went between: one for a
    /// reposition or transfer, one per donor hub for a move, one for a send.
    pub moved: Vec<(u32, u32)>,
    /// What the fleet manager is told about it next call, when feedback is on.
    pub feedback: String,
}

/// One call to the fleet manager.
#[derive(Clone, Debug, Serialize)]
pub struct ManagerCall {
    pub time_s: f64,
    pub source: ManagerKind,
    /// To the manager above the clusters.
    pub top: bool,
    /// No new event and nothing wrong in view: the right reply is nothing.
    pub quiet: bool,
    /// The events that caused it; empty for a check-in.
    pub events: Vec<u32>,
    /// Characters sent, over every round of the call, and received.
    pub prompt_chars: usize,
    pub reply_chars: usize,
    pub queries: u32,
    pub orders: u32,
    /// Proposed orders the veto manager dropped.
    pub dropped: u32,
    pub parse_failed: bool,
}
