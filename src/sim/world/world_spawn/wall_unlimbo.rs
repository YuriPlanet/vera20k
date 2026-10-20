//! Building440580's ordinary ToOverlay wall arm, before generic live Reveal.
//! Original gamemd; whole588750/constructor/Unlimbo controls:
//! tools/spatial_oracle/building_death_anims.py --wall-autofill.
//! HousePlace4FB1F7..4FB22E and588750 supply centered XY/Z0. The transient
//! Overlay owner retains those requested coordinates; the Building stays at
//! its constructor Location, outside occupancy, Logic and sustained sight.

use super::{RevealOutcome, RevealPosition, Simulation};
use crate::map::entities::EntityCategory;
use crate::map::overlay_mark::{
    MarkCrateImages, OverlayMarkOutcome, OverlayMarkRequest, mark_overlay,
};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::{overlay_types::OverlayTypeRegistry, ruleset::RuleSet};
use crate::sim::movement::{ground_pose, infantry_entry::InfantryEntryArgs};
use crate::sim::occupancy::CellObjectMember;
use crate::sim::overlay_mark_host::SimulationMarkHost;
use crate::sim::world::lifecycle::RevealFailure;
use crate::sim::world::{FrameEffects, UninitContext};

impl Simulation {
    /// None delegates nonwalls to the existing class/generic Reveal path.
    /// The native wall arm never reaches Building's live enrollment suffix.
    pub(super) fn unlimbo_constructed_wall(
        &mut self,
        id: u64,
        requested: RevealPosition,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Option<RevealOutcome> {
        let rules = rules?;
        let entity = self.substrate.entities.get(id)?;
        if entity.category() != EntityCategory::Structure
            || !self.object_type(entity.type_ref(), rules)?.wall
        {
            return None;
        }
        let rejected = Some(RevealOutcome::Failed(RevealFailure::RejectedEarly));
        let Some(registry) = registry else {
            return rejected;
        };
        let coord = ground_pose::position_world_coord(&requested);
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return rejected;
        };
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup_world(coord.x, coord.y);
        //4405A5 calls449440 even during ScenarioInit. Use the existing class
        //receiver, including its foundation/query order and scope admission.
        if !matches!(
            super::super::object_entry::classify_object(
                self,
                rules,
                Some(registry),
                CellObjectMember::Entity(id),
                cell,
                InfantryEntryArgs::REPAIR,
            ),
            Ok(0)
        ) {
            return rejected;
        }
        let (rx, ry) = cells.coord(cell);
        let (Ok(rx), Ok(ry)) = (u16::try_from(rx), u16::try_from(ry)) else {
            return rejected;
        };
        let Some((overlay_id, owner)) = self.substrate.entities.get(id).and_then(|entity| {
            Some((
                crate::sim::production::linked_wall_overlay_id(
                    self.object_type(entity.type_ref(), rules)?,
                    registry,
                )?,
                entity.owner(),
            ))
        }) else {
            return rejected;
        };
        let replaces_owned = self.overlay_grid.as_ref().is_some_and(|grid| {
            rx < grid.width()
                && ry < grid.height()
                && grid.cell(rx, ry).overlay_id == Some(overlay_id)
                && grid.cell(rx, ry).wall_owner == Some(owner)
        });
        if replaces_owned {
            //44065C ignores House4FCE80's answer; only the cue is suppressed.
            let house = self.resolve(owner).to_owned();
            self.sell_wall_at_cell(&house, rx as i16, ry as i16, true, rules, registry, effects);
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return rejected;
        };
        let cells = NativeCellQuery::canonical(terrain);
        //4407A0 repeats449440 AFTER sale, separately from Overlay's Track test.
        if !matches!(
            super::super::object_entry::classify_object(
                self,
                rules,
                Some(registry),
                CellObjectMember::Entity(id),
                cells.lookup_world(coord.x, coord.y),
                InfantryEntryArgs::REPAIR,
            ),
            Ok(0)
        ) {
            return rejected;
        }
        let Some(overlay_id) = self.substrate.entities.get(id).and_then(|entity| {
            crate::sim::production::linked_wall_overlay_id(
                self.object_type(entity.type_ref(), rules)?,
                registry,
            )
        }) else {
            return rejected;
        };
        if !self
            .overlay_grid
            .as_ref()
            .is_some_and(|grid| rx < grid.width() && ry < grid.height())
        {
            return rejected;
        }
        //5FE570 creates with EmptyCell/frame-1, so construction publishes no
        //cell.5FD270 then performs its own requested-cell Terrain admission.
        let stable_id = self.substrate.allocate_stable_id();
        let handle = self
            .load_objects
            .construct_overlay(stable_id, (rx, ry), || {
                crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(
                    &mut self.native_unique_ids,
                )
            })
            .expect("wall Overlay constructor joins its existing registries");
        if self.terrain_objects.cell_index().contains_key(&(rx, ry))
            || (!self.object_placement_scope_active()
                && !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (i32::from(rx), i32::from(ry)),
                    self.playfield_bounds,
                    self.resolved_terrain.as_ref(),
                ))
        {
            self.load_objects
                .finish_unrevealed_survivor(handle)
                .expect("early Overlay refusal retains its constructor");
            return rejected;
        }
        let dirty = self
            .load_objects
            .begin_mark(handle, Some([coord.x, coord.y, coord.z]))
            .expect("wall Overlay enters Mark once");
        self.dirty_cells.mark_tactical(dirty);
        //5FD270 reaches the one OverlayClass::Mark port; Location is the
        //requested coordinate, and its wall arm owns slope, Track and writes.
        let flags = registry
            .flags(overlay_id)
            .expect("linked wall Overlay has a registered type");
        let request = OverlayMarkRequest {
            overlay_id,
            flags,
            cell: (rx as i16, ry as i16),
            location: [coord.x, coord.y, coord.z],
            registry,
            tiberium_types: Some(&rules.tiberium_types),
            crate_images: MarkCrateImages::resolve(&rules.crate_rules, registry),
        };
        let lighting = self
            .session
            .lighting()
            .profile(crate::sim::scenario_session::ScenarioLightingProfile::Normal)
            .into();
        let outcome = match mark_overlay(
            &mut SimulationMarkHost::new(self, Some(rules), registry, lighting),
            &request,
        ) {
            Ok(outcome) => outcome,
            Err(never) => match never {},
        };
        match outcome {
            OverlayMarkOutcome::SlopeRefused => {
                self.load_objects
                    .finish_slope_survivor(handle)
                    .expect("slope refusal retains the live Overlay");
                return Some(RevealOutcome::Failed(RevealFailure::MarkFailed));
            }
            OverlayMarkOutcome::WallRefused => {
                self.load_objects
                    .finish_wall_reject(handle, &mut self.substrate.pending_delete)
                    .expect("Track-refused Overlay joins the shared deferred queue");
                return Some(RevealOutcome::Failed(RevealFailure::MarkFailed));
            }
            OverlayMarkOutcome::Marked => self
                .load_objects
                .finish_common(handle, &mut self.substrate.pending_delete)
                .expect("successful wall Overlay queues before its Building"),
        }
        //4407FC reads the Building's current owner after all Overlay callbacks.
        let owner = self
            .substrate
            .entities
            .get(id)
            .expect("pending wall constructor retained")
            .owner();
        self.overlay_grid
            .as_mut()
            .unwrap()
            .set_wall_owner(rx, ry, owner);
        crate::sim::radio::transmit_to_contact(
            self,
            id,
            crate::sim::radio::RadioMessage::Break,
            Some(rules),
            effects,
        );
        let context = UninitContext::new(Some(rules), Some(registry)).with_effects(effects);
        //44082D/440856 reread type/owner for the final0/final1 pair. No live
        //source, cached radius or567DA0 border belongs to this early arm.
        for final_pass in [false, true] {
            let entity = self
                .substrate
                .entities
                .get(id)
                .expect("wall retained through sight");
            let owner = entity.owner();
            let radius = self
                .object_type(entity.type_ref(), rules)
                .unwrap()
                .sight
                .clamp(0, i32::from(u16::MAX)) as u16;
            let config = self.sight_reveal_config(Some(rules));
            let heights = self.sight_height_grid(&config);
            let viewers = self.reveal_viewers.direct(
                &self.fog,
                self.house_alliances.as_map(),
                owner,
                &config,
                &self.interner,
            );
            let _ = self.reveal_area(
                &viewers,
                (rx, ry),
                coord.z,
                radius,
                Some(&heights),
                final_pass,
                &config,
                context,
                crate::sim::vision::MapRevealProjection::Transient,
                self.session.current_house(),
            );
        }
        self.uninit_with_context(id, context);
        Some(RevealOutcome::Consumed)
    }
}
