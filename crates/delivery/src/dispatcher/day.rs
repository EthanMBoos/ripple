//! The day as the dispatcher lives it: the requests drawn from the seed, and
//! each tick's bookkeeping before the fleet manager is called. The phases
//! here run in the order `step` lists them (`dispatcher.rs`).

use anyhow::{Result, ensure};
use ripple_core::plugin::{AgentContext, PluginRandom, SpawnRequest};

use super::{Dispatcher, PendingRelocation, Return};
use crate::event::EventKind;
use crate::messages::{
    DELIVERED_TOPIC, Delivered, EVENT_TOPIC, Phase, RELOCATE_TOPIC, REQUEST_TOPIC, Relocate,
    Request, RequestKind, STATUS_TOPIC, Status, Urgency,
};
use crate::region::SiteKind;

/// Spawn requests published per step: a large fleet is asked for over a few
/// ticks (two recipes at 1,024 hubs take five). The batching dates from a
/// limit on unsent messages and stays because a seed's day depends on the
/// tick each courier appears.
pub(super) const SPAWN_REQUESTS_PER_STEP: usize = 500;

/// Request ids by kind, so no two requests on a day share one. The day's
/// requests are `hub * 1000 + index + 1`; surge requests count up from
/// here by event, hub and request; stock transfers count up from their own
/// base as the day goes. The bases sit far apart because a 1,024-hub day
/// has a million day requests and thousands of events.
pub(super) const SURGE_REQUEST_BASE: i32 = 10_000_000;
pub(super) const TRANSFER_REQUEST_BASE: i32 = 2_000_000_000;

impl Dispatcher {
    /// The day's requests, each from its own keyed random numbers, plus the
    /// surges' trauma requests.
    pub(super) fn draw_requests(&mut self, random: &PluginRandom) -> Result<()> {
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
                // Unique across the day: event, then hub, then request. An
                // earlier `event * 100_000 + hub * 100 + k` collided once a
                // hub had 100 or more surge requests, and a colliding id made
                // the schedule wait for the wrong request (found 2026-11-09).
                let hub_stride = self.region.hubs.len() as i32 + 1;
                let per_hub = events.surge_requests_per_hub as i32;
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
                        id: SURGE_REQUEST_BASE
                            + (event.id as i32 * hub_stride + *hub as i32) * per_hub
                            + k as i32,
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
        ensure!(
            self.requests.len() == self.schedule.len(),
            "{} of the day's {} requests share an id with another",
            self.schedule.len() - self.requests.len(),
            self.schedule.len()
        );
        Ok(())
    }

    pub(super) fn deadline_s(&self, urgency: Urgency) -> f64 {
        match urgency {
            Urgency::Routine => self.config.requests.routine_deadline_s,
            Urgency::Urgent => self.config.requests.urgent_deadline_s,
            Urgency::Trauma => self.config.requests.trauma_deadline_s,
        }
    }

    pub(super) fn in_outage(&self, hub: u32, now_s: f64) -> bool {
        self.events
            .iter()
            .any(|e| e.kind == EventKind::Outage && e.hubs.contains(&hub) && e.active(now_s))
    }

    /// Asks the engine for the fleet, a few hundred couriers a tick.
    pub(super) fn send_spawn_requests(&mut self, context: &mut AgentContext) -> Result<()> {
        let requests_now = SPAWN_REQUESTS_PER_STEP.min(self.spawn_queue.len());
        for request in self.spawn_queue.drain(..requests_now) {
            context
                .messages
                .publish("GlobalNetwork", SpawnRequest::TOPIC, request)?;
        }
        Ok(())
    }

    /// Reads every courier's report: a vehicle freed, lost, relocated, or
    /// carrying a request.
    pub(super) fn read_reports(&mut self, context: &mut AgentContext) -> Result<()> {
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
                self.tallies.entry(status.hub).or_default().lost += 1;
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
        Ok(())
    }

    /// Reads the deliveries: a transfer's stock lands at its hub, and a
    /// delivery counts toward the fleet manager's tally.
    pub(super) fn read_deliveries(&mut self, context: &mut AgentContext) -> Result<()> {
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
                        let tally = self.tallies.entry(request.hub).or_default();
                        tally.delivered += 1;
                        if delivery.time_s <= request.deadline_s {
                            tally.on_time += 1;
                        } else {
                            tally.late += 1;
                            *self.late_by_hub.entry(request.hub).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Repositioned vehicles due home fly back once idle there.
    pub(super) fn send_returns_home(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
    ) -> Result<()> {
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
        Ok(())
    }

    /// Opens the day's requests whose time has come.
    pub(super) fn open_requests(&mut self, context: &mut AgentContext, now_s: f64) -> Result<()> {
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
        Ok(())
    }

    /// Starts the events whose time has come and returns their indices; a
    /// shortage empties its hubs' stock of the item.
    pub(super) fn start_events(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
    ) -> Result<Vec<usize>> {
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
        Ok(new_events)
    }

    /// Every hub receives its restock when the interval comes round.
    pub(super) fn restock(&mut self, now_s: f64) {
        if now_s >= self.next_restock_s {
            for stock in self.stock.values_mut() {
                for units in stock.values_mut() {
                    *units += self.config.stock.restock_units;
                }
            }
            self.next_restock_s += self.config.stock.restock_interval_s;
            self.changed = true;
        }
    }

    /// Rules past their time lapse, and orders a vehicle has not taken up
    /// in time are released.
    pub(super) fn expire(&mut self, now_s: f64) {
        self.rules.expire(now_s);
        let timeout_s = self.config.confirm_timeout_s;
        let before = self.pending.len() + self.pending_relocations.len();
        self.pending
            .retain(|_, &mut (_, sent_s)| now_s <= sent_s + timeout_s);
        self.pending_relocations
            .retain(|_, pending| now_s <= pending.sent_s + timeout_s);
        self.changed |= self.pending.len() + self.pending_relocations.len() != before;
    }
}
