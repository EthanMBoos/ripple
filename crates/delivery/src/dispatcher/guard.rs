//! The reserve guard (`manager.guard`): a hub that gives must keep half its
//! vehicles of the class and a quarter of its starting stock. The two
//! decisions here are proved with Verus against the fleet the dispatcher
//! knows; `apply.rs` refuses the order when they say so. This is the first
//! plugin rule proved on the engine's contracts; see the book's "Verifying the
//! engine" chapter.
use std::collections::BTreeMap;

use vstd::prelude::*;

use super::PendingRelocation;
use crate::messages::{Status, VehicleClass};

pub(super) mod proof;

verus! {

/// Hub `hub`'s vehicles of one class as the guard sees them. Crate-visible so
/// the proof can read its counts.
pub(crate) struct Donor {
    /// How many it has.
    pub total: usize,
    /// How many are idle and not already told to move.
    pub idle: usize,
}

impl Donor {
    /// Counts hub `hub`'s vehicles of `class` (every class when `None`).
    pub(super) fn of(
        fleet: &BTreeMap<i32, Status>,
        moving: &BTreeMap<i32, PendingRelocation>,
        hub: u32,
        class: Option<VehicleClass>,
    ) -> (donor: Donor)
        ensures proof::counted(fleet, moving, hub, class, &donor)
    {
        let mut total: usize = 0;
        let mut idle: usize = 0;
        for status in it: fleet.values()
            invariant proof::counted_so_far(fleet, moving, hub, class, total, idle, it)
        {
            let of_class = match class {
                Some(class) => status.card.class == class,
                None => true,
            };
            if status.hub == hub && of_class {
                total += 1;
                if status.free() && !moving.contains_key(&status.vehicle_id) {
                    idle += 1;
                }
            }
        }
        Donor { total, idle }
    }

    /// Whether the hub keeps at least half its vehicles after `count` leave.
    pub(super) fn keeps_half(&self, count: u32) -> (keeps: bool)
        ensures keeps == proof::keeps_half(self, count)
    {
        self.idle.saturating_sub(count as usize) >= self.total.div_ceil(2)
    }
}

/// Whether a hub with `have` units keeps at least `keep` after `units` leave.
pub(super) fn keeps_stock(have: u32, units: u32, keep: u32) -> (keeps: bool)
    ensures keeps == proof::keeps_stock(have, units, keep)
{
    have.saturating_sub(units) >= keep
}

} // verus!
