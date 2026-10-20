//! A Techno's playfield membership (`TechnoClass+0x3D5`): the record and the
//! writers that compute it from the map's playfield.

use super::{Simulation, UninitContext};
use crate::map::resolved_terrain::ResolvedTerrainGrid;

/// `TechnoClass+0x3D5`, the stored fact that the object is inside the map's
/// playfield. Consumers read it rather than a fresh bounds query, which would
/// erase the native movement hysteresis. The constructor clears it
/// (`0x006F2F5B`).
///
/// Writers:
/// - [`Self::enter`]: `BuildingClass::ExitObject_Main`'s prelude
///   (`0x00443C81`), whatever the exit's outcome; Fly's playfield latch
///   (`0x004CD5F0`); FootAI's BalloonHover admission (`0x004DA63B..0x004DA670`);
///   ordinary cell movement (`0x006F511A..0x006F5139`, the Teleport warp's
///   PerCell(2) included). These writers only promote.
/// - [`Self::store`]: the exact mode-one result of `TechnoClass::Unlimbo`
///   (`0x006F6CFE`) and of `MapClass::Set_Clipped_LocalSize @ 0x00567230`
///   (`0x0056730E`), which may demote as well as promote.
/// - [`Self::leave`]: the Chronosphere warp's arrival outside the playfield
///   (`0x00719A99`, `movement::teleport_chrono`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct PlayfieldMembership(bool);

impl PlayfieldMembership {
    /// The stored membership.
    pub(crate) const fn get(self) -> bool {
        self.0
    }

    /// Promote to a member through the admitted native writer.
    pub(crate) fn enter(&mut self) {
        self.0 = true;
    }

    /// Store an exact recomputation: Unlimbo and `Set_Clipped_LocalSize`.
    pub(crate) fn store(&mut self, member: bool) {
        self.0 = member;
    }

    /// The Chronosphere arrival's clear outside the playfield.
    pub(crate) fn leave(&mut self) {
        self.0 = false;
    }
}

impl Simulation {
    /// FootAI4DA63B..670, after Techno/radiation and before Process: an
    /// unadmitted BalloonHover Foot enters when its physical Cell is inside.
    /// GetCell+1BC is Object5F6960 on all three Foot class tables; reuse its
    /// two Map565730 lookups, then Map578540's retained-Cell query. PerCell
    /// and Unlimbo instead retain their packed-coordinate578460 semantics.
    /// Original gamemd.exe body/callers; no RNG, timer or detach operation.
    pub(super) fn promote_balloon_playfield_membership_before_process(
        &mut self,
        stable_id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if !entity.category().is_foot()
            || entity.in_playfield()
            || self
                .object_type(entity.type_ref(), rules)
                .is_none_or(|object| !object.balloon_hover)
        {
            return;
        }
        let (Some(bounds), Some(terrain)) = (self.playfield_bounds, self.resolved_terrain.as_ref())
        else {
            // A partial Rust world has no Map authority for this writer.
            return;
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let cell = crate::sim::movement::ground_pose::query_object_cell(
            &cells,
            crate::sim::movement::ground_pose::position_world_coord(&entity.position),
        );
        if crate::sim::cell_rect::retained_cell_is_in_playfield(cell, Some(bounds), &cells)
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.in_playfield.enter();
        }
    }

    /// Exact mode-one query for the stored TechnoClass+0x3D5 writer family.
    /// Absence means there is no live MapClass authority, not rectangular or
    /// permissive fallback authority.
    pub(super) fn entity_playfield_membership_mode_one(
        &self,
        stable_id: u64,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> Option<bool> {
        let bounds = self.playfield_bounds?;
        let entity = self.substrate.entities.get(stable_id)?;
        Some(crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (
                i32::from(entity.position.rx()),
                i32::from(entity.position.ry()),
            ),
            Some(bounds),
            terrain,
        ))
    }

    /// Unlimbo's exact establishment writer (`TechnoClass::Unlimbo @
    /// 0x006F6CFE`).
    pub(super) fn establish_entity_playfield_membership_on_unlimbo(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) {
        let Some(member) = self.entity_playfield_membership_mode_one(
            stable_id,
            context.terrain().or(self.resolved_terrain.as_ref()),
        ) else {
            return;
        };
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.in_playfield.store(member);
        }
    }

    /// Ordinary per-cell movement writer (`0x006F511A..0x006F5139`): only
    /// promote 0 -> 1. A unit that walks back outside retains membership until
    /// an exact writer (teleport or Set_Clipped_LocalSize) clears it.
    pub(crate) fn promote_entity_playfield_membership_after_move(&mut self, stable_id: u64) {
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_none_or(|entity| entity.in_playfield())
        {
            return;
        }
        if self.entity_playfield_membership_mode_one(stable_id, self.resolved_terrain.as_ref())
            == Some(true)
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.in_playfield.enter();
        }
    }
}

#[cfg(test)]
#[path = "playfield_membership_tests.rs"]
mod tests;
