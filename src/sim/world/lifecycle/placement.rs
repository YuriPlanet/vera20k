//! Cell marks of placement: Mark/UnMark (`mark_entity_put`,
//! `unmark_entity_remove`), building base-reservation rects and their
//! perimeter repair, BasePlan fill/invalidate, and drop-as-bomb.

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::rule_policy::AiBasePolicy;
use crate::sim::cell_rect::{CellRect, resolve_reservation_real_cell, scan_cell_rect};
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::occupancy::{
    BUILDING_OCCUPATION_BIT, CellListInsertion, cell_list_layer_for_entity, entity_occupancy_cells,
};
use crate::sim::world::{ObjectSubstrate, Simulation};

#[cfg(any(test, feature = "test-support"))]
use super::LifecycleTestEvent;
use super::{
    RevealPosition, UninitContext, base_reservation_perimeter_rect, building_base_reservation_rect,
    building_base_reservation_repair_rect, packed_reservation_coord,
};

impl Simulation {
    /// `BuildingClass::Unlimbo @ 0x00440580` calls
    /// `FUN_0042F260 @ 0x0042F260` at `0x0044159D..0x004415B3` for
    /// successful non-human BasePlan satisfaction.
    pub(super) fn fill_base_plan_from_successful_building_unlimbo(&mut self, stable_id: u64) {
        let Some((owner, type_index, packed_cell, has_undeploy_target)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category() == EntityCategory::Structure
                    && entity.seldom.type_facts.base_plan_type_index() >= 0)
                    .then_some((
                        entity.owner(),
                        entity.seldom.type_facts.base_plan_type_index(),
                        crate::sim::base_plan::pack_base_plan_cell(
                            i32::from(entity.position.rx()),
                            i32::from(entity.position.ry()),
                        ),
                        entity.seldom.type_facts.base_plan_has_undeploy_target(),
                    ))
            })
        else {
            return;
        };
        let policy = self.session.rule_policy();
        let Some(house) = self.houses.get_mut(&owner) else {
            return;
        };
        if house.is_controlled_by_human(policy) {
            return;
        }
        let _filled = house
            .base_plan_mut()
            .fill_successful_building(type_index, packed_cell, has_undeploy_target)
            .is_some();
        #[cfg(any(test, feature = "test-support"))]
        if _filled {
            self.trace_lifecycle_for_test(LifecycleTestEvent::BasePlanFilled { stable_id });
        }
    }

    /// `BuildingClass__Limbo @ 0x00445880` calls
    /// `FUN_0050A490 @ 0x0050A490` for BasePlan invalidation before the common
    /// Techno/Object concealment path. VERA has no map-editor runtime; every
    /// call to this lifecycle seam is therefore outside editor mode.
    pub(super) fn invalidate_base_plan_from_building_limbo(
        policy: AiBasePolicy,
        substrate: &ObjectSubstrate,
        houses: &mut Houses,
        stable_id: u64,
    ) {
        let Some((owner, type_index, packed_cell, is_base_defense, in_limbo)) =
            substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category() == EntityCategory::Structure
                    && entity.seldom.type_facts.base_plan_type_index() >= 0)
                    .then_some((
                        entity.owner(),
                        entity.seldom.type_facts.base_plan_type_index(),
                        crate::sim::base_plan::pack_base_plan_cell(
                            i32::from(entity.position.rx()),
                            i32::from(entity.position.ry()),
                        ),
                        entity.seldom.type_facts.base_plan_is_defense(),
                        entity.lifecycle.in_limbo,
                    ))
            })
        else {
            return;
        };
        if in_limbo {
            return;
        }
        if let Some(house) = houses.get_mut(&owner) {
            house.base_plan_mut().invalidate_limbo_building(
                type_index,
                packed_cell,
                is_base_defense,
                policy,
            );
        }
    }

    /// Mark(PUT) through the object's vt+0x124: `FootClass::Mark`
    /// ([`Simulation::foot_mark_put`]) for Infantry, Unit and Aircraft, the
    /// building transaction below for a Structure.
    pub(crate) fn mark_entity_put(
        &mut self,
        stable_id: u64,
        mut context: UninitContext<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        if entity.category() != EntityCategory::Structure {
            return self.foot_mark_put(
                stable_id,
                context.rules,
                context.registry(),
                context.effects(),
            );
        }
        if entity.lifecycle.cell_marked {
            return false;
        }
        entity.lifecycle.cell_marked = true;
        let cells = entity_occupancy_cells(entity);
        let layer = cell_list_layer_for_entity(
            entity,
            context.terrain().or(self.resolved_terrain.as_ref()),
        );
        let insertion = CellListInsertion::from_category(entity.category());
        let foundation = entity.seldom.type_facts.foundation().to_owned();
        let hidden_profile = entity.seldom.type_facts.building_hidden_occupancy();
        let current_cell = (entity.position.rx(), entity.position.ry());
        let (width, height) = foundation.dimensions();
        if let Some(layer) = layer {
            for &(rx, ry) in &cells {
                self.substrate
                    .occupancy
                    .add(rx, ry, stable_id, layer, None, insertion);
                self.discover_by_player(stable_id, Some((rx, ry)), context.reborrow());
            }
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationListLinked);
            if layer == crate::sim::movement::locomotor::MovementLayer::Ground
                && hidden_profile.is_some_and(|profile| {
                    self.substrate.hidden_occupation.enter_building(
                        current_cell,
                        &foundation,
                        profile,
                        Some((self.session.map_width(), self.session.map_height())),
                        context.rules.ares_form(),
                    )
                })
            {
                // A sleeping object reads its cell's hidden state only in a
                // turn (`world::idle_sleep`).
                self.substrate.entities.wake_all();
                #[cfg(any(test, feature = "test-support"))]
                self.trace_lifecycle_for_test(LifecycleTestEvent::HiddenOccupationEntered);
            }
            for &(rx, ry) in &cells {
                self.substrate
                    .raw_cell_occupation
                    .mark_ground(rx, ry, BUILDING_OCCUPATION_BIT);
            }
            #[cfg(any(test, feature = "test-support"))]
            if !cells.is_empty() {
                self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationMarked);
            }
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::CellMarked);
        // Building Mark43F180 -> PlaceDown5683C0 publishes AddContent and
        // Cell Recalc47D2B0 before returning. Keep the derived path view at
        // that boundary too; the marked structures remain its authority.
        self.publish_recalculated_cells(context.rules(), &cells);
        // BuildingMark43F738 moves its retained animations after PlaceDown
        // publishes the cells, before43F79B begins the smudge-clear scan.
        self.reposition_building_anim_slots(stable_id, context.rules());
        if let (Some(grid), Some(terrain)) =
            (self.smudge_grid.as_mut(), self.resolved_terrain.as_ref())
        {
            // BuildingMark43F79B..43F91E follows PlaceDown, visits the full
            // rectangle (including refinery holes), and clears by current
            // SmudgeData/Width rather than a placement-time origin cache.
            'sources: for dy in 0..height {
                for dx in 0..width {
                    let source = (
                        (current_cell.0 as i16).wrapping_add(dx as i16),
                        (current_cell.1 as i16).wrapping_add(dy as i16),
                    );
                    let cleared = grid.clear_footprint_identity_at(
                        source,
                        &context.rules().smudge_types,
                        terrain,
                        |identity, at, _| {
                            // Cell4852D0 invalidates every target, including
                            // already-empty overlaps, without a radar write.
                            if matches!(
                                identity,
                                crate::map::cell_index::NativeCellIdentity::Real(_)
                            ) {
                                self.dirty_cells.mark_tactical((at.0 as u16, at.1 as u16));
                            }
                        },
                    );
                    if cleared.is_none() {
                        // Native faults here on an invalid type/zero divisor.
                        // Keep committed placement and earlier clear writes.
                        log::debug!("BuildingMark stopped an invalid smudge clear");
                        break 'sources;
                    }
                }
            }
        }
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.cell_marked)
    }

    /// IsCellInPlayfield (`0x00578460`, mode 1) of the Reveal position's
    /// origin Cell, which AircraftClass::Unlimbo asks through `0x005785F0`
    /// (`0x00414352`) for its Z ([`Self::unlimbo_z`]). Unbounded synthetic
    /// fixtures retain their historical permissive behavior.
    pub(super) fn reveal_position_is_in_playfield(
        &self,
        position: RevealPosition,
        context: UninitContext<'_>,
    ) -> bool {
        let Some(bounds) = self.playfield_bounds else {
            return true;
        };
        crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(position.rx()), i32::from(position.ry())),
            Some(bounds),
            context.terrain().or(self.resolved_terrain.as_ref()),
        )
    }

    pub(crate) fn base_reservation_house_index(&self, owner: InternedId) -> Option<i32> {
        // Active YR Full_Init constructs the complete HouseClass array before
        // Terrain/Techno map sections reveal objects. Scenario loading preserves
        // that order, so Reveal writes the final house-index bit immediately.
        if self.houses.is_empty() {
            return None;
        }
        let index = self
            .houses
            .index_of(owner)
            .and_then(|index| i32::try_from(index).ok());
        debug_assert!(
            index.is_some(),
            "base-reservation owner must be registered in the House array"
        );
        index
    }

    pub(super) fn mark_building_base_reservation_with_arg(
        &mut self,
        stable_id: u64,
        repair_only: bool,
        context: UninitContext<'_>,
    ) -> bool {
        let Some((owner, rect)) = self.base_reservation_writer(stable_id) else {
            return false;
        };
        let Some(house_index) = self.base_reservation_house_index(owner) else {
            return false;
        };
        self.substrate.base_reservations.reserve_rect(
            context.terrain().or(self.resolved_terrain.as_ref()),
            rect,
            house_index,
        );
        if let Some(house) = self.houses.get_mut(&owner) {
            house
                .base_reservation_mut()
                .update_bounds(rect.x, rect.y, rect.width, rect.height);
        } else {
            debug_assert!(false, "base-reservation owner must have HouseState");
        }
        if !repair_only {
            Self::update_base_reservation_perimeter_after_mark(
                &self.substrate,
                &mut self.houses,
                &self.resolved_terrain,
                owner,
                house_index,
                rect,
                context,
            );
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::BaseReservationMarked);
        true
    }

    fn update_base_reservation_perimeter_after_mark(
        substrate: &ObjectSubstrate,
        houses: &mut Houses,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        owner: InternedId,
        house_index: i32,
        rect: CellRect,
        context: UninitContext<'_>,
    ) {
        scan_cell_rect(base_reservation_perimeter_rect(rect), |x, y| {
            let neighbor_mask = substrate.base_reservations.house_reservation_neighbor_mask(
                context.terrain().or(resolved_terrain.as_ref()),
                x,
                y,
                house_index,
            );
            let packed = packed_reservation_coord(x, y);
            if let Some(house) = houses.get_mut(&owner) {
                match neighbor_mask {
                    0x0000_00ff => house.base_reservation_mut().remove_perimeter_cell(packed),
                    1..=0x0000_00fe => house
                        .base_reservation_mut()
                        .append_perimeter_cell_if_absent(packed),
                    0 | u32::MAX => {}
                    _ => unreachable!("neighbor mask is eight bits or the absent-center sentinel"),
                }
            }
            true
        });
    }

    fn update_base_reservation_perimeter_after_clear(
        substrate: &mut ObjectSubstrate,
        houses: &mut Houses,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        owner: InternedId,
        house_index: i32,
        rect: CellRect,
        context: UninitContext<'_>,
    ) {
        scan_cell_rect(base_reservation_perimeter_rect(rect), |x, y| {
            let neighbor_mask = substrate.base_reservations.house_reservation_neighbor_mask(
                context.terrain().or(resolved_terrain.as_ref()),
                x,
                y,
                house_index,
            );
            let packed = packed_reservation_coord(x, y);
            match neighbor_mask {
                0x0000_00ff => {
                    if let Some(house) = houses.get_mut(&owner) {
                        house.base_reservation_mut().remove_perimeter_cell(packed);
                    }
                }
                1..=0x0000_00fe => {
                    if let Some(house) = houses.get_mut(&owner) {
                        house
                            .base_reservation_mut()
                            .append_perimeter_cell_if_absent(packed);
                    }
                }
                0 | u32::MAX => {
                    if let Some(house) = houses.get_mut(&owner) {
                        house.base_reservation_mut().remove_perimeter_cell(packed);
                    }
                    // Native resolves the center again before the extra clear.
                    substrate.base_reservations.clear(
                        context.terrain().or(resolved_terrain.as_ref()),
                        x,
                        y,
                        house_index,
                    );
                }
                _ => unreachable!("neighbor mask is eight bits or the absent-center sentinel"),
            }
            true
        });
    }

    /// The rectangle of the base-reservation writer `0x00455F10`: Location's cell
    /// less `AIBaseSpacing=`, the foundation plus twice the spacing.
    fn base_reservation_writer(&self, stable_id: u64) -> Option<(InternedId, CellRect)> {
        let entity = self.substrate.entities.get(stable_id)?;
        let spacing = entity.seldom.type_facts.base_reservation_spacing()?;
        // Ares IsBaseNormal (10011810, hooks 0x00440C08 Unlimbo and
        // 0x00456370 the neighbour repair): a delivered building marked
        // NoBaseNormal reserves no base space.
        (entity.category() == EntityCategory::Structure
            && entity.unit_delivery_provides_base_space()
            && entity.lifecycle.object_alive
            && !entity.lifecycle.in_limbo
            && entity.lifecycle.cell_marked
            && cell_list_layer_for_entity(entity, self.resolved_terrain.as_ref())
                == Some(crate::sim::movement::locomotor::MovementLayer::Ground))
        .then(|| {
            (
                entity.owner(),
                building_base_reservation_rect(
                    entity.position.rx(),
                    entity.position.ry(),
                    entity.seldom.type_facts.foundation(),
                    spacing,
                ),
            )
        })
    }

    /// The base-reservation clear and neighbor repair `0x004561F0` that Building
    /// Limbo calls (`0x00445A9A`), over the same rectangle from Location's cell.
    pub(super) fn clear_building_base_reservation_and_repair(
        &mut self,
        stable_id: u64,
        mut context: UninitContext<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        let Some(spacing) = entity.seldom.type_facts.base_reservation_spacing() else {
            return false;
        };
        // Ares IsBaseNormal at Limbo's test (hook 0x00445A72): NoBaseNormal
        // skips the clear.
        if entity.category() != EntityCategory::Structure
            || !entity.unit_delivery_provides_base_space()
            || !entity.lifecycle.cell_marked
            || cell_list_layer_for_entity(
                entity,
                context.terrain().or(self.resolved_terrain.as_ref()),
            ) != Some(crate::sim::movement::locomotor::MovementLayer::Ground)
        {
            return false;
        }
        let owner = entity.owner();
        let rect = building_base_reservation_rect(
            entity.position.rx(),
            entity.position.ry(),
            entity.seldom.type_facts.foundation(),
            spacing,
        );
        let (foundation_width, foundation_height) =
            entity.seldom.type_facts.foundation().dimensions();
        let repair_rect = building_base_reservation_repair_rect(
            entity.position.rx(),
            entity.position.ry(),
            foundation_width,
            foundation_height,
            spacing,
        );
        let Some(house_index) = self.base_reservation_house_index(owner) else {
            return false;
        };

        self.substrate.base_reservations.clear_rect(
            context.terrain().or(self.resolved_terrain.as_ref()),
            rect,
            house_index,
        );
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::BaseReservationCleared);

        // The native repair scan happens while this building is still linked in
        // the ground list. Each lookup selects only the first Building and calls
        // its repair-only writer immediately; identities are not deduplicated.
        scan_cell_rect(repair_rect, |x, y| {
            let neighbor_id = resolve_reservation_real_cell(
                context.terrain().or(self.resolved_terrain.as_ref()),
                x,
                y,
            )
            .and_then(|(rx, ry)| {
                self.substrate.occupancy.first_building_on_layer(
                    rx,
                    ry,
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                )
            });
            if let Some(neighbor_id) = neighbor_id
                && neighbor_id != stable_id
            {
                self.mark_building_base_reservation_with_arg(neighbor_id, true, context.reborrow());
            }
            true
        });
        Self::update_base_reservation_perimeter_after_clear(
            &mut self.substrate,
            &mut self.houses,
            &self.resolved_terrain,
            owner,
            house_index,
            rect,
            context,
        );
        true
    }

    /// Mark(REMOVE) through the object's vt+0x124: `FootClass::Mark`
    /// ([`Simulation::foot_mark_remove`]) for Infantry, Unit and Aircraft, the
    /// building transaction below for a Structure.
    pub(crate) fn unmark_entity_remove(
        &mut self,
        stable_id: u64,
        mut context: UninitContext<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        if entity.category() != EntityCategory::Structure {
            return self.foot_mark_remove(
                stable_id,
                context.rules,
                context.registry(),
                context.effects(),
            );
        }
        if !entity.lifecycle.cell_marked {
            return false;
        }
        entity.lifecycle.cell_marked = false;
        let cells = entity_occupancy_cells(entity);
        let layer = cell_list_layer_for_entity(
            entity,
            context.terrain().or(self.resolved_terrain.as_ref()),
        );
        let foundation = entity.seldom.type_facts.foundation().to_owned();
        let hidden_profile = entity.seldom.type_facts.building_hidden_occupancy();
        let current_cell = (entity.position.rx(), entity.position.ry());
        if let Some(layer) = layer {
            for &(rx, ry) in &cells {
                self.substrate
                    .occupancy
                    .remove_on_layer(rx, ry, stable_id, layer);
            }
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationListUnlinked);
            if layer == crate::sim::movement::locomotor::MovementLayer::Ground
                && hidden_profile.is_some_and(|profile| {
                    self.substrate.hidden_occupation.exit_building(
                        current_cell,
                        &foundation,
                        profile,
                        Some((self.session.map_width(), self.session.map_height())),
                        context.rules.ares_form(),
                    )
                })
            {
                self.substrate.entities.wake_all();
                #[cfg(any(test, feature = "test-support"))]
                self.trace_lifecycle_for_test(LifecycleTestEvent::HiddenOccupationExited);
            }
            for &(rx, ry) in &cells {
                self.substrate
                    .raw_cell_occupation
                    .clear_ground(rx, ry, BUILDING_OCCUPATION_BIT);
            }
            #[cfg(any(test, feature = "test-support"))]
            if !cells.is_empty() {
                self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationCleared);
            }
        }
        // Building Mark43F180 -> PlaceUp5687F0 unlinks each Cell47EA90,
        // clears occupation453DC0 and recalculates47D2B0 before Conceal
        // resumes at5F4D73. Fatal Bullet receivers can remove Logic here,
        // so no later object/frame visit may be required to release the path.
        // Executed shared death path: tools/spatial_oracle/
        // building_death_anims_joined_stock_smudges.json (building_mark).
        // This projects current marked owners, retaining overlapping or
        // still-marked dying structures; it adds no RNG/timer/detach work.
        self.publish_recalculated_cells(context.rules(), &cells);
        true
    }

    /// ObjectClass::DropAsBomb5F4160: latch the fall before removing the
    /// old layer, clear OnBridge, then re-enter the ground list. Location,
    /// FallRate, parachute byte and locomotor order remain authoritative.
    /// Native multi-cell content hooks differ from full teardown Mark:
    /// neither building smudges nor unrelated track reservations are retired.
    /// Residual: hidden-building content callbacks beyond this relayer.
    pub(crate) fn drop_as_bomb(&mut self, stable_id: u64, mut context: UninitContext<'_>) {
        let rules = context.rules;
        use crate::sim::movement::locomotor::MovementLayer;
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let cells = entity_occupancy_cells(entity);
        let sub_cell = entity.sub_cell();
        let insertion = CellListInsertion::from_category(entity.category());
        let old_layer = cell_list_layer_for_entity(entity, self.resolved_terrain.as_ref());
        let world_z = crate::sim::movement::ground_pose::object_world_z_leptons(
            entity,
            self.resolved_terrain.as_ref(),
        );
        self.substrate
            .entities
            .get_mut(stable_id)
            .unwrap()
            .begin_bomb_fall();
        let foot = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                matches!(
                    entity.category(),
                    EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                )
            });
        if foot {
            self.foot_mark_remove(stable_id, rules, context.registry(), &mut context.effects);
        } else {
            for &(rx, ry) in &cells {
                self.substrate.occupancy.remove_on_layer(
                    rx,
                    ry,
                    stable_id,
                    old_layer.unwrap_or(MovementLayer::Ground),
                );
            }
        }
        self.substrate.display.remove(stable_id);
        let entity = self
            .substrate
            .entities
            .get_mut(stable_id)
            .expect("member remains represented");
        entity.on_bridge.store(false);
        entity.position.put_z(world_z);
        Self::submit_entity_display(
            &self.interner,
            &mut self.substrate,
            &self.resolved_terrain,
            stable_id,
            rules,
            None,
        );
        if foot {
            self.foot_mark_put(stable_id, rules, context.registry(), &mut context.effects);
        } else {
            for &(rx, ry) in &cells {
                self.substrate.occupancy.add(
                    rx,
                    ry,
                    stable_id,
                    MovementLayer::Ground,
                    sub_cell,
                    insertion,
                );
            }
        }
        //5F41D0..5F4218: Foot's active Is_Moving chooses the shared
        // locomotor occupation release or the owner's raw Location receiver.
        if let Some(entity) = self.substrate.entities.get(stable_id)
            && matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
            && entity.locomotor.is_some()
        {
            let moving = crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false);
            let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            if moving {
                self.locomotor_mark_all_occupation_bits(stable_id, false, rules.ares_form());
            } else {
                Self::object_raw_receiver_at(
                    &mut self.substrate,
                    &self.path_grid,
                    &self.resolved_terrain,
                    stable_id,
                    coord,
                    false,
                    rules.ares_form(),
                );
            }
        }
    }

    /// Fixture boundary: puts a fixture object on the map and marks it
    /// (idempotent). Fixtures are constructed in Limbo, which Mark refuses
    /// (`0x005F5854`), so this clears InLimbo first.
    #[cfg(any(test, feature = "test-support"))]
    pub fn add_entity_occupancy(&mut self, stable_id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = false;
        }
        let _ = self.mark_entity_put(stable_id, UninitContext::default());
    }

    /// Fixture boundary; common lifecycle code calls the private unmark
    /// transaction instead.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn remove_entity_occupancy(&mut self, stable_id: u64) {
        self.unmark_entity_remove(stable_id, UninitContext::default());
    }
}
