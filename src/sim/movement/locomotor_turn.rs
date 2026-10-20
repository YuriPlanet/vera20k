//! ILocomotion+4C dispatch on the active complete locomotor. Each original
//! class body delegates to the existing FacingClass state owner; this adapter
//! selects which controller and whether Set or SetCurrent is called.
//!
//! Original Steam gamemd: Drive4B0EF0, Ship6A05C0,
//! Hover516370 and Teleport7192C0 Set PrimaryFacing; Walk75AE00 and
//! Jumpjet54B6E0 SetCurrent PrimaryFacing; Fly4CFC10 SetCurrent
//! SecondaryFacing. Rocket's active interface table7F0B1C+4C names the
//! inherited55AC30 no-op (constructor661F1F). tools/team_recruit_oracle.py
//! --force-facing runs these class bodies and the Script42 caller.

use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;

/// Return false only without an active locomotor. Native callers would issue
/// COM E_POINTER there; callers can retain their pending action instead.
/// This slot neither changes navigation nor asks movement/readiness, and a
/// Rocket's inherited no-op still represents a successful slot dispatch.
pub(crate) fn locomotor_do_turn(entity: &mut GameEntity, desired: u16, frame: u32) -> bool {
    let Some(kind) = entity
        .locomotor
        .as_ref()
        .map(|locomotor| locomotor.active_kind())
    else {
        return false;
    };
    match kind {
        LocomotorKind::Drive
        | LocomotorKind::Ship
        | LocomotorKind::Hover
        | LocomotorKind::Teleport => {
            super::drive_do_turn(entity, desired, frame);
        }
        LocomotorKind::Walk | LocomotorKind::Jumpjet => {
            walk_do_turn(entity, desired, frame);
        }
        LocomotorKind::Fly => {
            entity.ensure_fly_secondary_facing();
            entity
                .barrel_facing
                .as_mut()
                .expect("Fly secondary controller initialized")
                .snap(desired, frame);
        }
        LocomotorKind::Rocket => {}
    }
    true
}

/// Walk75AE00 and Jumpjet54B6E0 share the same body SetCurrent delegate.
/// Walk Process calls this concrete owner on its own interface, even when an
/// external complete instance owns the Foot's active slot during piggyback.
pub(super) fn walk_do_turn(entity: &mut GameEntity, desired: u16, frame: u32) {
    entity.body_facing.snap(desired, frame);
}
