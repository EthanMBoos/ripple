//! Requests to idle vehicles. Each waiting request, in priority then
//! deadline order, goes to the idle vehicle at an eligible hub with the right
//! skill, the stock, and the endurance for the round trip, that arrives
//! soonest. The orders in force decide which hubs are eligible, what is
//! grounded or reserved, what comes first, and where flights detour. A hub
//! lent by a help order (`help.rs`) is eligible with two protections: it
//! keeps half of each class idle, counting the vehicles taken earlier in the
//! same pass, and it flies only a request it arrives at before the deadline.

use std::collections::BTreeSet;

use anyhow::Result;
use ripple_core::Vec3;
use ripple_core::plugin::AgentContext;

use super::Dispatcher;
use crate::area::detour;
use crate::help;
use crate::messages::{
    ASSIGNMENT_TOPIC, Assignment, MISSED_TOPIC, MissReason, Missed, Request, SkillName, Status,
    VehicleClass,
};
use crate::region::horizontal_m;
use crate::skill;

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
    /// Requests open and unassigned.
    pub(super) fn waiting(&self) -> Vec<&Request> {
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

    /// Assigns when something changed, at most every decision interval and
    /// at least every retry interval while work waits.
    pub(super) fn assign_if_due(&mut self, context: &mut AgentContext, now_s: f64) -> Result<()> {
        let due = (self.changed && now_s >= self.last_decision_s + self.config.decision_interval_s)
            || now_s >= self.last_decision_s + self.config.retry_interval_s;
        if due {
            self.assign(context, now_s)?;
            self.changed = false;
            self.last_decision_s = now_s;
        }
        Ok(())
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
        for request in &waiting {
            match self.best_vehicle(request, &taken, now_s) {
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
                        *self
                            .tallies
                            .entry(request.hub)
                            .or_default()
                            .missed
                            .entry(reason)
                            .or_insert(0) += 1;
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
        now_s: f64,
    ) -> Result<Candidate, MissReason> {
        let mut eligible = vec![request.hub];
        if let Some(extra) = self.rules.serve_from.get(&request.site) {
            eligible.extend(extra.iter().copied());
        }
        let serving = eligible.clone();
        eligible.extend(self.rules.lenders(request.hub));
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
            // The areas closed to this hub's flights: every avoid over the
            // region, and its own cluster's.
            let areas = self.rules.areas_for(hub);
            if areas.iter().any(|area| area.contains(target)) {
                note(MissReason::Closed, &mut reason);
                continue;
            }
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
            // A lender, not a hub the main study's serve-from order named.
            let lent = !serving.contains(&hub);
            for vehicle in &idle_here {
                if lent {
                    let class = vehicle.card.class;
                    let total = self
                        .fleet
                        .values()
                        .filter(|v| v.hub == hub && v.card.class == class)
                        .count() as u32;
                    let idle = idle_here.iter().filter(|v| v.card.class == class).count() as u32;
                    if idle <= help::keep(total) {
                        continue;
                    }
                }
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
                    &areas,
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
                if lent && now_s + arrival_s > request.deadline_s {
                    continue;
                }
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
