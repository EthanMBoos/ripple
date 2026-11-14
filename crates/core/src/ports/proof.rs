//! What the port functions claim, in words a reader of ports.rs can check.
use super::PluginIo;
#[cfg(verus_only)]
use super::Signals;
use vstd::prelude::*;

verus! {

/// Lets Verus read `PluginIo`'s maps without a `verus!` block around the struct.
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExPluginIo(PluginIo);

/// The facts a port proof needs, so ports.rs names one group.
#[allow(dead_code)]
pub broadcast group port_facts {
    crate::proof::string_key_facts,
    crate::proof::text_facts,
    vstd::seq_lib::group_seq_lib_default,
    str_key_mutation_is_insert,
    one_missing,
    all_connected,
    all_copied,
}

/// The ports a signal map holds, by name, with their values.
pub open(crate) spec fn signals(m: &Signals) -> Map<Seq<char>, f64> {
    crate::proof::by_text(m@)
}

/// `name` is one of the plugin's input ports.
pub open(crate) spec fn has_port(io: &PluginIo, name: &str) -> bool {
    signals(&io.inputs).contains_key(name@)
}

/// `read` returned the named input's value, and an error exactly when there is
/// no such input.
pub open(crate) spec fn read_port(io: &PluginIo, name: &str, value: &Result<f64, anyhow::Error>) -> bool {
    &&& (value is Ok) == has_port(io, name)
    &&& value is Ok ==> value->Ok_0 == signals(&io.inputs)[name@]
}

/// `write` stored a finite value in the named output and changed no other
/// port; a nonfinite value or an unknown name is an error that changes nothing.
pub open(crate) spec fn wrote_port(before: &PluginIo, after: &PluginIo, name: &str, value: f64, outcome: &Result<(), anyhow::Error>) -> bool {
    &&& (outcome is Ok) == (axioms::is_finite(value) && signals(&before.outputs).contains_key(name@))
    &&& outcome is Ok ==> signals(&after.outputs) == signals(&before.outputs).insert(name@, value)
    &&& outcome is Err ==> signals(&after.outputs) == signals(&before.outputs)
    &&& signals(&after.inputs) == signals(&before.inputs)
}

/// Every input took the upstream value of the same name and the outputs are
/// untouched; an input with no upstream value is an error that changes nothing.
pub open(crate) spec fn received(before: &PluginIo, after: &PluginIo, upstream: &Signals, outcome: &Result<(), anyhow::Error>) -> bool {
    &&& (outcome is Ok) == signals(&before.inputs).dom().subset_of(signals(upstream).dom())
    &&& signals(&after.inputs).dom() == signals(&before.inputs).dom()
    &&& outcome is Ok ==> forall|name: Seq<char>| signals(&after.inputs).contains_key(name)
        ==> #[trigger] signals(&after.inputs)[name] == signals(upstream)[name]
    &&& outcome is Err ==> signals(&after.inputs) == signals(&before.inputs)
    &&& signals(&after.outputs) == signals(&before.outputs)
}

/// The check loop's invariant: `names` are the input names in order, and the
/// first `count` of them each have an upstream value.
pub open(crate) spec fn connected_so_far(inputs: &Signals, upstream: &Signals, names: Seq<&String>, count: int) -> bool {
    &&& names.unref().to_set() == inputs@.dom()
    &&& forall|i: int| 0 <= i < count ==> upstream@.contains_key(*#[trigger] names[i])
}

/// The copy loop's invariant: `pairs` are upstream's entries, the first `count`
/// have been copied to the inputs that declare them, and no input was added or
/// removed.
pub open(crate) spec fn copied_so_far(before: &PluginIo, now: &PluginIo, upstream: &Signals, pairs: Seq<(&String, &f64)>, count: int) -> bool {
    &&& crate::proof::lists(upstream@, pairs)
    &&& forall|k: String| #[trigger] now.inputs@.contains_key(k) <==> #[trigger] before.inputs@.contains_key(k)
    &&& forall|i: int| 0 <= i < count && now.inputs@.contains_key(*#[trigger] pairs[i].0) ==> now.inputs@[*pairs[i].0] == *pairs[i].1
    &&& now.outputs@ == before.outputs@
}

/// Changing the value behind a `&str` key changes that one port's value.
pub broadcast proof fn str_key_mutation_is_insert(before: Map<String, f64>, after: Map<String, f64>, name: &str, was: f64, now: f64)
    requires #[trigger] vstd::std_specs::btree::borrowed_key_mutated::<String, f64, str>(before, after, name, was, now),
    ensures crate::proof::by_text(after) == crate::proof::by_text(before).insert(name@, now),
{
    broadcast use crate::proof::string_key_facts, crate::proof::text_facts;
    let s = choose|s: String| #[trigger] before.contains_key(s) && s@ == name@ && before[s] == was;
    let rest = choose|rest: Map<String, f64>|
        vstd::std_specs::btree::borrowed_key_removed::<String, f64, str>(before, rest, name)
        && vstd::std_specs::btree::borrowed_key_removed::<String, f64, str>(after, rest, name);
    let s1 = choose|s1: String| #[trigger] before.contains_key(s1) && s1@ == name@ && rest == before.remove(s1);
    let s2 = choose|s2: String| #[trigger] after.contains_key(s2) && s2@ == name@ && rest == after.remove(s2);
    let s3 = choose|s3: String| #[trigger] after.contains_key(s3) && s3@ == name@ && after[s3] == now;
    assert(s1 == s && s2 == s && s3 == s);
    assert forall|k: String| #![auto] k != s implies after.contains_key(k) == before.contains_key(k)
        && (after.contains_key(k) ==> after[k] == before[k]) by {
        assert(after.remove(s).contains_key(k) == after.contains_key(k));
        assert(before.remove(s).contains_key(k) == before.contains_key(k));
    }
    assert(after =~= before.insert(s, now));
    assert forall|name2: Seq<char>| #![auto] crate::proof::by_text(after).contains_key(name2)
        implies crate::proof::by_text(after)[name2] == crate::proof::by_text(before).insert(name@, now)[name2] by {
        let t = choose|t: String| #[trigger] after.contains_key(t) && t@ == name2;
        assert(crate::proof::by_text(after)[name2] == after[t]);
        if t != s {
            assert(crate::proof::by_text(before)[name2] == before[t]);
        }
    }
    assert(crate::proof::by_text(after) =~= crate::proof::by_text(before).insert(name@, now));
}

/// When an input name has no upstream value, the inputs are not all connected.
pub broadcast proof fn one_missing(inputs: &Signals, upstream: &Signals, names: Seq<&String>, i: int)
    requires
        #[trigger] connected_so_far(inputs, upstream, names, i),
        0 <= i < names.len(),
        !upstream@.contains_key(*names[i]),
    ensures !signals(inputs).dom().subset_of(signals(upstream).dom()),
{
    broadcast use crate::proof::string_key_facts, crate::proof::text_facts, vstd::seq_lib::group_seq_lib_default;
    let s = *names[i];
    assert(names.unref()[i] == s);
    assert(names.unref().contains(s));
    assert(inputs@.contains_key(s));
    assert(signals(inputs).contains_key(s@));
    assert(!signals(upstream).contains_key(s@));
}

/// When every input name has an upstream value, the inputs are all connected.
pub broadcast proof fn all_connected(inputs: &Signals, upstream: &Signals, names: Seq<&String>, count: int)
    requires
        #[trigger] connected_so_far(inputs, upstream, names, count),
        count == names.len(),
    ensures signals(inputs).dom().subset_of(signals(upstream).dom()),
{
    broadcast use crate::proof::string_key_facts, crate::proof::text_facts, vstd::seq_lib::group_seq_lib_default;
    assert forall|name: Seq<char>| signals(inputs).contains_key(name)
        implies signals(upstream).contains_key(name) by {
        let s = choose|s: String| #[trigger] inputs@.contains_key(s) && s@ == name;
        assert(names.unref().contains(s));
        let i = choose|i: int| 0 <= i < names.len() && names.unref()[i] == s;
        assert(*names[i] == s);
        assert(upstream@.contains_key(s));
    }
}

/// When the copy loop has walked every upstream entry and every input has an
/// upstream value, every input holds its upstream value.
pub broadcast proof fn all_copied(before: &PluginIo, now: &PluginIo, upstream: &Signals, pairs: Seq<(&String, &f64)>, count: int)
    requires
        #[trigger] copied_so_far(before, now, upstream, pairs, count),
        count == pairs.len(),
        signals(&before.inputs).dom().subset_of(signals(upstream).dom()),
    ensures
        signals(&now.inputs).dom() == signals(&before.inputs).dom(),
        forall|name: Seq<char>| signals(&now.inputs).contains_key(name)
            ==> #[trigger] signals(&now.inputs)[name] == signals(upstream)[name],
{
    broadcast use crate::proof::string_key_facts, crate::proof::text_facts, vstd::seq_lib::group_seq_lib_default;
    let (was, is) = (&before.inputs, &now.inputs);
    assert(is@.dom() =~= was@.dom());
    assert forall|name: Seq<char>| #![auto] signals(is).contains_key(name)
        implies signals(is)[name] == signals(upstream)[name] by {
        let s = choose|s: String| #[trigger] is@.contains_key(s) && s@ == name;
        assert(signals(was).contains_key(name));
        assert(signals(upstream).contains_key(name));
        let u = choose|u: String| #[trigger] upstream@.contains_key(u) && u@ == name;
        assert(u == s);
        assert(pairs.contains((&s, &upstream@[s])));
        let i = choose|i: int| 0 <= i < pairs.len() && pairs[i] == (&s, &upstream@[s]);
        assert(*pairs[i].0 == s && *pairs[i].1 == upstream@[s]);
        assert(is@[s] == upstream@[s]);
    }
}

/// Trusted facts about floats the port functions use.
mod axioms {
    use vstd::prelude::*;

    verus! {

    /// What `f64::is_finite` decides; the proofs say nothing about which floats
    /// are finite.
    pub uninterp spec fn is_finite(x: f64) -> bool;

    /// Trusted: `is_finite` answers `is_finite`.
    pub assume_specification[ f64::is_finite ](x: f64) -> (b: bool)
        ensures b == is_finite(x);

    } // verus!
}

} // verus!
