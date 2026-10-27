//! An instruction from operations that arrives mid-mission in plain language
//! and that no planner was programmed for: a clinic closure, a no-fly zone, a
//! multirotor kept on standby, or a grounded vehicle.
//!
//! The dispatcher announces it at `announce_s` and shows its text to the
//! model until `end_s`.
//!
//! Not built yet, if broken instructions need explaining: a Dispatcher
//! setting `instructions: direct | translate`. `direct` is what runs now.
//! `translate` would first ask the model, once per instruction, to turn the
//! text into a rule from a fixed menu (vehicle unavailable, site closed,
//! no-fly zone, keep a multirotor idle), and the call check would refuse calls
//! that break it. Compliance under `translate` says whether breaches come from
//! misreading the instruction or from slipping while planning, which also
//! answers "why not translate to constraints" (FLEET) without an optimizer. The score checks
//! compliance from what the fleet actually does between `start_s` and `end_s`.
//! The mission gives each kind's wordings in full, by name (`wordings` in
//! the Dispatcher's `directive` block), and `wording` picks one, so a planner
//! can be tested on whether it follows the rule however it is worded. A
//! wording is a template: `{site}`, `{vehicle}`, `{from}` and `{to}` (minutes),
//! `{x}`, `{y}` and `{radius}` (kilometers) are filled from the fields below.

use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use ripple_core::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectiveKind {
    None,
    /// Nothing may be delivered to `site` during the window.
    Closure,
    /// No vehicle may fly within `radius_m` of `center_m` during the window.
    NoFly,
    /// At least one multirotor stays at the depot during the window.
    ReserveMultirotor,
    /// `vehicle` may not start a delivery during the window.
    GroundVehicle,
}

impl DirectiveKind {
    /// The key under `wordings`, as the mission spells the kind.
    fn name(self) -> &'static str {
        match self {
            DirectiveKind::None => "none",
            DirectiveKind::Closure => "closure",
            DirectiveKind::NoFly => "no_fly",
            DirectiveKind::ReserveMultirotor => "reserve_multirotor",
            DirectiveKind::GroundVehicle => "ground_vehicle",
        }
    }
}

/// Mission parameters; `Default` is no instruction.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Directive {
    pub kind: DirectiveKind,
    /// The name of the wording used, from `wordings[kind]`.
    pub wording: String,
    /// Every wording of every kind, by kind and then by name: the text the
    /// model sees, with the placeholders in the module doc.
    pub wordings: BTreeMap<String, BTreeMap<String, String>>,
    pub announce_s: f64,
    pub start_s: f64,
    pub end_s: f64,
    pub site: String,
    pub center_m: [f64; 3],
    pub radius_m: f64,
    pub vehicle: i32,
}

impl Default for Directive {
    fn default() -> Self {
        Self {
            kind: DirectiveKind::None,
            wording: String::new(),
            wordings: BTreeMap::new(),
            announce_s: 120.0,
            start_s: 300.0,
            end_s: 1500.0,
            site: String::new(),
            center_m: [0.0; 3],
            radius_m: 600.0,
            vehicle: 0,
        }
    }
}

impl Directive {
    pub fn validate(&self) -> Result<()> {
        if self.active() {
            ensure!(
                self.template().is_some(),
                "directive wordings[{}] has no wording named `{}`",
                self.kind.name(),
                self.wording
            );
        }
        ensure!(
            self.announce_s <= self.start_s && self.start_s <= self.end_s,
            "directive times must satisfy announce_s <= start_s <= end_s"
        );
        match self.kind {
            DirectiveKind::Closure => ensure!(!self.site.is_empty(), "a closure needs a site"),
            DirectiveKind::NoFly => {
                ensure!(self.radius_m > 0.0, "a no-fly radius must be positive")
            }
            DirectiveKind::GroundVehicle => ensure!(self.vehicle > 0, "grounding needs a vehicle"),
            DirectiveKind::None | DirectiveKind::ReserveMultirotor => {}
        }
        Ok(())
    }

    pub fn active(&self) -> bool {
        self.kind != DirectiveKind::None
    }

    pub fn in_window(&self, time_s: f64) -> bool {
        self.start_s <= time_s && time_s <= self.end_s
    }

    pub fn center_world_m(&self) -> Vec3 {
        Vec3::from(self.center_m)
    }

    fn template(&self) -> Option<&String> {
        self.wordings.get(self.kind.name())?.get(&self.wording)
    }

    /// The instruction as operations would write it: the wording with its
    /// placeholders filled.
    pub fn text(&self) -> String {
        let Some(template) = self.template() else {
            return String::new();
        };
        template
            .replace("{site}", &self.site)
            .replace("{vehicle}", &self.vehicle.to_string())
            .replace("{from}", &format!("{:.0}", self.start_s / 60.0))
            .replace("{to}", &format!("{:.0}", self.end_s / 60.0))
            .replace("{x}", &format!("{:.1}", self.center_m[0] / 1000.0))
            .replace("{y}", &format!("{:.1}", self.center_m[1] / 1000.0))
            .replace("{radius}", &format!("{:.1}", self.radius_m / 1000.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wording_is_filled_from_the_fields() {
        let directive = Directive {
            kind: DirectiveKind::NoFly,
            wording: "atc".into(),
            wordings: BTreeMap::from([(
                "no_fly".into(),
                BTreeMap::from([(
                    "atc".into(),
                    "No flights within {radius} km of ({x}, {y}) km from minute {from} to \
                     minute {to}."
                        .into(),
                )]),
            )]),
            center_m: [-900.0, 2500.0, 0.0],
            ..Directive::default()
        };
        directive.validate().expect("the wording exists");
        assert_eq!(
            directive.text(),
            "No flights within 0.6 km of (-0.9, 2.5) km from minute 5 to minute 25."
        );
        let missing = Directive {
            wording: "plain".into(),
            ..directive
        };
        assert!(missing.validate().is_err());
        assert_eq!(Directive::default().text(), "");
    }
}
