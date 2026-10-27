//! The active locomotor's three motion queries over existing retained state:
//! ILocomotion+10 `Is_Moving` ([`is_moving`]), +80 `Is_Moving_Now`
//! ([`is_moving_now`]) and +A8 `Is_Really_Moving_Now`
//! ([`is_really_moving_now`]). Native callers dispatch one slot or another,
//! and `Is_Moving` and `Is_Moving_Now` answer differently in every family
//! except Teleport. Native retained-state readers are shared here; Teleport
//! reads the active complete instance's request byte.
use super::track_process::TrackFamily;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;

/// ILocomotion+60, dispatched by Ares TechnoUpdate1004B49B. Hover516C70
/// uses its existing height-aware owner; the other active families use the
/// retained base power owner, rather than the object's Deactivated byte.
#[cfg(feature = "ares")]
pub(crate) fn is_powered(
    entity: &GameEntity,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> Option<bool> {
    let locomotor = entity.locomotor.as_ref()?;
    Some(if locomotor.active_kind() == LocomotorKind::Hover {
        super::hover::hover_is_powered(entity, cells)
    } else {
        locomotor.is_powered()
    })
}

/// ILocomotion+14 Destination, distinct from Head_To(+18) and the Foot's
/// NavCom. Original retail gamemd.exe: Drive4AFC90, Ship69F3A0,
/// Walk75ABA0, Hover514CB0, Fly4CCAE0, Jumpjet54AE60, Rocket661FB0 and
/// Teleport7180A0. No state, map query, RNG or timers are changed.
pub(crate) fn destination(entity: &GameEntity) -> Option<crate::sim::components::DriveCoord> {
    use crate::sim::components::DriveCoord;
    let null = DriveCoord { x: 0, y: 0, z: 0 };
    let loco = entity.locomotor.as_ref()?;
    Some(match loco.active_kind() {
        LocomotorKind::Drive => loco.track_destination(TrackFamily::Drive).unwrap_or(null),
        LocomotorKind::Ship => loco.track_destination(TrackFamily::Ship).unwrap_or(null),
        LocomotorKind::Hover => loco.hover_runtime()?.destination().unwrap_or(null),
        LocomotorKind::Rocket => loco.rocket_runtime()?.destination(),
        LocomotorKind::Teleport if !is_moving(entity)? => {
            //7180CC returns the linked object's Location when stopped.
            super::foot_coordinate::current_coordinate(entity)
        }
        LocomotorKind::Walk | LocomotorKind::Fly | LocomotorKind::Jumpjet
            if !is_moving(entity)? =>
        {
            null
        }
        LocomotorKind::Walk => loco.walk_destination().unwrap_or(null),
        LocomotorKind::Fly => loco.fly_runtime()?.destination(),
        LocomotorKind::Jumpjet => loco.jumpjet_runtime()?.destination(),
        LocomotorKind::Teleport => loco.teleport_runtime()?.armed_destination().unwrap_or(null),
    })
}

/// Drive4AFB80, Ship69F290, Walk75AB30, Fly4CCA90, Jumpjet54AE50,
/// Hover514C30 and Rocket661F50. Teleport718080 delegates to its existing
/// state owner.
/// Evidence: locomotor_moving, air_locomotor_moving, cmin_dock and
/// jumpjet_infantry_actions --default-motion native corpora; Teleport's
/// ordinary move/stop controls (infantry_teleport_destination) establish the
/// Cell setter/Stop boundaries; full Process/Chronosphere lifetime stays bounded.
pub fn is_moving(entity: &GameEntity) -> Option<bool> {
    let locomotor = entity.locomotor.as_ref()?;
    match locomotor.active_kind() {
        LocomotorKind::Drive => Some(super::track_head::motion_state(entity, TrackFamily::Drive).0),
        LocomotorKind::Ship => Some(super::track_head::motion_state(entity, TrackFamily::Ship).0),
        LocomotorKind::Walk => locomotor.walk_is_moving(),
        LocomotorKind::Fly => locomotor
            .fly_runtime()
            .map(|state| state.moving() || entity.flight_attitude.blocks_landing()),
        LocomotorKind::Jumpjet => locomotor.jumpjet_runtime().map(|state| state.moving()),
        LocomotorKind::Hover => locomotor
            .hover_runtime()
            .map(super::hover::HoverRuntime::is_moving),
        LocomotorKind::Teleport => locomotor
            .teleport_runtime()
            .map(super::teleport_movement::TeleportRuntime::is_moving),
        LocomotorKind::Rocket => locomotor
            .rocket_runtime()
            .map(super::rocket_movement::RocketRuntime::is_moving),
    }
}

/// `Is_Moving_Now` (ILocomotion+0x80) of the active locomotor: Fly
/// `0x004CCAC0`, Rocket `0x00661F90`, and the readiness families of
/// [`super::ready_producer::ready_state_for`] (Drive `0x004AFC20`, Ship,
/// Walk, Hover `0x00514C80`, Teleport and Jumpjet `0x0054D0D0`). No locomotor
/// answers false. `rules` gives Drive and Ship the owner's speed getter.
pub fn is_moving_now(entity: &GameEntity, rules: super::SpeedRules<'_>, binary_frame: u32) -> bool {
    let Some(locomotor) = entity.locomotor.as_ref() else {
        return false;
    };
    match locomotor.active_kind() {
        LocomotorKind::Fly => locomotor
            .fly_runtime()
            .is_some_and(super::fly_height::FlyRuntime::is_moving_now),
        LocomotorKind::Rocket => locomotor
            .rocket_runtime()
            .is_some_and(super::rocket_movement::RocketRuntime::is_moving_now),
        _ => super::ready_producer::ready_state_for(entity, rules, binary_frame)
            .is_some_and(super::locomotor_ready::LocomotorReadyState::is_moving_now),
    }
}

/// `Is_Really_Moving_Now` (ILocomotion+0xA8) of the active locomotor: Walk
/// answers its class byte +0x36 (`0x0075CB20`); every other locomotor's slot
/// is the base `0x004B4C50`, which asks [`is_moving_now`]. Its one native
/// caller is the Infantry locomotion action tail (`0x00521161`).
pub(crate) fn is_really_moving_now(
    entity: &GameEntity,
    rules: super::SpeedRules<'_>,
    binary_frame: u32,
) -> bool {
    match entity
        .locomotor
        .as_ref()
        .and_then(|locomotor| locomotor.walk_animation_moving())
    {
        Some(moving) => moving,
        None => is_moving_now(entity, rules, binary_frame),
    }
}

#[cfg(test)]
#[path = "motion_query_tests.rs"]
pub(crate) mod tests;
