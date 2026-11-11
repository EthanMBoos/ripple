//! The dispatcher's mission keys: the region, the fleet built at each hub,
//! the stock, the day's requests, the events, and the fleet manager, with
//! their defaults and the checks `configure` runs before a day starts.

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::area::tagged;
use crate::client::{Client, Provider, Via};
use crate::event::EventsConfig;
use crate::messages::ManagerKind;
use crate::order::Order;
use crate::policy::Policy;
use crate::region::{Region, RegionConfig};
use crate::view::ViewKind;

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
    /// A call budget that does not grow with the fleet: calls are at least
    /// 3600 / calls_per_hour seconds apart, and events that start between
    /// calls wait for the next one, together. 0 means a call per event.
    pub calls_per_hour: f64,
    /// A fleet manager per cluster of hubs instead of one over the region:
    /// clusters are square blocks of this many hubs a side (0: one manager).
    /// Each is called with its own hubs' view, the events and rules that
    /// touch them, and its own feedback, and may only name its own hubs and
    /// sites; its rules naming no hub reach its hubs only.
    pub cluster_side: u32,
    /// Hand-written orders for `kind: scripted`.
    pub scripted: Vec<ScriptedOrder>,
    /// The rules file for `kind: policy` (`policy.rs`), a path from the
    /// working directory.
    pub policy: String,
    /// The help manager (`none`, `replanner` or `model`), called after the
    /// managers over the region or the clusters: it sees the help hubs ask
    /// for with the offers the dispatcher lists, and may only grant help, a
    /// hub serving an asking hub's sites for a time (`help.rs`). It sees
    /// the hub view (`view`).
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
            view: ViewKind::Summaries,
            feedback: false,
            checker: false,
            cost_feedback: false,
            guard: false,
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
    pub(super) fn policy(&self) -> Result<Option<Policy>> {
        if self.kind != ManagerKind::Policy {
            return Ok(None);
        }
        ensure!(!self.policy.is_empty(), "manager.policy names no file");
        let text = std::fs::read_to_string(&self.policy)
            .with_context(|| format!("reading manager.policy {}", self.policy))?;
        Policy::parse(&text).map(Some)
    }

    pub(super) fn client(&self) -> Result<Client> {
        Client::new(
            self.provider,
            self.via,
            self.command.as_deref(),
            self.host.as_deref(),
            self.offline || (!self.kind.uses_model() && self.top_kind != ManagerKind::Model),
        )
    }
}

/// The couriers the dispatcher builds at every hub from one of the
/// mission's recipes.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FleetEntry {
    pub recipe: String,
    pub per_hub: u32,
    /// Where they start above the hub; a fixed-wing recipe starts in the air.
    pub altitude_m: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DispatcherConfig {
    pub region: RegionConfig,
    /// The couriers to build at every hub, each from a recipe of the
    /// mission. Empty when the mission writes the couriers out.
    pub fleet: Vec<FleetEntry>,
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
            fleet: Vec::new(),
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

impl DispatcherConfig {
    /// The checks on a parsed configuration, in the order a mission author
    /// would meet them.
    pub(super) fn validate(&self) -> Result<()> {
        let region = Region::new(&self.region)?;
        ensure!(
            self.manager.cluster_side == 0 || region.side() % self.manager.cluster_side == 0,
            "manager.cluster_side must divide the region's side ({})",
            region.side()
        );
        for entry in &self.fleet {
            ensure!(
                !entry.recipe.trim().is_empty(),
                "every fleet entry names a recipe of the mission"
            );
            ensure!(
                entry.altitude_m.is_finite() && entry.altitude_m >= 0.0,
                "fleet entry {}: altitude_m must be finite and nonnegative",
                entry.recipe
            );
        }
        ensure!(
            !self.stock.items.is_empty(),
            "stock.items must name the blood types"
        );
        ensure!(
            self.stock.package_units > 0 && self.stock.restock_interval_s > 0.0,
            "stock.package_units and stock.restock_interval_s must be positive"
        );
        let requests = &self.requests;
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
        self.events.validate()?;
        ensure!(
            self.range_margin > 0.0
                && self.decision_interval_s > 0.0
                && self.retry_interval_s > 0.0
                && self.manager.check_in_s > 0.0,
            "range_margin, decision_interval_s, retry_interval_s and manager.check_in_s must be positive"
        );
        ensure!(
            self.manager.calls_per_hour >= 0.0,
            "manager.calls_per_hour must be 0 (a call per event) or positive"
        );
        ensure!(
            matches!(
                self.manager.top_kind,
                ManagerKind::None | ManagerKind::Replanner | ManagerKind::Model
            ),
            "manager.top_kind must be none, replanner or model"
        );
        if self.manager.top_kind == ManagerKind::Model {
            ensure!(
                !self.manager.prompt.system.trim().is_empty(),
                "manager.prompt.system is empty"
            );
        }
        if self.manager.kind.uses_model() {
            ensure!(
                !self.manager.prompt.system.trim().is_empty(),
                "manager.prompt.system is empty"
            );
            self.events.texts_complete()?;
        }
        self.manager.client()?;
        self.manager.policy()?;
        Ok(())
    }
}
