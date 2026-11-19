//! What one entity's share of a phase claims, in words a reader of entity.rs
//! can check. Every share is proved down to the call into each plugin.
#[cfg(verus_only)]
use super::Entity;
#[cfg(verus_only)]
use super::EntityDefinition;
#[cfg(verus_only)]
use super::Phase;
use super::compiled_stack::CompiledStack;
#[cfg(verus_only)]
use super::plugin_stack::proof as stack;
#[cfg(verus_only)]
use crate::KinematicState;
use crate::common::Terrain;
#[cfg(verus_only)]
use crate::common::TruthContacts;
use crate::math::EulerAngles;
#[cfg(verus_only)]
use crate::math::Vec3;
#[cfg(verus_only)]
use crate::plugin::estimator::PerfectContacts;
#[cfg(verus_only)]
use crate::plugin::{EntityInfo, StepTime};
use vstd::prelude::*;

verus! {

/// Declares `Terrain` as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExTerrain(Terrain);

/// Declares a nalgebra matrix, and so `Vec3`, as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
#[verifier::reject_recursive_types(T)]
#[verifier::reject_recursive_types(R)]
#[verifier::reject_recursive_types(C)]
#[verifier::reject_recursive_types(S)]
pub struct ExMatrix<T, R, C, S>(nalgebra::Matrix<T, R, C, S>);

/// Declares a nalgebra fixed dimension as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExConst<const R: usize>(nalgebra::Const<R>);

/// Declares a nalgebra fixed-size storage as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
#[verifier::reject_recursive_types(T)]
pub struct ExArrayStorage<T, const R: usize, const C: usize>(nalgebra::ArrayStorage<T, R, C>);

/// Declares the attitude angles and a group's compiled plugins as values with
/// no visible structure; a definition carries them to the constructor.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExEulerAngles(EulerAngles);
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub(crate) struct ExCompiledStack(CompiledStack);

/// `Entity::new` built the entity: it carries the ID, the group, the
/// definition's team and health, and its birth time, is active and not yet
/// started, and has no applied forces. Its truth and plugins are whatever
/// the constructor made of the start and the definition.
pub open(crate) spec fn built(entity: &Entity, id: i32, group_index: i32, definition: &EntityDefinition, born: StepTime) -> bool {
    &&& *entity == Entity {
        id, group_index, team_id: definition.team_id, truth: entity.truth, health: definition.initial_health, born, active: true,
        started: false, applied_forces_world_n: entity.applied_forces_world_n, plugins: entity.plugins,
    }
    &&& entity.applied_forces_world_n@ == Seq::<Vec3>::empty()
}

/// `start`: an entity not yet started is marked started and its stack is
/// started once, at its birth time, with its truth and the perfect detections;
/// one already started is untouched and the result is Ok.
pub open(crate) spec fn started(before: &Entity, after: &Entity, perfect_contacts: &PerfectContacts, ok: bool) -> bool {
    if before.started {
        *after == *before && ok
    } else {
        &&& *after == Entity { started: true, plugins: after.plugins, ..*before }
        &&& stack::axioms::started(&before.plugins, &after.plugins, info_of(before), before.born, &before.truth, perfect_contacts, ok)
    }
}

/// The entity is in the run: it takes part in every phase.
pub open(crate) spec fn is_active(entity: &Entity) -> bool {
    entity.active
}

/// The entity has health left; one with none is removed at the end of the tick.
pub open(crate) spec fn is_alive(entity: &Entity) -> bool {
    entity.health > 0
}

/// The entity's applied forces are gone and nothing else of it changed.
pub open(crate) spec fn forces_cleared(before: &Entity, after: &Entity) -> bool {
    &&& after.applied_forces_world_n@ == Seq::<Vec3>::empty()
    &&& *after == Entity { applied_forces_world_n: after.applied_forces_world_n, ..*before }
}

/// The entity is inactive and nothing else of it changed.
pub open(crate) spec fn deactivated(before: &Entity, after: &Entity) -> bool {
    *after == Entity { active: false, ..*before }
}

/// The entity as its plugins are told of it.
pub open(crate) spec fn info_of(entity: &Entity) -> EntityInfo {
    EntityInfo { id: entity.id, team_id: entity.team_id, group_index: entity.group_index }
}

/// `step_autonomy` ran the entity's autonomy slots at `time`, told of this
/// entity, and changed nothing of the entity but its plugins.
pub open(crate) spec fn autonomy_stepped(before: &Entity, after: &Entity, time: StepTime, ok: bool) -> bool {
    &&& stack::autonomy_stepped(&before.plugins, &after.plugins, info_of(before), time, ok)
    &&& *after == Entity { plugins: after.plugins, ..*before }
}

/// `step_controller` made belief follow truth first when `after_motion`,
/// then ran the entity's controller slots at `time`, told of this entity,
/// and changed nothing of the entity but its plugins.
pub open(crate) spec fn controllers_stepped(before: &Entity, after: &Entity, time: StepTime, after_motion: bool, ok: bool) -> bool {
    let start = if after_motion { stack::followed(&before.plugins, &before.truth) } else { before.plugins };
    &&& stack::controllers_stepped(&start, &after.plugins, info_of(before), time, ok)
    &&& *after == Entity { plugins: after.plugins, ..*before }
}

/// `step_motion` ran the entity's motion model at `time` on its truth and
/// applied forces, told of this entity, and changed nothing of the entity
/// but its truth and plugins.
pub open(crate) spec fn motion_stepped(before: &Entity, after: &Entity, time: StepTime, terrain: Option<&Terrain>, ok: bool) -> bool {
    &&& stack::motion_stepped(
        &before.plugins, &after.plugins, info_of(before), time, &before.truth, &after.truth,
        before.applied_forces_world_n@, terrain, ok,
    )
    &&& *after == Entity { plugins: after.plugins, truth: after.truth, ..*before }
}

/// `step_sensors` ran the entity's sensor slots at `time` on its truth and
/// every entity's true state, told of this entity, and changed nothing of
/// the entity but its plugins.
pub open(crate) spec fn sensors_stepped(before: &Entity, after: &Entity, time: StepTime, truth: TruthContacts, ok: bool) -> bool {
    &&& stack::sensed(&before.plugins, &after.plugins, info_of(before), time, &before.truth, truth, ok)
    &&& *after == Entity { plugins: after.plugins, ..*before }
}

/// `estimate` delivered the entity's readings and ran its estimators at
/// `time`, told of this entity, with its truth and the population as the
/// perfect measurements, and changed nothing of the entity but its plugins.
pub open(crate) spec fn estimated(before: &Entity, after: &Entity, time: StepTime, perfect_contacts: &PerfectContacts, ok: bool) -> bool {
    &&& stack::estimated(&before.plugins, &after.plugins, info_of(before), time, &before.truth, perfect_contacts, ok)
    &&& *after == Entity { plugins: after.plugins, ..*before }
}

/// `close` closed the entity's plugins at `time` and changed nothing else of
/// the entity.
pub open(crate) spec fn closed(before: &Entity, after: &Entity, time: StepTime, ok: bool) -> bool {
    &&& stack::axioms::closed(&before.plugins, &after.plugins, time, ok)
    &&& *after == Entity { plugins: after.plugins, ..*before }
}

/// `step_phase` ran the entity's share of `phase`, from `before` to `after`,
/// and returned Ok when `ok`: each share's claim.
pub open(crate) spec fn phase_stepped(before: &Entity, after: &Entity, phase: Phase, ok: bool) -> bool {
    match phase {
        Phase::Autonomy(time) => autonomy_stepped(before, after, time, ok),
        Phase::Controllers { time, after_motion } => controllers_stepped(before, after, time, after_motion, ok),
        Phase::Motion { time, terrain } => motion_stepped(before, after, time, terrain, ok),
        Phase::Sensors { time, truth } => sensors_stepped(before, after, time, truth, ok),
        Phase::Estimate { time, perfect_contacts } => estimated(before, after, time, perfect_contacts, ok),
    }
}

/// Trusted facts about what the entity's constructor computes.
pub(crate) mod axioms {
    use super::*;

    verus! {

    /// Trusted: `Entity::new` fills the named fields from its arguments as
    /// its body's final `Self { .. }` shows; the truth it samples and the
    /// plugins it builds are not modeled. It fails only when the plugins fail
    /// to initialize.
    pub assume_specification[ Entity::new ](
        id: i32, group_index: i32, definition: &EntityDefinition, position_world_m: Vec3, heading_world_deg: f64, born: StepTime,
        seed: u32, terrain: Option<&Terrain>,
    ) -> (r: anyhow::Result<Entity>)
        ensures r is Ok ==> super::built(&r->Ok_0, id, group_index, definition, born);

    } // verus!
}

} // verus!
