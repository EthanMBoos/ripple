//! What the proofs say about the messages: the vehicle status as Verus sees
//! it, and when a vehicle is free. The guard's proof (dispatcher/guard/proof.rs)
//! reads these.
use vstd::prelude::*;

use super::{Phase, Status, VehicleCard, VehicleClass};

verus! {

// The status types are plain Rust; these declarations show Verus their
// fields and variants.
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExVehicleClass(VehicleClass);
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExPhase(Phase);
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExVehicleCard(VehicleCard);
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExStatus(Status);

/// `Status::free`: idle and carrying no request.
pub open(crate) spec fn is_free(status: &Status) -> bool {
    status.phase is Idle && status.request_id is None
}

} // verus!
