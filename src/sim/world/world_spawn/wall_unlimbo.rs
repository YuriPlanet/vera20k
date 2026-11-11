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
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::{ground_pose, infantry_entry::InfantryEntryArgs};
use crate::sim::occupancy::CellObjectMember;
use crate::sim::overlay_mark_host::SimulationMarkHost;
use crate::sim::world::lifecycle::RevealFailure;
use crate::sim::world::{FrameEffects, UninitContext};

impl Simulation {
    /// None delegates nonwalls to the existing class/generic Reveal path,
    /// after [`Self::clear_site_before_building_unlimbo`] has removed what
    /// they stand on. The native wall arm never reaches Building's live
    /// enrollment suffix. `game_active` is native `0x00A8E9A0`, which the
    /// cell's building lookup (`0x0047C520`) answers null without.
    /// RESIDUAL: callers pass false during scenario load, as
    /// `building_upgrade_host_at` does, but `Main__PrepareSession` sets the
    /// byte at `0x0052D9D7`, before Start_Scenario, so native runs the
    /// LaserFencePost arm for a map-authored post too. Trigger: a map that
    /// authors a `LaserFencePost=` on its house's `LaserFence=` building
    /// (no retail type is either). Effect: both buildings stay on the cell.
    pub(super) fn unlimbo_constructed_wall(
        &mut self,
        id: u64,
        requested: RevealPosition,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
        game_active: bool,
    ) -> Option<RevealOutcome> {
        let entity = self.substrate.entities.get(id)?;
        if entity.category() != EntityCategory::Structure {
            return None;
        }
        if !self.object_type(entity.type_ref(), rules)?.wall {
            self.clear_site_before_building_unlimbo(
                id,
                requested,
                rules,
                registry,
                effects,
                game_active,
            );
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
        if self.substrate.entities.get(id).is_none_or(|entity| {
            self.object_type(entity.type_ref(), rules)
                .and_then(|ty| crate::sim::production::linked_wall_overlay_id(ty, registry))
                .is_none()
        }) {
            return rejected;
        }
        self.sell_owned_walls_before_building_unlimbo(id, (rx, ry), rules, registry, effects);
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
        let outcome = match mark_overlay(
            &mut SimulationMarkHost::new(
                self,
                rules,
                registry,
                crate::sim::overlay_mark_host::OverlayRecalc::Immediate,
                effects,
            ),
            &request,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                // Refuse this construction on native-invalid Image state,
                // keeping prior cell writes but retiring the transient object.
                log::error!("Overlay reveal failed: {error}");
                self.load_objects
                    .finish_common(handle, &mut self.substrate.pending_delete)
                    .expect("failed Mark retires its transient Overlay");
                return Some(RevealOutcome::Failed(RevealFailure::MarkFailed));
            }
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
            rules,
            effects,
        );
        let context = UninitContext::new(rules, Some(registry)).with_effects(effects);
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
            let config = self.sight_reveal_config(rules);
            let heights = self.sight_height_grid(&config);
            let clients = self.fog_viewers();
            let viewers = self.reveal_viewers.direct(
                self.house_alliances.as_map(),
                owner,
                &config,
                &self.interner,
                &clients,
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

    /// `BuildingClass::Unlimbo @ 0x00440580` for a building that is not a
    /// wall, between its own Can_Enter_Cell (`0x004405A5`) and
    /// `TechnoClass::Unlimbo` (`0x00440B0F`): the arms that remove what an
    /// admitted building stands on. Both need that Can_Enter_Cell to answer 0
    /// (`0x004405B6`, `0x00440A6D`), and neither refuses the Unlimbo.
    ///
    /// - `0x004405BC..0x00440770`:
    ///   [`Self::sell_owned_walls_before_building_unlimbo`].
    /// - `0x00440A6D..0x00440B01`: a `LaserFencePost=` type (`+0x16BE`)
    ///   UnInits (vt+0xF8) the first building of its coordinate's cell
    ///   (`0x0047C520`) when that is a `LaserFence=` type (`+0x16BF`) of the
    ///   same house. Its `+0x618 = 0` write (`0x00440A83`), like the
    ///   LaserFence facing frames at `0x00440A32..0x00440A63`, has no VERA
    ///   reader: fence sections are not ported (see the residual at
    ///   `0x00440E2F` in `lifecycle.rs`).
    ///
    /// No RNG draw and no timer write on either arm. Residual: an upgrade
    /// type is consumed before this (`unlimbo_constructed_upgrade`), where
    /// native walks the sell arms first; only an upgrade that is also the
    /// WallTower, a ToOverlay or a Gate type could tell the two apart.
    fn clear_site_before_building_unlimbo(
        &mut self,
        id: u64,
        requested: RevealPosition,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
        game_active: bool,
    ) {
        let Some((owner, ty)) =
            self.substrate.entities.get(id).and_then(|entity| {
                Some((entity.owner(), self.object_type(entity.type_ref(), rules)?))
            })
        else {
            return;
        };
        let gates = &rules.general.building_types;
        let wall_tower = gates
            .wall_tower
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&ty.id));
        if !(wall_tower || ty.to_overlay.is_some() || ty.gate || ty.laser_fence_post) {
            return;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let cells = NativeCellQuery::canonical(terrain);
        let coord = ground_pose::position_world_coord(&requested);
        let cell = cells.lookup_world(coord.x, coord.y);
        if !matches!(
            super::super::object_entry::classify_object(
                self,
                rules,
                registry,
                CellObjectMember::Entity(id),
                cell,
                InfantryEntryArgs::REPAIR,
            ),
            Ok(0)
        ) {
            return;
        }
        let (rx, ry) = cells.coord(cell);
        let (Ok(rx), Ok(ry)) = (u16::try_from(rx), u16::try_from(ry)) else {
            return;
        };
        if let Some(registry) = registry {
            self.sell_owned_walls_before_building_unlimbo(id, (rx, ry), rules, registry, effects);
        }
        if !(ty.laser_fence_post && game_active) {
            return;
        }
        let own_fence = self
            .substrate
            .occupancy
            .first_building_on_layer(rx, ry, MovementLayer::Ground)
            .filter(|&other| {
                self.substrate.entities.get(other).is_some_and(|other| {
                    other.owner() == owner
                        && self
                            .object_type(other.type_ref(), rules)
                            .is_some_and(|other| other.laser_fence)
                })
            });
        if let Some(fence) = own_fence {
            self.uninit_with_context(
                fence,
                UninitContext::new(rules, registry).with_effects(effects),
            );
        }
    }

    /// `BuildingClass::Unlimbo @ 0x00440580`, `0x004405BC..0x00440770`, once
    /// its Can_Enter_Cell has admitted building `id` at cell `(rx, ry)`: the
    /// owner's walls under it go through `HouseClass::Sell_Building_At_Cell
    /// @ 0x004FCE80` with the sell cue suppressed (second argument 1). The
    /// first arm that matches the type decides, and the others are not tried:
    ///
    /// - `[General] WallTower=` names the type (`Rules+0x87C`, `0x004405C7`):
    ///   its cell, when the overlay is 0 or 2 (GASAND, GAWALL).
    /// - The type has a `ToOverlay=` (`+0xE54`, `0x00440606`): its cell, when
    ///   the overlay is that one (`0x0044064A`).
    /// - The type is `Gate=` (`+0x16B7`, read at `0x004609EA..0x00460A13`,
    ///   default no): each foundation cell (vt+0x90(1)) whose overlay is 0, 2
    ///   or 0x1A (NAWALL), `0x00440674..0x00440720`.
    ///
    /// A cell counts only when its wall owner (`Cell+0x50`) is the building's
    /// house. The sale's answer is ignored. That sale refunds nothing, sells
    /// only a human's wall and draws no RNG; a wall it refuses stays under
    /// the building.
    ///
    /// Residual: the Gate arm also calls `0x00453240` on each foundation cell
    /// whose first building is a `LaserFence=` type of the same house
    /// (`0x00440725..0x0044075F`). That walks the fence run along the fence's
    /// facing to its post, reconnects the post (`0x00453060`) or else UnInits
    /// the fence, and belongs to the fence-section mechanism VERA does not
    /// have. Until then a Gate placed on its owner's LaserFence building
    /// shares the cell with it. No retail type is `LaserFence=`.
    fn sell_owned_walls_before_building_unlimbo(
        &mut self,
        id: u64,
        (rx, ry): (u16, u16),
        rules: &RuleSet,
        registry: &OverlayTypeRegistry,
        effects: FrameEffects<'_>,
    ) {
        use crate::sim::build_site::{OVERLAY_GASAND, OVERLAY_GAWALL, OVERLAY_NAWALL};
        const TOWER_WALLS: &[Option<u8>] = &[Some(OVERLAY_GASAND), Some(OVERLAY_GAWALL)];
        const GATE_WALLS: &[Option<u8>] = &[
            Some(OVERLAY_GASAND),
            Some(OVERLAY_GAWALL),
            Some(OVERLAY_NAWALL),
        ];
        let Some((owner, ty)) =
            self.substrate.entities.get(id).and_then(|entity| {
                Some((entity.owner(), self.object_type(entity.type_ref(), rules)?))
            })
        else {
            return;
        };
        let wall_tower = rules
            .general
            .building_types
            .wall_tower
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&ty.id));
        let to_overlay = [ty
            .to_overlay
            .as_deref()
            .and_then(|name| registry.id_for_name(name))];
        let (offsets, sold) = if wall_tower {
            (vec![(0, 0)], TOWER_WALLS)
        } else if to_overlay[0].is_some() {
            (vec![(0, 0)], to_overlay.as_slice())
        } else if ty.gate {
            (ty.foundation.cell_offsets(), GATE_WALLS)
        } else {
            return;
        };
        let house = self.resolve(owner).to_owned();
        for (dx, dy) in offsets {
            let (Ok(x), Ok(y)) = (
                u16::try_from(i32::from(rx) + i32::from(dx)),
                u16::try_from(i32::from(ry) + i32::from(dy)),
            ) else {
                continue;
            };
            let owned = self.overlay_grid.as_ref().is_some_and(|grid| {
                x < grid.width()
                    && y < grid.height()
                    && grid.cell(x, y).wall_owner == Some(owner)
                    && sold.contains(&grid.cell(x, y).overlay_id)
            });
            if owned {
                //44065C/440720 ignore House4FCE80's answer; only the cue is suppressed.
                self.sell_wall_at_cell(&house, x as i16, y as i16, true, rules, registry, effects);
            }
        }
    }
}

#[cfg(test)]
#[path = "wall_unlimbo_tests.rs"]
mod tests;
