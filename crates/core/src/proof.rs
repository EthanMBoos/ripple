//! Trusted facts the crate's proofs share: how a map keyed by `String` answers
//! a `&str` lookup, and what the `anyhow` error path does. Each item's doc
//! comment begins `Trusted:` and says why it holds; the table at the end of
//! docs/FORMAL_VERIFICATION.md lists every one. A module's own claims live in
//! its `proof.rs`.
use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::std_specs::btree::{
    borrowed_key_ordering_matches, borrowed_key_removed, contains_borrowed_key,
    maps_borrowed_key_to_value,
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
