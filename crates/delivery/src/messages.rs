//! The messages between the dispatcher, the couriers, and the score, all on
//! GlobalNetwork.
use ripple_core::Vec3;
use serde::{Deserialize, Serialize};

/// Each courier's state, every tick.
pub const STATUS_TOPIC: &str = "DeliveryStatus";
/// The dispatcher's orders; each courier keeps only its own.
pub const ASSIGNMENT_TOPIC: &str = "DeliveryAssignment";
/// A package reached its site.
pub const DELIVERED_TOPIC: &str = "Delivered";
/// A request opened.
pub const REQUEST_TOPIC: &str = "DeliveryRequest";
/// What one dispatch decision proposed and what was refused.
pub const DECISION_TOPIC: &str = "DispatchDecision";
/// The shared field parameters for a soft avoidance trial.
pub const SOFT_AVOIDANCE_TOPIC: &str = "SoftAvoidance";
/// Fleet field and full request schedule, for scoring unopened requests too.
pub const SOFT_FLEET_TOPIC: &str = "SoftFleet";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SoftFleet {
    pub field: crate::planner::SoftAvoidance,
    pub requests: Vec<Request>,
}

/// An instruction from operations, when the dispatcher announces it.
pub const DIRECTIVE_TOPIC: &str = "Directive";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VehicleKind {
    FixedWing,
    Multirotor,
}

/// What a vehicle can do, as the dispatcher is told it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct VehicleCard {
    pub kind: VehicleKind,
    pub cruise_speed_mps: f64,
    /// Zero for a vehicle that can hover.
    pub min_speed_mps: f64,
    /// Zero for a vehicle that can turn in place.
    pub turn_radius_m: f64,
    /// Flight time on a full battery or tank.
    pub endurance_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Outbound,
    /// A multirotor on the ground at the site.
    Landed,
    Returning,
    Reloading,
    /// Ran out of endurance away from the depot: down, with its package, and
    /// out of service for the rest of the run.
    Lost,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Status {
    pub vehicle_id: i32,
    pub card: VehicleCard,
    pub phase: Phase,
    pub position_world_m: Vec3,
    pub endurance_left_s: f64,
    /// The request it is carrying, if any.
    pub request_id: Option<i32>,
}

/// The name of a skill a call asks for; each is described in `skill/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillName {
    Drop,
    Land,
    DropVia,
}

impl SkillName {
    /// The name the model writes.
    pub fn as_str(self) -> &'static str {
        match self {
            SkillName::Drop => "drop",
            SkillName::Land => "land",
            SkillName::DropVia => "drop_via",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Assignment {
    pub vehicle_id: i32,
    pub request_id: i32,
    pub skill: SkillName,
    pub site_world_m: Vec3,
    /// A waypoint to pass on the way, for skills that take one.
    pub via_world_m: Option<Vec3>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Delivered {
    pub vehicle_id: i32,
    pub request_id: i32,
    pub time_s: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: i32,
    pub site: String,
    pub site_world_m: Vec3,
    /// Rooftops take only a landing.
    pub rooftop: bool,
    /// What and how much: told to the model, but they change nothing in
    /// flight or in the score.
    pub item: String,
    pub units: u32,
    pub urgent: bool,
    pub opened_at_s: f64,
    pub deadline_s: f64,
}

/// One dispatch decision in full: what the planner saw, what it proposed, and
/// what became of each call. Rerun a case with `--trace` to read them
/// (crates/delivery/trace.py).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Decision {
    pub time_s: f64,
    /// Instructions in force, as shown to the model.
    pub instructions: Vec<String>,
    pub waiting: Vec<Waiting>,
    /// Vehicles free to take a request.
    pub idle: Vec<i32>,
    /// The model's reply could not be read as assignments.
    pub parse_failed: bool,
    pub calls: Vec<CallRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Waiting {
    pub request_id: i32,
    pub site: String,
    pub urgent: bool,
    pub deadline_s: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallRecord {
    pub vehicle_id: i32,
    /// `None` when the model named a skill that doesn't exist.
    pub skill: Option<SkillName>,
    pub request_id: i32,
    pub via_world_m: Option<Vec3>,
    /// `assigned`, or why the call was refused, such as `out_of_range`.
    pub outcome: String,
    /// Flown as planned, it would break the instruction in force
    /// (`planner::breaks_instruction`), whether or not it was refused for it.
    pub breaks_instruction: bool,
    /// Straight-line time from the vehicle to the site, if both are known.
    pub arrival_s: Option<f64>,
    /// The deadline less the predicted arrival time; negative means late.
    pub slack_s: Option<f64>,
    /// Predicted cost, separate from the measured flight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_prediction: Option<crate::planner::SoftPrediction>,
}
