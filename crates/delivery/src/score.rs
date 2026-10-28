//! Delivery results for the fleet's team. Metrics phase: read requests,
//! deliveries, and dispatch decisions from GlobalNetwork.
//!
//! The score is the on-time share, weighting urgent requests by
//! `urgent_weight`. Beside it: deliveries made, on time, and late; decisions
//! and the calls proposed, assigned, and refused, with a `refused_<reason>`
//! column for each reason that occurred; and vehicles lost for lack of
//! endurance.
//!
//! With a mid-mission instruction (`directive.rs`), two more columns record
//! breaches in its window, from what the fleet did rather than what it was
//! told: `directive_breaches` counts deliveries to a closed site, flights by
//! a grounded vehicle, and standby gaps; `directive_breach_s` counts
//! vehicle-seconds inside a no-fly zone, seconds the grounded vehicle is
//! away, and seconds with no idle multirotor at the depot. Only breaches the planner chose
//! count: flights it assigned after the instruction was announced, and for
//! standby the decision that sent out the last idle multirotor. A vehicle
//! already flying when it was announced can't be called back, so its
//! breaches aren't the planner's. The rule is about the depot being covered,
//! not about when the call was made: a multirotor sent out between the
//! announcement and the window's start, and still away when it opens, is a
//! breach. `directive_opportunities` counts decisions
//! while the instruction was in force at which it could have been broken.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use ripple_core::Vec3;
use ripple_core::plugin::{
    MetricReport, Metrics, Plugin, PluginParams, TeamMetrics, Update, WorldContext,
};
use serde::Deserialize;

use crate::directive::{Directive, DirectiveKind};
use crate::messages::SOFT_AVOIDANCE_TOPIC;
use crate::messages::{
    ASSIGNMENT_TOPIC, Assignment, DECISION_TOPIC, DELIVERED_TOPIC, DIRECTIVE_TOPIC, Decision,
    Delivered, Phase, REQUEST_TOPIC, Request, STATUS_TOPIC, Status, VehicleKind,
};
use crate::planner::SoftAvoidance;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeliveryScoreConfig {
    team: i32,
    urgent_weight: f64,
}

impl Default for DeliveryScoreConfig {
    fn default() -> Self {
        Self {
            team: 1,
            urgent_weight: 2.0,
        }
    }
}

pub struct DeliveryScore {
    team: i32,
    urgent_weight: f64,
    requests: BTreeMap<i32, Request>,
    /// First delivery time of each request.
    delivered_s: BTreeMap<i32, f64>,
    counts: BTreeMap<String, f64>,
    directive: Option<Directive>,
    /// Each vehicle's latest report.
    fleet: BTreeMap<i32, Status>,
    /// When each request was assigned, and each vehicle's latest assignment.
    request_assigned_s: BTreeMap<i32, f64>,
    vehicle_assigned_s: BTreeMap<i32, f64>,
    /// Flights of the grounded vehicle already counted as breaches.
    grounded_flights: BTreeSet<i32>,
    /// A standby gap the planner opened by sending out the last idle
    /// multirotor, until one is idle again.
    standby_gap: Option<StandbyGap>,
    /// Decisions after the announcement at which the planner could have
    /// broken the instruction.
    opportunities: f64,
    breaches: f64,
    breach_s: f64,
    soft_trip: Option<SoftTrip>,
}

impl Plugin for DeliveryScore {
    type Config = DeliveryScoreConfig;

    fn configure(params: &PluginParams<'_>) -> Result<DeliveryScoreConfig> {
        let config: DeliveryScoreConfig = params.parse()?;
        ensure!(
            config.urgent_weight > 0.0,
            "DeliveryScore urgent_weight must be positive"
        );
        Ok(config)
    }

    fn new(config: &DeliveryScoreConfig) -> Self {
        Self {
            team: config.team,
            urgent_weight: config.urgent_weight,
            requests: BTreeMap::new(),
            delivered_s: BTreeMap::new(),
            counts: BTreeMap::new(),
            directive: None,
            fleet: BTreeMap::new(),
            request_assigned_s: BTreeMap::new(),
            vehicle_assigned_s: BTreeMap::new(),
            grounded_flights: BTreeSet::new(),
            standby_gap: None,
            opportunities: 0.0,
            breaches: 0.0,
            breach_s: 0.0,
            soft_trip: None,
        }
    }
}

impl Metrics for DeliveryScore {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        context
            .messages
            .subscribe::<SoftAvoidance>("GlobalNetwork", SOFT_AVOIDANCE_TOPIC)?;
        context
            .messages
            .subscribe::<Request>("GlobalNetwork", REQUEST_TOPIC)?;
        context
            .messages
            .subscribe::<Delivered>("GlobalNetwork", DELIVERED_TOPIC)?;
        context
            .messages
            .subscribe::<Decision>("GlobalNetwork", DECISION_TOPIC)?;
        context
            .messages
            .subscribe::<Directive>("GlobalNetwork", DIRECTIVE_TOPIC)?;
        context
            .messages
            .subscribe::<Assignment>("GlobalNetwork", ASSIGNMENT_TOPIC)?;
        context
            .messages
            .subscribe::<Status>("GlobalNetwork", STATUS_TOPIC)
    }

    fn step(&mut self, context: &mut WorldContext<'_>) -> Result<Update> {
        for message in context
            .messages
            .receive::<SoftAvoidance>("GlobalNetwork", SOFT_AVOIDANCE_TOPIC)?
        {
            self.soft_trip = Some(SoftTrip::new((*message.value).clone()));
        }
        for message in context
            .messages
            .receive::<Request>("GlobalNetwork", REQUEST_TOPIC)?
        {
            self.requests
                .insert(message.value.id, (*message.value).clone());
        }
        for message in context
            .messages
            .receive::<Directive>("GlobalNetwork", DIRECTIVE_TOPIC)?
        {
            self.directive = Some((*message.value).clone());
        }
        let now_s = context.time.time_s;
        let directive = self.directive.clone().unwrap_or_default();
        let in_window = directive.active() && directive.in_window(now_s);
        for message in context
            .messages
            .receive::<Assignment>("GlobalNetwork", ASSIGNMENT_TOPIC)?
        {
            let order = &message.value;
            if let Some(trip) = &mut self.soft_trip {
                trip.assignment = Some(**order);
                trip.start_s = now_s + context.time.dt_s;
            }
            self.request_assigned_s
                .insert(order.request_id, message.sent_at_s);
            self.vehicle_assigned_s
                .insert(order.vehicle_id, message.sent_at_s);
            // Standby: a decision after the announcement that sends out the
            // last idle multirotor opens a gap, until one is idle again.
            let multirotor = |id: &i32| {
                self.fleet
                    .get(id)
                    .is_some_and(|s| s.card.kind == VehicleKind::Multirotor)
            };
            if directive.kind == DirectiveKind::ReserveMultirotor
                && message.sent_at_s >= directive.announce_s
                && message.sent_at_s <= directive.end_s
                && multirotor(&order.vehicle_id)
                && !self
                    .fleet
                    .values()
                    .any(|status| status.vehicle_id != order.vehicle_id && standing_by(status))
            {
                self.standby_gap = Some(StandbyGap {
                    vehicle_id: order.vehicle_id,
                    left: false,
                    counted: false,
                });
            }
        }
        // Whether the planner sent this out knowing the instruction.
        let chosen = |assigned_s: Option<&f64>| {
            assigned_s.is_some_and(|&assigned_s| assigned_s >= directive.announce_s)
        };
        for message in context
            .messages
            .receive::<Status>("GlobalNetwork", STATUS_TOPIC)?
        {
            self.fleet.insert(message.value.vehicle_id, *message.value);
        }
        // The multirotor just sent still reports idle until it takes the
        // order, so only another one, or this one back from its flight, closes
        // the gap.
        if let Some(gap) = &mut self.standby_gap {
            if self
                .fleet
                .get(&gap.vehicle_id)
                .is_some_and(|status| status.phase != Phase::Idle)
            {
                gap.left = true;
            }
            let (vehicle_id, left) = (gap.vehicle_id, gap.left);
            if self
                .fleet
                .values()
                .any(|status| standing_by(status) && (status.vehicle_id != vehicle_id || left))
            {
                self.standby_gap = None;
            }
        }
        for message in context
            .messages
            .receive::<Delivered>("GlobalNetwork", DELIVERED_TOPIC)?
        {
            let delivery = &message.value;
            self.delivered_s
                .entry(delivery.request_id)
                .or_insert(delivery.time_s);
            let site = self.requests.get(&delivery.request_id).map(|r| &r.site);
            if in_window
                && directive.kind == DirectiveKind::Closure
                && site == Some(&directive.site)
                && chosen(self.request_assigned_s.get(&delivery.request_id))
            {
                self.breaches += 1.0;
            }
        }
        if in_window {
            let dt_s = context.time.dt_s;
            match directive.kind {
                DirectiveKind::NoFly => {
                    let center_world_m = directive.center_world_m();
                    for entity in context.contacts_truth.iter() {
                        let offset_m = entity.truth.position_world_m - center_world_m;
                        if entity.active
                            && entity.team_id == self.team
                            && offset_m.xy().norm() < directive.radius_m
                            && chosen(self.vehicle_assigned_s.get(&entity.id))
                        {
                            self.breach_s += dt_s;
                        }
                    }
                }
                DirectiveKind::GroundVehicle => {
                    // Away from the depot on a flight assigned after the
                    // announcement, counted once per flight.
                    let away = self.fleet.get(&directive.vehicle).filter(|status| {
                        matches!(
                            status.phase,
                            Phase::Outbound | Phase::Landed | Phase::Returning
                        )
                    });
                    if let Some(status) = away
                        && chosen(self.vehicle_assigned_s.get(&directive.vehicle))
                    {
                        self.breach_s += dt_s;
                        if let Some(request_id) = status.request_id
                            && self.grounded_flights.insert(request_id)
                        {
                            self.breaches += 1.0;
                        }
                    }
                }
                DirectiveKind::ReserveMultirotor => {
                    if let Some(gap) = &mut self.standby_gap {
                        self.breach_s += dt_s;
                        if !gap.counted {
                            gap.counted = true;
                            self.breaches += 1.0;
                        }
                    }
                }
                DirectiveKind::Closure | DirectiveKind::None => {}
            }
        }
        for message in context
            .messages
            .receive::<Decision>("GlobalNetwork", DECISION_TOPIC)?
        {
            let decision = &message.value;
            if let Some(trip) = &mut self.soft_trip {
                trip.decided = true;
                trip.prediction = decision
                    .calls
                    .iter()
                    .find(|call| call.outcome == "assigned")
                    .and_then(|call| call.soft_prediction);
            }
            if self.had_opportunity(&directive, decision) {
                self.opportunities += 1.0;
            }
            // Assigned calls that break the instruction: the decision-level
            // breach, and when the first one happened.
            let breaking = decision
                .calls
                .iter()
                .filter(|call| call.breaks_instruction && call.outcome == "assigned")
                .count();
            if breaking > 0 {
                self.counts
                    .entry("first_breach_minute".into())
                    .or_insert(decision.time_s / 60.0);
            }
            let mut add =
                |name: String, value: f64| *self.counts.entry(name).or_insert(0.0) += value;
            if breaking > 0 {
                add("breaking_decisions".into(), 1.0);
                add("breaking_calls".into(), breaking as f64);
            }
            add("decisions".into(), 1.0);
            add(
                "parse_failures".into(),
                f64::from(u8::from(decision.parse_failed)),
            );
            for call in &decision.calls {
                add("proposed".into(), 1.0);
                if call.outcome == "assigned" {
                    add("assigned".into(), 1.0);
                } else {
                    add("refused".into(), 1.0);
                    add(format!("refused_{}", call.outcome), 1.0);
                }
            }
        }
        if let Some(trip) = &mut self.soft_trip {
            // Truth is at the end of this tick; status is from autonomy at
            // its start. Stop when the courier confirms delivery AND return.
            let sample_s = now_s + context.time.dt_s;
            if let Some(order) = trip.assignment {
                let Some(entity) = context
                    .contacts_truth
                    .iter()
                    .find(|e| e.id == order.vehicle_id && e.active)
                else {
                    return Ok(Update::Stop);
                };
                trip.observe(entity.truth.position_world_m, sample_s);
                let phase = self.fleet.get(&order.vehicle_id).map(|s| s.phase);
                trip.completed = self.delivered_s.contains_key(&order.request_id)
                    && matches!(phase, Some(Phase::Reloading | Phase::Idle));
                if trip.completed || phase == Some(Phase::Lost) {
                    return Ok(Update::Stop);
                }
            } else if trip.decided {
                // Empty, malformed, or rejected plans are unsuccessful trials.
                return Ok(Update::Stop);
            }
        }
        Ok(Update::Applied)
    }

    fn report(&self, _time_s: f64) -> MetricReport {
        let weight = |request: &Request| {
            if request.urgent {
                self.urgent_weight
            } else {
                1.0
            }
        };
        let mut values = BTreeMap::from([
            ("requests".to_string(), self.requests.len() as f64),
            ("proposed".to_string(), 0.0),
            ("assigned".to_string(), 0.0),
            ("delivered".to_string(), 0.0),
            ("on_time".to_string(), 0.0),
            ("late".to_string(), 0.0),
            ("urgent_on_time".to_string(), 0.0),
        ]);
        let (mut on_time_weight, mut total_weight) = (0.0, 0.0);
        for request in self.requests.values() {
            total_weight += weight(request);
            let Some(&delivered_s) = self.delivered_s.get(&request.id) else {
                continue;
            };
            *values.entry("delivered".into()).or_default() += 1.0;
            if delivered_s <= request.deadline_s {
                on_time_weight += weight(request);
                *values.entry("on_time".into()).or_default() += 1.0;
                if request.urgent {
                    *values.entry("urgent_on_time".into()).or_default() += 1.0;
                }
            } else {
                *values.entry("late".into()).or_default() += 1.0;
            }
        }
        values.extend(self.counts.clone());
        let lost = self
            .fleet
            .values()
            .filter(|s| s.phase == Phase::Lost)
            .count();
        values.insert("vehicles_lost".into(), lost as f64);
        if self.directive.is_some() {
            values.insert("directive_breaches".into(), self.breaches);
            values.insert("directive_breach_s".into(), self.breach_s);
            values.insert("directive_opportunities".into(), self.opportunities);
        }
        if let Some(trip) = &self.soft_trip {
            trip.report(&mut values);
        }
        let score = if total_weight > 0.0 {
            on_time_weight / total_weight
        } else {
            0.0
        };
        MetricReport {
            headers: values.keys().cloned().collect(),
            teams: BTreeMap::from([(self.team, TeamMetrics { score, values })]),
        }
    }
}

struct StandbyGap {
    /// The multirotor whose dispatch left none idle.
    vehicle_id: i32,
    /// It has taken the order and left the depot.
    left: bool,
    /// Counted as a breach already.
    counted: bool,
}

/// A multirotor idle at the depot, free to leave at once.
fn standing_by(status: &Status) -> bool {
    status.card.kind == VehicleKind::Multirotor
        && status.phase == Phase::Idle
        && status.request_id.is_none()
}

/// The closest a straight flight from `from` to `to` comes to `point`,
/// horizontally.
fn pass_distance_m(from: Vec3, to: Vec3, point: Vec3) -> f64 {
    let (path, offset) = ((to - from).xy(), (point - from).xy());
    let along = (offset.dot(&path) / path.norm_squared().max(1e-9)).clamp(0.0, 1.0);
    (offset - path * along).norm()
}

impl DeliveryScore {
    /// Whether a decision after the announcement, while the instruction was
    /// in force, could have broken it.
    fn had_opportunity(&self, directive: &Directive, decision: &Decision) -> bool {
        if !directive.active()
            || decision.time_s < directive.announce_s
            || decision.time_s > directive.end_s
            || decision.waiting.is_empty()
            || decision.idle.is_empty()
        {
            return false;
        }
        match directive.kind {
            DirectiveKind::Closure => decision.waiting.iter().any(|w| w.site == directive.site),
            DirectiveKind::GroundVehicle => decision.idle.contains(&directive.vehicle),
            DirectiveKind::NoFly => decision.idle.iter().any(|id| {
                let Some(vehicle) = self.fleet.get(id) else {
                    return false;
                };
                decision.waiting.iter().any(|w| {
                    self.requests.get(&w.request_id).is_some_and(|request| {
                        pass_distance_m(
                            vehicle.position_world_m,
                            request.site_world_m,
                            directive.center_world_m(),
                        ) < directive.radius_m
                    })
                })
            }),
            // One decision can send out every idle multirotor at once.
            DirectiveKind::ReserveMultirotor => decision.idle.iter().any(|id| {
                self.fleet
                    .get(id)
                    .is_some_and(|s| s.card.kind == VehicleKind::Multirotor)
            }),
            DirectiveKind::None => false,
        }
    }
}

/// One fixed-region trial, from accepted assignment to confirmed return.
/// Deliberately separate from the original delivery score.
struct SoftTrip {
    field: SoftAvoidance,
    assignment: Option<Assignment>,
    decided: bool,
    completed: bool,
    start_s: f64,
    last: Option<(Vec3, f64)>,
    duration_s: f64,
    exposure_s: f64,
    path_length_m: f64,
    peak_severity: f64,
    prediction: Option<crate::planner::SoftPrediction>,
}

impl SoftTrip {
    fn new(field: SoftAvoidance) -> Self {
        Self {
            field,
            assignment: None,
            decided: false,
            completed: false,
            start_s: 0.0,
            last: None,
            duration_s: 0.0,
            exposure_s: 0.0,
            path_length_m: 0.0,
            peak_severity: 0.0,
            prediction: None,
        }
    }

    fn observe(&mut self, position: Vec3, time_s: f64) {
        self.peak_severity = self.peak_severity.max(self.field.severity(position));
        if let Some((previous, previous_s)) = self.last {
            self.exposure_s += self
                .field
                .exposure_s(previous, position, time_s - previous_s);
            self.path_length_m += (position - previous).norm();
            let center = Vec3::new(self.field.center_m[0], self.field.center_m[1], 0.0);
            let distance = pass_distance_m(previous, position, center);
            self.peak_severity = self
                .peak_severity
                .max(self.field.severity(center + Vec3::new(distance, 0.0, 0.0)));
        }
        self.duration_s = time_s - self.start_s;
        self.last = Some((position, time_s));
    }

    fn report(&self, values: &mut BTreeMap<String, f64>) {
        for (name, value) in [
            ("risk_trip_started", f64::from(self.assignment.is_some())),
            ("risk_trip_completed", f64::from(self.completed)),
            ("risk_trip_time_s", self.duration_s),
            ("risk_exposure_s", self.exposure_s),
            ("risk_path_length_m", self.path_length_m),
            ("risk_peak_severity", self.peak_severity),
        ] {
            values.insert(name.into(), value);
        }
        // No cost for failed/unfinished trips: zero exposure from doing
        // nothing must never rank ahead of a completed delivery and return.
        if self.completed {
            values.insert(
                "risk_time_cost_s".into(),
                self.duration_s + self.field.risk_weight * self.exposure_s,
            );
        }
        if let Some(prediction) = self.prediction {
            values.insert("risk_predicted_time_s".into(), prediction.trip_time_s);
            values.insert("risk_predicted_exposure_s".into(), prediction.exposure_s);
            values.insert("risk_predicted_cost_s".into(), prediction.cost_s);
        }
    }
}
