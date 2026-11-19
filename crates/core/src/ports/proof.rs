//! What the port functions claim, in words a reader of ports.rs can check.
#[cfg(verus_only)]
use super::{Frame, Port, Unit};
#[cfg(verus_only)]
use super::{PluginIo, Ports, Signals};
#[cfg(verus_only)]
use std::collections::BTreeMap;
use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::std_specs::btree::{borrowed_key_mutated, borrowed_key_removed};

verus! {

/// The facts a port proof needs, so ports.rs names one group.
#[allow(dead_code)]
pub broadcast group port_facts {
    crate::proof::string_key_facts,
    crate::proof::text_facts,
    overwritten_by_text,
}

/// Overwriting the value a name holds through `get_mut` is, by text, the same
/// as inserting it, so `write` can skip the allocation an insert needs.
pub broadcast proof fn overwritten_by_text(before: Map<String, f64>, after: Map<String, f64>, name: &str, was: f64, value: f64)
    ensures
        #[trigger] borrowed_key_mutated::<String, f64, str>(before, after, name, was, value)
            ==> crate::proof::by_text(after) == crate::proof::by_text(before).insert(name@, value),
{
    broadcast use crate::proof::string_key_facts, crate::proof::text_facts;
    if borrowed_key_mutated::<String, f64, str>(before, after, name, was, value) {
        let rest = choose|rest: Map<String, f64>|
            borrowed_key_removed::<String, f64, str>(before, rest, name)
                && borrowed_key_removed::<String, f64, str>(after, rest, name);
        let key = choose|s: String| #[trigger] before.contains_key(s) && s@ == name@ && rest == before.remove(s);
        let same = choose|s: String| #[trigger] after.contains_key(s) && s@ == name@ && rest == after.remove(s);
        assert(key == same);
        assert(after[key] == value) by {
            let held = choose|s: String| #[trigger] after.contains_key(s) && s@ == name@ && after[s] == value;
            assert(held == key);
        }
        assert(before =~= rest.insert(key, before[key]));
        assert(after =~= rest.insert(key, value));
        assert(crate::proof::by_text(before).insert(name@, value) =~= crate::proof::by_text(rest).insert(name@, value));
    }
}

/// The port values a signal map holds, by name.
pub open(crate) spec fn signals(m: &Signals) -> Map<Seq<char>, f64> {
    crate::proof::by_text(m@)
}

/// The names a declaration map declares.
pub open(crate) spec fn declared(ports: &BTreeMap<String, Port>) -> Set<Seq<char>> {
    crate::proof::by_text(ports@).dom()
}

/// `read` returned the named port's value when the plugin declares it as an
/// input and it has a value, and an error otherwise.
pub open(crate) spec fn read_port(io: &PluginIo, name: &str, value: &Result<f64, anyhow::Error>) -> bool {
    &&& (value is Ok) == (declared(&io.ports.inputs).contains(name@) && signals(io.signals).contains_key(name@))
    &&& value is Ok ==> value->Ok_0 == signals(io.signals)[name@]
}

/// `write` stored a finite value under a name the plugin declares as an output
/// and changed no other port; a nonfinite value or an undeclared name is an
/// error that changes nothing. The declarations never change.
pub open(crate) spec fn wrote_port(before: &PluginIo, after: &PluginIo, name: &str, value: f64, outcome: &Result<(), anyhow::Error>) -> bool {
    &&& (outcome is Ok) == (crate::proof::is_finite(value) && declared(&before.ports.outputs).contains(name@))
    &&& outcome is Ok ==> signals(after.signals) == signals(before.signals).insert(name@, value)
    &&& outcome is Err ==> signals(after.signals) == signals(before.signals)
    &&& after.ports == before.ports
}

} // verus!
