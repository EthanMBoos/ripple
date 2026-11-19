//! The events of a day. Each day has a seeded script, or the mission's own
//! list; each event has a structured form (its fields) and one text, filled
//! from the mission's template for its kind. The dispatcher announces an event
//! when it starts; the fleet manager reads the text or the fields; the score
//! measures breaches against the fields.
//!
//! Regional events span several hubs: a surge of trauma requests, a shortage
//! of one blood type, a large airspace closure, and a hub outage. Local
//! events touch one hub or site: weather grounding multirotors, a priority
//! hospital, a closed rooftop, and a helicopter corridor.
//!
//! Some events are facts the dispatcher lives with (surge requests, zero
//! stock, a hub that cannot launch); the rest are rules that the dispatcher
//! alone does not know, so breaking them is what a fleet manager prevents.

use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use ripple_core::plugin::PluginRandom;
use serde::{Deserialize, Serialize};

use crate::area::{Area, tagged_option};
use crate::messages::VehicleClass;
use crate::region::{Region, RegionConfig, SiteKind, horizontal_m};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Surge,
    Shortage,
    Closure,
    Outage,
    Weather,
    Priority,
    RooftopClosed,
    Corridor,
}

/// The kinds drawn from `per_hub_per_day`.
pub const KINDS: [EventKind; 8] = [
    EventKind::Surge,
    EventKind::Shortage,
    EventKind::Closure,
    EventKind::Outage,
    EventKind::Weather,
    EventKind::Priority,
    EventKind::RooftopClosed,
    EventKind::Corridor,
];

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Surge => "surge",
            EventKind::Shortage => "shortage",
            EventKind::Closure => "closure",
            EventKind::Outage => "outage",
            EventKind::Weather => "weather",
            EventKind::Priority => "priority",
            EventKind::RooftopClosed => "rooftop_closed",
            EventKind::Corridor => "corridor",
        }
    }

    /// Whether the help manager is shown the event: a surge or
    /// an outage asks for help, a shortage or weather explains a cluster's
    /// state. A closure, corridor, rooftop closure, or priority is handled
    /// where it happens, by the cluster's own rules.
    pub fn shown_above(self) -> bool {
        matches!(
            self,
            EventKind::Surge | EventKind::Outage | EventKind::Shortage | EventKind::Weather
        )
    }

    /// How long the event lasts unless the mission says otherwise.
    fn default_duration_s(self) -> f64 {
        match self {
            EventKind::Surge => 3600.0,
            EventKind::Shortage => 7200.0,
            EventKind::Closure => 7200.0,
            EventKind::Outage => 5400.0,
            EventKind::Weather => 3600.0,
            EventKind::Priority => f64::INFINITY,
            EventKind::RooftopClosed => 7200.0,
            EventKind::Corridor => 3600.0,
        }
    }
}

/// One event. In the mission's `scripted` list only `kind` and `at_min` are
/// needed; the rest take the kind's defaults and `text` is filled from the
/// template.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Event {
    pub id: u32,
    pub kind: EventKind,
    pub at_min: f64,
    /// Infinity for the rest of the day.
    pub until_min: f64,
    pub hubs: Vec<u32>,
    pub site: String,
    pub item: String,
    #[serde(default, deserialize_with = "tagged_option")]
    pub area: Option<Area>,
    pub class: Option<VehicleClass>,
    pub text: String,
}

impl Default for Event {
    fn default() -> Self {
        Self {
            id: 0,
            kind: EventKind::Closure,
            at_min: 0.0,
            until_min: 0.0,
            hubs: Vec::new(),
            site: String::new(),
            item: String::new(),
            area: None,
            class: None,
            text: String::new(),
        }
    }
}

impl Event {
    pub fn at_s(&self) -> f64 {
        self.at_min * 60.0
    }

    pub fn until_s(&self) -> f64 {
        self.until_min * 60.0
    }

    pub fn active(&self, time_s: f64) -> bool {
        self.at_s() <= time_s && time_s <= self.until_s()
    }

    /// The event as its fields, one line; the text of an event kind the
    /// mission gives no wording for.
    pub fn structured(&self) -> String {
        let mut fields = vec![
            format!("kind: {}", self.kind.as_str()),
            format!("from minute {:.0}", self.at_min),
        ];
        if self.until_min.is_finite() {
            fields.push(format!("until minute {:.0}", self.until_min));
        } else {
            fields.push("until the end of the day".into());
        }
        if !self.hubs.is_empty() {
            fields.push(format!("hubs: {}", hub_list(&self.hubs)));
        }
        if !self.site.is_empty() {
            fields.push(format!("site: {}", self.site));
        }
        if !self.item.is_empty() {
            fields.push(format!("item: {}", self.item));
        }
        if let Some(area) = &self.area {
            fields.push(format!("area: {}", area.describe()));
        }
        if let Some(class) = self.class {
            fields.push(format!("class: {}", class.as_str()));
        }
        fields.join("; ")
    }
}

fn pick_item(items: &[String], pick: f64) -> String {
    if items.is_empty() {
        return String::new();
    }
    items[((pick * items.len() as f64) as usize).min(items.len() - 1)].clone()
}

pub fn hub_list(hubs: &[u32]) -> String {
    hubs.iter()
        .map(|hub| hub.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EventsConfig {
    /// Expected events of each kind per hub per 8-hour day, scaled to the
    /// day's length; the count is drawn from the seed. A kind left out does
    /// not happen.
    pub per_hub_per_day: BTreeMap<EventKind, f64>,
    /// A fixed list instead of the drawn script, when not empty.
    pub scripted: Vec<Event>,
    /// Trauma requests at each surge hub's hospitals.
    pub surge_requests_per_hub: u32,
    pub surge_window_min: f64,
    pub closure_radius_km: f64,
    pub corridor_length_km: f64,
    pub corridor_half_width_km: f64,
    /// One text per kind, with `{hubs}`, `{site}`, `{item}`, `{area}`,
    /// `{class}`, `{from}` and `{to}` (minutes) filled in.
    pub text: BTreeMap<EventKind, String>,
}

impl Default for EventsConfig {
    fn default() -> Self {
        Self {
            per_hub_per_day: BTreeMap::new(),
            scripted: Vec::new(),
            surge_requests_per_hub: 16,
            surge_window_min: 15.0,
            closure_radius_km: 6.0,
            corridor_length_km: 10.0,
            corridor_half_width_km: 0.5,
            text: BTreeMap::new(),
        }
    }
}

impl EventsConfig {
    pub fn validate(&self) -> Result<()> {
        for (kind, rate) in &self.per_hub_per_day {
            ensure!(
                rate.is_finite() && *rate >= 0.0,
                "events.per_hub_per_day.{} must be nonnegative",
                kind.as_str()
            );
        }
        ensure!(
            self.surge_window_min > 0.0
                && self.closure_radius_km > 0.0
                && self.corridor_length_km > 0.0
                && self.corridor_half_width_km > 0.0,
            "events: surge window, closure radius and corridor sizes must be positive"
        );
        for event in &self.scripted {
            ensure!(
                event.at_min.is_finite() && event.at_min >= 0.0,
                "events.scripted: at_min must be a nonnegative number of minutes"
            );
            if let Some(area) = &event.area {
                ensure!(area.valid(), "events.scripted: an area is malformed");
            }
        }
        Ok(())
    }

    /// Every text named, for a mission that shows events to a model.
    pub fn texts_complete(&self) -> Result<()> {
        for kind in KINDS {
            ensure!(
                self.text.get(&kind).is_some_and(|t| !t.trim().is_empty()),
                "events.text has no text for {}",
                kind.as_str()
            );
        }
        Ok(())
    }

    /// The day's events: the mission's list completed, or a script drawn from
    /// the seed, sorted by start. `day_s` is when requests stop opening;
    /// drawn events start at least an hour before.
    pub fn script(
        &self,
        region: &Region,
        day_s: f64,
        items: &[String],
        random: &PluginRandom,
    ) -> Vec<Event> {
        // Event membership uses the nominal grid so displaced layouts get
        // the same workload. Only physical areas follow the actual layout.
        // Policy donor choices still use actual distances in Region::neighbors.
        let nominal = Region::new(&RegionConfig {
            hubs: region.hubs.len() as u32,
            hub_spacing_m: region.hub_spacing_m,
            ..RegionConfig::default()
        })
        .expect("region already validated");
        let mut events = Vec::new();
        if !self.scripted.is_empty() {
            // The list is written for one block and laid over every block of
            // the region, the hubs it names turned with the block.
            for event in &self.scripted {
                for block in 0..region.blocks() {
                    let mut event = event.clone();
                    event.id = events.len() as u32 + 1;
                    event.hubs = event
                        .hubs
                        .iter()
                        .map(|&hub| region.tiled_hub(block, hub))
                        .collect();
                    self.complete(&mut event, &nominal, region, items, random);
                    events.push(event);
                }
            }
        } else {
            let draw_until_s = (day_s - 3600.0).max(0.0);
            for kind in KINDS {
                let rate = self.per_hub_per_day.get(&kind).copied().unwrap_or(0.0);
                let expected = rate * region.hubs.len() as f64 * day_s / 28_800.0;
                let key = |name: &str, index: u32| {
                    [
                        PluginRandom::name(kind.as_str()),
                        PluginRandom::name(name),
                        u64::from(index),
                    ]
                };
                let mut count = expected.floor() as u32;
                if random.uniform(&key("extra", 0)) < expected - expected.floor() {
                    count += 1;
                }
                for index in 0..count {
                    let hub =
                        1 + (random.uniform(&key("hub", index)) * region.hubs.len() as f64) as u32;
                    let mut event = Event {
                        kind,
                        at_min: random.uniform(&key("time", index)) * draw_until_s / 60.0,
                        hubs: vec![hub.min(region.hubs.len() as u32)],
                        ..Event::default()
                    };
                    let pick = random.uniform(&key("pick", index));
                    self.fill(&mut event, &nominal, region, items, pick);
                    self.complete(&mut event, &nominal, region, items, random);
                    events.push(event);
                }
            }
            events.sort_by(|a, b| a.at_min.total_cmp(&b.at_min).then(a.kind.cmp(&b.kind)));
            for (index, event) in events.iter_mut().enumerate() {
                event.id = index as u32 + 1;
            }
        }
        events
    }

    /// The fields a drawn event of its kind needs, from its first hub and one
    /// uniform number.
    fn fill(
        &self,
        event: &mut Event,
        nominal: &Region,
        region: &Region,
        items: &[String],
        pick: f64,
    ) {
        let hub = event.hubs[0];
        let center = region.hub(hub).expect("a drawn hub").position_world_m;
        let hospital = |index: usize| {
            region
                .sites_of(hub)
                .filter(|site| site.kind == SiteKind::Hospital)
                .nth(index)
                .cloned()
        };
        match event.kind {
            EventKind::Surge | EventKind::Weather => {
                event.hubs = nominal.group(hub);
                if event.kind == EventKind::Weather {
                    event.class = Some(VehicleClass::Multirotor);
                }
            }
            EventKind::Shortage => {
                event.hubs = nominal.group(hub);
                event.item = pick_item(items, pick);
            }
            EventKind::Closure => {
                // Keep the same pair of hubs across layouts; move the circle
                // to their actual midpoint without changing its radius.
                let neighbor = nominal.neighbors(hub).first().map(|h| h.id);
                let other = neighbor
                    .and_then(|id| region.hub(id))
                    .map(|h| h.position_world_m)
                    .unwrap_or(center + ripple_core::Vec3::new(region.hub_spacing_m, 0.0, 0.0));
                let middle = (center + other) / 2.0;
                event.area = Some(Area::Circle {
                    center_km: [middle.x / 1000.0, middle.y / 1000.0],
                    radius_km: self.closure_radius_km,
                });
                let nominal_center = nominal.hub(hub).expect("valid event hub").position_world_m;
                let nominal_other = neighbor
                    .and_then(|id| nominal.hub(id))
                    .map(|h| h.position_world_m)
                    .unwrap_or(
                        nominal_center + ripple_core::Vec3::new(region.hub_spacing_m, 0.0, 0.0),
                    );
                let nominal_middle = (nominal_center + nominal_other) / 2.0;
                event.hubs = nominal
                    .hubs
                    .iter()
                    .filter(|h| {
                        horizontal_m(h.position_world_m, nominal_middle)
                            <= self.closure_radius_km * 1000.0 + 13_000.0
                    })
                    .map(|h| h.id)
                    .collect();
            }
            EventKind::Outage => {}
            EventKind::Priority | EventKind::RooftopClosed => {
                let index = if pick < 0.5 { 0 } else { 1 };
                event.site = hospital(index).map(|s| s.code).unwrap_or_default();
            }
            EventKind::Corridor => {
                let start = hospital(0).map(|s| s.position_world_m).unwrap_or(center);
                let angle_rad = pick * std::f64::consts::TAU;
                let length_m = self.corridor_length_km * 1000.0;
                event.area = Some(Area::Corridor {
                    from_km: [start.x / 1000.0, start.y / 1000.0],
                    to_km: [
                        (start.x + length_m * angle_rad.cos()) / 1000.0,
                        (start.y + length_m * angle_rad.sin()) / 1000.0,
                    ],
                    half_width_km: self.corridor_half_width_km,
                });
            }
        }
    }

    /// Defaults for what the event leaves out: the fields its kind needs, drawn
    /// from its first hub, then its duration. A scripted event keeps every
    /// field it gives.
    fn complete(
        &self,
        event: &mut Event,
        nominal: &Region,
        region: &Region,
        items: &[String],
        random: &PluginRandom,
    ) {
        let needs_fill = match event.kind {
            EventKind::Closure | EventKind::Corridor => event.area.is_none(),
            EventKind::Priority | EventKind::RooftopClosed => event.site.is_empty(),
            EventKind::Shortage => event.item.is_empty(),
            // A scripted surge or weather names its hubs exactly; only one
            // naming none is spread over a neighborhood like a drawn one.
            EventKind::Surge | EventKind::Weather => event.hubs.is_empty(),
            EventKind::Outage => false,
        };
        if needs_fill {
            if event.hubs.is_empty() {
                event.hubs = vec![1];
            }
            let pick = random.uniform(&[PluginRandom::name("fill"), u64::from(event.id)]);
            self.fill(event, nominal, region, items, pick);
        }
        if event.until_min <= event.at_min {
            let duration_s = event.kind.default_duration_s();
            event.until_min = if duration_s.is_finite() {
                event.at_min + duration_s / 60.0
            } else {
                f64::INFINITY
            };
        }
        if event.hubs.is_empty()
            && !event.site.is_empty()
            && let Some(site) = region.site(&event.site)
        {
            event.hubs = vec![site.hub];
        }
        if event.hubs.is_empty() && event.area.is_none() {
            event.hubs = vec![1];
        }
        if event.kind == EventKind::Weather && event.class.is_none() {
            event.class = Some(VehicleClass::Multirotor);
        }
    }

    /// The text for an event whose fields are final.
    pub fn text(&self, event: &Event) -> String {
        let Some(template) = self.text.get(&event.kind) else {
            return event.structured();
        };
        let hubs = if event.hubs.len() == 1 {
            format!("hub {}", event.hubs[0])
        } else {
            format!("hubs {}", hub_list(&event.hubs))
        };
        let to = if event.until_min.is_finite() {
            format!("{:.0}", event.until_min)
        } else {
            "the end of the day".to_string()
        };
        template
            .replace("{hubs}", &hubs)
            .replace("{site}", &event.site)
            .replace("{item}", &event.item)
            .replace(
                "{area}",
                &event
                    .area
                    .as_ref()
                    .map(|a| a.describe())
                    .unwrap_or_default(),
            )
            .replace(
                "{class}",
                event.class.map(|c| c.as_str()).unwrap_or("vehicle"),
            )
            .replace("{from}", &format!("{:.0}", event.at_min))
            .replace("{to}", &to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawn_script_is_the_same_for_the_same_seed_and_scales_with_hubs() {
        let config = EventsConfig {
            per_hub_per_day: BTreeMap::from([(EventKind::Surge, 0.5), (EventKind::Closure, 1.0)]),
            ..EventsConfig::default()
        };
        let small = Region::new(&RegionConfig {
            hubs: 4,
            hub_spacing_m: 15_000.0,
            ..RegionConfig::default()
        })
        .unwrap();
        let large = Region::new(&RegionConfig {
            hubs: 16,
            hub_spacing_m: 15_000.0,
            ..RegionConfig::default()
        })
        .unwrap();
        let random = PluginRandom::new(3, 1, "test");
        let items = ["O-".to_string(), "A+".to_string()];
        let a = config.script(&small, 28_800.0, &items, &random);
        let b = config.script(&small, 28_800.0, &items, &random);
        assert_eq!(a.len(), b.len());
        assert!(
            a.iter()
                .zip(&b)
                .all(|(x, y)| x.at_min == y.at_min && x.kind == y.kind)
        );
        let closures = |events: &[Event]| {
            events
                .iter()
                .filter(|e| e.kind == EventKind::Closure)
                .count()
        };
        assert_eq!(closures(&a), 4);
        assert_eq!(
            closures(&config.script(&large, 28_800.0, &items, &random)),
            16
        );
        assert!(a.iter().all(|e| e.at_min <= (28_800.0 - 3600.0) / 60.0));
        let surge = a
            .iter()
            .find(|e| e.kind == EventKind::Surge)
            .expect("two surges expected");
        assert!(surge.hubs.len() >= 3, "{:?}", surge.hubs);
        assert!(a.windows(2).all(|w| w[0].at_min <= w[1].at_min));
    }

    #[test]
    fn a_scripted_event_takes_its_kinds_defaults_and_its_text() {
        let config = EventsConfig {
            scripted: vec![Event {
                kind: EventKind::RooftopClosed,
                at_min: 30.0,
                site: "2h1".into(),
                ..Event::default()
            }],
            text: BTreeMap::from([(
                EventKind::RooftopClosed,
                "The rooftop at {site} is closed from minute {from} to minute {to}.".into(),
            )]),
            ..EventsConfig::default()
        };
        let region = Region::new(&RegionConfig {
            hubs: 4,
            hub_spacing_m: 15_000.0,
            ..RegionConfig::default()
        })
        .unwrap();
        let events = config.script(&region, 7200.0, &[], &PluginRandom::new(1, 1, "test"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].until_min, 150.0);
        assert_eq!(events[0].hubs, vec![2]);
        assert_eq!(
            config.text(&events[0]),
            "The rooftop at 2h1 is closed from minute 30 to minute 150."
        );
    }

    #[test]
    fn a_scripted_surge_keeps_the_hubs_it_names() {
        let surge = |hubs: Vec<u32>| Event {
            kind: EventKind::Surge,
            at_min: 60.0,
            hubs,
            ..Event::default()
        };
        let config = EventsConfig {
            scripted: vec![surge(vec![1]), surge(vec![2, 3]), surge(Vec::new())],
            ..EventsConfig::default()
        };
        let region = Region::new(&RegionConfig {
            hubs: 4,
            hub_spacing_m: 15_000.0,
            ..RegionConfig::default()
        })
        .unwrap();
        let events = config.script(&region, 28_800.0, &[], &PluginRandom::new(1, 1, "test"));
        assert_eq!(events[0].hubs, vec![1], "one hub named stays one hub");
        assert_eq!(events[1].hubs, vec![2, 3]);
        assert_eq!(
            events[2].hubs,
            vec![1, 2, 3],
            "no hub named: a neighborhood"
        );
        assert_eq!(events[0].until_min, 120.0);
    }

    #[test]
    fn a_scripted_surge_is_laid_over_every_block() {
        let config = EventsConfig {
            scripted: vec![Event {
                kind: EventKind::Surge,
                at_min: 60.0,
                hubs: vec![6, 11],
                ..Event::default()
            }],
            ..EventsConfig::default()
        };
        let region = Region::new(&RegionConfig {
            hubs: 64,
            hub_spacing_m: 7000.0,
            block_side: 4,
            ..RegionConfig::default()
        })
        .unwrap();
        let events = config.script(&region, 28_800.0, &[], &PluginRandom::new(1, 1, "test"));
        assert_eq!(events.len(), 4);
        for (block, event) in events.iter().enumerate() {
            assert_eq!(event.id, block as u32 + 1);
            assert_eq!(event.at_min, 60.0);
            let expected: Vec<u32> = [6, 11]
                .iter()
                .map(|&h| region.tiled_hub(block, h))
                .collect();
            assert_eq!(event.hubs, expected, "block {block}");
        }
        assert_eq!(events[0].hubs, vec![10, 19]);
    }
}
