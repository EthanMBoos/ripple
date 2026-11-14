//! Facts the crate's proofs share: how a map or set keyed by `String` answers a
//! `&str` lookup, the view of such a map by the text of its keys, and what the
//! `anyhow` error path does. Each trusted item's doc comment begins `Trusted:`
//! and says why it holds; the table at the end of docs/FORMAL_VERIFICATION.md
//! lists every one. A module's own claims live in its `proof.rs`.
use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::std_specs::btree::{
    borrowed_key_ordering_matches, borrowed_key_removed, contains_borrowed_key,
    maps_borrowed_key_to_value, set_contains_borrowed_key,
};

verus! {

/// Declares `anyhow::Error` to Verus as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExAnyhowError(anyhow::Error);

/// The library facts and trusted items a proof over a `BTreeMap<String, _>`
/// looked up by `&str` needs.
#[allow(dead_code)]
pub broadcast group string_key_facts {
    vstd::std_specs::btree::group_btree_axioms,
    vstd::laws_cmp::group_laws_cmp,
    string_obeys_cmp,
    str_lookup_matches_string_order,
    str_key_contained,
    str_key_maps_to_value,
    str_key_removed,
    string_text_is_identity,
    str_in_set,
}

/// The facts a proof that reads a string-keyed map by text needs.
#[allow(dead_code)]
pub broadcast group text_facts {
    key_is_text,
    text_is_key,
    by_text_insert,
    by_text_union,
    by_text_empty,
}

/// The map's entries by the text of their keys.
pub open(crate) spec fn by_text<V>(m: Map<String, V>) -> Map<Seq<char>, V> {
    Map::new(
        m.dom().map(|s: String| s@),
        |name: Seq<char>| m[choose|s: String| #[trigger] m.contains_key(s) && s@ == name],
    )
}

/// `pairs` holds exactly the map's entries.
pub open(crate) spec fn lists<V>(m: Map<String, V>, pairs: Seq<(&String, &V)>) -> bool {
    &&& forall|i: int| 0 <= i < pairs.len()
        ==> m.contains_key(*#[trigger] pairs[i].0) && m[*pairs[i].0] == *pairs[i].1
    &&& forall|k: String| #[trigger] m.contains_key(k) ==> pairs.contains((&k, &m[k]))
}

/// A key of the map is an entry by text, with the key's value.
pub broadcast proof fn key_is_text<V>(m: Map<String, V>, s: String)
    requires #[trigger] m.contains_key(s),
    ensures by_text(m).contains_key(s@) && by_text(m)[s@] == m[s],
{
    broadcast use string_text_is_identity;
    assert(m.dom().map(|t: String| t@).contains(s@));
}

/// An entry by text is some key of the map with that text.
pub broadcast proof fn text_is_key<V>(m: Map<String, V>, name: Seq<char>)
    requires #[trigger] by_text(m).contains_key(name),
    ensures exists|s: String| #[trigger] m.contains_key(s) && s@ == name,
{
}

/// Inserting a key inserts its text.
pub broadcast proof fn by_text_insert<V>(m: Map<String, V>, k: String, v: V)
    ensures #[trigger] by_text(m.insert(k, v)) == by_text(m).insert(k@, v),
{
    broadcast use string_text_is_identity, key_is_text, text_is_key, vstd::set_lib::group_set_lib_default;
    let after = m.insert(k, v);
    assert forall|name: Seq<char>| #![auto] by_text(after).contains_key(name)
        <==> by_text(m).insert(k@, v).contains_key(name) by {
        if by_text(after).contains_key(name) {
            let t = choose|t: String| #[trigger] after.contains_key(t) && t@ == name;
            if t != k {
                assert(m.contains_key(t));
            }
        }
        if by_text(m).contains_key(name) {
            let t = choose|t: String| #[trigger] m.contains_key(t) && t@ == name;
            assert(after.contains_key(t));
        }
        if name == k@ {
            assert(after.contains_key(k));
        }
    }
    assert forall|name: Seq<char>| #![auto] by_text(after).contains_key(name)
        implies by_text(after)[name] == by_text(m).insert(k@, v)[name] by {
        let t = choose|t: String| #[trigger] after.contains_key(t) && t@ == name;
        if t != k {
            assert(m.contains_key(t));
        }
    }
    assert(by_text(after) =~= by_text(m).insert(k@, v));
}

/// A union of two maps is the union of their entries by text.
pub broadcast proof fn by_text_union<V>(a: Map<String, V>, b: Map<String, V>)
    ensures #[trigger] by_text(a.union_prefer_right(b)) == by_text(a).union_prefer_right(by_text(b)),
{
    broadcast use string_text_is_identity, key_is_text, text_is_key, vstd::set_lib::group_set_lib_default;
    let u = a.union_prefer_right(b);
    assert forall|name: Seq<char>| #![auto] by_text(u).contains_key(name)
        <==> by_text(a).union_prefer_right(by_text(b)).contains_key(name) by {
        if by_text(u).contains_key(name) {
            let t = choose|t: String| #[trigger] u.contains_key(t) && t@ == name;
            assert(a.contains_key(t) || b.contains_key(t));
        }
        if by_text(a).contains_key(name) {
            let t = choose|t: String| #[trigger] a.contains_key(t) && t@ == name;
            assert(u.contains_key(t));
        }
        if by_text(b).contains_key(name) {
            let t = choose|t: String| #[trigger] b.contains_key(t) && t@ == name;
            assert(u.contains_key(t));
        }
    }
    assert forall|name: Seq<char>| #![auto] by_text(u).contains_key(name)
        implies by_text(u)[name] == by_text(a).union_prefer_right(by_text(b))[name] by {
        let t = choose|t: String| #[trigger] u.contains_key(t) && t@ == name;
        if b.contains_key(t) {
            assert(by_text(b)[name] == b[t]);
        } else {
            assert(by_text(a)[name] == a[t]);
            if by_text(b).contains_key(name) {
                let t2 = choose|t2: String| #[trigger] b.contains_key(t2) && t2@ == name;
                assert(t2 == t);
            }
        }
    }
    assert(by_text(u) =~= by_text(a).union_prefer_right(by_text(b)));
}

/// An empty map has no entries by text.
pub broadcast proof fn by_text_empty<V>()
    ensures #[trigger] by_text(Map::<String, V>::empty()) == Map::<Seq<char>, V>::empty(),
{
    assert(by_text(Map::<String, V>::empty()) =~= Map::<Seq<char>, V>::empty());
}

/// Trusted: std's `String` ordering is a total order consistent with its
/// equality, so a map keyed by strings behaves as a map.
#[verifier::external_body]
pub broadcast proof fn string_obeys_cmp()
    ensures #[trigger] vstd::laws_cmp::obeys_cmp::<String>(),
{
}

/// Trusted: a `String` compares with a `&str` as it compares with the
/// `String` holding that text, which is what std's `Borrow<str>` promises.
#[verifier::external_body]
pub broadcast proof fn str_lookup_matches_string_order()
    ensures #[trigger] borrowed_key_ordering_matches::<String, str>(),
{
}

/// Trusted: a `&str` is a key of a string-keyed map exactly when some key has
/// its text. Verus leaves this predicate open for borrowed key types.
#[verifier::external_body]
pub broadcast proof fn str_key_contained<V>(m: Map<String, V>, k: &str)
    ensures
        #[trigger] contains_borrowed_key::<String, V, str>(m, k)
            <==> exists|s: String| #[trigger] m.contains_key(s) && s@ == k@,
{
}

/// Trusted: a `&str` maps to `v` exactly when the key with its text maps to `v`.
#[verifier::external_body]
pub broadcast proof fn str_key_maps_to_value<V>(m: Map<String, V>, k: &str, v: V)
    ensures
        #[trigger] maps_borrowed_key_to_value::<String, V, str>(m, k, v)
            <==> exists|s: String| #[trigger] m.contains_key(s) && s@ == k@ && m[s] == v,
{
}

/// Trusted: removing a `&str` key removes the key with its text.
#[verifier::external_body]
pub broadcast proof fn str_key_removed<V>(m: Map<String, V>, rest: Map<String, V>, k: &str)
    ensures
        #[trigger] borrowed_key_removed::<String, V, str>(m, rest, k)
            <==> exists|s: String| #[trigger] m.contains_key(s) && s@ == k@ && rest == m.remove(s),
{
}

/// Trusted: a `&str` is in a string set exactly when some member has its text.
#[verifier::external_body]
pub broadcast proof fn str_in_set(s: Set<String>, k: &str)
    ensures
        #[trigger] set_contains_borrowed_key::<String, str>(s, k)
            <==> exists|m: String| #[trigger] s.contains(m) && m@ == k@,
{
}

/// Trusted: two `String`s with the same text are the same string.
#[verifier::external_body]
pub broadcast proof fn string_text_is_identity(a: String, b: String)
    ensures #[trigger] a@ == #[trigger] b@ ==> a == b,
{
}

/// Trusted: `with_context` on an `Option` keeps a present value as `Ok` and
/// turns an absent one into an error; the message is not modeled.
pub assume_specification<T, C: core::fmt::Display + Send + Sync + 'static, F: FnOnce() -> C>[
    <Option<T> as anyhow::Context<T, core::convert::Infallible>>::with_context::<C, F>
](o: Option<T>, f: F) -> (r: anyhow::Result<T>)
    ensures
        o is Some ==> r is Ok && r->Ok_0 == o->0,
        o is None ==> r is Err,
;

/// Trusted: `bail!` and `anyhow!` build an error value from a message and do
/// nothing else.
pub assume_specification[ anyhow::__private::format_err ](args: core::fmt::Arguments<'_>) -> anyhow::Error;

/// Trusted: the same, for the wrapper `anyhow!` puts around the value.
pub assume_specification[ anyhow::__private::must_use ](error: anyhow::Error) -> anyhow::Error;

} // verus!
