//! The depot's dispatcher: open the episode's requests over time and ask a
//! language model to assign idle vehicles to them. Autonomy phase: read vehicle reports and deliveries,
//! decide when something changed, check every proposed call, send the ones a
//! vehicle can fly, and hold the depot still.
//!
//! The requests are drawn at the start from random numbers keyed by request
//! index, so each seed is one episode and every planner meets the same one. A
//! decision happens when a request opens, a vehicle comes free, or an
//! instruction is announced, while work and an idle vehicle are both waiting,
//! and again every `retry_interval_s` while they still are. An instruction's
//! text goes to the model from its announcement to its end. Every decision is
//! published in full on `DispatchDecision` for tracing.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use ripple_core::Vec3;
use ripple_core::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, PluginRandom, Port, Ports, Unit,
    Update,
};
use serde::Deserialize;

use crate::directive::Directive;
use crate::messages::{
    ASSIGNMENT_TOPIC, Assignment, CallRecord, DECISION_TOPIC, DELIVERED_TOPIC, DIRECTIVE_TOPIC,
    Decision, Delivered, Phase, REQUEST_TOPIC, Request, STATUS_TOPIC, SkillName, Status, Waiting,
};
use crate::planner::{self, ModelPlanner, View};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Site {
    name: String,
    position_m: [f64; 3],
    #[serde(default)]
    rooftop: bool,
}

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DispatcherConfig {
    model: String,
    sample: u32,
    limits_in_prompt: bool,
    cache_dir: String,
    command: String,
    /// Delivery sites. Required.
    sites: Vec<Site>,
    requests: u32,
    /// Requests open uniformly over this span from the start.
    request_window_s: f64,
    urgent_share: f64,
    urgent_deadline_s: f64,
    routine_deadline_s: f64,
    items: Vec<String>,
    retry_interval_s: f64,
    /// A round trip may use at most this share of a vehicle's endurance left.
    range_margin: f64,
    /// The couriers' landing time, for the range check.
    dwell_s: f64,
    /// An order the vehicle hasn't taken up by then is released.
    confirm_timeout_s: f64,
    /// A mid-mission instruction from operations, if any.
    directive: Directive,
    /// The skills offered to the model, from the library in `skill.rs`.
    skills: Vec<SkillName>,
    /// Refuse calls that would break the instruction, as if the rule were
    /// enforced in code (a reference, not a planner).
    enforce_instruction: bool,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            model: "claude-haiku-5-5".into(),
            sample: 0,
            limits_in_prompt: true,
            cache_dir: "runs/llm-cache".into(),
            command: "claude".into(),
            sites: Vec::new(),
            requests: 12,
            request_window_s: 1500.0,
            urgent_share: 0.4,
            urgent_deadline_s: 1200.0,
            routine_deadline_s: 2700.0,
            items: ["blood", "vaccines", "antivenom", "insulin"]
                .map(String::from)
                .to_vec(),
            retry_interval_s: 300.0,
            range_margin: 0.9,
            dwell_s: 30.0,
            confirm_timeout_s: 5.0,
            directive: Directive::default(),
            skills: vec![SkillName::Drop, SkillName::Land],
            enforce_instruction: false,
        }
    }
}

pub struct Dispatcher {
    model: ModelPlanner,
    sites: Vec<Site>,
    request_count: u32,
    request_window_s: f64,
    urgent_share: f64,
    urgent_deadline_s: f64,
    routine_deadline_s: f64,
    items: Vec<String>,
    retry_interval_s: f64,
    range_margin: f64,
    dwell_s: f64,
    confirm_timeout_s: f64,
    directive: Directive,
    announced: bool,
    skills: Vec<SkillName>,
    enforce_instruction: bool,
    /// Every request of the episode, by opening time; IDs count from 1.
    requests: Vec<Request>,
    opened: usize,
    fleet: BTreeMap<i32, Status>,
    /// Orders sent but not yet taken up: request to (vehicle, sent at).
    pending: BTreeMap<i32, (i32, f64)>,
    /// Requests a vehicle has taken or delivered.
    carried: BTreeSet<i32>,
    delivered: BTreeSet<i32>,
    changed: bool,
    last_decision_s: f64,
}

impl Plugin for Dispatcher {
    type Config = DispatcherConfig;

    fn configure(params: &PluginParams<'_>) -> Result<DispatcherConfig> {
        let config: DispatcherConfig = params.parse()?;
        ensure!(!config.sites.is_empty(), "Dispatcher needs sites");
        ensure!(!config.items.is_empty(), "Dispatcher needs items");
        ensure!(
            config.request_window_s >= 0.0 && (0.0..=1.0).contains(&config.urgent_share),
            "Dispatcher request window must be nonnegative and urgent_share in [0, 1]"
        );
        ensure!(
            config.range_margin > 0.0 && config.retry_interval_s > 0.0,
            "Dispatcher range margin and retry interval must be positive"
        );
        config.directive.validate()?;
        ensure!(
            !config.skills.is_empty(),
            "Dispatcher needs at least one skill"
        );
        Ok(config)
    }

    fn new(config: &DispatcherConfig) -> Self {
        Self {
            model: ModelPlanner {
                model: config.model.clone(),
                sample: config.sample,
                limits_in_prompt: config.limits_in_prompt,
                cache_dir: config.cache_dir.clone().into(),
                command: config.command.clone(),
            },
            sites: config.sites.clone(),
            request_count: config.requests,
            request_window_s: config.request_window_s,
            urgent_share: config.urgent_share,
            urgent_deadline_s: config.urgent_deadline_s,
            routine_deadline_s: config.routine_deadline_s,
            items: config.items.clone(),
            retry_interval_s: config.retry_interval_s,
            range_margin: config.range_margin,
            dwell_s: config.dwell_s,
            confirm_timeout_s: config.confirm_timeout_s,
            directive: config.directive.clone(),
            announced: false,
            skills: config.skills.clone(),
            enforce_instruction: config.enforce_instruction,
            requests: Vec::new(),
            opened: 0,
            fleet: BTreeMap::new(),
            pending: BTreeMap::new(),
            carried: BTreeSet::new(),
            delivered: BTreeSet::new(),
            changed: false,
            last_decision_s: f64::NEG_INFINITY,
        }
    }

    fn ports(_: &DispatcherConfig) -> Ports {
        let mut ports = Ports::default();
        for name in ["velocity_x", "velocity_y", "velocity_z"] {
            ports = ports.output(Port::new(name, Unit::MetersPerSecond, Frame::World));
        }
        ports
    }
}

impl Autonomy for Dispatcher {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        context
            .messages
            .subscribe::<Status>("GlobalNetwork", STATUS_TOPIC)?;
        context
            .messages
            .subscribe::<Delivered>("GlobalNetwork", DELIVERED_TOPIC)?;
        self.requests = self.draw_requests(&context.random);
        Ok(())
    }

    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let now_s = context.time.time_s;

        // Input: reports, deliveries, and requests that open now.
        for message in context
            .messages
            .receive::<Status>("GlobalNetwork", STATUS_TOPIC)?
        {
            let status = *message.value;
            let was_idle = self.fleet.get(&status.vehicle_id).map(|s| s.phase);
            if status.phase == Phase::Idle && was_idle != Some(Phase::Idle) {
                self.changed = true;
            }
            if let Some(request_id) = status.request_id {
                self.carried.insert(request_id);
                self.pending.remove(&request_id);
            }
            self.fleet.insert(status.vehicle_id, status);
        }
        for message in context
            .messages
            .receive::<Delivered>("GlobalNetwork", DELIVERED_TOPIC)?
        {
            self.delivered.insert(message.value.request_id);
        }
        while self.opened < self.requests.len() && self.requests[self.opened].opened_at_s <= now_s {
            let request = self.requests[self.opened].clone();
            context
                .messages
                .publish("GlobalNetwork", REQUEST_TOPIC, request)?;
            self.opened += 1;
            self.changed = true;
        }
        if self.directive.active() && !self.announced && now_s >= self.directive.announce_s {
            context
                .messages
                .publish("GlobalNetwork", DIRECTIVE_TOPIC, self.directive.clone())?;
            self.announced = true;
            self.changed = true;
        }
        let timeout_s = self.confirm_timeout_s;
        let before = self.pending.len();
        self.pending
            .retain(|_, &mut (_, sent_s)| now_s <= sent_s + timeout_s);
        self.changed |= self.pending.len() != before;

        // Calculation and output: decide when something changed.
        let view = self.view(now_s, context.state.position_world_m);
        let idle = view
            .vehicles
            .iter()
            .any(|v| v.phase == Phase::Idle && v.request_id.is_none());
        let due = self.changed || now_s >= self.last_decision_s + self.retry_interval_s;
        if !view.waiting.is_empty() && idle && due {
            let plan = self.model.plan(&view)?;
            let mut decision = Decision {
                time_s: now_s,
                instructions: view.instructions.clone(),
                waiting: view
                    .waiting
                    .iter()
                    .map(|r| Waiting {
                        request_id: r.id,
                        site: r.site.clone(),
                        urgent: r.urgent,
                        deadline_s: r.deadline_s,
                    })
                    .collect(),
                idle: view
                    .vehicles
                    .iter()
                    .filter(|v| v.phase == Phase::Idle && v.request_id.is_none())
                    .map(|v| v.vehicle_id)
                    .collect(),
                parse_failed: plan.parse_failed,
                calls: Vec::new(),
            };
            let mut orders = Vec::new();
            for call in &plan.calls {
                let repeated = orders.iter().any(|o: &Assignment| {
                    o.vehicle_id == call.vehicle_id || o.request_id == call.request_id
                });
                let verdict = if repeated {
                    Err("duplicate")
                } else {
                    planner::check(call, &view)
                };
                // Whether a call that can be flown would break the instruction
                // shown to the model; refused only with `enforce_instruction`.
                let breaks_instruction = verdict.is_ok()
                    && !view.instructions.is_empty()
                    && planner::breaks_instruction(&self.directive, call, &view, &orders);
                let verdict = if breaks_instruction && self.enforce_instruction {
                    Err("breaks_instruction")
                } else {
                    verdict
                };
                let vehicle = view
                    .vehicles
                    .iter()
                    .find(|v| v.vehicle_id == call.vehicle_id);
                let request = view.waiting.iter().find(|r| r.id == call.request_id);
                let arrival_s = vehicle
                    .zip(request)
                    .map(|(v, r)| planner::arrival_s(v, r, call.via_world_m));
                if let (Ok(()), Some(skill), Some(request)) = (verdict, call.skill, request) {
                    orders.push(Assignment {
                        vehicle_id: call.vehicle_id,
                        request_id: call.request_id,
                        skill,
                        site_world_m: request.site_world_m,
                        via_world_m: call.via_world_m,
                    });
                }
                decision.calls.push(CallRecord {
                    vehicle_id: call.vehicle_id,
                    skill: call.skill,
                    request_id: call.request_id,
                    via_world_m: call.via_world_m,
                    outcome: verdict.err().unwrap_or("assigned").into(),
                    breaks_instruction,
                    arrival_s,
                    slack_s: request
                        .zip(arrival_s)
                        .map(|(r, a)| r.deadline_s - now_s - a),
                });
            }
            for order in orders {
                self.pending
                    .insert(order.request_id, (order.vehicle_id, now_s));
                if let Some(status) = self.fleet.get_mut(&order.vehicle_id) {
                    // Busy until its next report says otherwise.
                    status.request_id = Some(order.request_id);
                }
                context
                    .messages
                    .publish("GlobalNetwork", ASSIGNMENT_TOPIC, order)?;
            }
            context
                .messages
                .publish("GlobalNetwork", DECISION_TOPIC, decision)?;
            self.changed = false;
            self.last_decision_s = now_s;
        }
        for name in ["velocity_x", "velocity_y", "velocity_z"] {
            io.write(name, 0.0)?;
        }
        Ok(Update::Applied)
    }
}

impl Dispatcher {
    fn view(&self, now_s: f64, depot_world_m: Vec3) -> View<'_> {
        View {
            time_s: now_s,
            depot_world_m,
            vehicles: self.fleet.values().collect(),
            waiting: self.requests[..self.opened]
                .iter()
                .filter(|r| {
                    !self.delivered.contains(&r.id)
                        && !self.carried.contains(&r.id)
                        && !self.pending.contains_key(&r.id)
                })
                .collect(),
            instructions: if self.announced && now_s <= self.directive.end_s {
                vec![self.directive.text()]
            } else {
                Vec::new()
            },
            skills: &self.skills,
            range_margin: self.range_margin,
            dwell_s: self.dwell_s,
        }
    }

    /// The episode's requests, each from its own keyed random numbers.
    fn draw_requests(&self, random: &PluginRandom) -> Vec<Request> {
        let key = |name: &str, index: u32| [PluginRandom::name(name), u64::from(index)];
        let pick = |u: f64, count: usize| ((u * count as f64) as usize).min(count - 1);
        let mut drawn: Vec<(f64, u32)> = (0..self.request_count)
            .map(|index| {
                (
                    random.uniform(&key("time", index)) * self.request_window_s,
                    index,
                )
            })
            .collect();
        drawn.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        drawn
            .into_iter()
            .enumerate()
            .map(|(order, (opened_at_s, index))| {
                let site = &self.sites[pick(random.uniform(&key("site", index)), self.sites.len())];
                let urgent = random.uniform(&key("urgent", index)) < self.urgent_share;
                let units = 1 + pick(random.uniform(&key("units", index)), 3) as u32;
                Request {
                    id: order as i32 + 1,
                    site: site.name.clone(),
                    site_world_m: Vec3::from(site.position_m),
                    rooftop: site.rooftop,
                    item: self.items[pick(random.uniform(&key("item", index)), self.items.len())]
                        .clone(),
                    units,
                    urgent,
                    opened_at_s,
                    deadline_s: opened_at_s
                        + if urgent {
                            self.urgent_deadline_s
                        } else {
                            self.routine_deadline_s
                        },
                }
            })
            .collect()
    }
}
