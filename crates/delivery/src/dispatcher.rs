//! The region's dispatcher, on one entity: it holds the hubs and their stock,
//! opens the day's requests and events, calls the fleet manager, applies its
//! orders, and assigns waiting requests to idle couriers. Autonomy phase: read
//! vehicle reports and deliveries, decide, send assignments and relocations,
//! and hold the entity still.
//!
//! The day is drawn once at the start from random numbers keyed by hub and
//! request index, so a seed is one day, the same however it is managed. The
//! classical assignment is the same under every fleet manager: each waiting
//! request, in priority then deadline order, goes to the idle vehicle at an
//! eligible hub with the right skill, the stock, and the endurance for the
//! round trip, that arrives soonest. Orders change which hubs are eligible,
//! what is grounded or reserved, what comes first, where flights detour, and
//! where vehicles and stock are.
//!
//! The fleet manager is called when events start and every `check_in_s`.
//! Its orders are checked for sense (a hub that exists, a site that exists)
//! under every interface; the `checker` interface also refuses orders that
//! break a rule or cannot be flown, with the reason. With `feedback`, the
//! next call reports each order's estimate and what was flown since.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::{Context, Result, ensure};
use ripple_core::Vec3;
use ripple_core::plugin::{
    AgentContext, Autonomy, Drawing, Frame, PluginIo, PluginParams, PluginRandom, Port, Ports, Unit,
};
use serde::Deserialize;

use crate::area::{Area, detour, tagged};
use crate::client::{Client, Provider, Via};
use crate::event::{Event, EventKind, EventsConfig, hub_list};
use crate::messages::{
    ASSIGNMENT_TOPIC, Assignment, CALL_TOPIC, DELIVERED_TOPIC, Delivered, EVENT_TOPIC,
    MISSED_TOPIC, ManagerCall, ManagerKind, MissReason, Missed, ORDER_TOPIC, OrderRecord, Phase,
    RELOCATE_TOPIC, REQUEST_TOPIC, Relocate, Request, RequestKind, STATUS_TOPIC, SkillName, Status,
    Urgency, VehicleClass,
};
use crate::model::{Asked, ModelManager};
use crate::order::{
    Grounded, Order, Priority, QUERY_SYNTAX, Reserve, Rules, SYNTAX, TOP_SYNTAX, VETO_SYNTAX,
    parse_veto,
};
use crate::policy::Policy;
use crate::region::{Region, RegionConfig, SiteKind, horizontal_m};
use crate::replanner;
use crate::skill;
use crate::view::{HubView, Snapshot, ViewKind};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StockConfig {
    /// The blood types.
    pub items: Vec<String>,
    /// Starting units of each item at each hub.
    pub units_per_item: u32,
    pub restock_interval_s: f64,
    pub restock_units: u32,
    /// The most units one flight carries.
    pub package_units: u32,
}

impl Default for StockConfig {
    fn default() -> Self {
        Self {
            items: ["O-", "O+", "A+", "B+"].map(String::from).to_vec(),
            units_per_item: 10,
            restock_interval_s: 7200.0,
            restock_units: 6,
            package_units: 4,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RequestsConfig {
    pub per_hub_per_hour: f64,
    /// Requests open until this time; the day. Events are drawn before it.
    pub until_s: f64,
    pub urgent_share: f64,
    pub trauma_share: f64,
    pub routine_deadline_s: f64,
    pub urgent_deadline_s: f64,
    pub trauma_deadline_s: f64,
}

impl Default for RequestsConfig {
    fn default() -> Self {
        Self {
            per_hub_per_hour: 6.0,
            until_s: 28_800.0,
            urgent_share: 0.3,
            trauma_share: 0.1,
            routine_deadline_s: 7200.0,
            urgent_deadline_s: 1800.0,
            trauma_deadline_s: 900.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventsAs {
    Text,
    Structured,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptedOrder {
    pub at_min: f64,
    #[serde(deserialize_with = "tagged")]
    pub order: Order,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PromptConfig {
    /// The system prompt, written once and frozen.
    pub system: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManagerConfig {
    pub kind: ManagerKind,
    pub check_in_s: f64,
    /// How the model reads events: their text, or their fields.
    pub events_as: EventsAs,
    /// The interface: what the model sees of the fleet, whether its orders
    /// come back with estimates and results, and whether a checker refuses
    /// orders that break a rule or cannot be flown.
    pub view: ViewKind,
    pub feedback: bool,
    pub checker: bool,
    /// With feedback: after each relocation or stock flight, the donor hub's
    /// idle vehicles now and its late deliveries since, as one fixed line,
    /// so the move's cost is in the next prompt as a fact.
    pub cost_feedback: bool,
    /// Refuses a reposition that leaves the donor hub under half its
    /// vehicles of the class, or a transfer that leaves it under a quarter
    /// of its starting stock: the replanner's own keep rules as a rule.
    pub guard: bool,
    pub max_queries: u32,
    /// A call budget that does not grow with the fleet: calls are at least
    /// 3600 / calls_per_hour seconds apart, and events that start between
    /// calls wait for the next one, together. 0 means a call per event.
    pub calls_per_hour: f64,
    /// A fleet manager per cluster of hubs instead of one over the region:
    /// clusters are square blocks of this many hubs a side (0: one manager).
    /// Each is called with its own hubs' view and events and may only name
    /// its own hubs; `top_kind` puts a manager above them.
    pub cluster_side: u32,
    /// Hand-written orders for `kind: scripted`.
    pub scripted: Vec<ScriptedOrder>,
    /// The rules file for `kind: policy` (`policy.rs`), a path from the
    /// working directory.
    pub policy: String,
    /// The manager above the clusters (`none`, `replanner` or `model`): it
    /// sees each cluster's totals and moves vehicles and stock between
    /// clusters, after the cluster managers have been called. Needs
    /// `cluster_side`.
    pub top_kind: ManagerKind,
    /// The model for a `top_kind: model`; empty means `model`.
    pub top_model: String,
    /// Whose model, and how it is reached (`client.rs`).
    pub provider: Provider,
    pub via: Option<Via>,
    pub model: String,
    pub sample: u32,
    pub prompt: PromptConfig,
    pub cache_dir: String,
    pub offline: bool,
    pub command: Option<String>,
    pub host: Option<String>,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            kind: ManagerKind::None,
            check_in_s: 1800.0,
            events_as: EventsAs::Text,
            view: ViewKind::Summaries,
            feedback: false,
            checker: false,
            cost_feedback: false,
            guard: false,
            max_queries: 6,
            calls_per_hour: 0.0,
            cluster_side: 0,
            scripted: Vec::new(),
            policy: String::new(),
            top_kind: ManagerKind::None,
            top_model: String::new(),
            provider: Provider::Claude,
            via: None,
            model: "claude-haiku-5-5".into(),
            sample: 0,
            prompt: PromptConfig::default(),
            cache_dir: "runs/llm-cache".into(),
            offline: false,
            command: None,
            host: None,
        }
    }
}

impl ManagerConfig {
    fn policy(&self) -> Result<Option<Policy>> {
        if self.kind != ManagerKind::Policy {
            return Ok(None);
        }
        ensure!(!self.policy.is_empty(), "manager.policy names no file");
        let text = std::fs::read_to_string(&self.policy)
            .with_context(|| format!("reading manager.policy {}", self.policy))?;
        Policy::parse(&text).map(Some)
    }

    fn client(&self) -> Result<Client> {
        Client::new(
            self.provider,
            self.via,
            self.command.as_deref(),
            self.host.as_deref(),
            self.offline || (!self.kind.uses_model() && self.top_kind != ManagerKind::Model),
        )
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DispatcherConfig {
    pub region: RegionConfig,
    pub stock: StockConfig,
    pub requests: RequestsConfig,
    pub events: EventsConfig,
    pub manager: ManagerConfig,
    /// A round trip may use at most this share of a vehicle's endurance left.
    pub range_margin: f64,
    /// The couriers' landing time, for the range check.
    pub dwell_s: f64,
    /// An order the vehicle hasn't taken up by then is released.
    pub confirm_timeout_s: f64,
    /// Decide at most this often, and at least this often while work waits.
    pub decision_interval_s: f64,
    pub retry_interval_s: f64,
    /// How far outside a closed area a detour waypoint sits.
    pub detour_margin_m: f64,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            region: RegionConfig::default(),
            stock: StockConfig::default(),
            requests: RequestsConfig::default(),
            events: EventsConfig::default(),
            manager: ManagerConfig::default(),
            range_margin: 0.9,
            dwell_s: 30.0,
            confirm_timeout_s: 5.0,
            decision_interval_s: 5.0,
            retry_interval_s: 60.0,
            detour_margin_m: 500.0,
        }
    }
}

/// When a vehicle was told to relocate, until it reports doing so.
struct PendingRelocation {
    sent_s: f64,
}

/// A repositioned vehicle due back at its home hub.
#[derive(Clone, Debug)]
struct Return {
    vehicle_id: i32,
    home: u32,
    home_world_m: Vec3,
    until_s: f64,
}

/// What happened since the fleet manager's last call, for feedback.
#[derive(Default)]
struct Tally {
    delivered: u32,
    on_time: u32,
    late: u32,
    missed: BTreeMap<MissReason, u32>,
    lost: u32,
}

pub struct Dispatcher {
    config: DispatcherConfig,
    region: Region,
    /// Units of each item at each hub, by hub number.
    stock: BTreeMap<u32, BTreeMap<String, u32>>,
    next_restock_s: f64,
    /// Every request known, by ID: the day's schedule and the transfers.
    requests: BTreeMap<i32, Request>,
    /// The day's request IDs in opening order, and how many have opened.
    schedule: Vec<i32>,
    opened: usize,
    /// Open requests: not yet carried, delivered, or waiting on an order.
    open: BTreeSet<i32>,
    next_transfer_id: i32,
    fleet: BTreeMap<i32, Status>,
    /// Orders sent but not yet taken up: request to (vehicle, sent at).
    pending: BTreeMap<i32, (i32, f64)>,
    pending_relocations: BTreeMap<i32, PendingRelocation>,
    /// Repositioned vehicles due home: when, and the home hub.
    returns: Vec<Return>,
    carried: BTreeSet<i32>,
    delivered: BTreeSet<i32>,
    missed: BTreeSet<i32>,
    events: Vec<Event>,
    announced: usize,
    rules: Rules,
    model: Option<ModelManager>,
    policy: Option<Policy>,
    /// The model above the clusters, and what it is told about its orders.
    top_model: Option<ModelManager>,
    top_feedback: Vec<String>,
    /// Relocations and stock flights applied since the last call, for the
    /// cost feedback: the order's words and the donor hub.
    move_costs: Vec<(String, u32)>,
    /// Late deliveries since the last call, by hub.
    late_by_hub: BTreeMap<u32, u32>,
    scripted_fired: usize,
    last_call_s: f64,
    /// Events that started since the last call, waiting for the next one.
    queued_events: Vec<usize>,
    feedback: Vec<String>,
    tally: Tally,
    changed: bool,
    last_decision_s: f64,
}

impl Autonomy for Dispatcher {
    type Config = DispatcherConfig;

    fn configure(params: &PluginParams) -> Result<DispatcherConfig> {
        let config: DispatcherConfig = params.parse()?;
        let region = Region::new(&config.region)?;
        ensure!(
            config.manager.cluster_side == 0 || region.side() % config.manager.cluster_side == 0,
            "manager.cluster_side must divide the region's side ({})",
            region.side()
        );
        ensure!(
            !config.stock.items.is_empty(),
            "stock.items must name the blood types"
        );
        ensure!(
            config.stock.package_units > 0 && config.stock.restock_interval_s > 0.0,
            "stock.package_units and stock.restock_interval_s must be positive"
        );
        let requests = &config.requests;
        ensure!(
            requests.per_hub_per_hour >= 0.0 && requests.until_s >= 0.0,
            "requests.per_hub_per_hour and requests.until_s must be nonnegative"
        );
        ensure!(
            requests.urgent_share >= 0.0
                && requests.trauma_share >= 0.0
                && requests.urgent_share + requests.trauma_share <= 1.0,
            "requests.urgent_share and trauma_share must be nonnegative and sum to at most 1"
        );
        config.events.validate()?;
        ensure!(
            config.range_margin > 0.0
                && config.decision_interval_s > 0.0
                && config.retry_interval_s > 0.0
                && config.manager.check_in_s > 0.0,
            "range_margin, decision_interval_s, retry_interval_s and manager.check_in_s must be positive"
        );
        ensure!(
            config.manager.calls_per_hour >= 0.0,
            "manager.calls_per_hour must be 0 (a call per event) or positive"
        );
        ensure!(
            matches!(
                config.manager.top_kind,
                ManagerKind::None | ManagerKind::Replanner | ManagerKind::Model
            ),
            "manager.top_kind must be none, replanner or model"
        );
        ensure!(
            config.manager.top_kind == ManagerKind::None || config.manager.cluster_side > 0,
            "manager.top_kind needs manager.cluster_side"
        );
        if config.manager.top_kind == ManagerKind::Model {
            ensure!(
                !config.manager.prompt.system.trim().is_empty(),
                "manager.prompt.system is empty"
            );
        }
        if config.manager.kind.uses_model() {
            ensure!(
                !config.manager.prompt.system.trim().is_empty(),
                "manager.prompt.system is empty"
            );
            if config.manager.events_as == EventsAs::Text {
                config.events.texts_complete()?;
            }
        }
        config.manager.client()?;
        config.manager.policy()?;
        Ok(config)
    }

    fn new(config: &DispatcherConfig) -> Self {
        let region = Region::new(&config.region).expect("checked in configure");
        let stock = region
            .hubs
            .iter()
            .map(|hub| {
                (
                    hub.id,
                    config
                        .stock
                        .items
                        .iter()
                        .map(|item| (item.clone(), config.stock.units_per_item))
                        .collect(),
                )
            })
            .collect();
        let model = config.manager.kind.uses_model().then(|| ModelManager {
            model: config.manager.model.clone(),
            sample: config.manager.sample,
            system: config.manager.prompt.system.clone(),
            cache_dir: config.manager.cache_dir.clone().into(),
            client: config.manager.client().expect("checked in configure"),
            offline: config.manager.offline,
            max_queries: if config.manager.view == ViewKind::Queries {
                config.manager.max_queries
            } else {
                0
            },
        });
        let top_model = (config.manager.top_kind == ManagerKind::Model).then(|| ModelManager {
            model: if config.manager.top_model.is_empty() {
                config.manager.model.clone()
            } else {
                config.manager.top_model.clone()
            },
            sample: config.manager.sample,
            system: config.manager.prompt.system.clone(),
            cache_dir: config.manager.cache_dir.clone().into(),
            client: config.manager.client().expect("checked in configure"),
            offline: config.manager.offline,
            max_queries: 0,
        });
        Self {
            config: config.clone(),
            region,
            stock,
            next_restock_s: config.stock.restock_interval_s,
            requests: BTreeMap::new(),
            schedule: Vec::new(),
            opened: 0,
            open: BTreeSet::new(),
            next_transfer_id: 100_000_000,
            fleet: BTreeMap::new(),
            pending: BTreeMap::new(),
            pending_relocations: BTreeMap::new(),
            returns: Vec::new(),
            carried: BTreeSet::new(),
            delivered: BTreeSet::new(),
            missed: BTreeSet::new(),
            events: Vec::new(),
            announced: 0,
            rules: Rules::default(),
            model,
            policy: config.manager.policy().expect("checked in configure"),
            top_model,
            top_feedback: Vec::new(),
            move_costs: Vec::new(),
            late_by_hub: BTreeMap::new(),
            scripted_fired: 0,
            last_call_s: 0.0,
            queued_events: Vec::new(),
            feedback: Vec::new(),
            tally: Tally::default(),
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

    fn initialize(&mut self, context: &mut AgentContext) -> Result<()> {
        context
            .messages
            .subscribe::<Status>("GlobalNetwork", STATUS_TOPIC)?;
        context
            .messages
            .subscribe::<Delivered>("GlobalNetwork", DELIVERED_TOPIC)?;
        self.events = self.config.events.script(
            &self.region,
            self.config.requests.until_s,
            &self.config.stock.items,
            &context.random.with(&[PluginRandom::name("events")]),
        );
        for event in &mut self.events {
            event.text = self.config.events.text(event);
        }
        self.draw_requests(&context.random.with(&[PluginRandom::name("requests")]));
        Ok(())
    }

    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let now_s = context.time.time_s;

        // Input: reports and deliveries.
        for message in context
            .messages
            .receive::<Status>("GlobalNetwork", STATUS_TOPIC)?
        {
            let status = *message.value;
            let before = self.fleet.get(&status.vehicle_id).map(|s| s.phase);
            if status.free() && before != Some(Phase::Idle) {
                self.changed = true;
            }
            if before.is_some_and(|p| p != Phase::Lost) && status.phase == Phase::Lost {
                self.tally.lost += 1;
            }
            if let Some(request_id) = status.request_id {
                self.carried.insert(request_id);
                self.pending.remove(&request_id);
            }
            if status.phase == Phase::Relocating {
                self.pending_relocations.remove(&status.vehicle_id);
            }
            self.fleet.insert(status.vehicle_id, status);
        }
        for message in context
            .messages
            .receive::<Delivered>("GlobalNetwork", DELIVERED_TOPIC)?
        {
            let delivery = *message.value;
            if !self.delivered.insert(delivery.request_id) {
                continue;
            }
            self.open.remove(&delivery.request_id);
            if let Some(request) = self.requests.get(&delivery.request_id) {
                match request.kind {
                    RequestKind::Transfer => {
                        if let Some(to_hub) = request.to_hub
                            && let Some(stock) = self.stock.get_mut(&to_hub)
                        {
                            *stock.entry(request.item.clone()).or_insert(0) += request.units;
                        }
                    }
                    RequestKind::Delivery => {
                        self.tally.delivered += 1;
                        if delivery.time_s <= request.deadline_s {
                            self.tally.on_time += 1;
                        } else {
                            self.tally.late += 1;
                            *self.late_by_hub.entry(request.hub).or_insert(0) += 1;
                        }
                    }
                }
            }
        }

        // Repositioned vehicles due home fly back once idle there.
        self.returns.retain(|r| {
            now_s < r.until_s
                || self
                    .fleet
                    .get(&r.vehicle_id)
                    .is_some_and(|s| s.hub != r.home)
        });
        let going: Vec<Return> = self
            .returns
            .iter()
            .filter(|r| {
                now_s >= r.until_s
                    && self.fleet.get(&r.vehicle_id).is_some_and(|s| {
                        s.free() && !self.pending_relocations.contains_key(&r.vehicle_id)
                    })
            })
            .cloned()
            .collect();
        for r in going {
            context.messages.publish(
                "GlobalNetwork",
                RELOCATE_TOPIC,
                Relocate {
                    vehicle_id: r.vehicle_id,
                    hub: r.home,
                    hub_world_m: r.home_world_m,
                },
            )?;
            self.pending_relocations
                .insert(r.vehicle_id, PendingRelocation { sent_s: now_s });
            if let Some(status) = self.fleet.get_mut(&r.vehicle_id) {
                status.phase = Phase::Relocating;
            }
            self.returns.retain(|x| x.vehicle_id != r.vehicle_id);
            self.changed = true;
        }

        // The day: requests open, events start, stock arrives, rules lapse.
        while self.opened < self.schedule.len()
            && self.requests[&self.schedule[self.opened]].opened_at_s <= now_s
        {
            let id = self.schedule[self.opened];
            context
                .messages
                .publish("GlobalNetwork", REQUEST_TOPIC, self.requests[&id].clone())?;
            self.open.insert(id);
            self.opened += 1;
            self.changed = true;
        }
        let mut new_events: Vec<usize> = Vec::new();
        while self.announced < self.events.len() && self.events[self.announced].at_s() <= now_s {
            let event = self.events[self.announced].clone();
            if event.kind == EventKind::Shortage {
                for hub in &event.hubs {
                    if let Some(stock) = self.stock.get_mut(hub) {
                        stock.insert(event.item.clone(), 0);
                    }
                }
            }
            context
                .messages
                .publish("GlobalNetwork", EVENT_TOPIC, event)?;
            new_events.push(self.announced);
            self.announced += 1;
            self.changed = true;
        }
        if now_s >= self.next_restock_s {
            for stock in self.stock.values_mut() {
                for units in stock.values_mut() {
                    *units += self.config.stock.restock_units;
                }
            }
            self.next_restock_s += self.config.stock.restock_interval_s;
            self.changed = true;
        }
        self.rules.expire(now_s);
        let timeout_s = self.config.confirm_timeout_s;
        let before = self.pending.len() + self.pending_relocations.len();
        self.pending
            .retain(|_, &mut (_, sent_s)| now_s <= sent_s + timeout_s);
        self.pending_relocations
            .retain(|_, pending| now_s <= pending.sent_s + timeout_s);
        self.changed |= self.pending.len() + self.pending_relocations.len() != before;

        // The fleet manager.
        self.manage(context, now_s, &new_events)?;

        // Assignment, when something changed, at most every decision interval
        // and at least every retry interval while work waits.
        let due = (self.changed && now_s >= self.last_decision_s + self.config.decision_interval_s)
            || now_s >= self.last_decision_s + self.config.retry_interval_s;
        if due {
            self.assign(context, now_s)?;
            self.changed = false;
            self.last_decision_s = now_s;
        }
        for name in ["velocity_x", "velocity_y", "velocity_z"] {
            io.write(name, 0.0)?;
        }
        Ok(())
    }

    /// The region for the viewer: each hub, each site by its code, and a
    /// faint spoke from each site to its hub, since a hub's sites reach
    /// past its neighbors and the map alone does not say whose they are.
    fn fixed_drawings(&self) -> Vec<Drawing> {
        let mut drawings = Vec::new();
        for site in &self.region.sites {
            let Some(hub) = self.region.hub(site.hub) else {
                continue;
            };
            drawings.push(Drawing::Line {
                points_world_m: vec![hub.position_world_m, site.position_world_m],
                label: String::new(),
                color: [55, 55, 62],
            });
        }
        for hub in &self.region.hubs {
            drawings.push(Drawing::Point {
                position_world_m: hub.position_world_m,
                label: format!("hub {}", hub.id),
                color: [255, 210, 80],
                prominent: true,
            });
        }
        for site in &self.region.sites {
            drawings.push(Drawing::Point {
                position_world_m: site.position_world_m,
                label: site.code.clone(),
                color: match site.kind {
                    SiteKind::Clinic => [110, 110, 110],
                    SiteKind::Hospital => [255, 255, 255],
                },
                prominent: false,
            });
        }
        drawings
    }

    /// The areas closed at `time_s`; usually none.
    fn drawings(&self, time_s: f64) -> Vec<Drawing> {
        let mut drawings = Vec::new();
        for event in self.events.iter().filter(|event| event.active(time_s)) {
            if let Some(area) = &event.area {
                let color = match event.kind {
                    EventKind::Corridor => [255, 160, 60],
                    _ => [255, 90, 77],
                };
                drawings.push(area.drawing(event.kind.as_str(), color));
            }
        }
        drawings
    }
}

/// A vehicle that could fly a request, and how.
struct Candidate {
    vehicle_id: i32,
    hub: u32,
    class: VehicleClass,
    skill: SkillName,
    target_world_m: Vec3,
    via_world_m: Option<Vec3>,
    arrival_s: f64,
}

impl Dispatcher {
    /// The day's requests, each from its own keyed random numbers, plus the
    /// surges' trauma requests.
    fn draw_requests(&mut self, random: &PluginRandom) {
        let config = &self.config.requests;
        let items = &self.config.stock.items;
        let pick = |u: f64, count: usize| ((u * count as f64) as usize).min(count - 1);
        let mut drawn: Vec<(f64, Request)> = Vec::new();
        for hub in &self.region.hubs {
            let sites: Vec<_> = self.region.sites_of(hub.id).collect();
            let count = (config.per_hub_per_hour * config.until_s / 3600.0).round() as u32;
            for index in 0..count {
                let key = |name: &str| {
                    [
                        u64::from(hub.id),
                        PluginRandom::name(name),
                        u64::from(index),
                    ]
                };
                let opened_at_s = random.uniform(&key("time")) * config.until_s;
                let site = sites[pick(random.uniform(&key("site")), sites.len())];
                let urgency_draw = random.uniform(&key("urgency"));
                let urgency = if urgency_draw < config.trauma_share {
                    Urgency::Trauma
                } else if urgency_draw < config.trauma_share + config.urgent_share {
                    Urgency::Urgent
                } else {
                    Urgency::Routine
                };
                let request = Request {
                    id: hub.id as i32 * 1000 + index as i32 + 1,
                    kind: RequestKind::Delivery,
                    hub: hub.id,
                    site: site.code.clone(),
                    site_world_m: site.position_world_m,
                    rooftop: site.rooftop(),
                    pad_world_m: site.pad_world_m,
                    item: items[pick(random.uniform(&key("item")), items.len())].clone(),
                    units: 1 + pick(random.uniform(&key("units")), 3) as u32,
                    urgency,
                    opened_at_s,
                    deadline_s: opened_at_s + self.deadline_s(urgency),
                    to_hub: None,
                };
                drawn.push((opened_at_s, request));
            }
        }
        // Surges: trauma requests at the surge hubs' hospitals.
        let events = &self.config.events;
        for event in self.events.iter().filter(|e| e.kind == EventKind::Surge) {
            for hub in &event.hubs {
                // Hospitals and clinics in turn, so neighbors' fixed-wing
                // aircraft can help with the clinics' share.
                let hospitals: Vec<_> = self
                    .region
                    .sites_of(*hub)
                    .filter(|site| site.kind == SiteKind::Hospital)
                    .collect();
                let clinics: Vec<_> = self
                    .region
                    .sites_of(*hub)
                    .filter(|site| site.kind == SiteKind::Clinic)
                    .collect();
                if hospitals.is_empty() || clinics.is_empty() {
                    continue;
                }
                for k in 0..events.surge_requests_per_hub {
                    let key = |name: &str| {
                        [
                            PluginRandom::name("surge"),
                            u64::from(event.id),
                            u64::from(*hub),
                            PluginRandom::name(name),
                            u64::from(k),
                        ]
                    };
                    let opened_at_s = event.at_s()
                        + random.uniform(&key("time")) * events.surge_window_min * 60.0;
                    let site = if k % 2 == 0 {
                        hospitals[(k / 2) as usize % hospitals.len()]
                    } else {
                        clinics[(k / 2) as usize % clinics.len()]
                    };
                    let request = Request {
                        id: 1_000_000 + event.id as i32 * 100_000 + *hub as i32 * 100 + k as i32,
                        kind: RequestKind::Delivery,
                        hub: *hub,
                        site: site.code.clone(),
                        site_world_m: site.position_world_m,
                        rooftop: site.rooftop(),
                        pad_world_m: site.pad_world_m,
                        item: items[pick(random.uniform(&key("item")), items.len())].clone(),
                        units: 1 + pick(random.uniform(&key("units")), 3) as u32,
                        urgency: Urgency::Trauma,
                        opened_at_s,
                        deadline_s: opened_at_s + self.deadline_s(Urgency::Trauma),
                        to_hub: None,
                    };
                    drawn.push((opened_at_s, request));
                }
            }
        }
        drawn.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        self.schedule = drawn.iter().map(|(_, request)| request.id).collect();
        self.requests = drawn
            .into_iter()
            .map(|(_, request)| (request.id, request))
            .collect();
    }

    fn deadline_s(&self, urgency: Urgency) -> f64 {
        match urgency {
            Urgency::Routine => self.config.requests.routine_deadline_s,
            Urgency::Urgent => self.config.requests.urgent_deadline_s,
            Urgency::Trauma => self.config.requests.trauma_deadline_s,
        }
    }

    fn in_outage(&self, hub: u32, now_s: f64) -> bool {
        self.events
            .iter()
            .any(|e| e.kind == EventKind::Outage && e.hubs.contains(&hub) && e.active(now_s))
    }

    /// Requests open and unassigned.
    fn waiting(&self) -> Vec<&Request> {
        self.open
            .iter()
            .filter(|id| {
                !self.carried.contains(id)
                    && !self.delivered.contains(id)
                    && !self.pending.contains_key(id)
            })
            .map(|id| &self.requests[id])
            .collect()
    }

    /// The state at one call, limited to a cluster's hubs when given one.
    fn snapshot_of(&self, now_s: f64, cluster: Option<&[u32]>) -> Snapshot<'_> {
        let waiting = self.waiting();
        let mut hubs: Vec<HubView> = self
            .region
            .hubs
            .iter()
            .filter(|hub| cluster.is_none_or(|c| c.contains(&hub.id)))
            .map(|hub| HubView {
                id: hub.id,
                position_world_m: hub.position_world_m,
                stock: self.stock.get(&hub.id).cloned().unwrap_or_default(),
                out: self.in_outage(hub.id, now_s),
                ..HubView::default()
            })
            .collect();
        for vehicle in self.fleet.values() {
            let Some(view) = hubs.iter_mut().find(|h| h.id == vehicle.hub) else {
                continue;
            };
            *view.total.entry(vehicle.card.class).or_insert(0) += 1;
            match vehicle.phase {
                Phase::Idle
                    if vehicle.free()
                        && !self.pending_relocations.contains_key(&vehicle.vehicle_id) =>
                {
                    *view.idle.entry(vehicle.card.class).or_insert(0) += 1;
                }
                Phase::Lost => view.lost += 1,
                Phase::Relocating => view.incoming += 1,
                _ => view.flying += 1,
            }
        }
        for id in &self.open {
            let request = &self.requests[id];
            if let Some(to_hub) = request.to_hub
                && let Some(view) = hubs.iter_mut().find(|h| h.id == to_hub)
            {
                *view.arriving.entry(request.item.clone()).or_insert(0) += request.units;
            }
        }
        for request in &waiting {
            if request.kind != RequestKind::Delivery {
                continue;
            }
            if let Some(view) = hubs.iter_mut().find(|h| h.id == request.hub) {
                *view.waiting.entry(request.urgency).or_insert(0) += 1;
                if request.deadline_s < now_s {
                    view.late += 1;
                }
            }
        }
        let mut cards = BTreeMap::new();
        for vehicle in self.fleet.values() {
            cards.entry(vehicle.card.class).or_insert(vehicle.card);
        }
        Snapshot {
            time_s: now_s,
            day_s: self.config.requests.until_s,
            region: &self.region,
            hubs,
            cards,
            range_margin: self.config.range_margin,
            vehicles: self
                .fleet
                .values()
                .filter(|v| cluster.is_none_or(|c| c.contains(&v.hub)))
                .collect(),
            active_events: self.events.iter().filter(|e| e.active(now_s)).collect(),
            items: &self.config.stock.items,
            units_per_item: self.config.stock.units_per_item,
            cluster: cluster.map(<[u32]>::to_vec),
        }
    }

    /// The clusters a manager is called for: every hub at once, or blocks.
    fn clusters(&self) -> Vec<Option<Vec<u32>>> {
        match self.config.manager.cluster_side {
            0 => vec![None],
            side => self.region.clusters(side).into_iter().map(Some).collect(),
        }
    }

    /// Calls the fleet manager when events start and at check-ins, and
    /// applies what it orders.
    fn manage(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
        new_events: &[usize],
    ) -> Result<()> {
        let manager = &self.config.manager;
        let kind = manager.kind;
        let top_kind = manager.top_kind;
        // Each order with the cluster it came from, if any, and whether its
        // call was quiet; each call made.
        let mut orders: Vec<(Order, Option<Vec<u32>>, bool)> = Vec::new();
        let mut calls: Vec<ManagerCall> = Vec::new();
        // The events of this call, for the manager above the clusters.
        let mut due_events: Vec<u32> = Vec::new();
        match kind {
            ManagerKind::None if top_kind == ManagerKind::None => return Ok(()),
            ManagerKind::None => {
                let Some(due) = self.due(now_s, new_events) else {
                    return Ok(());
                };
                due_events = due.iter().map(|&i| self.events[i].id).collect();
            }
            ManagerKind::Scripted => {
                while self.scripted_fired < manager.scripted.len()
                    && manager.scripted[self.scripted_fired].at_min * 60.0 <= now_s
                {
                    orders.push((
                        manager.scripted[self.scripted_fired].order.clone(),
                        None,
                        false,
                    ));
                    self.scripted_fired += 1;
                }
                if orders.is_empty() {
                    return Ok(());
                }
            }
            ManagerKind::Replanner | ManagerKind::Policy => {
                let Some(due) = self.due(now_s, new_events) else {
                    return Ok(());
                };
                let events = self.started(&due);
                due_events = events.iter().map(|e| e.id).collect();
                for cluster in self.clusters() {
                    let snapshot = self.snapshot_of(now_s, cluster.as_deref());
                    let mine = events_touching(&events, cluster.as_deref());
                    let mine: Vec<&Event> = mine.iter().collect();
                    let planned = match &self.policy {
                        Some(policy) => policy.plan(&mine, &snapshot),
                        None => replanner::plan(&mine, &snapshot),
                    };
                    let quiet = mine.is_empty() && !snapshot.has_exceptions();
                    calls.push(ManagerCall {
                        time_s: now_s,
                        source: kind,
                        top: false,
                        quiet,
                        events: mine.iter().map(|e| e.id).collect(),
                        prompt_chars: 0,
                        reply_chars: 0,
                        queries: 0,
                        orders: planned.len() as u32,
                        dropped: 0,
                        parse_failed: false,
                    });
                    orders.extend(planned.into_iter().map(|o| (o, cluster.clone(), quiet)));
                }
            }
            ManagerKind::Veto => {
                let Some(due) = self.due(now_s, new_events) else {
                    return Ok(());
                };
                let events = self.started(&due);
                due_events = events.iter().map(|e| e.id).collect();
                for cluster in self.clusters() {
                    let snapshot = self.snapshot_of(now_s, cluster.as_deref());
                    let mine = events_touching(&events, cluster.as_deref());
                    let mine: Vec<&Event> = mine.iter().collect();
                    let planned = replanner::plan(&mine, &snapshot);
                    let quiet = mine.is_empty() && !snapshot.has_exceptions();
                    let situation = self.situation(&snapshot, &mine, Some(&planned));
                    let model = self.model.as_ref().expect("a model manager");
                    let (reply, parse_failed, dropped) = if planned.is_empty() {
                        // Nothing to review: no call.
                        (String::new(), false, Vec::new())
                    } else {
                        let reply = model.ask_text(&situation)?;
                        match parse_veto(&reply) {
                            Some(drop) => (reply, false, drop),
                            None => (reply, true, Vec::new()),
                        }
                    };
                    let kept: Vec<Order> = planned
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| !dropped.contains(&(i + 1)))
                        .map(|(_, o)| o.clone())
                        .collect();
                    if !planned.is_empty() {
                        calls.push(ManagerCall {
                            time_s: now_s,
                            source: kind,
                            top: false,
                            quiet,
                            events: mine.iter().map(|e| e.id).collect(),
                            prompt_chars: situation.len(),
                            reply_chars: reply.len(),
                            queries: 0,
                            orders: kept.len() as u32,
                            dropped: (planned.len() - kept.len()) as u32,
                            parse_failed,
                        });
                    }
                    orders.extend(kept.into_iter().map(|o| (o, cluster.clone(), quiet)));
                }
                self.feedback.clear();
                self.tally = Tally::default();
                self.move_costs.clear();
                self.late_by_hub.clear();
            }
            ManagerKind::Model => {
                let Some(due) = self.due(now_s, new_events) else {
                    return Ok(());
                };
                let events = self.started(&due);
                due_events = events.iter().map(|e| e.id).collect();
                let mut unreadable = 0;
                for cluster in self.clusters() {
                    let snapshot = self.snapshot_of(now_s, cluster.as_deref());
                    let mine = events_touching(&events, cluster.as_deref());
                    let mine: Vec<&Event> = mine.iter().collect();
                    let situation = self.situation(&snapshot, &mine, None);
                    let quiet = mine.is_empty() && !snapshot.has_exceptions();
                    let asked: Asked = self
                        .model
                        .as_ref()
                        .expect("a model manager")
                        .ask(&situation, &snapshot)?;
                    calls.push(ManagerCall {
                        time_s: now_s,
                        source: kind,
                        top: false,
                        quiet,
                        events: mine.iter().map(|e| e.id).collect(),
                        prompt_chars: asked.prompt_chars,
                        reply_chars: asked.reply_chars,
                        queries: asked.queries,
                        orders: asked.orders.len() as u32 + asked.unreadable,
                        dropped: 0,
                        parse_failed: asked.parse_failed,
                    });
                    unreadable += asked.unreadable;
                    orders.extend(
                        asked
                            .orders
                            .into_iter()
                            .map(|o| (o, cluster.clone(), quiet)),
                    );
                }
                self.feedback.clear();
                self.tally = Tally::default();
                self.move_costs.clear();
                self.late_by_hub.clear();
                for _ in 0..unreadable {
                    self.feedback
                        .push("an order could not be read and was dropped".into());
                }
            }
        };
        self.last_call_s = now_s;
        for (order, cluster, quiet) in orders {
            let outside = cluster
                .as_ref()
                .is_some_and(|c| order.hubs_named().iter().any(|h| !c.contains(h)));
            let (outcome, feedback) = if order.is_top() {
                (
                    "refused_top_only".to_string(),
                    "only the manager above the clusters moves between clusters".to_string(),
                )
            } else if outside {
                (
                    "refused_outside_cluster".to_string(),
                    "names a hub outside your cluster".to_string(),
                )
            } else {
                self.apply(context, now_s, &order)?
            };
            let record = OrderRecord {
                time_s: now_s,
                source: kind,
                top: false,
                quiet,
                order: order.clone(),
                outcome: outcome.clone(),
                feedback: feedback.clone(),
            };
            context
                .messages
                .publish("GlobalNetwork", ORDER_TOPIC, record)?;
            if kind.uses_model() {
                self.feedback
                    .push(format!("{}: {outcome}; {feedback}", order.describe()));
            }
            self.changed = true;
        }
        for call in calls {
            context
                .messages
                .publish("GlobalNetwork", CALL_TOPIC, call)?;
        }
        if top_kind != ManagerKind::None {
            self.manage_top(context, now_s, &due_events)?;
        }
        Ok(())
    }

    /// The call above the clusters, after theirs: it sees cluster totals and
    /// may only move vehicles and stock between clusters.
    fn manage_top(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
        event_ids: &[u32],
    ) -> Result<()> {
        let top_kind = self.config.manager.top_kind;
        let clusters = self.region.clusters(self.config.manager.cluster_side);
        let snapshot = self.snapshot_of(now_s, None);
        let events: Vec<&Event> = self
            .events
            .iter()
            .filter(|e| event_ids.contains(&e.id))
            .collect();
        let quiet = events.is_empty() && !snapshot.has_exceptions();
        let (orders, call) = match top_kind {
            ManagerKind::Replanner => {
                let planned = replanner::plan_top(&events, &snapshot, &clusters);
                let call = ManagerCall {
                    time_s: now_s,
                    source: top_kind,
                    top: true,
                    quiet,
                    events: event_ids.to_vec(),
                    prompt_chars: 0,
                    reply_chars: 0,
                    queries: 0,
                    orders: planned.len() as u32,
                    dropped: 0,
                    parse_failed: false,
                };
                (planned, call)
            }
            ManagerKind::Model => {
                let situation = self.top_situation(&snapshot, &events, &clusters);
                let asked: Asked = self
                    .top_model
                    .as_ref()
                    .expect("a top model")
                    .ask(&situation, &snapshot)?;
                let call = ManagerCall {
                    time_s: now_s,
                    source: top_kind,
                    top: true,
                    quiet,
                    events: event_ids.to_vec(),
                    prompt_chars: asked.prompt_chars,
                    reply_chars: asked.reply_chars,
                    queries: 0,
                    orders: asked.orders.len() as u32 + asked.unreadable,
                    dropped: 0,
                    parse_failed: asked.parse_failed,
                };
                self.top_feedback.clear();
                for _ in 0..asked.unreadable {
                    self.top_feedback
                        .push("an order could not be read and was dropped".into());
                }
                (asked.orders, call)
            }
            _ => unreachable!("checked in configure"),
        };
        for order in orders {
            let (outcome, feedback) = if order.is_top() {
                self.apply(context, now_s, &order)?
            } else {
                (
                    "refused_moves_only".to_string(),
                    "the manager above the clusters only moves vehicles and stock between clusters"
                        .to_string(),
                )
            };
            let record = OrderRecord {
                time_s: now_s,
                source: top_kind,
                top: true,
                quiet,
                order: order.clone(),
                outcome: outcome.clone(),
                feedback: feedback.clone(),
            };
            context
                .messages
                .publish("GlobalNetwork", ORDER_TOPIC, record)?;
            if top_kind == ManagerKind::Model {
                self.top_feedback
                    .push(format!("{}: {outcome}; {feedback}", order.describe()));
            }
            self.changed = true;
        }
        context
            .messages
            .publish("GlobalNetwork", CALL_TOPIC, call)?;
        Ok(())
    }

    /// The prompt for the manager above the clusters.
    fn top_situation(
        &self,
        snapshot: &Snapshot<'_>,
        new_events: &[&Event],
        clusters: &[Vec<u32>],
    ) -> String {
        let manager = &self.config.manager;
        let minute = |s: f64| format!("{:.0}", s / 60.0);
        let mut text = String::new();
        let _ = writeln!(
            text,
            "Time: minute {} of the day; requests stop coming at minute {}.",
            minute(snapshot.time_s),
            minute(snapshot.day_s)
        );
        let _ = writeln!(
            text,
            "You are the fleet manager above {} cluster managers, each running its own hubs' \
             flights and local rules. You see each cluster's totals, and you may only move \
             vehicles and stock between clusters.",
            clusters.len()
        );
        let event_line = |event: &Event| match manager.events_as {
            EventsAs::Text => event.text.clone(),
            EventsAs::Structured => event.structured(),
        };
        if new_events.is_empty() {
            text.push_str("\nNo new events; this is a check-in.\n");
        } else {
            text.push_str("\nNew events:\n");
            for event in new_events {
                let _ = writeln!(text, "- {}", event_line(event));
            }
        }
        let ongoing: Vec<&Event> = snapshot
            .active_events
            .iter()
            .copied()
            .filter(|e| !new_events.iter().any(|n| n.id == e.id))
            .collect();
        if !ongoing.is_empty() {
            text.push_str("\nEvents still in force:\n");
            for event in ongoing {
                let _ = writeln!(text, "- {}", event_line(event));
            }
        }
        if manager.feedback && !self.top_feedback.is_empty() {
            text.push_str("\nYour last orders:\n");
            for line in &self.top_feedback {
                let _ = writeln!(text, "- {line}");
            }
            if manager.cost_feedback && !self.move_costs.is_empty() {
                text.push_str("\nAt the hubs your moves took from:\n");
                for line in self.cost_lines() {
                    let _ = writeln!(text, "- {line}");
                }
            }
        }
        text.push('\n');
        text.push_str(&snapshot.clusters(clusters));
        text.push('\n');
        text.push_str(TOP_SYNTAX);
        text.push_str(
            "\n\nReply with {\"orders\": [...]} and nothing else; {\"orders\": []} changes nothing.\n",
        );
        text
    }

    /// Whether the fleet manager is called now, and with which events: the
    /// ones queued since the last call, once the call budget allows it, or a
    /// check-in with none.
    fn due(&mut self, now_s: f64, new_events: &[usize]) -> Option<Vec<usize>> {
        let manager = &self.config.manager;
        self.queued_events.extend_from_slice(new_events);
        let spacing_s = if manager.calls_per_hour > 0.0 {
            3600.0 / manager.calls_per_hour
        } else {
            0.0
        };
        if now_s < self.last_call_s + spacing_s {
            return None;
        }
        let check_in = now_s >= self.last_call_s + manager.check_in_s && !self.fleet.is_empty();
        if self.queued_events.is_empty() && !check_in {
            return None;
        }
        Some(std::mem::take(&mut self.queued_events))
    }

    /// The events that just started, by their indices from `due`.
    fn started(&self, due: &[usize]) -> Vec<&Event> {
        due.iter().map(|&index| &self.events[index]).collect()
    }

    /// The prompt for one call: the time, the events, the feedback, the fleet
    /// view, the rules in force, and the orders available. With `proposed`,
    /// the replanner's orders are listed for the veto manager instead of the
    /// order syntax.
    fn situation(
        &self,
        snapshot: &Snapshot<'_>,
        new_events: &[&Event],
        proposed: Option<&[Order]>,
    ) -> String {
        let manager = &self.config.manager;
        let minute = |s: f64| format!("{:.0}", s / 60.0);
        let mut text = String::new();
        let _ = writeln!(
            text,
            "Time: minute {} of the day; requests stop coming at minute {}.",
            minute(snapshot.time_s),
            minute(snapshot.day_s)
        );
        if let Some(cluster) = &snapshot.cluster {
            let _ = writeln!(
                text,
                "You manage hubs {} only, one cluster of the region; the other clusters \
                 have their own fleet manager, and an order naming a hub outside yours \
                 is refused.",
                hub_list(cluster)
            );
        }
        let event_line = |event: &Event| match manager.events_as {
            EventsAs::Text => event.text.clone(),
            EventsAs::Structured => event.structured(),
        };
        if new_events.is_empty() {
            text.push_str("\nNo new events; this is a check-in.\n");
        } else {
            text.push_str("\nNew events:\n");
            for event in new_events {
                let _ = writeln!(text, "- {}", event_line(event));
            }
        }
        let ongoing: Vec<&Event> = snapshot
            .active_events
            .iter()
            .copied()
            .filter(|e| !new_events.iter().any(|n| n.id == e.id))
            .collect();
        if !ongoing.is_empty() {
            text.push_str("\nEvents still in force:\n");
            for event in ongoing {
                let _ = writeln!(text, "- {}", event_line(event));
            }
        }
        if manager.feedback {
            if !self.feedback.is_empty() {
                text.push_str("\nYour last orders:\n");
                for line in &self.feedback {
                    let _ = writeln!(text, "- {line}");
                }
            }
            let tally = &self.tally;
            let mut since = format!(
                "\nSince your last call: {} delivered ({} on time, {} late)",
                tally.delivered, tally.on_time, tally.late
            );
            for (reason, count) in &tally.missed {
                let _ = write!(
                    since,
                    ", {count} missed for {}",
                    reason.as_str().replace('_', " ")
                );
            }
            if tally.lost > 0 {
                let _ = write!(since, ", {} vehicles lost to endurance", tally.lost);
            }
            text.push_str(&since);
            text.push_str(".\n");
            if manager.cost_feedback && !self.move_costs.is_empty() {
                text.push_str("\nAt the hubs your moves took from:\n");
                for line in self.cost_lines() {
                    let _ = writeln!(text, "- {line}");
                }
            }
        }
        text.push('\n');
        text.push_str(&snapshot.write(manager.view));
        let rules = self.rules.describe();
        if !rules.is_empty() {
            text.push_str("\nOrders in force:\n");
            for line in rules {
                let _ = writeln!(text, "- {line}");
            }
        }
        text.push('\n');
        if let Some(proposed) = proposed {
            text.push_str("Proposed orders from the dispatcher's planner:\n");
            for (i, order) in proposed.iter().enumerate() {
                let _ = writeln!(text, "{}. {}", i + 1, order.describe());
            }
            text.push('\n');
            text.push_str(VETO_SYNTAX);
            text.push('\n');
            return text;
        }
        text.push_str(SYNTAX);
        text.push('\n');
        if manager.view == ViewKind::Queries {
            text.push_str(QUERY_SYNTAX);
            text.push('\n');
        }
        text.push_str(
            "\nReply with {\"orders\": [...]} and nothing else; {\"orders\": []} changes nothing.\n",
        );
        text
    }

    /// One line per move since the last call: the donor hub's idle vehicles
    /// now and its late deliveries since.
    fn cost_lines(&self) -> Vec<String> {
        self.move_costs
            .iter()
            .map(|(words, donor)| {
                let idle = |class: VehicleClass| {
                    self.fleet
                        .values()
                        .filter(|v| v.hub == *donor && v.card.class == class && v.free())
                        .count()
                };
                format!(
                    "{words}: hub {donor} now has {} fixed-wing and {} multirotor idle, {} of its deliveries late since",
                    idle(VehicleClass::FixedWing),
                    idle(VehicleClass::Multirotor),
                    self.late_by_hub.get(donor).copied().unwrap_or(0)
                )
            })
            .collect()
    }

    /// Checks an order and applies it. Returns the outcome (`applied`, or
    /// `refused_<reason>`) and what the fleet manager is told about it.
    fn apply(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
        order: &Order,
    ) -> Result<(String, String)> {
        let checker = self.config.manager.checker;
        let refuse = |reason: &str, why: String| Ok((format!("refused_{reason}"), why));
        let until = |until_min: f64| -> Result<f64, (String, String)> {
            let until_s = until_min * 60.0;
            if !until_s.is_finite() || until_s <= now_s {
                return Err((
                    "refused_past_time".into(),
                    format!(
                        "until_min {until_min} is not after minute {:.0}",
                        now_s / 60.0
                    ),
                ));
            }
            Ok(until_s)
        };
        let hub_exists = |hub: u32| self.region.hub(hub).is_some();
        match order {
            Order::Reserve {
                hub,
                count,
                site,
                until_min,
            } => {
                if !hub_exists(*hub) {
                    return refuse("unknown_hub", format!("there is no hub {hub}"));
                }
                let Some(site) = self.region.site(site) else {
                    return refuse("unknown_site", format!("there is no site {site}"));
                };
                let until_s = match until(*until_min) {
                    Ok(u) => u,
                    Err(e) => return Ok(e),
                };
                if *count == 0 {
                    return refuse("bad_count", "count must be positive".into());
                }
                let at_hub = self
                    .fleet
                    .values()
                    .filter(|v| v.hub == *hub && v.phase != Phase::Lost)
                    .count() as u32;
                if checker && *count > at_hub {
                    return refuse(
                        "too_few_vehicles",
                        format!("hub {hub} has {at_hub} vehicles"),
                    );
                }
                self.rules.reserves.push(Reserve {
                    hub: *hub,
                    count: *count,
                    site: site.code.clone(),
                    until_s,
                });
                Ok((
                    "applied".into(),
                    format!(
                        "{count} of hub {hub}'s {at_hub} vehicles held for {}",
                        site.code
                    ),
                ))
            }
            Order::Avoid { area, until_min } => {
                if !area.valid() {
                    return refuse("malformed_area", "the area is malformed".into());
                }
                let until_s = match until(*until_min) {
                    Ok(u) => u,
                    Err(e) => return Ok(e),
                };
                self.rules.avoids.push((*area, until_s));
                let hubs_inside: Vec<String> = self
                    .region
                    .hubs
                    .iter()
                    .filter(|h| area.contains(h.position_world_m))
                    .map(|h| h.id.to_string())
                    .collect();
                let note = if hubs_inside.is_empty() {
                    "flights now detour around it".to_string()
                } else {
                    format!(
                        "hubs {} are inside it; their flights start inside",
                        hubs_inside.join(", ")
                    )
                };
                Ok(("applied".into(), note))
            }
            Order::Reposition {
                class,
                count,
                from_hub,
                to_hub,
                until_min,
            } => {
                if !hub_exists(*from_hub) || !hub_exists(*to_hub) {
                    return refuse(
                        "unknown_hub",
                        format!("hubs are 1 to {}", self.region.hubs.len()),
                    );
                }
                if from_hub == to_hub {
                    return refuse("same_hub", "from_hub and to_hub are the same".into());
                }
                if *count == 0 {
                    return refuse("bad_count", "count must be positive".into());
                }
                let destination = self.region.hub(*to_hub).expect("checked").position_world_m;
                let mut idle: Vec<&Status> = self
                    .fleet
                    .values()
                    .filter(|v| {
                        v.hub == *from_hub
                            && v.free()
                            && class.is_none_or(|c| c == v.card.class)
                            && !self.pending_relocations.contains_key(&v.vehicle_id)
                    })
                    .collect();
                // Fixed-wing first when the class is open: they go farther.
                idle.sort_by(|a, b| {
                    a.card
                        .class
                        .cmp(&b.card.class)
                        .then(a.vehicle_id.cmp(&b.vehicle_id))
                });
                let class_name = class.map_or("vehicles", |c| c.as_str());
                if self.config.manager.guard {
                    let total = self
                        .fleet
                        .values()
                        .filter(|v| v.hub == *from_hub && class.is_none_or(|c| c == v.card.class))
                        .count();
                    let keep = total.div_ceil(2);
                    if idle.len().saturating_sub(*count as usize) < keep {
                        return refuse(
                            "donor_thin",
                            format!(
                                "hub {from_hub} would keep fewer than half its {class_name}: {} idle of {total}",
                                idle.len()
                            ),
                        );
                    }
                }
                if checker {
                    if idle.len() < *count as usize {
                        return refuse(
                            "too_few_idle",
                            format!("hub {from_hub} has {} idle {class_name}", idle.len()),
                        );
                    }
                    if let Some(c) = class
                        && self.rules.is_grounded(*from_hub, *c)
                    {
                        return refuse(
                            "grounded",
                            format!("{class_name} are grounded at hub {from_hub}"),
                        );
                    }
                    if self.in_outage(*from_hub, now_s) {
                        return refuse("hub_out", format!("hub {from_hub} cannot launch"));
                    }
                }
                let mut moved = Vec::new();
                let mut notes = Vec::new();
                let mut lost_warning = false;
                for vehicle in idle.iter().take(*count as usize) {
                    if self.rules.is_grounded(*from_hub, vehicle.card.class)
                        || self.in_outage(*from_hub, now_s)
                    {
                        continue;
                    }
                    let distance_m = horizontal_m(vehicle.position_world_m, destination);
                    let flight_s = distance_m / vehicle.card.cruise_speed_mps;
                    let feasible = flight_s <= vehicle.endurance_left_s * self.config.range_margin;
                    if checker && !feasible {
                        return refuse(
                            "out_of_range",
                            format!(
                                "{:.0} km at {:.0} m/s takes {:.0} min; a {} has {:.0} min",
                                distance_m / 1000.0,
                                vehicle.card.cruise_speed_mps,
                                flight_s / 60.0,
                                vehicle.card.class.as_str(),
                                vehicle.endurance_left_s / 60.0
                            ),
                        );
                    }
                    lost_warning |= !feasible;
                    moved.push((vehicle.vehicle_id, vehicle.card.class, flight_s));
                }
                let home_world_m = self
                    .region
                    .hub(*from_hub)
                    .expect("checked")
                    .position_world_m;
                for (vehicle_id, _, _) in &moved {
                    context.messages.publish(
                        "GlobalNetwork",
                        RELOCATE_TOPIC,
                        Relocate {
                            vehicle_id: *vehicle_id,
                            hub: *to_hub,
                            hub_world_m: destination,
                        },
                    )?;
                    self.pending_relocations
                        .insert(*vehicle_id, PendingRelocation { sent_s: now_s });
                    if let Some(status) = self.fleet.get_mut(vehicle_id) {
                        status.phase = Phase::Relocating;
                    }
                    self.returns.retain(|r| r.vehicle_id != *vehicle_id);
                    if let Some(until_min) = until_min {
                        self.returns.push(Return {
                            vehicle_id: *vehicle_id,
                            home: *from_hub,
                            home_world_m,
                            until_s: until_min * 60.0,
                        });
                    }
                }
                if moved.is_empty() {
                    return refuse(
                        "too_few_idle",
                        format!("hub {from_hub} has no idle {class_name} free to go"),
                    );
                }
                let longest_min = moved.iter().map(|m| m.2).fold(0.0, f64::max) / 60.0;
                notes.push(format!(
                    "{} of {count} sent, {:.0} km, arriving in about {longest_min:.0} min",
                    moved.len(),
                    horizontal_m(
                        self.region
                            .hub(*from_hub)
                            .expect("checked")
                            .position_world_m,
                        destination
                    ) / 1000.0
                ));
                if lost_warning {
                    notes.push(
                        "beyond the endurance of some of them: they will be lost on the way".into(),
                    );
                }
                // Partial: the order asked for more than was there, which the
                // fleet manager's view should have shown.
                let outcome = if moved.len() < *count as usize {
                    "partial"
                } else {
                    "applied"
                };
                self.move_costs.push((order.describe(), *from_hub));
                Ok((outcome.into(), notes.join("; ")))
            }
            Order::Transfer {
                item,
                units,
                from_hub,
                to_hub,
            } => {
                if !hub_exists(*from_hub) || !hub_exists(*to_hub) {
                    return refuse(
                        "unknown_hub",
                        format!("hubs are 1 to {}", self.region.hubs.len()),
                    );
                }
                if from_hub == to_hub {
                    return refuse("same_hub", "from_hub and to_hub are the same".into());
                }
                let Some(item) = self
                    .config
                    .stock
                    .items
                    .iter()
                    .find(|i| i.eq_ignore_ascii_case(item))
                    .cloned()
                else {
                    return refuse(
                        "unknown_item",
                        format!("the items are {}", self.config.stock.items.join(", ")),
                    );
                };
                if *units == 0 {
                    return refuse("bad_count", "units must be positive".into());
                }
                let have = self.stock[from_hub].get(&item).copied().unwrap_or(0);
                let from = self
                    .region
                    .hub(*from_hub)
                    .expect("checked")
                    .position_world_m;
                let to = self.region.hub(*to_hub).expect("checked").position_world_m;
                let distance_m = horizontal_m(from, to);
                // The fastest class carries transfers when it can.
                let best_round_trip_s = self
                    .fleet
                    .values()
                    .filter(|v| v.hub == *from_hub && v.phase != Phase::Lost)
                    .map(|v| {
                        2.0 * distance_m
                            / v.card.cruise_speed_mps
                            / (v.card.endurance_s * self.config.range_margin)
                    })
                    .fold(f64::INFINITY, f64::min);
                if have == 0 {
                    return refuse("no_stock", format!("hub {from_hub} has no {item}"));
                }
                if self.config.manager.guard {
                    let keep = (f64::from(self.config.stock.units_per_item) * 0.25).round() as u32;
                    if have.saturating_sub(*units) < keep {
                        return refuse(
                            "donor_low",
                            format!(
                                "hub {from_hub} would keep fewer than {keep} units of {item}: it has {have}"
                            ),
                        );
                    }
                }
                if checker {
                    if have < *units {
                        return refuse(
                            "no_stock",
                            format!("hub {from_hub} has {have} units of {item}"),
                        );
                    }
                    if best_round_trip_s > 1.0 {
                        return refuse(
                            "out_of_range",
                            format!(
                                "{:.0} km there and back exceeds every vehicle's endurance at hub {from_hub}",
                                distance_m / 1000.0
                            ),
                        );
                    }
                }
                let package = self.config.stock.package_units;
                // Without the checker, what is there goes; the outcome says so.
                let mut remaining = (*units).min(have);
                let outcome = if remaining < *units {
                    "partial"
                } else {
                    "applied"
                };
                let mut flights = 0;
                while remaining > 0 {
                    let carried = remaining.min(package);
                    let id = self.next_transfer_id;
                    self.next_transfer_id += 1;
                    let request = Request {
                        id,
                        kind: RequestKind::Transfer,
                        hub: *from_hub,
                        site: format!("hub {to_hub}"),
                        site_world_m: to,
                        rooftop: false,
                        pad_world_m: to,
                        item: item.clone(),
                        units: carried,
                        urgency: Urgency::Routine,
                        opened_at_s: now_s,
                        deadline_s: self.config.requests.until_s + 3600.0,
                        to_hub: Some(*to_hub),
                    };
                    context
                        .messages
                        .publish("GlobalNetwork", REQUEST_TOPIC, request.clone())?;
                    self.requests.insert(id, request);
                    self.open.insert(id);
                    remaining -= carried;
                    flights += 1;
                }
                let mut note = format!(
                    "{} units in {flights} flights of up to {package}, {:.0} km each way; hub {from_hub} holds {have} units of {item}",
                    (*units).min(have),
                    distance_m / 1000.0
                );
                if have < *units {
                    note.push_str(": only what it holds goes");
                }
                if best_round_trip_s > 1.0 {
                    note.push_str("; beyond every vehicle's round-trip range, so the flights cannot be assigned");
                }
                self.move_costs.push((order.describe(), *from_hub));
                Ok((outcome.into(), note))
            }
            Order::Ground {
                class,
                hubs,
                until_min,
            } => {
                if hubs.is_empty() || hubs.iter().any(|h| !hub_exists(*h)) {
                    return refuse(
                        "unknown_hub",
                        format!("hubs are 1 to {}", self.region.hubs.len()),
                    );
                }
                let until_s = match until(*until_min) {
                    Ok(u) => u,
                    Err(e) => return Ok(e),
                };
                self.rules.grounded.push(Grounded {
                    class: *class,
                    hubs: hubs.clone(),
                    until_s,
                });
                Ok((
                    "applied".into(),
                    "no launches of that class from those hubs until then".into(),
                ))
            }
            Order::Prioritize {
                site,
                urgency,
                until_min,
            } => {
                let site = match site {
                    Some(code) => match self.region.site(code) {
                        Some(site) => Some(site.code.clone()),
                        None => return refuse("unknown_site", format!("there is no site {code}")),
                    },
                    None => None,
                };
                if site.is_none() && urgency.is_none() {
                    return refuse("no_target", "give a site or an urgency".into());
                }
                let until_s = match until(*until_min) {
                    Ok(u) => u,
                    Err(e) => return Ok(e),
                };
                self.rules.priorities.push(Priority {
                    site,
                    urgency: *urgency,
                    until_s,
                });
                Ok(("applied".into(), "those requests now come first".into()))
            }
            Order::ServeFrom { site, hubs } => {
                let Some(site) = self.region.site(site) else {
                    return refuse("unknown_site", format!("there is no site {site}"));
                };
                if hubs.is_empty() || hubs.iter().any(|h| !hub_exists(*h)) {
                    return refuse(
                        "unknown_hub",
                        format!("hubs are 1 to {}", self.region.hubs.len()),
                    );
                }
                let farthest_km = hubs
                    .iter()
                    .map(|h| {
                        horizontal_m(
                            self.region.hub(*h).expect("checked").position_world_m,
                            site.position_world_m,
                        )
                    })
                    .fold(0.0, f64::max)
                    / 1000.0;
                // A fixed-wing round trip at 25 m/s and 60 min reaches 40 km out.
                let reach_km = self
                    .fleet
                    .values()
                    .map(|v| {
                        v.card.cruise_speed_mps * v.card.endurance_s * self.config.range_margin
                            / 2000.0
                    })
                    .fold(0.0, f64::max);
                if checker && farthest_km > reach_km {
                    return refuse(
                        "out_of_range",
                        format!(
                            "{} is {farthest_km:.0} km from the farthest of those hubs; nothing flies a {reach_km:.0} km round trip",
                            site.code
                        ),
                    );
                }
                let list = self.rules.serve_from.entry(site.code.clone()).or_default();
                for hub in hubs {
                    if !list.contains(hub) {
                        list.push(*hub);
                    }
                }
                let note = if farthest_km > reach_km {
                    format!(
                        "{farthest_km:.0} km from the farthest hub, beyond every vehicle's reach"
                    )
                } else {
                    format!("{farthest_km:.0} km from the farthest of those hubs")
                };
                Ok(("applied".into(), note))
            }
            Order::Move {
                class,
                count,
                from_cluster,
                to_cluster,
            } => {
                let (from, to) = match self.cluster_pair(*from_cluster, *to_cluster) {
                    Ok(pair) => pair,
                    Err(refused) => return Ok(refused),
                };
                if *count == 0 {
                    return refuse("bad_count", "count must be positive".into());
                }
                // To the hub with the most waiting; from the nearest hubs that
                // keep half their own, as the replanner's repositions do.
                let waiting = self.waiting();
                let load = |hub: u32| {
                    waiting
                        .iter()
                        .filter(|r| r.hub == hub && r.kind == RequestKind::Delivery)
                        .count()
                };
                let to_hub = *to
                    .iter()
                    .max_by_key(|&&h| (load(h), std::cmp::Reverse(h)))
                    .expect("a cluster has hubs");
                let destination = self.region.hub(to_hub).expect("checked").position_world_m;
                let mut donors: Vec<u32> = from.clone();
                donors.sort_by(|a, b| {
                    let d = |h: u32| {
                        horizontal_m(
                            self.region.hub(h).expect("checked").position_world_m,
                            destination,
                        )
                    };
                    d(*a).total_cmp(&d(*b)).then(a.cmp(b))
                });
                let mut wanted = *count;
                let mut sent = 0;
                let mut notes = Vec::new();
                for donor in donors {
                    if wanted == 0 {
                        break;
                    }
                    let (idle, total) =
                        self.fleet.values().fold((0u32, 0u32), |(idle, total), v| {
                            if v.hub != donor || class.is_some_and(|c| c != v.card.class) {
                                return (idle, total);
                            }
                            let free =
                                v.free() && !self.pending_relocations.contains_key(&v.vehicle_id);
                            (idle + u32::from(free), total + 1)
                        });
                    let spare = idle.saturating_sub(total.div_ceil(2));
                    if spare == 0 {
                        continue;
                    }
                    let n = spare.min(wanted);
                    let sub = Order::Reposition {
                        class: *class,
                        count: n,
                        from_hub: donor,
                        to_hub,
                        until_min: None,
                    };
                    let (outcome, note) = self.apply(context, now_s, &sub)?;
                    if outcome.starts_with("refused") {
                        continue;
                    }
                    notes.push(format!("hub {donor} to hub {to_hub}: {note}"));
                    wanted -= n;
                    sent += n;
                }
                if sent == 0 {
                    return refuse(
                        "too_few_idle",
                        format!("no hub of cluster {from_cluster} can spare a vehicle"),
                    );
                }
                let outcome = if sent < *count { "partial" } else { "applied" };
                Ok((outcome.into(), notes.join("; ")))
            }
            Order::Send {
                item,
                units,
                from_cluster,
                to_cluster,
            } => {
                let (from, to) = match self.cluster_pair(*from_cluster, *to_cluster) {
                    Ok(pair) => pair,
                    Err(refused) => return Ok(refused),
                };
                let Some(item) = self
                    .config
                    .stock
                    .items
                    .iter()
                    .find(|i| i.eq_ignore_ascii_case(item))
                    .cloned()
                else {
                    return refuse(
                        "unknown_item",
                        format!("the items are {}", self.config.stock.items.join(", ")),
                    );
                };
                let held = |hub: u32| self.stock[&hub].get(&item).copied().unwrap_or(0);
                // From the hub holding the most to the hub holding the least.
                let from_hub = *from
                    .iter()
                    .max_by_key(|&&h| (held(h), std::cmp::Reverse(h)))
                    .expect("a cluster has hubs");
                let to_hub = *to
                    .iter()
                    .min_by_key(|&&h| (held(h), h))
                    .expect("a cluster has hubs");
                let sub = Order::Transfer {
                    item,
                    units: *units,
                    from_hub,
                    to_hub,
                };
                let (outcome, note) = self.apply(context, now_s, &sub)?;
                Ok((outcome, format!("hub {from_hub} to hub {to_hub}: {note}")))
            }
            Order::UsePad { site, until_min } => {
                let Some(site) = self.region.site(site) else {
                    return refuse("unknown_site", format!("there is no site {site}"));
                };
                if site.kind != SiteKind::Hospital {
                    return refuse(
                        "not_hospital",
                        format!("{} is a clinic; drops already go to it", site.code),
                    );
                }
                let until_s = match until(*until_min) {
                    Ok(u) => u,
                    Err(e) => return Ok(e),
                };
                self.rules.pads.insert(site.code.clone(), until_s);
                Ok((
                    "applied".into(),
                    format!(
                        "drops at {}'s pad, 300 m from the rooftop; any vehicle can fly them",
                        site.code
                    ),
                ))
            }
        }
    }

    /// The hubs of two clusters named in a move or send, or why not.
    #[allow(clippy::type_complexity)]
    fn cluster_pair(&self, from: u32, to: u32) -> Result<(Vec<u32>, Vec<u32>), (String, String)> {
        let clusters = self.region.clusters(self.config.manager.cluster_side);
        let get = |index: u32| {
            (index >= 1)
                .then(|| clusters.get(index as usize - 1).cloned())
                .flatten()
        };
        match (get(from), get(to)) {
            (Some(_), Some(_)) if from == to => Err((
                "refused_same_cluster".into(),
                "from_cluster and to_cluster are the same".into(),
            )),
            (Some(from), Some(to)) => Ok((from, to)),
            _ => Err((
                "refused_unknown_cluster".into(),
                format!("clusters are 1 to {}", clusters.len()),
            )),
        }
    }

    /// Assigns waiting requests to idle vehicles, and records the requests
    /// past their deadline that nothing can serve.
    fn assign(&mut self, context: &mut AgentContext, now_s: f64) -> Result<()> {
        let mut waiting: Vec<Request> = self.waiting().into_iter().cloned().collect();
        waiting.sort_by(|a, b| {
            self.rules
                .prioritized(b)
                .cmp(&self.rules.prioritized(a))
                .then(a.deadline_s.total_cmp(&b.deadline_s))
                .then(a.id.cmp(&b.id))
        });
        let mut taken: BTreeSet<i32> = BTreeSet::new();
        let areas: Vec<Area> = self.rules.active_areas().into_iter().copied().collect();
        for request in &waiting {
            match self.best_vehicle(request, &taken, &areas, now_s) {
                Ok(candidate) => {
                    taken.insert(candidate.vehicle_id);
                    if let Some(stock) = self.stock.get_mut(&candidate.hub)
                        && let Some(units) = stock.get_mut(&request.item)
                    {
                        *units = units.saturating_sub(request.units);
                    }
                    self.pending
                        .insert(request.id, (candidate.vehicle_id, now_s));
                    if let Some(status) = self.fleet.get_mut(&candidate.vehicle_id) {
                        status.request_id = Some(request.id);
                    }
                    context.messages.publish(
                        "GlobalNetwork",
                        ASSIGNMENT_TOPIC,
                        Assignment {
                            vehicle_id: candidate.vehicle_id,
                            request_id: request.id,
                            hub: candidate.hub,
                            class: candidate.class,
                            skill: candidate.skill,
                            site_world_m: candidate.target_world_m,
                            via_world_m: candidate.via_world_m,
                        },
                    )?;
                }
                Err(reason) => {
                    if request.deadline_s < now_s && self.missed.insert(request.id) {
                        *self.tally.missed.entry(reason).or_insert(0) += 1;
                        context.messages.publish(
                            "GlobalNetwork",
                            MISSED_TOPIC,
                            Missed {
                                request_id: request.id,
                                reason,
                                time_s: now_s,
                            },
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// The idle vehicle that reaches the request soonest, or the most telling
    /// reason none can.
    fn best_vehicle(
        &self,
        request: &Request,
        taken: &BTreeSet<i32>,
        areas: &[Area],
        now_s: f64,
    ) -> Result<Candidate, MissReason> {
        let mut eligible = vec![request.hub];
        if let Some(extra) = self.rules.serve_from.get(&request.site) {
            eligible.extend(extra.iter().copied());
        }
        let use_pad = request.rooftop && self.rules.uses_pad(&request.site);
        let skill = if request.rooftop && !use_pad {
            SkillName::Land
        } else {
            SkillName::Drop
        };
        let target = if use_pad {
            request.pad_world_m
        } else {
            request.site_world_m
        };
        let area_refs: Vec<&Area> = areas.iter().collect();
        if area_refs.iter().any(|area| area.contains(target)) {
            return Err(MissReason::Closed);
        }
        let mut best: Option<Candidate> = None;
        // Reasons rank from least to most informative for the fleet manager.
        let mut reason = MissReason::NoVehicle;
        let note = |found: MissReason, reason: &mut MissReason| {
            if rank(found) > rank(*reason) {
                *reason = found;
            }
        };
        for hub in eligible {
            let Some(hub_position) = self.region.hub(hub).map(|h| h.position_world_m) else {
                continue;
            };
            if self.in_outage(hub, now_s) {
                note(MissReason::Grounded, &mut reason);
                continue;
            }
            if self
                .stock
                .get(&hub)
                .and_then(|s| s.get(&request.item))
                .copied()
                .unwrap_or(0)
                < request.units
            {
                note(MissReason::NoStock, &mut reason);
                continue;
            }
            let idle_here: Vec<&Status> = self
                .fleet
                .values()
                .filter(|v| {
                    v.hub == hub
                        && v.free()
                        && !taken.contains(&v.vehicle_id)
                        && !self.pending_relocations.contains_key(&v.vehicle_id)
                })
                .collect();
            let reserved: u32 = self
                .rules
                .reserves
                .iter()
                .filter(|r| r.hub == hub && !r.site.eq_ignore_ascii_case(&request.site))
                .map(|r| r.count)
                .sum();
            for vehicle in &idle_here {
                if self.rules.is_grounded(hub, vehicle.card.class) {
                    note(MissReason::Grounded, &mut reason);
                    continue;
                }
                if skill::info(skill).multirotor_only
                    && vehicle.card.class != VehicleClass::Multirotor
                {
                    continue;
                }
                if reserved > 0 && (idle_here.len() as u32) <= reserved {
                    continue;
                }
                let via = detour(
                    vehicle.position_world_m,
                    target,
                    &area_refs,
                    self.config.detour_margin_m,
                );
                let out_m = match via {
                    Some(via) => {
                        horizontal_m(vehicle.position_world_m, via) + horizontal_m(via, target)
                    }
                    None => horizontal_m(vehicle.position_world_m, target),
                };
                let back_m = match via {
                    Some(via) => horizontal_m(target, via) + horizontal_m(via, hub_position),
                    None => horizontal_m(target, hub_position),
                };
                let dwell_s = if skill::info(skill).lands {
                    self.config.dwell_s
                } else {
                    0.0
                };
                let round_trip_s = (out_m + back_m) / vehicle.card.cruise_speed_mps + dwell_s;
                if round_trip_s > vehicle.endurance_left_s * self.config.range_margin {
                    note(MissReason::OutOfRange, &mut reason);
                    continue;
                }
                let arrival_s = out_m / vehicle.card.cruise_speed_mps;
                if best.as_ref().is_none_or(|b| arrival_s < b.arrival_s) {
                    best = Some(Candidate {
                        vehicle_id: vehicle.vehicle_id,
                        hub,
                        class: vehicle.card.class,
                        skill,
                        target_world_m: target,
                        via_world_m: via,
                        arrival_s,
                    });
                }
            }
        }
        best.ok_or(reason)
    }
}

fn rank(reason: MissReason) -> u8 {
    match reason {
        MissReason::NoVehicle => 0,
        MissReason::OutOfRange => 1,
        MissReason::Grounded => 2,
        MissReason::NoStock => 3,
        MissReason::Closed => 4,
    }
}

/// The events a cluster's manager is told about: those naming one of its
/// hubs, with the hubs outside the cluster left out, and those naming none
/// (an area only).
fn events_touching(events: &[&Event], cluster: Option<&[u32]>) -> Vec<Event> {
    events
        .iter()
        .filter(|e| {
            cluster.is_none_or(|c| e.hubs.is_empty() || e.hubs.iter().any(|h| c.contains(h)))
        })
        .map(|e| {
            let mut event = (*e).clone();
            if let Some(cluster) = cluster {
                event.hubs.retain(|h| cluster.contains(h));
            }
            event
        })
        .collect()
}
