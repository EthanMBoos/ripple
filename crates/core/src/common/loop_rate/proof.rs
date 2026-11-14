//! What `Rate::due` claims, in words a reader of loop_rate.rs can check.
use super::Rate;
use vstd::prelude::*;

verus! {

/// Lets Verus read `Rate`'s fields without a `verus!` block around the struct.
#[allow(dead_code)]
#[verifier::external_type_specification]
pub(crate) struct ExRate(Rate);

/// The facts a rate proof needs, so `due` names one group.
#[allow(dead_code)]
pub broadcast group rate_facts {
    axioms::div_is_ieee,
    axioms::elapsed_add_is_ieee,
    axioms::remaining_lt_is_ieee,
    axioms::reload_is_ieee,
    axioms::remaining_sub_is_ieee,
    axioms::period_eq_is_ieee,
}

/// A rate of zero means the plugin runs every call.
pub open(crate) spec fn no_rate(rate: &Rate) -> bool {
    rate.period_s.ieee_eq(0.0f64)
}

/// Half a step, the margin the countdown is compared with.
pub open(crate) spec fn half(dt_s: f64) -> f64 {
    dt_s / 2.0f64
}

/// One call of `due` ran the plugin exactly when its countdown was within half
/// a step of due; when it ran, it handed over the time since the last run and
/// restarted that clock, otherwise the clock kept counting; the period is
/// unchanged; the countdown was reloaded by one period when the plugin ran and
/// then counted down by the step, and stays at zero without a rate, so such a
/// plugin runs every call. All arithmetic is IEEE double.
pub open(crate) spec fn stepped(before: &Rate, after: &Rate, dt_s: f64, ran: Option<f64>) -> bool {
    &&& after.period_s == before.period_s
    &&& (ran is Some) == (before.remaining_s < half(dt_s))
    &&& ran is Some ==> ran == Some(before.elapsed_s + dt_s) && after.elapsed_s == 0.0f64
    &&& ran is None ==> after.elapsed_s == before.elapsed_s + dt_s
    &&& no_rate(before) ==> after.remaining_s == 0.0f64
    &&& !no_rate(before) && ran is Some ==> after.remaining_s == before.remaining_s + before.period_s - dt_s
    &&& !no_rate(before) && ran is None ==> after.remaining_s == before.remaining_s - dt_s
}

/// Trusted facts about the floating-point operators `due` uses.
mod axioms {
    #[cfg(verus_only)]
    use super::super::Rate;
    use vstd::prelude::*;
    #[cfg(verus_only)]
    use vstd::std_specs::cmp::{eq_ensures, lt_ensures};
    #[cfg(verus_only)]
    use vstd::std_specs::ops::{add_ensures, div_ensures, sub_ensures};

    verus! {

    /// Trusted: `x += y` on an `f64` is `x = x + y`; Verus's library states
    /// only the binary form.
    pub assume_specification[ <f64 as core::ops::AddAssign>::add_assign ](x: &mut f64, y: f64)
        ensures add_ensures::<f64>(*old(x), y, *final(x));

    /// Trusted: `x -= y` on an `f64` is `x = x - y`.
    pub assume_specification[ <f64 as core::ops::SubAssign>::sub_assign ](x: &mut f64, y: f64)
        ensures sub_ensures::<f64>(*old(x), y, *final(x));

    // Verus's library leaves each float operator's result uninterpreted,
    // because Rust does not promise the same bits on every platform. Rust
    // does promise IEEE double arithmetic for these operators, which is what
    // the lemmas below assume.
    //
    // DEVNOTE: the lemmas quantify over the `Rate` rather than over bare
    // floats. Verus 0.2026.11.02 encodes an `f64` as the integer holding its
    // bits and guards every float lemma with "this integer fits 64 bits" for
    // each quantified variable. It asserts that fact for parameters and call
    // results but emits no field axiom for float fields (`datatypes_invs` in
    // `vir/context.rs` lists `TypX::Float` as needing none, while
    // `typ_invariant` in `vir/sst_to_air.rs` produces one), so a lemma over
    // floats never fires when an operand is `self.elapsed_s`, and the
    // postcondition fails with no diagnostic. Seen with `verus --log-all` on
    // a one-struct probe: `(uInv 64 b!)` for the parameter, nothing for the
    // field; adding one `u64` field to the struct made the same proof pass.
    // A struct without invariants is always well typed, so a lemma over the
    // struct fires. When Verus emits the field fact, these six collapse to
    // five lemmas over plain floats and nothing in loop_rate.rs changes.

    /// Trusted: dividing two doubles is IEEE division.
    #[verifier::external_body]
    pub broadcast proof fn div_is_ieee(a: f64, b: f64, o: f64)
        requires #[trigger] div_ensures(a, b, o)
        ensures o == a / b
    {}

    /// Trusted: adding to the elapsed time is IEEE addition.
    #[verifier::external_body]
    pub(crate) broadcast proof fn elapsed_add_is_ieee(rate: Rate, b: f64, o: f64)
        requires #[trigger] add_ensures(rate.elapsed_s, b, o)
        ensures o == rate.elapsed_s + b
    {}

    /// Trusted: comparing the countdown is IEEE less-than.
    #[verifier::external_body]
    pub(crate) broadcast proof fn remaining_lt_is_ieee(rate: Rate, b: f64, o: bool)
        requires #[trigger] lt_ensures(rate.remaining_s, b, o)
        ensures o == (rate.remaining_s < b)
    {}

    /// Trusted: reloading the countdown by one period is IEEE addition.
    #[verifier::external_body]
    pub(crate) broadcast proof fn reload_is_ieee(rate: Rate, o: f64)
        requires #[trigger] add_ensures(rate.remaining_s, rate.period_s, o)
        ensures o == rate.remaining_s + rate.period_s
    {}

    /// Trusted: counting the countdown down is IEEE subtraction.
    #[verifier::external_body]
    pub(crate) broadcast proof fn remaining_sub_is_ieee(rate: Rate, b: f64, o: f64)
        requires #[trigger] sub_ensures(rate.remaining_s, b, o)
        ensures o == rate.remaining_s - b
    {}

    /// Trusted: testing the period against zero is IEEE equality, which
    /// holds for both signs of zero.
    #[verifier::external_body]
    pub(crate) broadcast proof fn period_eq_is_ieee(rate: Rate, b: f64, o: bool)
        requires #[trigger] eq_ensures(rate.period_s, b, o)
        ensures o == rate.period_s.ieee_eq(b)
    {}

    } // verus!
}

} // verus!
