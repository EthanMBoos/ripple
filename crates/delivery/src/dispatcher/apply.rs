//! Each order's checks and effect. `apply` hands an order to its function,
//! which returns the outcome (applied, partial, or refused with a reason)
//! and what the fleet manager is told about it. Every order is checked for
//! sense (a hub that exists, a site that exists); the `checker` interface
//! also refuses what breaks a rule or cannot be flown.

use anyhow::Result;
use ripple_core::plugin::AgentContext;

use super::{Dispatcher, PendingRelocation, Return};
use crate::help;
use crate::messages::{
    Phase, RELOCATE_TOPIC, REQUEST_TOPIC, Relocate, Request, RequestKind, Status, Urgency,
};
use crate::order::{Order, Outcome};
use crate::region::{SiteKind, horizontal_m};
use crate::rules::{Avoid, Grounded, Lend, Priority, Reserve};
use crate::scope::Scope;

/// A refusal: the outcome names the reason, the note says why.
fn refuse(reason: &'static str, why: String) -> Result<(Outcome, String)> {
    Ok((Outcome::Refused(reason), why))
}

/// A minute of the day as seconds, if it is still to come.
fn until(now_s: f64, until_min: f64) -> Result<f64, (Outcome, String)> {
    let until_s = until_min * 60.0;
    if !until_s.is_finite() || until_s <= now_s {
        return Err((
            Outcome::Refused("past_time"),
            format!(
                "until_min {until_min} is not after minute {:.0}",
                now_s / 60.0
            ),
        ));
    }
    Ok(until_s)
}

impl Dispatcher {
    /// Checks an order and applies it. Returns the outcome and what the
    /// fleet manager is told about it. `scope` is the giver's, already
    /// checked to cover every hub and site the order names; a rule that
    /// names no place (an avoid, a priority by urgency) is placed by it.
    pub(super) fn apply(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
        order: &Order,
        scope: &Scope,
    ) -> Result<(Outcome, String)> {
        let (outcome, note) = match order {
            Order::Reserve { .. } => self.apply_reserve(now_s, order)?,
            Order::Avoid { .. } => self.apply_avoid(now_s, order, scope)?,
            Order::Reposition { .. } => self.apply_reposition(context, now_s, order)?,
            Order::Transfer { .. } => self.apply_transfer(context, now_s, order)?,
            Order::Ground { .. } => self.apply_ground(now_s, order)?,
            Order::Prioritize { .. } => self.apply_prioritize(now_s, order, scope)?,
            Order::ServeFrom { .. } => self.apply_serve_from(order)?,
            Order::UsePad { .. } => self.apply_use_pad(now_s, order)?,
            Order::Help { .. } => self.apply_help(now_s, order)?,
        };
        // A relocation or stock flight that went ahead is a cost at its donor
        // hub, reported with the next feedback.
        if !outcome.is_refused() {
            match order {
                Order::Reposition {
                    from_hub, to_hub, ..
                }
                | Order::Transfer {
                    from_hub, to_hub, ..
                } => {
                    self.move_costs.push((order.describe(), *from_hub));
                    self.moved.push((*from_hub, *to_hub));
                }
                _ => {}
            }
        }
        Ok((outcome, note))
    }

    fn hub_exists(&self, hub: u32) -> bool {
        self.region.hub(hub).is_some()
    }

    /// How far out the longest-legged vehicle flies and still returns, in km.
    /// A fixed-wing round trip at 25 m/s and 60 min reaches 40 km out.
    fn reach_km(&self) -> f64 {
        self.fleet
            .values()
            .map(|v| {
                v.card.cruise_speed_mps * v.card.endurance_s * self.config.range_margin / 2000.0
            })
            .fold(0.0, f64::max)
    }

    /// From the help manager: lets a hub serve another hub's sites for a
    /// time (`help.rs`). The pair must be one the dispatcher offered, which
    /// is the study's contract whether or not the checker is on. The asking
    /// hub's request is answered.
    fn apply_help(&mut self, now_s: f64, order: &Order) -> Result<(Outcome, String)> {
        let Order::Help { hub, from_hub } = order else {
            unreachable!("matched in apply")
        };
        if !self.hub_exists(*hub) || !self.hub_exists(*from_hub) {
            return refuse(
                "unknown_hub",
                format!("hubs are 1 to {}", self.region.hubs.len()),
            );
        }
        if hub == from_hub {
            return refuse("same_hub", "hub and from_hub are the same".into());
        }
        let Some(request) = self.help_requests.iter().find(|r| r.hub == *hub) else {
            return refuse("not_asking", format!("hub {hub} is not asking for help"));
        };
        let until_s = request.until_s;
        if self.rules.lends.iter().any(|l| l.hub == *hub) {
            return refuse(
                "already_helped",
                format!("hub {hub} has been granted help already"),
            );
        }
        // The offers are those shown at the call, less the hubs lent since,
        // so a hub granted earlier in this call is refused for a second
        // asking hub. The granted request is dropped before the next call
        // (`ask_for_help`), not here, so the list of asking hubs holds.
        let snapshot = self.snapshot_of(now_s, &Scope::Region);
        if !help::offered(*hub, *from_hub, &snapshot, &self.help_requests) {
            return refuse(
                "not_offered",
                format!(
                    "hub {from_hub} is not offered to hub {hub} now; take from_hub from an offer"
                ),
            );
        }
        let donor = self
            .region
            .hub(*from_hub)
            .expect("checked")
            .position_world_m;
        let farthest_km = self
            .region
            .sites_of(*hub)
            .map(|site| horizontal_m(donor, site.position_world_m))
            .fold(0.0, f64::max)
            / 1000.0;
        let (idle, total) = self.fleet.values().fold((0u32, 0u32), |(idle, total), v| {
            if v.hub != *from_hub {
                return (idle, total);
            }
            (idle + u32::from(v.free()), total + 1)
        });
        self.rules.lends.push(Lend {
            hub: *hub,
            from_hub: *from_hub,
            until_s,
        });
        let sites = self.region.sites_of(*hub).count();
        Ok((
            Outcome::Applied,
            format!(
                "hub {from_hub}'s {idle} idle of {total} may serve hub {hub}'s {sites} sites until minute {:.0}, {farthest_km:.0} km to the farthest; it keeps half idle and flies only what arrives on time",
                until_s / 60.0
            ),
        ))
    }

    /// Holds vehicles at a hub for one site.
    fn apply_reserve(&mut self, now_s: f64, order: &Order) -> Result<(Outcome, String)> {
        let Order::Reserve {
            hub,
            count,
            site,
            until_min,
        } = order
        else {
            unreachable!("matched in apply")
        };
        let checker = self.config.manager.checker;
        if !self.hub_exists(*hub) {
            return refuse("unknown_hub", format!("there is no hub {hub}"));
        }
        let Some(site) = self.region.site(site) else {
            return refuse("unknown_site", format!("there is no site {site}"));
        };
        let until_s = match until(now_s, *until_min) {
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
            Outcome::Applied,
            format!(
                "{count} of hub {hub}'s {at_hub} vehicles held for {}",
                site.code
            ),
        ))
    }

    /// Routes the giver's flights around an area.
    fn apply_avoid(
        &mut self,
        now_s: f64,
        order: &Order,
        scope: &Scope,
    ) -> Result<(Outcome, String)> {
        let Order::Avoid { area, until_min } = order else {
            unreachable!("matched in apply")
        };
        if !area.valid() {
            return refuse("malformed_area", "the area is malformed".into());
        }
        let until_s = match until(now_s, *until_min) {
            Ok(u) => u,
            Err(e) => return Ok(e),
        };
        self.rules.avoids.push(Avoid {
            area: *area,
            until_s,
            scope: scope.clone(),
        });
        let hubs_inside: Vec<String> = self
            .region
            .hubs
            .iter()
            .filter(|h| scope.covers_hub(h.id))
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
        Ok((Outcome::Applied, note))
    }

    /// Flies idle vehicles to another hub, for good or until a minute.
    fn apply_reposition(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
        order: &Order,
    ) -> Result<(Outcome, String)> {
        let Order::Reposition {
            class,
            count,
            from_hub,
            to_hub,
            until_min,
        } = order
        else {
            unreachable!("matched in apply")
        };
        let checker = self.config.manager.checker;
        if !self.hub_exists(*from_hub) || !self.hub_exists(*to_hub) {
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
            notes.push("beyond the endurance of some of them: they will be lost on the way".into());
        }
        // Partial: the order asked for more than was there, which the
        // fleet manager's view should have shown.
        let outcome = if moved.len() < *count as usize {
            Outcome::Partial
        } else {
            Outcome::Applied
        };
        Ok((outcome, notes.join("; ")))
    }

    /// Flies stock to another hub as transfer requests.
    fn apply_transfer(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
        order: &Order,
    ) -> Result<(Outcome, String)> {
        let Order::Transfer {
            item,
            units,
            from_hub,
            to_hub,
        } = order
        else {
            unreachable!("matched in apply")
        };
        let checker = self.config.manager.checker;
        if !self.hub_exists(*from_hub) || !self.hub_exists(*to_hub) {
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
            Outcome::Partial
        } else {
            Outcome::Applied
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
            note.push_str(
                "; beyond every vehicle's round-trip range, so the flights cannot be assigned",
            );
        }
        Ok((outcome, note))
    }

    /// Stops a class, or every vehicle, launching from hubs.
    fn apply_ground(&mut self, now_s: f64, order: &Order) -> Result<(Outcome, String)> {
        let Order::Ground {
            class,
            hubs,
            until_min,
        } = order
        else {
            unreachable!("matched in apply")
        };
        if hubs.is_empty() || hubs.iter().any(|h| !self.hub_exists(*h)) {
            return refuse(
                "unknown_hub",
                format!("hubs are 1 to {}", self.region.hubs.len()),
            );
        }
        let until_s = match until(now_s, *until_min) {
            Ok(u) => u,
            Err(e) => return Ok(e),
        };
        self.rules.grounded.push(Grounded {
            class: *class,
            hubs: hubs.clone(),
            until_s,
        });
        Ok((
            Outcome::Applied,
            "no launches of that class from those hubs until then".into(),
        ))
    }

    /// Puts a site's or an urgency's requests first.
    fn apply_prioritize(
        &mut self,
        now_s: f64,
        order: &Order,
        scope: &Scope,
    ) -> Result<(Outcome, String)> {
        let Order::Prioritize {
            site,
            urgency,
            until_min,
        } = order
        else {
            unreachable!("matched in apply")
        };
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
        let until_s = match until(now_s, *until_min) {
            Ok(u) => u,
            Err(e) => return Ok(e),
        };
        // A site priority is placed by its site; an urgency alone names no
        // place, so the giver's scope places it.
        let scope = if site.is_none() {
            scope.clone()
        } else {
            Scope::Region
        };
        self.rules.priorities.push(Priority {
            site,
            urgency: *urgency,
            until_s,
            scope,
        });
        Ok((Outcome::Applied, "those requests now come first".into()))
    }

    /// Lets other hubs serve a site besides its own.
    fn apply_serve_from(&mut self, order: &Order) -> Result<(Outcome, String)> {
        let Order::ServeFrom { site, hubs } = order else {
            unreachable!("matched in apply")
        };
        let checker = self.config.manager.checker;
        let Some(site) = self.region.site(site) else {
            return refuse("unknown_site", format!("there is no site {site}"));
        };
        if hubs.is_empty() || hubs.iter().any(|h| !self.hub_exists(*h)) {
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
        let reach_km = self.reach_km();
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
            format!("{farthest_km:.0} km from the farthest hub, beyond every vehicle's reach")
        } else {
            format!("{farthest_km:.0} km from the farthest of those hubs")
        };
        Ok((Outcome::Applied, note))
    }

    /// Delivers to a hospital's ground pad by drop while its rooftop is closed.
    fn apply_use_pad(&mut self, now_s: f64, order: &Order) -> Result<(Outcome, String)> {
        let Order::UsePad { site, until_min } = order else {
            unreachable!("matched in apply")
        };
        let Some(site) = self.region.site(site) else {
            return refuse("unknown_site", format!("there is no site {site}"));
        };
        if site.kind != SiteKind::Hospital {
            return refuse(
                "not_hospital",
                format!("{} is a clinic; drops already go to it", site.code),
            );
        }
        let until_s = match until(now_s, *until_min) {
            Ok(u) => u,
            Err(e) => return Ok(e),
        };
        self.rules.pads.insert(site.code.clone(), until_s);
        Ok((
            Outcome::Applied,
            format!(
                "drops at {}'s pad, 300 m from the rooftop; any vehicle can fly them",
                site.code
            ),
        ))
    }
}
