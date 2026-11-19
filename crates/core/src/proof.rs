//! Facts the crate's proofs share: how a map or set keyed by `String` answers a
//! `&str` lookup, the view of a map by the text of its keys (strings, or pairs
//! of them), what `f64::is_finite` decides, and what the `anyhow` error path
//! does. Each trusted item's doc comment begins `Trusted:` and says why it
//! holds, so `git grep 'Trusted:'` lists every one.
//! A module's own claims live in its `proof.rs`.
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

/// The facts a proof that reads a text-keyed map by text needs.
#[allow(dead_code)]
pub broadcast group text_facts {
    string_texts_identify,
    key_is_text,
    text_is_key,
    by_text_insert,
    by_text_remove,
    by_text_union,
    by_text_empty,
}

/// Keys with the same text are the same key, so a map read by text loses no
/// entry. Holds for `String` (below) and for pairs of them (messages/proof.rs).
pub open(crate) spec fn text_is_identity<K: View>() -> bool {
    forall|a: K, b: K| #[trigger] a@ == #[trigger] b@ ==> a == b
}

/// The map's entries by the text of their keys.
pub open(crate) spec fn by_text<K: View, V>(m: Map<K, V>) -> Map<K::V, V> {
    Map::new(
        m.dom().map(|k: K| k@),
        |text: K::V| m[choose|k: K| #[trigger] m.contains_key(k) && k@ == text],
    )
}

/// `pairs` holds exactly the map's entries.
pub open(crate) spec fn lists<V>(m: Map<String, V>, pairs: Seq<(&String, &V)>) -> bool {
    &&& forall|i: int| 0 <= i < pairs.len()
        ==> m.contains_key(*#[trigger] pairs[i].0) && m[*pairs[i].0] == *pairs[i].1
    &&& forall|k: String| #[trigger] m.contains_key(k) ==> pairs.contains((&k, &m[k]))
}

/// Two `String`s with the same text are the same string, as a fact about the type.
pub broadcast proof fn string_texts_identify()
    ensures #[trigger] text_is_identity::<String>(),
{
    broadcast use string_text_is_identity;
    assert forall|a: String, b: String| #[trigger] a@ == #[trigger] b@ implies a == b by {}
}

/// A key of the map is an entry by text, with the key's value.
pub broadcast proof fn key_is_text<K: View, V>(m: Map<K, V>, k: K)
    requires text_is_identity::<K>(), #[trigger] m.contains_key(k),
    ensures by_text(m).contains_key(k@) && by_text(m)[k@] == m[k],
{
    assert(m.dom().map(|t: K| t@).contains(k@));
}

/// An entry by text is some key of the map with that text.
pub broadcast proof fn text_is_key<K: View, V>(m: Map<K, V>, text: K::V)
    requires #[trigger] by_text(m).contains_key(text),
    ensures exists|k: K| #[trigger] m.contains_key(k) && k@ == text,
{
}

/// Inserting a key inserts its text.
pub broadcast proof fn by_text_insert<K: View, V>(m: Map<K, V>, k: K, v: V)
    requires text_is_identity::<K>(),
    ensures #[trigger] by_text(m.insert(k, v)) == by_text(m).insert(k@, v),
{
    broadcast use key_is_text, text_is_key, vstd::set_lib::group_set_lib_default;
    let after = m.insert(k, v);
    assert forall|text: K::V| #![auto] by_text(after).contains_key(text)
        <==> by_text(m).insert(k@, v).contains_key(text) by {
        if by_text(after).contains_key(text) {
            let t = choose|t: K| #[trigger] after.contains_key(t) && t@ == text;
            if t != k {
                assert(m.contains_key(t));
            }
        }
        if by_text(m).contains_key(text) {
            let t = choose|t: K| #[trigger] m.contains_key(t) && t@ == text;
            assert(after.contains_key(t));
        }
        if text == k@ {
            assert(after.contains_key(k));
        }
    }
    assert forall|text: K::V| #![auto] by_text(after).contains_key(text)
        implies by_text(after)[text] == by_text(m).insert(k@, v)[text] by {
        let t = choose|t: K| #[trigger] after.contains_key(t) && t@ == text;
        if t != k {
            assert(m.contains_key(t));
        }
    }
    assert(by_text(after) =~= by_text(m).insert(k@, v));
}

/// Removing a key removes its text.
pub broadcast proof fn by_text_remove<K: View, V>(m: Map<K, V>, k: K)
    requires text_is_identity::<K>(),
    ensures #[trigger] by_text(m.remove(k)) == by_text(m).remove(k@),
{
    broadcast use key_is_text, text_is_key, vstd::set_lib::group_set_lib_default;
    let after = m.remove(k);
    assert forall|text: K::V| #![auto] by_text(after).contains_key(text)
        <==> by_text(m).remove(k@).contains_key(text) by {
        if by_text(after).contains_key(text) {
            let t = choose|t: K| #[trigger] after.contains_key(t) && t@ == text;
            assert(m.contains_key(t));
            assert(t != k);
        }
        if by_text(m).contains_key(text) && text != k@ {
            let t = choose|t: K| #[trigger] m.contains_key(t) && t@ == text;
            assert(after.contains_key(t));
        }
    }
    assert forall|text: K::V| #![auto] by_text(after).contains_key(text)
        implies by_text(after)[text] == by_text(m).remove(k@)[text] by {
        let t = choose|t: K| #[trigger] after.contains_key(t) && t@ == text;
        assert(m.contains_key(t));
    }
    assert(by_text(after) =~= by_text(m).remove(k@));
}

/// A union of two maps is the union of their entries by text.
pub broadcast proof fn by_text_union<K: View, V>(a: Map<K, V>, b: Map<K, V>)
    requires text_is_identity::<K>(),
    ensures #[trigger] by_text(a.union_prefer_right(b)) == by_text(a).union_prefer_right(by_text(b)),
{
    broadcast use key_is_text, text_is_key, vstd::set_lib::group_set_lib_default;
    let u = a.union_prefer_right(b);
    assert forall|text: K::V| #![auto] by_text(u).contains_key(text)
        <==> by_text(a).union_prefer_right(by_text(b)).contains_key(text) by {
        if by_text(u).contains_key(text) {
            let t = choose|t: K| #[trigger] u.contains_key(t) && t@ == text;
            assert(a.contains_key(t) || b.contains_key(t));
        }
        if by_text(a).contains_key(text) {
            let t = choose|t: K| #[trigger] a.contains_key(t) && t@ == text;
            assert(u.contains_key(t));
        }
        if by_text(b).contains_key(text) {
            let t = choose|t: K| #[trigger] b.contains_key(t) && t@ == text;
            assert(u.contains_key(t));
        }
    }
    assert forall|text: K::V| #![auto] by_text(u).contains_key(text)
        implies by_text(u)[text] == by_text(a).union_prefer_right(by_text(b))[text] by {
        let t = choose|t: K| #[trigger] u.contains_key(t) && t@ == text;
        if b.contains_key(t) {
            assert(by_text(b)[text] == b[t]);
        } else {
            assert(by_text(a)[text] == a[t]);
            if by_text(b).contains_key(text) {
                let t2 = choose|t2: K| #[trigger] b.contains_key(t2) && t2@ == text;
                assert(t2 == t);
            }
        }
    }
    assert(by_text(u) =~= by_text(a).union_prefer_right(by_text(b)));
}

/// An empty map has no entries by text.
pub broadcast proof fn by_text_empty<K: View, V>()
    ensures #[trigger] by_text(Map::<K, V>::empty()) == Map::<K::V, V>::empty(),
{
    assert(by_text(Map::<K, V>::empty()) =~= Map::<K::V, V>::empty());
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

/// What `f64::is_finite` decides; the proofs say nothing about which floats
/// are finite.
pub uninterp spec fn is_finite(x: f64) -> bool;

/// Trusted: `is_finite` answers `is_finite`.
pub assume_specification[ f64::is_finite ](x: f64) -> (b: bool)
    ensures b == is_finite(x);

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
