//! What the fleet flew. Metrics phase: read requests, deliveries, misses,
//! events, assignments, orders and calls from GlobalNetwork, and the truth
//! positions for breaches of closed areas.
//!
//! The score is the on-time share of deliveries, weighting urgent requests by
//! `urgent_weight` and trauma by `trauma_weight`. Beside it: deliveries on
//! time and late by urgency, requests missed by reason, transfers flown,
//! vehicles lost, and for each event's rule the breaches measured from what
//! the fleet did: vehicle-seconds inside a closure or corridor, launches of a
//! grounded class, deliveries by landing on a closed rooftop, and launches
//! from a hub in an outage. Per hub, numbered `hub_001` on: its sites'
//! requests, on time, late and undelivered, and the relocations and stock
//! flights it gave and got, so a day's losses can be laid against the hubs
//! that donated. Only flights sent after an event started count
//! against it: a vehicle already in the air when a closure begins cannot be
//! called back, so its seconds inside are nobody's decision. A delivery whose
//! flight broke a rule does not count as delivered (`delivered_in_breach`),
//! so obeying the rules is never a loss on the score. Orders and calls are
//! counted with their outcomes.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use ripple_core::plugin::{MetricReport, Metrics, PluginParams, TeamMetrics, Update, WorldContext};
use serde::Deserialize;

use crate::event::{Event, EventKind};
use crate::messages::{
    ASSIGNMENT_TOPIC, Assignment, CALL_TOPIC, DELIVERED_TOPIC, Delivered, EVENT_TOPIC,
    MISSED_TOPIC, ManagerCall, Missed, ORDER_TOPIC, OrderRecord, Phase, RELOCATE_TOPIC,
    REQUEST_TOPIC, Relocate, Request, RequestKind, STATUS_TOPIC, SkillName, Status, Urgency,
};
use crate::order::Outcome;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeliveryScoreConfig {
    team: i32,
    urgent_weight: f64,
    trauma_weight: f64,
}

impl Default for DeliveryScoreConfig {
    fn default() -> Self {
        Self {
            team: 1,
            urgent_weight: 2.0,
            trauma_weight: 4.0,
        }
    }
}

pub struct DeliveryScore {
    team: i32,
    urgent_weight: f64,
    trauma_weight: f64,
    requests: BTreeMap<i32, Request>,
    /// First delivery of each request: time and skill.
    delivered: BTreeMap<i32, (f64, SkillName)>,
    /// Requests whose delivery flight broke a rule; they count as undelivered.
    voided: BTreeSet<i32>,
    /// Vehicles that broke a rule on their current flight.
    breached: BTreeSet<i32>,
    missed: BTreeMap<i32, Missed>,
    events: Vec<Event>,
    fleet: BTreeMap<i32, Status>,
    /// When each vehicle was last sent out, on a delivery or a relocation.
    sent_s: BTreeMap<i32, f64>,
    counts: BTreeMap<String, f64>,
    /// Vehicle-seconds inside each event's area, by event kind.
    area_breach_s: BTreeMap<EventKind, f64>,
}

impl Metrics for DeliveryScore {
    type Config = DeliveryScoreConfig;

    fn configure(params: &PluginParams) -> Result<DeliveryScoreConfig> {
        let config: DeliveryScoreConfig = params.parse()?;
        ensure!(
            config.urgent_weight > 0.0 && config.trauma_weight > 0.0,
            "DeliveryScore weights must be positive"
        );
        Ok(config)
    }

    fn new(config: &DeliveryScoreConfig) -> Self {
        Self {
            team: config.team,
            urgent_weight: config.urgent_weight,
            trauma_weight: config.trauma_weight,
            requests: BTreeMap::new(),
            delivered: BTreeMap::new(),
            voided: BTreeSet::new(),
            breached: BTreeSet::new(),
            missed: BTreeMap::new(),
            events: Vec::new(),
            fleet: BTreeMap::new(),
            sent_s: BTreeMap::new(),
            counts: BTreeMap::new(),
            area_breach_s: BTreeMap::new(),
        }
    }

    fn initialize(&mut self, context: &mut WorldContext) -> Result<()> {
        context
            .messages
            .subscribe::<Request>("GlobalNetwork", REQUEST_TOPIC)?;
        context
            .messages
            .subscribe::<Delivered>("GlobalNetwork", DELIVERED_TOPIC)?;
        context
            .messages
            .subscribe::<Missed>("GlobalNetwork", MISSED_TOPIC)?;
        context
            .messages
            .subscribe::<Event>("GlobalNetwork", EVENT_TOPIC)?;
        context
            .messages
            .subscribe::<Assignment>("GlobalNetwork", ASSIGNMENT_TOPIC)?;
        context
            .messages
            .subscribe::<Relocate>("GlobalNetwork", RELOCATE_TOPIC)?;
        context
            .messages
            .subscribe::<Status>("GlobalNetwork", STATUS_TOPIC)?;
        context
            .messages
            .subscribe::<OrderRecord>("GlobalNetwork", ORDER_TOPIC)?;
        context
            .messages
            .subscribe::<ManagerCall>("GlobalNetwork", CALL_TOPIC)
    }

    /// One tick: read what the fleet did since the last one, then measure
    /// the breaches of closed areas over this tick.
    fn step(&mut self, context: &mut WorldContext) -> Result<Update> {
        self.read_requests(context)?;
        self.read_events(context)?;
        self.read_statuses(context)?;
        self.read_assignments(context)?;
        self.read_relocations(context)?;
        self.read_deliveries(context)?;
        self.read_misses(context)?;
        self.read_orders(context)?;
        self.read_calls(context)?;
        self.measure_area_breaches(context);
        Ok(Update::Applied)
    }

    fn report(&self, _time_s: f64) -> MetricReport {
        let weight = |request: &Request| match request.urgency {
            Urgency::Routine => 1.0,
            Urgency::Urgent => self.urgent_weight,
            Urgency::Trauma => self.trauma_weight,
        };
        let mut values: BTreeMap<String, f64> = BTreeMap::new();
        let mut add = |name: String, value: f64| {
            *values.entry(name).or_insert(0.0) += value;
        };
        let (mut on_time_weight, mut total_weight) = (0.0, 0.0);
        for name in [
            "requests",
            "delivered",
            "on_time",
            "late",
            "undelivered",
            "transfers",
            "transfers_delivered",
        ] {
            add(name.into(), 0.0);
        }
        for urgency in [Urgency::Routine, Urgency::Urgent, Urgency::Trauma] {
            add(format!("{}_requests", urgency.as_str()), 0.0);
            add(format!("{}_on_time", urgency.as_str()), 0.0);
        }
        for request in self.requests.values() {
            if request.kind == RequestKind::Transfer {
                add("transfers".into(), 1.0);
                if self.delivered.contains_key(&request.id) {
                    add("transfers_delivered".into(), 1.0);
                }
                continue;
            }
            add("requests".into(), 1.0);
            add(format!("{}_requests", request.urgency.as_str()), 1.0);
            let hub = format!("hub_{:03}", request.hub);
            add(format!("{hub}_requests"), 1.0);
            for outcome in ["on_time", "late", "undelivered"] {
                add(format!("{hub}_{outcome}"), 0.0);
            }
            total_weight += weight(request);
            match self.delivered.get(&request.id) {
                Some((delivered_s, _)) => {
                    add("delivered".into(), 1.0);
                    if *delivered_s <= request.deadline_s {
                        on_time_weight += weight(request);
                        add("on_time".into(), 1.0);
                        add(format!("{}_on_time", request.urgency.as_str()), 1.0);
                        add(format!("{hub}_on_time"), 1.0);
                    } else {
                        add("late".into(), 1.0);
                        add(format!("{hub}_late"), 1.0);
                    }
                }
                None => {
                    add("undelivered".into(), 1.0);
                    add(format!("{hub}_undelivered"), 1.0);
                }
            }
        }
        for missed in self.missed.values() {
            add(format!("missed_{}", missed.reason.as_str()), 1.0);
        }
        for (name, value) in &self.counts {
            add(name.clone(), *value);
        }
        for (kind, seconds) in &self.area_breach_s {
            add(format!("{}_breach_s", kind.as_str()), *seconds);
        }
        add(
            "vehicles_lost".into(),
            self.fleet
                .values()
                .filter(|s| s.phase == Phase::Lost)
                .count() as f64,
        );
        // Requests for a priority hospital during its event.
        let (mut priority_requests, mut priority_on_time) = (0.0, 0.0);
        for event in self.events.iter().filter(|e| e.kind == EventKind::Priority) {
            for request in self.requests.values().filter(|r| {
                r.kind == RequestKind::Delivery
                    && r.site.eq_ignore_ascii_case(&event.site)
                    && r.opened_at_s >= event.at_s()
                    && r.opened_at_s <= event.until_s()
            }) {
                priority_requests += 1.0;
                if self
                    .delivered
                    .get(&request.id)
                    .is_some_and(|(t, _)| *t <= request.deadline_s)
                {
                    priority_on_time += 1.0;
                }
            }
        }
        if priority_requests > 0.0 {
            add("priority_requests".into(), priority_requests);
            add("priority_on_time".into(), priority_on_time);
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

impl DeliveryScore {
    /// Every request, for the report's per-request outcomes.
    fn read_requests(&mut self, context: &mut WorldContext) -> Result<()> {
        for message in context
            .messages
            .receive::<Request>("GlobalNetwork", REQUEST_TOPIC)?
        {
            self.requests
                .insert(message.value.id, (*message.value).clone());
        }
        Ok(())
    }

    /// The day's events, counted by kind.
    fn read_events(&mut self, context: &mut WorldContext) -> Result<()> {
        for message in context
            .messages
            .receive::<Event>("GlobalNetwork", EVENT_TOPIC)?
        {
            count(&mut self.counts, "events", 1.0);
            count(
                &mut self.counts,
                &format!("events_{}", message.value.kind.as_str()),
                1.0,
            );
            self.events.push((*message.value).clone());
        }
        Ok(())
    }

    /// Each vehicle's last status.
    fn read_statuses(&mut self, context: &mut WorldContext) -> Result<()> {
        for message in context
            .messages
            .receive::<Status>("GlobalNetwork", STATUS_TOPIC)?
        {
            self.fleet.insert(message.value.vehicle_id, *message.value);
        }
        Ok(())
    }

    /// Each flight sent: when, and whether its launch broke a weather
    /// or outage rule in force.
    fn read_assignments(&mut self, context: &mut WorldContext) -> Result<()> {
        for message in context
            .messages
            .receive::<Assignment>("GlobalNetwork", ASSIGNMENT_TOPIC)?
        {
            let order = &message.value;
            count(&mut self.counts, "flights", 1.0);
            self.sent_s.insert(order.vehicle_id, message.sent_at_s);
            self.breached.remove(&order.vehicle_id);
            for event in self.events.iter().filter(|e| e.active(message.sent_at_s)) {
                match event.kind {
                    EventKind::Weather
                        if event.hubs.contains(&order.hub)
                            && event.class.is_none_or(|c| c == order.class) =>
                    {
                        count(&mut self.counts, "weather_breaches", 1.0);
                        self.breached.insert(order.vehicle_id);
                    }
                    EventKind::Outage if event.hubs.contains(&order.hub) => {
                        count(&mut self.counts, "outage_breaches", 1.0);
                        self.breached.insert(order.vehicle_id);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Each relocation flight sent.
    fn read_relocations(&mut self, context: &mut WorldContext) -> Result<()> {
        for message in context
            .messages
            .receive::<Relocate>("GlobalNetwork", RELOCATE_TOPIC)?
        {
            self.sent_s
                .insert(message.value.vehicle_id, message.sent_at_s);
            self.breached.remove(&message.value.vehicle_id);
            count(&mut self.counts, "relocations", 1.0);
        }
        Ok(())
    }

    /// Each first delivery of a request, or its voiding when the flight
    /// broke a rule, including a landing on a closed rooftop.
    fn read_deliveries(&mut self, context: &mut WorldContext) -> Result<()> {
        for message in context
            .messages
            .receive::<Delivered>("GlobalNetwork", DELIVERED_TOPIC)?
        {
            let delivery = &message.value;
            if self.delivered.contains_key(&delivery.request_id)
                || self.voided.contains(&delivery.request_id)
            {
                continue;
            }
            let site = self
                .requests
                .get(&delivery.request_id)
                .map(|r| r.site.clone());
            let mut in_breach = self.breached.contains(&delivery.vehicle_id);
            if delivery.skill == SkillName::Land
                && let Some(site) = site
                && self.events.iter().any(|e| {
                    e.kind == EventKind::RooftopClosed
                        && e.active(delivery.time_s)
                        && e.site.eq_ignore_ascii_case(&site)
                })
            {
                count(&mut self.counts, "rooftop_breaches", 1.0);
                in_breach = true;
            }
            if in_breach {
                self.voided.insert(delivery.request_id);
                count(&mut self.counts, "delivered_in_breach", 1.0);
            } else {
                self.delivered
                    .insert(delivery.request_id, (delivery.time_s, delivery.skill));
            }
        }
        Ok(())
    }

    /// Each request missed, with its reason.
    fn read_misses(&mut self, context: &mut WorldContext) -> Result<()> {
        for message in context
            .messages
            .receive::<Missed>("GlobalNetwork", MISSED_TOPIC)?
        {
            self.missed.insert(message.value.request_id, *message.value);
        }
        Ok(())
    }

    /// Each order the fleet manager gave, by name and outcome, and the
    /// hubs that gave and got.
    fn read_orders(&mut self, context: &mut WorldContext) -> Result<()> {
        for message in context
            .messages
            .receive::<OrderRecord>("GlobalNetwork", ORDER_TOPIC)?
        {
            let record = &message.value;
            count(&mut self.counts, "orders", 1.0);
            if record.top {
                count(&mut self.counts, "top_orders", 1.0);
            }
            if record.quiet {
                count(&mut self.counts, "quiet_orders", 1.0);
            }
            if !record.outcome.is_refused() {
                count(&mut self.counts, "orders_applied", 1.0);
                // Relocations and stock flights against lent flights.
                match record.order.name() {
                    "reposition" | "transfer" | "move" | "send" => {
                        count(&mut self.counts, "moves", 1.0);
                    }
                    "serve_from" => count(&mut self.counts, "lends", 1.0),
                    _ => {}
                }
                for (from_hub, to_hub) in &record.moved {
                    count(&mut self.counts, &format!("hub_{from_hub:03}_gave"), 1.0);
                    count(&mut self.counts, &format!("hub_{to_hub:03}_got"), 1.0);
                }
                count(
                    &mut self.counts,
                    &format!("applied_{}", record.order.name()),
                    1.0,
                );
                if record.outcome == Outcome::Partial {
                    count(
                        &mut self.counts,
                        &format!("partial_{}", record.order.name()),
                        1.0,
                    );
                }
            } else {
                count(&mut self.counts, "orders_refused", 1.0);
                count(&mut self.counts, &record.outcome.to_string(), 1.0);
            }
        }
        Ok(())
    }

    /// Each fleet manager call and its size.
    fn read_calls(&mut self, context: &mut WorldContext) -> Result<()> {
        for message in context
            .messages
            .receive::<ManagerCall>("GlobalNetwork", CALL_TOPIC)?
        {
            let call = &message.value;
            count(&mut self.counts, "calls", 1.0);
            if call.top {
                count(&mut self.counts, "top_calls", 1.0);
            }
            if call.quiet {
                count(&mut self.counts, "quiet_calls", 1.0);
            }
            count(&mut self.counts, "dropped", f64::from(call.dropped));
            count(&mut self.counts, "prompt_chars", call.prompt_chars as f64);
            count(&mut self.counts, "reply_chars", call.reply_chars as f64);
            count(&mut self.counts, "queries", f64::from(call.queries));
            count(
                &mut self.counts,
                "parse_failures",
                f64::from(u8::from(call.parse_failed)),
            );
        }
        Ok(())
    }

    /// Vehicle-seconds inside each closed area over this tick, from truth,
    /// counting only vehicles sent after the event began.
    fn measure_area_breaches(&mut self, context: &WorldContext) {
        let now_s = context.time.time_s;
        let dt_s = context.time.dt_s;
        for event in self.events.iter().filter(|e| e.active(now_s)) {
            let Some(area) = &event.area else {
                continue;
            };
            let mut inside = 0;
            for entity in context.contacts_truth.iter() {
                let sent_after_closure = self
                    .sent_s
                    .get(&entity.id)
                    .is_some_and(|sent_s| *sent_s >= event.at_s());
                if entity.active
                    && entity.team_id == self.team
                    && sent_after_closure
                    && area.contains(entity.truth.position_world_m)
                {
                    inside += 1;
                    self.breached.insert(entity.id);
                }
            }
            if inside > 0 {
                *self.area_breach_s.entry(event.kind).or_insert(0.0) += f64::from(inside) * dt_s;
            }
        }
    }
}

/// Adds to a named count.
fn count(counts: &mut BTreeMap<String, f64>, name: &str, value: f64) {
    *counts.entry(name.to_string()).or_insert(0.0) += value;
}
