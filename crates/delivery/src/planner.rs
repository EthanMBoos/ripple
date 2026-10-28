//! Who flies which request: a language model, asked in plain language. It
//! sees a `View` of the fleet and returns calls, which the dispatcher checks
//! before sending.
//!
//! Replies are kept in one folder, one readable JSON file per prompt, named by
//! a hash of the model, the sample index, and the prompt. A rerun sends the
//! same prompts, finds every reply in the folder, and so replays the whole
//! episode exactly without calling the model. `sample` selects an independent
//! reply to the same prompt, so samples 0, 1, and 2 are three live reruns of
//! one episode.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use ripple_core::Vec3;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::client::Client;
use crate::directive::{Directive, DirectiveKind};
use crate::messages::{Assignment, Phase, Request, SkillName, Status, VehicleKind};
use crate::skill;

/// What a planner sees at one decision.
pub struct View<'a> {
    pub time_s: f64,
    pub depot_world_m: Vec3,
    /// Every vehicle's latest report, by ID.
    pub vehicles: Vec<&'a Status>,
    /// Open requests nobody is carrying, by ID.
    pub waiting: Vec<&'a Request>,
    /// Instructions from operations in force now, in plain language.
    pub instructions: Vec<String>,
    /// The skills this mission offers the model.
    pub skills: &'a [SkillName],
    /// The largest share of a vehicle's endurance left a round trip may use.
    pub range_margin: f64,
    pub dwell_s: f64,
    pub soft_avoidance: Option<&'a SoftAvoidance>,
}

/// A static, horizontal severity field. This is a shifted and normalized
/// exponential, with a finite cutoff; severity is not a probability density.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SoftAvoidance {
    pub center_m: [f64; 2],
    pub peak_severity: f64,
    pub decay_distance_m: f64,
    pub radius_m: f64,
    pub risk_weight: f64,
}

impl Default for SoftAvoidance {
    fn default() -> Self {
        Self {
            center_m: [1000.0, 0.0],
            peak_severity: 1.0,
            decay_distance_m: 200.0,
            radius_m: 600.0,
            risk_weight: 5.0,
        }
    }
}

impl SoftAvoidance {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.center_m.iter().all(|v| v.is_finite()),
            "soft center must be finite"
        );
        ensure!(
            [self.radius_m, self.decay_distance_m]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0),
            "soft radius and decay distance must be finite and positive"
        );
        ensure!(
            [self.peak_severity, self.risk_weight]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0),
            "soft peak severity and risk weight must be finite and nonnegative"
        );
        Ok(())
    }

    pub fn severity(&self, point: Vec3) -> f64 {
        let distance_m = (point.x - self.center_m[0]).hypot(point.y - self.center_m[1]);
        if distance_m >= self.radius_m {
            return 0.0;
        }
        // exp_m1 avoids cancellation when the decay distance is large.
        self.peak_severity
            * (1.0
                - (-distance_m / self.decay_distance_m).exp_m1()
                    / (-self.radius_m / self.decay_distance_m).exp_m1())
    }

    /// Integrate in time along a straight segment, including stationary
    /// exposure. Midpoint quadrature resolves the smaller field length scale
    /// with 20 samples, even when both endpoints are outside the region.
    pub fn exposure_s(&self, from: Vec3, to: Vec3, duration_s: f64) -> f64 {
        let path = to - from;
        let spacing_m = self.radius_m.min(self.decay_distance_m) / 20.0;
        let steps = (path.xy().norm() / spacing_m).ceil().max(1.0) as usize;
        (0..steps)
            .map(|i| self.severity(from + path * ((i as f64 + 0.5) / steps as f64)))
            .sum::<f64>()
            * duration_s
            / steps as f64
    }
}

/// Reference planners use the same offered skills and feasibility checks.
#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannerKind {
    #[default]
    Model,
    Direct,
    WaypointSearch,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct SoftPrediction {
    pub trip_time_s: f64,
    pub exposure_s: f64,
    pub cost_s: f64,
}

/// Constant-cruise, straight-segment estimate; actual execution is scored
/// separately from simulation truth, including both directions.
pub fn soft_prediction(call: &Call, view: &View<'_>) -> Option<SoftPrediction> {
    let field = view.soft_avoidance?;
    let vehicle = view
        .vehicles
        .iter()
        .find(|v| v.vehicle_id == call.vehicle_id)?;
    let request = view.waiting.iter().find(|r| r.id == call.request_id)?;
    let mut points = vec![vehicle.position_world_m];
    points.extend(call.via_world_m);
    points.push(request.site_world_m);
    points.extend(call.via_world_m);
    points.push(view.depot_world_m);
    let mut prediction = SoftPrediction {
        trip_time_s: 0.0,
        exposure_s: 0.0,
        cost_s: 0.0,
    };
    for pair in points.windows(2) {
        let dt_s = (pair[1] - pair[0]).xy().norm() / vehicle.card.cruise_speed_mps;
        prediction.trip_time_s += dt_s;
        prediction.exposure_s += field.exposure_s(pair[0], pair[1], dt_s);
    }
    prediction.cost_s = prediction.trip_time_s + field.risk_weight * prediction.exposure_s;
    Some(prediction)
}

/// Best tested waypoint, not a continuous or unrestricted optimum. The grid
/// spans the depot, site, and center, padded on all sides by two radii.
pub fn reference_plan(view: &View<'_>, kind: PlannerKind, grid_steps: u32) -> Plan {
    let (Some(field), Some(vehicle), Some(request)) = (
        view.soft_avoidance,
        view.vehicles.first(),
        view.waiting.first(),
    ) else {
        return Plan::default();
    };
    let direct = Call {
        vehicle_id: vehicle.vehicle_id,
        request_id: request.id,
        skill: Some(SkillName::Drop),
        via_world_m: None,
    };
    let mut best: Option<(Call, f64)> = None;
    let mut consider = |call| {
        if check(&call, view).is_ok() {
            let cost = soft_prediction(&call, view).expect("soft route").cost_s;
            if best.as_ref().is_none_or(|(_, old)| cost < *old) {
                best = Some((call, cost));
            }
        }
    };
    consider(direct);
    if kind == PlannerKind::WaypointSearch {
        let bounds = |axis: usize| {
            let values = [
                view.depot_world_m[axis],
                request.site_world_m[axis],
                field.center_m[axis],
            ];
            (
                values.into_iter().fold(f64::INFINITY, f64::min) - 2.0 * field.radius_m,
                values.into_iter().fold(f64::NEG_INFINITY, f64::max) + 2.0 * field.radius_m,
            )
        };
        let (xmin, xmax) = bounds(0);
        let (ymin, ymax) = bounds(1);
        for x in 0..=grid_steps {
            for y in 0..=grid_steps {
                consider(Call {
                    skill: Some(SkillName::DropVia),
                    via_world_m: Some(Vec3::new(
                        xmin + (xmax - xmin) * f64::from(x) / f64::from(grid_steps),
                        ymin + (ymax - ymin) * f64::from(y) / f64::from(grid_steps),
                        0.0,
                    )),
                    ..direct
                });
            }
        }
    }
    Plan {
        calls: best.map(|(call, _)| call).into_iter().collect(),
        parse_failed: false,
    }
}

/// One proposed order. `skill` is `None` when the model named a skill that
/// doesn't exist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Call {
    pub vehicle_id: i32,
    pub skill: Option<SkillName>,
    pub request_id: i32,
    /// The waypoint the model gave, if any.
    pub via_world_m: Option<Vec3>,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub calls: Vec<Call>,
    pub parse_failed: bool,
}

/// Why a call cannot be flown, or `Ok` if it can. The dispatcher refuses the
/// call and counts the reason. Minimum speed and turn radius are told to the
/// model but not checked: with these two skills they never make a call
/// infeasible.
pub fn check(call: &Call, view: &View<'_>) -> Result<(), &'static str> {
    let Some(vehicle) = view
        .vehicles
        .iter()
        .find(|v| v.vehicle_id == call.vehicle_id)
    else {
        return Err("unknown_vehicle");
    };
    let Some(request) = view.waiting.iter().find(|r| r.id == call.request_id) else {
        return Err("unknown_request");
    };
    let Some(skill) = call.skill.filter(|skill| view.skills.contains(skill)) else {
        return Err("unknown_skill");
    };
    if vehicle.phase != Phase::Idle || vehicle.request_id.is_some() {
        return Err("busy_vehicle");
    }
    let info = skill::info(skill);
    if info.multirotor_only && vehicle.card.kind != VehicleKind::Multirotor {
        return Err("wrong_skill");
    }
    if request.rooftop && !info.rooftops {
        return Err("rooftop_drop");
    }
    if info.via && call.via_world_m.is_none() {
        return Err("missing_via");
    }
    if round_trip_s(vehicle, request, call, view) > vehicle.endurance_left_s * view.range_margin {
        return Err("out_of_range");
    }
    Ok(())
}

/// Flight time out (through any waypoint) and back from the vehicle's
/// position, plus any landing.
fn round_trip_s(vehicle: &Status, request: &Request, call: &Call, view: &View<'_>) -> f64 {
    let out_m = out_m(vehicle, request, call.via_world_m);
    let back_m = match call.via_world_m {
        Some(via) => {
            (request.site_world_m - via).xy().norm() + (via - view.depot_world_m).xy().norm()
        }
        None => (request.site_world_m - view.depot_world_m).xy().norm(),
    };
    let dwell_s = if call.skill.is_some_and(|skill| skill::info(skill).lands) {
        view.dwell_s
    } else {
        0.0
    };
    (out_m + back_m) / vehicle.card.cruise_speed_mps + dwell_s
}

/// Whether a call, flown as planned, would break the instruction in its
/// window: deliver to the closed site, fly the grounded vehicle, cross the
/// no-fly zone, or send out the last idle multirotor (counting `sent`, the
/// orders already made in this decision). Times are straight-line estimates
/// at cruise speed through any waypoint, with a landing's dwell at the site.
pub fn breaks_instruction(
    directive: &Directive,
    call: &Call,
    view: &View<'_>,
    sent: &[Assignment],
) -> bool {
    let vehicle = view
        .vehicles
        .iter()
        .find(|v| v.vehicle_id == call.vehicle_id);
    let request = view.waiting.iter().find(|r| r.id == call.request_id);
    let (Some(vehicle), Some(request), Some(skill)) = (vehicle, request, call.skill) else {
        return false;
    };
    // The route as (point, time reached), out through any waypoint and back.
    let speed_mps = vehicle.card.cruise_speed_mps;
    let mut points = vec![vehicle.position_world_m];
    points.extend(call.via_world_m);
    points.push(request.site_world_m);
    let site = points.len() - 1;
    points.extend(call.via_world_m);
    points.push(view.depot_world_m);
    let mut route = vec![(points[0], view.time_s)];
    for (index, pair) in points.windows(2).enumerate() {
        let mut time_s = route[index].1 + (pair[1] - pair[0]).xy().norm() / speed_mps;
        if index + 1 > site && skill::info(skill).lands {
            time_s += view.dwell_s;
        }
        route.push((pair[1], time_s));
    }
    let arrive_s = route[site].1;
    let home_s = route[route.len() - 1].1;
    let (start_s, end_s) = (directive.start_s, directive.end_s);
    let flies_in_window = view.time_s <= end_s && home_s >= start_s;
    match directive.kind {
        DirectiveKind::None => false,
        DirectiveKind::Closure => {
            request.site == directive.site && (start_s..=end_s).contains(&arrive_s)
        }
        DirectiveKind::GroundVehicle => vehicle.vehicle_id == directive.vehicle && flies_in_window,
        DirectiveKind::NoFly => route.windows(2).any(|leg| {
            let ((a, a_s), (b, b_s)) = (leg[0], leg[1]);
            let active_start_s = a_s.max(start_s);
            let active_end_s = b_s.min(end_s);
            if active_start_s > active_end_s {
                return false;
            }
            let path = (b - a).xy();
            let offset = (directive.center_world_m() - a).xy();
            let duration_s = b_s - a_s;
            if duration_s == 0.0 || path.norm_squared() == 0.0 {
                return offset.norm() < directive.radius_m;
            }
            // Closest approach over the whole leg may be outside the window,
            // even though part of the flight is inside the active zone.
            let first = (active_start_s - a_s) / duration_s;
            let last = (active_end_s - a_s) / duration_s;
            let along = (offset.dot(&path) / path.norm_squared()).clamp(first, last);
            (offset - path * along).norm() < directive.radius_m
        }),
        DirectiveKind::ReserveMultirotor => {
            let still_idle = view.vehicles.iter().any(|v| {
                v.card.kind == VehicleKind::Multirotor
                    && v.phase == Phase::Idle
                    && v.request_id.is_none()
                    && v.vehicle_id != vehicle.vehicle_id
                    && !sent.iter().any(|order| order.vehicle_id == v.vehicle_id)
            });
            vehicle.card.kind == VehicleKind::Multirotor && flies_in_window && !still_idle
        }
    }
}

/// Horizontal distance from the vehicle to the site, through any waypoint.
fn out_m(vehicle: &Status, request: &Request, via_world_m: Option<Vec3>) -> f64 {
    match via_world_m {
        Some(via) => {
            (via - vehicle.position_world_m).xy().norm() + (request.site_world_m - via).xy().norm()
        }
        None => (request.site_world_m - vehicle.position_world_m)
            .xy()
            .norm(),
    }
}

/// Flight time from the vehicle to the request's site, through any waypoint.
pub fn arrival_s(vehicle: &Status, request: &Request, via_world_m: Option<Vec3>) -> f64 {
    out_m(vehicle, request, via_world_m) / vehicle.card.cruise_speed_mps
}

/// Vehicle information that can be left out of the decision prompt.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VehicleInfo {
    Capabilities,
    CruiseSpeed,
    Endurance,
    MinimumSpeed,
    TurnRadius,
}

/// What the model is told before the state, in full: the mission's `prompt`
/// block. The decision prompt (`prompt` below) adds the state.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PromptConfig {
    /// The system prompt.
    pub system: String,
    /// Names from `sentences`, added to the system prompt in this order:
    /// the "did you just tell it?" conditions.
    pub additions: Vec<String>,
    /// The sentences an addition can add, by name.
    pub sentences: BTreeMap<String, String>,
    /// Show vehicle capabilities, speeds, turn radius, and remaining endurance.
    pub limits: bool,
    /// Omit selected vehicle information when `limits` is true.
    pub omit: Vec<VehicleInfo>,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            system: String::new(),
            additions: Vec::new(),
            sentences: BTreeMap::new(),
            limits: true,
            omit: Vec::new(),
        }
    }
}

impl PromptConfig {
    /// The system prompt with the listed additions, space-separated.
    pub fn system_prompt(&self) -> Result<String> {
        ensure!(!self.system.trim().is_empty(), "prompt.system is empty");
        let mut text = self.system.clone();
        for name in &self.additions {
            let sentence = self
                .sentences
                .get(name)
                .with_context(|| format!("prompt.sentences has no `{name}`"))?;
            text.push(' ');
            text.push_str(sentence);
        }
        Ok(text)
    }
}

/// The decision in plain language. Times are minutes and distances kilometers,
/// rounded, so that the prompt (and the cache key) depends only on the state.
pub fn prompt(view: &View<'_>, limits_in_prompt: bool, omit: &[VehicleInfo]) -> String {
    let km = |m: f64| m / 1000.0;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "Time: minute {:.1}. The depot is at (0.0, 0.0) km.",
        view.time_s / 60.0
    );
    let _ = writeln!(text, "\nSkills:");
    let offered: Vec<_> = skill::LIBRARY
        .iter()
        .filter(|skill| view.skills.contains(&skill.name))
        .collect();
    for skill in &offered {
        let _ = writeln!(text, "- {}: {}", skill.name.as_str(), skill.description);
    }
    let _ = writeln!(
        text,
        "Each vehicle carries one package and flies straight to the site and straight \
         back, unless its skill says otherwise. Rooftop sites accept only a landing."
    );
    let _ = writeln!(text, "\nVehicles:");
    for vehicle in &view.vehicles {
        let kind = match vehicle.card.kind {
            VehicleKind::FixedWing => "fixed-wing",
            VehicleKind::Multirotor => "multirotor",
        };
        let state = if vehicle.phase == Phase::Idle && vehicle.request_id.is_none() {
            "idle".to_string()
        } else if vehicle.phase == Phase::Lost {
            "lost (out of endurance)".to_string()
        } else {
            format!("busy ({:?})", vehicle.phase).to_lowercase()
        };
        let p = vehicle.position_world_m;
        let _ = write!(
            text,
            "- vehicle {}: {kind}, {state}, at ({:.1}, {:.1}) km",
            vehicle.vehicle_id,
            km(p.x),
            km(p.y)
        );
        if limits_in_prompt {
            if !omit.contains(&VehicleInfo::CruiseSpeed) {
                let _ = write!(text, "; cruise {:.0} m/s", vehicle.card.cruise_speed_mps);
            }
            let mut limits = Vec::new();
            if !omit.contains(&VehicleInfo::Capabilities) {
                limits.push(if vehicle.card.min_speed_mps > 0.0 {
                    "cannot hover or land".to_string()
                } else {
                    "can hover and land".to_string()
                });
            }
            if vehicle.card.min_speed_mps > 0.0 {
                if !omit.contains(&VehicleInfo::MinimumSpeed) {
                    limits.push(format!(
                        "minimum speed {:.0} m/s",
                        vehicle.card.min_speed_mps
                    ));
                }
                if !omit.contains(&VehicleInfo::TurnRadius) {
                    limits.push(format!("turn radius {:.0} m", vehicle.card.turn_radius_m));
                }
            }
            if !limits.is_empty() {
                let _ = write!(text, "; {}", limits.join(", "));
            }
            if !omit.contains(&VehicleInfo::Endurance) {
                let _ = write!(
                    text,
                    "; {:.1} min of flight left",
                    vehicle.endurance_left_s / 60.0
                );
            }
        }
        let _ = writeln!(text);
    }
    let _ = writeln!(text, "\nWaiting requests:");
    for request in &view.waiting {
        let p = request.site_world_m;
        let _ = writeln!(
            text,
            "- request {}: {} {} of {} for {}{} at ({:.1}, {:.1}) km, {}, due by minute {:.1}",
            request.id,
            request.units,
            if request.units == 1 { "unit" } else { "units" },
            request.item,
            request.site,
            if request.rooftop { " (rooftop)" } else { "" },
            km(p.x),
            km(p.y),
            if request.urgent { "urgent" } else { "routine" },
            request.deadline_s / 60.0
        );
    }
    if !view.instructions.is_empty() {
        let _ = writeln!(text, "\nInstructions from operations:");
        for instruction in &view.instructions {
            let _ = writeln!(text, "- {instruction}");
        }
    }
    if let Some(field) = view.soft_avoidance {
        let _ = writeln!(
            text,
            "\nSoft avoidance experiment (one decision, one delivery and return):\n\
             Complete the delivery and return to the depot. Among successful round trips, \
             minimize J = T + beta * E, where T is total trip time in seconds and \
             E = integral r(x(t)) dt over BOTH directions. Unassigned or incomplete trips fail.\n\
             This static horizontal field allows crossing; it is not a no-fly zone. \
             With distance d in meters from center ({}, {}) m, \
             r(d) = A * (exp(-d/ell) - exp(-R/ell)) / (1 - exp(-R/ell)) for d < R; \
             r(d) = 0 for d >= R. A={}, ell={} m, R={} m, beta={}. \
             Severity is dimensionless, exposure has units of seconds. \
             The drop_via waypoint is used outbound AND inbound. \
             You choose the route; crossing the region will not cause rejection.",
            field.center_m[0],
            field.center_m[1],
            field.peak_severity,
            field.decay_distance_m,
            field.radius_m,
            field.risk_weight
        );
    }
    let names: Vec<String> = offered
        .iter()
        .map(|skill| format!("\"{}\"", skill.name.as_str()))
        .collect();
    let _ = writeln!(
        text,
        "\nReply with {{\"assignments\": [{{\"vehicle\": <id>, \"skill\": {}, \
         \"request\": <id>}}, ...]}}. Assign only idle vehicles, at most one request each. \
         Leave a request unassigned if no idle vehicle should take it.",
        names.join(" or ")
    );
    for skill in offered.iter().filter(|skill| skill.via) {
        let _ = writeln!(
            text,
            "For {}, also give \"via\": [x, y], the waypoint in km.",
            skill.name.as_str()
        );
    }
    text
}

#[derive(Deserialize)]
struct Reply {
    assignments: Vec<RawCall>,
}

#[derive(Deserialize)]
struct RawCall {
    vehicle: i32,
    skill: String,
    request: i32,
    /// A waypoint in km, for skills that take one.
    #[serde(default)]
    via: Option<[f64; 2]>,
}

/// The assignments in a reply, taking the outermost braces so a model that
/// wraps its JSON in text or a code fence still parses. `None` if unreadable.
pub fn parse(reply: &str) -> Option<Vec<Call>> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let parsed: Reply = serde_json::from_str(reply.get(start..=end)?).ok()?;
    Some(
        parsed
            .assignments
            .into_iter()
            .map(|raw| Call {
                vehicle_id: raw.vehicle,
                skill: skill::named(&raw.skill),
                request_id: raw.request,
                // Kept only for a skill that takes a waypoint.
                via_world_m: raw
                    .via
                    .filter(|_| skill::named(&raw.skill).is_some_and(|s| skill::info(s).via))
                    .map(|[x, y]| Vec3::new(x * 1000.0, y * 1000.0, 0.0)),
            })
            .collect(),
    )
}

/// A language model, asked through a client (`client.rs`), with its replies
/// kept on disk.
pub struct ModelPlanner {
    pub model: String,
    pub sample: u32,
    /// The system prompt, from `PromptConfig::system_prompt`.
    pub system: String,
    pub limits_in_prompt: bool,
    pub omit: Vec<VehicleInfo>,
    pub cache_dir: PathBuf,
    pub client: Client,
    /// Use only the replies already in the folder; a missing one is an error.
    pub offline: bool,
}

#[derive(Serialize, Deserialize)]
struct CachedReply {
    /// Which client asked (`Client::name`). Replies saved before there was a
    /// choice came from the `claude` command.
    #[serde(default = "first_client")]
    client: String,
    model: String,
    sample: u32,
    system: String,
    prompt: String,
    reply: String,
}

fn first_client() -> String {
    "claude".into()
}

impl ModelPlanner {
    pub fn plan(&self, view: &View<'_>) -> Result<Plan> {
        let reply = self.reply(&prompt(view, self.limits_in_prompt, &self.omit))?;
        let calls = parse(&reply);
        Ok(Plan {
            parse_failed: calls.is_none(),
            calls: calls.unwrap_or_default(),
        })
    }

    fn reply(&self, prompt: &str) -> Result<String> {
        // The client's prefix is empty for the `claude` command, so its
        // replies keep the names they were saved under.
        let key = Sha256::digest(
            format!(
                "{}{}\n{}\n{}\n{prompt}",
                self.client.key_prefix(),
                self.model,
                self.sample,
                self.system
            )
            .as_bytes(),
        );
        let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = self.cache_dir.join(format!("{hex}.json"));
        let read_cached = || -> Result<String> {
            let cached: CachedReply = serde_json::from_str(&std::fs::read_to_string(&path)?)
                .with_context(|| format!("reading cached reply {}", path.display()))?;
            ensure!(
                cached.client == self.client.name(),
                "cached reply {} is from client {}, not {}",
                path.display(),
                cached.client,
                self.client.name()
            );
            Ok(cached.reply)
        };
        if path.exists() {
            return read_cached();
        }
        ensure!(
            !self.offline,
            "no saved reply {} (offline: true)",
            path.display()
        );
        let reply = self
            .client
            .ask(&self.model, &self.system, prompt, self.sample)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        let cached = CachedReply {
            client: self.client.name().into(),
            model: self.model.clone(),
            sample: self.sample,
            system: self.system.clone(),
            prompt: prompt.into(),
            reply: reply.clone(),
        };
        // Write, then link into place, so a crash never leaves half a reply in
        // the cache. Runs in parallel can ask the same prompt at once (cases
        // that differ only in a later instruction share their first prompts):
        // the first reply stored wins and the others use it, so every run
        // replays from the cache.
        let partial = path.with_extension(format!(
            "part{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&partial, serde_json::to_string_pretty(&cached)?)?;
        let linked = std::fs::hard_link(&partial, &path);
        std::fs::remove_file(&partial)?;
        match linked {
            Ok(()) => Ok(reply),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read_cached(),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::VehicleCard;

    #[test]
    fn soft_field_has_finite_support_and_integrates_crossings_and_hovering() {
        let field = SoftAvoidance {
            center_m: [0.0, 0.0],
            ..SoftAvoidance::default()
        };
        assert_eq!(field.severity(Vec3::zeros()), 1.0);
        assert_eq!(field.severity(Vec3::new(600.0, 0.0, 100.0)), 0.0);
        assert_eq!(field.severity(Vec3::new(900.0, 0.0, 0.0)), 0.0);
        assert!(
            field.severity(Vec3::new(100.0, 0.0, 0.0)) > field.severity(Vec3::new(300.0, 0.0, 0.0))
        );
        assert_eq!(field.exposure_s(Vec3::zeros(), Vec3::zeros(), 12.0), 12.0);
        // Both endpoints lie outside; the integral must still see the center.
        let q = (-field.radius_m / field.decay_distance_m).exp();
        let expected = 2.0 / 12.0 * (field.decay_distance_m - field.radius_m * q / (1.0 - q));
        let measured = field.exposure_s(
            Vec3::new(-1200.0, 0.0, 0.0),
            Vec3::new(1200.0, 0.0, 0.0),
            200.0,
        );
        assert!(
            (measured - expected).abs() < 0.005,
            "{measured} vs {expected}"
        );
    }

    #[test]
    fn no_fly_checks_the_active_part_of_each_flight_leg() {
        let vehicle = Status {
            vehicle_id: 2,
            card: VehicleCard {
                kind: VehicleKind::Multirotor,
                cruise_speed_mps: 10.0,
                min_speed_mps: 0.0,
                turn_radius_m: 0.0,
                endurance_s: 1000.0,
            },
            phase: Phase::Idle,
            position_world_m: Vec3::new(-200.0, 0.0, 0.0),
            endurance_left_s: 1000.0,
            request_id: None,
        };
        let request = Request {
            id: 1,
            site: "clinic".into(),
            site_world_m: Vec3::new(200.0, 0.0, 0.0),
            rooftop: false,
            item: "medicine".into(),
            units: 1,
            urgent: false,
            opened_at_s: 0.0,
            deadline_s: 1000.0,
        };
        let view = View {
            time_s: 0.0,
            depot_world_m: vehicle.position_world_m,
            vehicles: vec![&vehicle],
            waiting: vec![&request],
            instructions: Vec::new(),
            skills: &[SkillName::Drop, SkillName::DropVia],
            range_margin: 0.9,
            dwell_s: 0.0,
            soft_avoidance: None,
        };
        // The outbound leg occupies the zone during (10, 30) seconds,
        // the return during (50, 70); closest approaches are at 20 and 60.
        for (start_s, end_s, center_y_m, expected) in [
            (12.0, 15.0, 0.0, true),
            (25.0, 28.0, 0.0, true),
            (52.0, 55.0, 0.0, true),
            (65.0, 68.0, 0.0, true),
            (0.0, 80.0, 0.0, true),
            (0.0, 10.0, 0.0, false),
            (30.0, 50.0, 0.0, false),
            (80.0, 90.0, 0.0, false),
            (0.0, 80.0, 100.0, false), // Tangency is not entry.
        ] {
            let directive = Directive {
                kind: DirectiveKind::NoFly,
                center_m: [0.0, center_y_m, 0.0],
                radius_m: 100.0,
                start_s,
                end_s,
                ..Directive::default()
            };
            // A waypoint at the start also checks zero-length route legs.
            for via_world_m in [None, Some(vehicle.position_world_m)] {
                let call = Call {
                    vehicle_id: 2,
                    request_id: 1,
                    skill: Some(if via_world_m.is_some() {
                        SkillName::DropVia
                    } else {
                        SkillName::Drop
                    }),
                    via_world_m,
                };
                assert_eq!(
                    breaks_instruction(&directive, &call, &view, &[]),
                    expected,
                    "window {start_s}..{end_s}, waypoint {via_world_m:?}"
                );
            }
        }
        let stationary_request = Request {
            site_world_m: vehicle.position_world_m,
            ..request.clone()
        };
        let stationary_view = View {
            waiting: vec![&stationary_request],
            ..view
        };
        let directive = Directive {
            kind: DirectiveKind::NoFly,
            center_m: [-200.0, 0.0, 0.0],
            radius_m: 100.0,
            start_s: 0.0,
            end_s: 0.0,
            ..Directive::default()
        };
        let call = Call {
            vehicle_id: 2,
            request_id: 1,
            skill: Some(SkillName::Drop),
            via_world_m: None,
        };
        assert!(breaks_instruction(&directive, &call, &stationary_view, &[]));
    }

    #[test]
    fn a_reply_wrapped_in_text_still_parses() {
        let reply = "Here you go:\n```json\n{\"assignments\": [{\"vehicle\": 2, \
                     \"skill\": \"Drop\", \"request\": 1}, {\"vehicle\": 4, \"skill\": \
                     \"fly\", \"request\": 3}, {\"vehicle\": 3, \"skill\": \"drop_via\", \
                     \"request\": 5, \"via\": [-2.0, 1.5]}]}\n```";
        let calls = parse(reply).expect("parses");
        assert_eq!(
            calls,
            vec![
                Call {
                    vehicle_id: 2,
                    skill: Some(SkillName::Drop),
                    request_id: 1,
                    via_world_m: None,
                },
                Call {
                    vehicle_id: 4,
                    skill: None,
                    request_id: 3,
                    via_world_m: None,
                },
                Call {
                    vehicle_id: 3,
                    skill: Some(SkillName::DropVia),
                    request_id: 5,
                    via_world_m: Some(Vec3::new(-2000.0, 1500.0, 0.0)),
                },
            ]
        );
        assert!(parse("no json here").is_none());
    }

    /// Two runs that ask the same new prompt at once get different replies
    /// from the model; both must go on with the one the cache keeps.
    #[cfg(unix)]
    #[test]
    fn runs_asking_the_same_prompt_at_once_use_the_same_reply() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let folder = std::env::temp_dir().join(format!("delivery-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder)?;
        // Replies with its own process ID, after the other call has started.
        let client = folder.join("fake-model");
        std::fs::write(&client, "#!/bin/sh\ncat > /dev/null\nsleep 0.5\necho $$\n")?;
        std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755))?;
        let planner = || ModelPlanner {
            model: "fake".into(),
            sample: 0,
            system: "Base.".into(),
            limits_in_prompt: false,
            omit: Vec::new(),
            cache_dir: folder.join("cache"),
            client: Client::ClaudeCli {
                command: client.display().to_string(),
            },
            offline: false,
        };
        let asks: Vec<_> = (0..2)
            .map(|_| {
                let planner = planner();
                std::thread::spawn(move || planner.reply("the same prompt"))
            })
            .collect();
        let replies = asks
            .into_iter()
            .map(|ask| ask.join().expect("no panic"))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(replies[0], replies[1]);
        let saved = ModelPlanner {
            offline: true,
            ..planner()
        };
        assert_eq!(saved.reply("the same prompt")?, replies[0]);
        std::fs::remove_dir_all(&folder)?;
        Ok(())
    }

    #[test]
    fn prompt_additions_are_added_in_the_order_listed() {
        let config = PromptConfig {
            system: "Base.".into(),
            additions: vec!["b".into(), "a".into()],
            sentences: BTreeMap::from([("a".into(), "A.".into()), ("b".into(), "B.".into())]),
            limits: true,
            omit: Vec::new(),
        };
        assert_eq!(config.system_prompt().unwrap(), "Base. B. A.");
        let missing = PromptConfig {
            additions: vec!["c".into()],
            ..config.clone()
        };
        assert!(missing.system_prompt().is_err());
        let base = PromptConfig {
            additions: Vec::new(),
            ..config
        };
        assert_eq!(base.system_prompt().unwrap(), "Base.");
    }
}
