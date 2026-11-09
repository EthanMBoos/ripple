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
//!
//! The helpers sit beside this file: `dispatcher/config.rs` holds the
//! mission keys and their checks, `day.rs` the day's requests and each
//! tick's bookkeeping, `calls.rs` when and how each fleet manager is called,
//! `prompt.rs` what a model is shown, `apply.rs` each order's checks and
//! effect, and `assign.rs` requests to idle vehicles.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::Result;
use ripple_core::Vec3;
use ripple_core::plugin::{
    AgentContext, Autonomy, Drawing, Frame, PluginIo, PluginParams, PluginRandom, Port, Ports,
    SpawnRequest, Unit,
};
use serde::Serialize;

use crate::event::{Event, EventKind};
use crate::messages::{
    DELIVERED_TOPIC, Delivered, ManagerKind, MissReason, Request, STATUS_TOPIC, Status,
};
use crate::model::ModelManager;
use crate::order::Rules;
use crate::policy::Policy;
use crate::region::{Region, RegionConfig, SiteKind};
use crate::view::ViewKind;

mod apply;
mod assign;
mod calls;
mod config;
mod day;
mod prompt;

pub use config::DispatcherConfig;
use day::TRANSFER_REQUEST_BASE;

/// What a spawn request lays over a courier recipe: its hub and the region.
#[derive(Serialize)]
struct CourierHome {
    hub: u32,
    region: RegionConfig,
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
    /// The fleet still to ask the engine for.
    spawn_queue: VecDeque<SpawnRequest>,
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
        config.validate()?;
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
            spawn_queue: VecDeque::new(),
            stock,
            next_restock_s: config.stock.restock_interval_s,
            requests: BTreeMap::new(),
            schedule: Vec::new(),
            opened: 0,
            open: BTreeSet::new(),
            next_transfer_id: TRANSFER_REQUEST_BASE,
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
        self.draw_requests(&context.random.with(&[PluginRandom::name("requests")]))?;
        // The fleet: each hub's couriers, built by the engine from their
        // recipes at the hub, with the hub and the region laid over.
        for hub in &self.region.hubs {
            for entry in &self.config.fleet {
                let start_world_m = hub.position_world_m + Vec3::new(0.0, 0.0, entry.altitude_m);
                let request = SpawnRequest::recipe(&entry.recipe)
                    .count(entry.per_hub as usize)
                    .at(start_world_m)
                    .autonomy(
                        "Courier",
                        CourierHome {
                            hub: hub.id,
                            region: self.config.region.clone(),
                        },
                    )?;
                self.spawn_queue.push_back(request);
            }
        }
        Ok(())
    }

    /// One tick of the day, in this order: the fleet is asked for, reports
    /// and deliveries are read, repositioned vehicles due home are sent,
    /// requests open and events start, stock arrives and rules lapse, the
    /// fleet manager is called, and waiting requests are assigned.
    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let now_s = context.time.time_s;
        self.send_spawn_requests(context)?;
        self.read_reports(context)?;
        self.read_deliveries(context)?;
        self.send_returns_home(context, now_s)?;
        self.open_requests(context, now_s)?;
        let new_events = self.start_events(context, now_s)?;
        self.restock(now_s);
        self.expire(now_s);
        self.manage(context, now_s, &new_events)?;
        self.assign_if_due(context, now_s)?;
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
