//! What the readings functions claim, in words a reader of readings.rs can check.
use super::{AnyReading, Reading, Readings};
#[cfg(verus_only)]
use std::collections::{BTreeMap, BTreeSet};
use vstd::prelude::*;

verus! {

/// Lets Verus read `Readings`' fields without a `verus!` block around the struct.
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExReadings(Readings);

/// Lets Verus read a `Reading`'s fields.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::reject_recursive_types(T)]
pub struct ExReading<T>(Reading<T>);

/// Declares `AnyReading` to Verus as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExAnyReading(AnyReading);

/// The facts a readings proof needs, so readings.rs names one group.
#[allow(dead_code)]
pub broadcast group reading_facts {
    crate::proof::string_key_facts,
    crate::proof::text_facts,
    vstd::seq_lib::group_seq_lib_default,
    vstd::set_lib::group_set_lib_default,
    all_marked,
}

/// The readings a slot map holds, by sensor name.
pub open(crate) spec fn slots(m: &BTreeMap<String, AnyReading>) -> Map<Seq<char>, AnyReading> {
    crate::proof::by_text(m@)
}

/// The sensor names a set holds.
pub open(crate) spec fn names(s: &BTreeSet<String>) -> Set<Seq<char>> {
    s@.map(|k: String| k@)
}

/// `get` returned the sensor's reading when it has one of the asked type, `None`
/// when it has none, and an error when its reading is of another type.
pub open(crate) spec fn got<T>(io: &Readings, sensor: &str, reading: &Result<Option<&Reading<T>>, anyhow::Error>) -> bool {
    let present = slots(&io.values).contains_key(sensor@);
    &&& !present ==> reading is Ok && reading->Ok_0 is None
    &&& present ==> {
        let stored = slots(&io.values)[sensor@];
        &&& (reading is Ok) == axioms::holds::<T>(stored)
        &&& reading is Ok ==> reading->Ok_0 is Some && *reading->Ok_0->0 == axioms::contents::<T>(stored)
    }
}

/// `sensor` is among the names delivered since the last estimation step.
pub open(crate) spec fn is_fresh(io: &Readings, sensor: &str) -> bool {
    names(&io.fresh).contains(sensor@)
}

/// `publish` stored the reading, with its time, in the sensor's slot and no
/// other, and left the fresh set alone.
pub open(crate) spec fn published<T>(before: &Readings, after: &Readings, sensor: &str, time_s: f64, value: T) -> bool {
    let stored = slots(&after.values)[sensor@];
    &&& slots(&after.values) == slots(&before.values).insert(sensor@, stored)
    &&& axioms::holds::<T>(stored)
    &&& axioms::contents::<T>(stored) == Reading { sampled_at_s: time_s, value }
    &&& after.fresh@ == before.fresh@
}

/// `deliver` moved every pending reading into its sensor's slot, marked exactly
/// those sensors fresh on top of the ones already fresh, and left `pending`
/// with no readings and its fresh set untouched.
pub open(crate) spec fn delivered(before: &Readings, after: &Readings, taken: &Readings, left: &Readings) -> bool {
    &&& slots(&after.values) == slots(&before.values).union_prefer_right(slots(&taken.values))
    &&& names(&after.fresh) == names(&before.fresh).union(slots(&taken.values).dom())
    &&& slots(&left.values) == Map::<Seq<char>, AnyReading>::empty()
    &&& left.fresh@ == taken.fresh@
}

/// `clear_fresh` emptied the fresh set and touched no reading.
pub open(crate) spec fn cleared(before: &Readings, after: &Readings) -> bool {
    &&& after.fresh@ == Set::<String>::empty()
    &&& after.values@ == before.values@
}

/// The marking loop's invariant: `sensors` are the pending names, the first
/// `count` of them are fresh on top of the names that were, and no reading has
/// moved.
pub open(crate) spec fn marked_so_far(before: &Readings, now: &Readings, pending: &Readings, sensors: Seq<&String>, count: int) -> bool {
    &&& sensors.unref().to_set() == pending.values@.dom()
    &&& now.values@ == before.values@
    &&& forall|k: String| #[trigger] now.fresh@.contains(k) <==> before.fresh@.contains(k) || exists|i: int| 0 <= i < count && *#[trigger] sensors[i] == k
}

/// Once every pending name is marked, the fresh names are the old ones and the
/// pending sensors.
pub broadcast proof fn all_marked(before: &Readings, now: &Readings, pending: &Readings, sensors: Seq<&String>)
    requires #[trigger] marked_so_far(before, now, pending, sensors, sensors.len() as int),
    ensures names(&now.fresh) == names(&before.fresh).union(slots(&pending.values).dom()),
{
    broadcast use crate::proof::string_key_facts, crate::proof::text_facts, vstd::seq_lib::group_seq_lib_default, vstd::set_lib::group_set_lib_default;
    assert forall|name: Seq<char>| #![auto] names(&now.fresh).contains(name)
        <==> names(&before.fresh).contains(name) || slots(&pending.values).dom().contains(name) by {
        if names(&now.fresh).contains(name) {
            let k = choose|k: String| #[trigger] now.fresh@.contains(k) && k@ == name;
            if !before.fresh@.contains(k) {
                let i = choose|i: int| 0 <= i < sensors.len() && *#[trigger] sensors[i] == k;
                assert(sensors.unref()[i] == k);
                assert(sensors.unref().contains(k));
                assert(pending.values@.contains_key(k));
            }
        }
        if names(&before.fresh).contains(name) {
            let k = choose|k: String| #[trigger] before.fresh@.contains(k) && k@ == name;
            assert(now.fresh@.contains(k));
        }
        if slots(&pending.values).dom().contains(name) {
            let s = choose|s: String| #[trigger] pending.values@.contains_key(s) && s@ == name;
            assert(sensors.unref().contains(s));
            let i = choose|i: int| 0 <= i < sensors.len() && sensors.unref()[i] == s;
            assert(*sensors[i] == s);
            assert(now.fresh@.contains(s));
        }
    }
    assert(names(&now.fresh) =~= names(&before.fresh).union(slots(&pending.values).dom()));
}

/// Trusted: what the methods of `any_reading.rs` return, and what `append` does.
pub mod axioms {
    #[cfg(verus_only)]
    use super::{AnyReading, Reading};
    use vstd::prelude::*;

    verus! {

    /// The erased reading is a `Reading<T>`.
    pub uninterp spec fn holds<T>(r: AnyReading) -> bool;

    /// The `Reading<T>` the erased reading holds, when it holds one.
    pub uninterp spec fn contents<T>(r: AnyReading) -> Reading<T>;

    /// Trusted: boxing a reading keeps it, as a reading of its type.
    pub assume_specification<T: Send + Sync + 'static>[ AnyReading::new::<T> ](reading: Reading<T>) -> (r: AnyReading)
        ensures holds::<T>(r) && contents::<T>(r) == reading;

    /// Trusted: `downcast_ref` recovers the reading exactly when it is of the asked type.
    pub assume_specification<T: 'static>[ AnyReading::downcast_ref::<T> ](r: &AnyReading) -> (got: Option<&Reading<T>>)
        ensures
            got is Some <==> holds::<T>(*r),
            got is Some ==> *got->0 == contents::<T>(*r);

    /// Trusted: `append` moves every entry of `other` into `m`, `other`'s value
    /// winning for a shared key, and leaves `other` empty.
    pub assume_specification<K, V, A: std::alloc::Allocator + Clone>[ std::collections::BTreeMap::<K, V, A>::append ](m: &mut std::collections::BTreeMap<K, V, A>, other: &mut std::collections::BTreeMap<K, V, A>) where K: Ord, A: Clone
        ensures
            vstd::laws_cmp::obeys_cmp::<K>() ==> {
                &&& final(m)@ == old(m)@.union_prefer_right(old(other)@)
                &&& final(other)@ == Map::<K, V>::empty()
            };

    } // verus!
}

} // verus!
