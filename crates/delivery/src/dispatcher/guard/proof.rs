//! What the reserve guard's proofs say. `counted` is `Donor::of`'s contract:
//! the counts are taken over the fleet's statuses, each ID once; `keeps_half`
//! and `keeps_stock` are the two decisions.
// Verus erases the predicates from a plain build; what only they use is
// behind `verus_only`.
#[cfg(verus_only)]
use std::collections::BTreeMap;

use vstd::prelude::*;

use super::super::PendingRelocation;
#[cfg(verus_only)]
use super::Donor;
#[cfg(verus_only)]
use crate::messages::proof::is_free;
#[cfg(verus_only)]
use crate::messages::{Status, VehicleClass};

verus! {

/// Declares a pending relocation as a value with no visible structure; the
/// guard only asks whether a vehicle has one.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExPendingRelocation(PendingRelocation);

/// Trusted: the standard library's division rounding up.
pub assume_specification[usize::div_ceil](x: usize, y: usize) -> (quotient: usize)
    requires y > 0
    ensures quotient == (x + y - 1) / y as int;

/// The status is hub `hub`'s and of `class`, or of any class when `None`.
pub open(crate) spec fn at_hub(status: &Status, hub: u32, class: Option<VehicleClass>) -> bool {
    &&& status.hub == hub
    &&& match class {
        Some(class) => status.card.class == class,
        None => true,
    }
}

/// `at_hub`, and free, and not already told to move.
pub open(crate) spec fn idle_at(status: &Status, moving: &BTreeMap<i32, PendingRelocation>, hub: u32, class: Option<VehicleClass>) -> bool {
    at_hub(status, hub, class) && is_free(status) && !moving@.contains_key(status.vehicle_id)
}

/// How many of the first `n` statuses satisfy `pred`.
pub open(crate) spec fn count_of(statuses: Seq<&Status>, n: int, pred: spec_fn(&Status) -> bool) -> int
    decreases n
{
    if n <= 0 { 0 } else { count_of(statuses, n - 1, pred) + if pred(statuses[n - 1]) { 1int } else { 0int } }
}

/// `statuses` is the fleet's statuses, one per vehicle ID.
pub open(crate) spec fn fleet_statuses(statuses: Seq<&Status>, fleet: &BTreeMap<i32, Status>) -> bool {
    exists|ids: Seq<i32>| {
        &&& #[trigger] ids.to_set() == fleet@.dom()
        &&& ids.no_duplicates()
        &&& statuses == ids.map(|i: int, id: i32| &fleet@[id])
    }
}

/// `Donor::of` counted the fleet's statuses at the hub and the idle ones.
pub open(crate) spec fn counted(fleet: &BTreeMap<i32, Status>, moving: &BTreeMap<i32, PendingRelocation>, hub: u32, class: Option<VehicleClass>, donor: &Donor) -> bool {
    exists|statuses: Seq<&Status>| {
        &&& #[trigger] fleet_statuses(statuses, fleet)
        &&& donor.total == count_of(statuses, statuses.len() as int, |status: &Status| at_hub(status, hub, class))
        &&& donor.idle == count_of(statuses, statuses.len() as int, |status: &Status| idle_at(status, moving, hub, class))
    }
}

/// The loop walking the fleet's statuses, as Verus sees it.
#[cfg(verus_only)]
pub(crate) type StatusWalk<'a> = vstd::std_specs::iter::VerusForLoopWrapper<std::collections::btree_map::Values<'a, i32, Status>>;

/// Partway through the count: the statuses walked are the fleet's, and the
/// counts cover those passed so far.
#[verifier::prophetic]
pub open(crate) spec fn counted_so_far(fleet: &BTreeMap<i32, Status>, moving: &BTreeMap<i32, PendingRelocation>, hub: u32, class: Option<VehicleClass>, total: usize, idle: usize, it: StatusWalk) -> bool {
    &&& fleet_statuses(it.seq(), fleet)
    &&& it.seq().len() == fleet.len()
    &&& total == count_of(it.seq(), it.index(), |status: &Status| at_hub(status, hub, class))
    &&& idle == count_of(it.seq(), it.index(), |status: &Status| idle_at(status, moving, hub, class))
    &&& idle <= total <= it.index()
}

/// What stays idle after `count` leave (none, if fewer are idle) is at least
/// half of all the hub has, rounded up.
pub open(crate) spec fn keeps_half(donor: &Donor, count: u32) -> bool {
    vstd::math::max(donor.idle - count, 0) >= (donor.total + 1) / 2
}

/// What stays after `units` leave (none, if fewer are there) is at least `keep`.
pub open(crate) spec fn keeps_stock(have: u32, units: u32, keep: u32) -> bool {
    vstd::math::max(have - units, 0) >= keep
}

} // verus!
