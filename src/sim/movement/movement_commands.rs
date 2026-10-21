//! Destination acceptance shared by the class setters: admission and the
//! locomotor Move_To tails.
//!
//! No order searches a route here. The class setters (`Simulation::
//! set_unit_destination`, `Simulation::assign_infantry_destination`) accept a
//! destination; the first ordinary Process owns the route search.
//!
//! ## Dependency rules
//! - Internal to sim/movement — called via re-exports in mod.rs.

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;

/// Whether the Unit setter takes a destination for `entity`: not while it
/// carries the `TerminalLifetime` dying mark.
///
/// VERA-only: `UnitClass::Set_Destination` (`0x00741970`) has no such gate.
/// gamemd needs none, because `ObjectClass::UnInit` (`0x005F65F0`) expires
/// every pointer to the object and limboes it at once, so no caller can
/// reach it to order it. VERA keeps a dying object in the store until the
/// drain, where callers that walk the store can still reach it.
pub(crate) fn can_accept_destination(entity: &GameEntity) -> bool {
    !entity.death.is_dying()
}

/// Read the movement goal for diagnostics without retaining another copy,
/// while the active locomotor answers Is_Moving (`motion_query::is_moving`).
/// Drive/Ship display Foot's requested Cell, including a Move_To refusal
/// that leaves a different locomotor destination (track_destination native
/// warp controls). Object requests use the captured locomotor coordinate;
/// after Stop the paid head remains. Other families display Foot's NavCom
/// when it is a Cell (for a Jumpjet, the cell its `Move_To` placed it in),
/// and nothing for an object. This projection is for presentation; it makes
/// no simulation decision.
pub fn movement_goal_cell(entity: &GameEntity) -> Option<(u16, u16)> {
    if super::motion_query::is_moving(entity) != Some(true) {
        return None;
    }
    if entity.locomotor.as_ref().is_some_and(|loco| {
        matches!(
            loco.active_kind(),
            LocomotorKind::Drive | LocomotorKind::Ship
        )
    }) {
        if let Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) =
            entity.navigation.nav_com()
        {
            return Some((rx, ry));
        }
        return super::track_path::track_destination(entity)
            .or_else(|| super::track_head::committed_track_head(entity))
            .map(|coord| ((coord.x / 256) as u16, (coord.y / 256) as u16));
    }
    match entity.navigation.nav_com() {
        Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) => Some((rx, ry)),
        _ => None,
    }
}

/// The caller's native frame and configured Foot blocked timer duration.
/// Passing this context keeps accepted destination timers anchored even when
/// Scatter and the ordinary object turn both process one actor in a frame.
#[derive(Debug, Clone, Copy)]
pub struct DestinationTiming {
    pub binary_frame: u32,
    pub blockage_path_delay_ticks: i32,
}

impl DestinationTiming {
    pub const fn new(binary_frame: u32, blockage_path_delay_ticks: i32) -> Self {
        Self {
            binary_frame,
            blockage_path_delay_ticks,
        }
    }

    pub fn from_rules(binary_frame: u32, rules: &crate::rules::ruleset::RuleSet) -> Self {
        Self::new(binary_frame, rules.general.blockage_path_delay_ticks)
    }

    /// Set_Destination_Internal 0x004D94B0 tail 0x4D96C2..0x4D9707: +6B7 = 0,
    /// +668 = (frame, Rules+1768), +640 = (frame, 0) for every accepted
    /// setter. The setter never writes +64C; the no-head Process FindPath
    /// success continuation 0x75B2E2 owns that reset.
    pub(crate) fn accept(self, entity: &mut crate::sim::game_entity::GameEntity) {
        entity
            .navigation
            .path_runtime_mut()
            .accept_destination(self.binary_frame, self.blockage_path_delay_ticks);
    }
}

/// Accepted destination -> Walk75ACB0, without searching or advancing
/// Process. The class caller owns preceding admission and its path-head
/// write; the captured coordinate comes from the shared target +4C owner.
/// No PathGrid is needed until the next ordinary Process (or the immediate
/// Process specifically required by NULL-source Scatter51D478).
pub(crate) fn prepare_walk_destination(
    entities: &mut EntityStore,
    entity_id: u64,
    destination: (
        crate::sim::components::NavTargetRef,
        crate::sim::components::DriveCoord,
    ),
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    timing: crate::sim::movement::DestinationTiming,
) -> bool {
    let Some(entity) = entities.get_mut(entity_id) else {
        return false;
    };
    if !entity
        .locomotor
        .as_ref()
        .is_some_and(|l| l.active_kind() == LocomotorKind::Walk)
    {
        return false;
    }
    let (reference, coord) = destination;
    super::navcom::set_destination_internal_coord(
        entity,
        reference,
        coord,
        resolved_terrain,
        timing.binary_frame,
    );
    timing.accept(entity);
    true
}

/// Enter, current (Get_Mission) or queued (`+0xB4`), without a radio contact
/// (`0x0065AE30`): the class setters run their Enter radio arm instead of
/// clearing the path head (Infantry `0x0051AC25..0x0051AC4B`, Unit
/// `0x00741C4F..0x00741C78`).
pub(crate) fn enter_without_contact(entity: &GameEntity) -> bool {
    (entity.mission.effective().raw() == 7 || entity.mission.queued().raw() == 7)
        && entity.radio_contacts.is_empty()
}

/// The setters' one-word PathHead clear, which Enter (current or queued)
/// without a radio contact skips: Infantry 51AC25..51AD17, Unit
/// 741C4F..741C78 -> 741E88 (both the NULL and the non-NULL destination).
/// NavQueue, suffix and reference survive.
pub(crate) fn clear_destination_path_head(entity: &mut GameEntity) {
    if !enter_without_contact(entity) {
        entity.navigation.path_replay_mut().clear_live_head();
    }
}
