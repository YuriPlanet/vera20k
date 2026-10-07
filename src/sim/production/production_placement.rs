//! Building placement validation, sell/repair, and producer focus management.
//!
//! Handles placement preview, build area checks, sell refunds with crew ejection,
//! repair tick, and producer cycling.

use crate::map::entities::EntityCategory;
use crate::map::houses::are_houses_friendly;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::components::BuildingUp;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::Simulation;

use super::production_tech::{
    foundation_dimensions, producer_candidates_for_owner_category, production_category_for_object,
};
use super::production_types::*;
use super::wall_placement;

/// Placement preview for object types that do not require overlay metadata.
///
/// Wall callers must use `placement_preview_for_owner_with_overlays`; naming
/// this compatibility entry point explicitly prevents live wall-capable paths
/// from silently dropping the overlay registry.
pub fn placement_preview_for_owner_without_overlays(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
) -> Option<BuildingPlacementPreview> {
    placement_preview_for_owner_with_overlays(sim, rules, owner, type_id, rx, ry, None)
}

pub fn placement_preview_for_owner_with_overlays(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<BuildingPlacementPreview> {
    let obj = rules.object(type_id)?;
    let (width, height) = foundation_dimensions(&obj.foundation);
    let reason =
        evaluate_building_placement(sim, rules, owner, type_id, rx, ry, overlay_registry).err();
    let in_build_area = reason
        .as_ref()
        .is_none_or(|r| !matches!(r, BuildingPlacementError::OutOfBuildArea));
    let owner_id = sim.interner.get(owner);
    let wall_overlay_id = overlay_registry.and_then(|registry| {
        obj.wall
            .then(|| wall_placement::linked_overlay_id(obj, registry))
            .flatten()
    });
    // The cursor paints each cell with `CellClass::Is_Clear_To_Build`
    // (`BuildingPlacement_per_cell_draw`, `0x0047EE93`) for the placing house.
    let mut cell_valid: Vec<bool> = Vec::with_capacity((width as usize) * (height as usize));
    for dy in 0..height {
        for dx in 0..width {
            let cx: u16 = rx.saturating_add(dx);
            let cy: u16 = ry.saturating_add(dy);
            let ok = (!obj.wall || wall_overlay_id.is_some())
                && placement_cell_clear(sim, rules, overlay_registry, obj, owner_id, cx, cy);
            cell_valid.push(in_build_area && ok);
        }
    }
    let wall_autofill_cells = match (owner_id, wall_overlay_id, overlay_registry) {
        (Some(owner_id), Some(overlay_id), Some(registry)) if obj.wall => {
            wall_placement::autofill_cells(
                sim,
                rules,
                registry,
                obj,
                (rx, ry),
                owner_id,
                overlay_id,
            )
        }
        _ => Vec::new(),
    };
    let type_interned = sim.interner.get(type_id).unwrap_or_default();
    Some(BuildingPlacementPreview {
        type_id: type_interned,
        rx,
        ry,
        width,
        height,
        valid: reason.is_none(),
        reason,
        cell_valid,
        wall_autofill_cells,
    })
}

pub fn active_producer_for_owner_category(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    category: ProductionCategory,
) -> Option<ProducerFocusView> {
    let candidates = producer_candidates_for_owner_category(
        &sim.substrate.entities,
        rules,
        owner,
        category,
        true,
        &sim.interner,
    );
    let owner_id = sim.interner.get(owner);
    let active_sid = owner_id.and_then(|id| sim.production.primary_factory(id, category));
    let selected = active_sid
        .and_then(|sid| {
            candidates
                .iter()
                .find(|candidate| candidate.0 == sid)
                .cloned()
        })
        .or_else(|| candidates.into_iter().next())?;
    let display_name = rules
        .object(&selected.3)
        .and_then(|obj| obj.name.clone())
        .unwrap_or_else(|| selected.3.clone());
    Some(ProducerFocusView {
        stable_id: selected.0,
        display_name,
        category,
        rx: selected.1,
        ry: selected.2,
    })
}

pub fn cycle_active_producer_for_owner_category(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    category: ProductionCategory,
) -> bool {
    let candidates = producer_candidates_for_owner_category(
        &sim.substrate.entities,
        rules,
        owner,
        category,
        true,
        &sim.interner,
    );
    if candidates.is_empty() {
        return false;
    }

    let owner_id = sim.interner.intern(owner);
    let current = sim.production.primary_factory(owner_id, category);
    let next_sid =
        match current.and_then(|sid| candidates.iter().position(|candidate| candidate.0 == sid)) {
            Some(index) => candidates[(index + 1) % candidates.len()].0,
            None => candidates[0].0,
        };
    sim.production
        .set_primary_factory(owner_id, category, next_sid);
    true
}

/// One HouseClass::Place_Production4FB0E0 owner. A building PLACE uses
/// its clicked type/cell; mobile type_index=-1 resolves the current category
/// head when the queued event executes, not the object held when Strip issued it.
pub fn place_production_with_overlays(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    request: ProductionPlacement<'_>,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    let (type_id, (rx, ry)) = match request {
        ProductionPlacement::Building { type_id, cell } => (type_id, cell),
        ProductionPlacement::Mobile { category } => {
            use crate::sim::ai_base_building::BuildingExit;
            let Some(owner_id) = sim.interner.get(owner) else {
                return false;
            };
            let Some(factory) = sim
                .production
                .factory_shadow
                .view(owner_id, category)
                .filter(|factory| factory.ready)
            else {
                return false;
            };
            let Some(object) = factory.object else {
                return false;
            };
            let Some(entity_id) = object.entity_id else {
                return false;
            };
            let Some(obj) = sim.object_type(object.type_id, rules) else {
                return false;
            };
            if obj.category == ObjectCategory::Building
                || production_category_for_object(obj) != category
            {
                return false;
            }
            // Native wrapper5F5C20 -> TypeFindFactory5F7900. A null
            // Infantry/Aircraft producer leaves the head and queue untouched.
            // Unit alone retries with its radio-selection restriction skipped.
            let producer = super::find_factory(sim, rules, owner_id, obj, false, true, false)
                .or_else(|| {
                    (obj.category == ObjectCategory::Vehicle)
                        .then(|| super::find_factory(sim, rules, owner_id, obj, true, true, false))
                        .flatten()
                });
            let Some(producer) = producer else {
                return false;
            };
            let exit = super::production_queue::exit_produced_object(
                sim,
                rules,
                producer,
                entity_id,
                overlay_registry,
            );
            let accepted = exit == BuildingExit::Placed
                || exit == BuildingExit::TryLater
                    && sim
                        .production
                        .factory_shadow
                        .building_factory(producer)
                        .is_some();
            // House4FB57F's +524 is a producer Factory pointer, not a
            // product pointer. The Exit1/+524 exception is instruction-established
            // only; no human/AI held-pointer alias transfer is invented here.
            if accepted {
                // House4FB5FB queries the selected producer's GetCoords447AC0,
                // then4FB600..FB622 truncates XY into packed CellStruct words.
                // Source: factory_infantry_output native notification controls;
                // a fallback GI can occupy14,15 while radar6 names15,15.
                if sim.houses.get(&owner_id).is_some_and(|house| {
                    house.is_controlled_by_human(sim.session.game_mode_nonzero)
                }) && let Some(factory_entity) = sim.substrate.entities.get(producer)
                {
                    let coords = crate::sim::movement::ground_pose::object_get_coords(
                        factory_entity,
                        sim.resolved_terrain.as_ref(),
                    );
                    sim.sound_events
                        .push(crate::sim::world::SimSoundEvent::UnitComplete {
                            owner: owner_id,
                            radar: crate::sim::radar::RadarEventRequest::new(
                                crate::sim::radar::RadarEventType::UnitReady,
                                crate::util::lepton::lepton_to_cell_packed(coords.x) as u16,
                                crate::util::lepton::lepton_to_cell_packed(coords.y) as u16,
                            ),
                        });
                }
                super::factory_lifecycle::release_delivered_mobile(sim, rules, owner_id, category);
            } else {
                // Exit0 or Exit1 with producer+524zero: refund the actually
                // charged complete object, destroy it and start the queued head.
                super::factory_lifecycle::refund_failed_delivery(sim, rules, owner_id, category);
            }
            return accepted;
        }
    };
    let Some(obj) = rules.object(type_id) else {
        return false;
    };
    if obj.category != ObjectCategory::Building {
        return false;
    }

    let owner_id = sim.interner.intern(owner);
    let type_interned = sim.interner.intern(type_id);
    let Some(ready_queue) = sim.production.ready_by_owner.get(&owner_id) else {
        return false;
    };
    if !ready_queue.iter().any(|&queued| queued == type_interned) {
        return false;
    }
    if obj.wall {
        if evaluate_building_placement(sim, rules, owner, type_id, rx, ry, overlay_registry)
            .is_err()
        {
            return false;
        }
        let category = production_category_for_object(obj);
        let Some(held) =
            super::factory_lifecycle::ready_object(sim, owner_id, category, type_interned)
        else {
            return false;
        };
        let Some(registry) = overlay_registry else {
            return false;
        };
        if !wall_placement::stamp_wall_with_autofill(sim, rules, registry, obj, (rx, ry), owner_id)
        {
            return false;
        }
        sim.sound_events
            .push(crate::sim::world::SimSoundEvent::BuildingPlaced { owner: owner_id });
        // Wall placement consumes the factory-created BuildingClass into
        // overlay state; the constructor identity is destroyed, never
        // reconstructed at placement.
        return held.consume_after_wall_stamp(sim, rules);
    }
    let foundation_str: String = rules
        .object(type_id)
        .map(|o| o.foundation.clone())
        .unwrap_or_else(|| "?".to_string());
    let z: u8 = sim.terrain_cell_level(rx, ry).unwrap_or(0);
    log::info!(
        "Placing building {} at ({},{}) z={} foundation={}",
        type_id,
        rx,
        ry,
        z,
        foundation_str,
    );
    let category = production_category_for_object(obj);
    let Some(held) = super::factory_lifecycle::ready_object(sim, owner_id, category, type_interned)
    else {
        return false;
    };
    // Original PLACE4FB1DA keeps Object::FindFactory(0,0)'s yard through
    // HELLO, Unlimbo, the child's C message and the yard's FirstContact BREAK.
    // The completed object is owned by the house factory, not by this yard.
    let Some(yard) = super::find_factory(sim, rules, owner_id, obj, false, false, false) else {
        return false;
    };
    crate::sim::radio::transmit(
        sim,
        yard,
        held.entity_id(),
        crate::sim::radio::RadioMessage::Hello,
        crate::sim::radio::RadioPayload::default(),
        Some(rules),
    );
    // Event::Execute4C70E1..4C710B passes an admitted PLACE straight to
    // House4FB0E0. Its placement refusal occurs after HELLO4FB1F1, and
    // still reaches the yard's FirstContact BREAK4FB4A6.
    if evaluate_building_placement(sim, rules, owner, type_id, rx, ry, overlay_registry).is_err() {
        crate::sim::radio::transmit_to_contact(
            sim,
            yard,
            crate::sim::radio::RadioMessage::Break,
            Some(rules),
        );
        return false;
    }
    let Some(new_sid) = sim.reveal_constructed_object_at_height(
        held.entity_id(),
        rx,
        ry,
        0,
        z,
        crate::sim::world::PlacementEvidence::EvaluateMark,
        rules,
    ) else {
        // The failed placement reaches the same4FB4A6 FirstContact teardown;
        // its retained limbo object remains in the house factory for retry.
        crate::sim::radio::transmit_to_contact(
            sim,
            yard,
            crate::sim::radio::RadioMessage::Break,
            Some(rules),
        );
        return false;
    };
    // `0x004452FA`/`0x004FB252`: a placed building's slave manager (a Slave
    // Miner refinery's) takes the hand-off once its Unlimbo succeeds.
    sim.slave_manager_hand_off(new_sid, rules);
    // Log screen position for debugging placement alignment.
    if let Some(ge) = sim.substrate.entities.get(new_sid) {
        let (fw, fh) = foundation_dimensions(&foundation_str);
        // The entity anchor — the projection of the building's own coordinate,
        // which is its north-west cell's diamond centre. The drawn art sits half
        // a tile above this (render's building render-coordinate lift); this is
        // a debug log, and sim/ cannot call render/ to apply it.
        let screen = crate::util::lepton::lepton_to_screen(
            ge.position.rx,
            ge.position.ry,
            ge.position.sub_x,
            ge.position.sub_y,
            ge.position.z,
        );
        log::info!(
            "  → spawned sid={} screen=({:.0},{:.0}) foundation_cells: ({},{})..({},{})",
            new_sid,
            screen.0,
            screen.1,
            rx,
            ry,
            rx + fw - 1,
            ry + fh - 1,
        );
    }
    // The placed building builds up (`sim::building_construction`) through
    // the PLACE event (`HouseClass::Place_Production`); a computer house's
    // yard places its own through `sim::ai_base_building::exit_building`.
    let control = rules.buildup_control(type_id);
    let now = sim.session.binary_frame as i32;
    if let Some(ge) = sim.substrate.entities.get_mut(new_sid) {
        ge.install_building_up(BuildingUp::placed_by_player(control, now), now);
    }
    // Refresh superweapon grants — newly placed building may provide a SW.
    if sim.session.game_options.super_weapons {
        crate::sim::superweapon::refresh_super_weapons_for_owner(sim, rules, owner_id);
    }

    let released = held.release_after_placement(sim, rules);
    if released {
        crate::sim::radio::transmit(
            sim,
            new_sid,
            yard,
            crate::sim::radio::RadioMessage::DockArrived,
            crate::sim::radio::RadioPayload::default(),
            Some(rules),
        );
    }
    // `0x004FB2CC..0x004FB314`: BuildingSlam follows every successful
    // Unlimbo, before the yard's FirstContact BREAK, whatever the release.
    sim.sound_events
        .push(crate::sim::world::SimSoundEvent::BuildingPlaced { owner: owner_id });
    crate::sim::radio::transmit_to_contact(
        sim,
        yard,
        crate::sim::radio::RadioMessage::Break,
        Some(rules),
    );
    if released {
        // Original4FB4B7 records the successful build after FirstContact BREAK,
        // not during CompletedProduction4FB2A1 or its C message4FB2AD.
        super::factory_lifecycle::record_last_built(sim, rules, owner_id, type_interned);
    }
    released
}

fn evaluate_building_placement(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Result<(), BuildingPlacementError> {
    let Some(obj) = rules.object(type_id) else {
        return Err(BuildingPlacementError::NotBuilding);
    };
    let (width, height) = foundation_dimensions(&obj.foundation);
    if obj.category != ObjectCategory::Building {
        return Err(BuildingPlacementError::NotBuilding);
    }
    let owner_id = sim.interner.get(owner);
    let type_interned = sim.interner.get(type_id);
    let ready_for_owner = owner_id.and_then(|id| sim.production.ready_by_owner.get(&id));
    let Some(ready_for_owner) = ready_for_owner else {
        return Err(BuildingPlacementError::NotReady);
    };
    let has_type = type_interned.map_or(false, |tid| {
        ready_for_owner.iter().any(|&queued| queued == tid)
    });
    if !has_type {
        return Err(BuildingPlacementError::NotReady);
    }
    if obj.wall
        && overlay_registry
            .and_then(|registry| wall_placement::linked_overlay_id(obj, registry))
            .is_none()
    {
        return Err(BuildingPlacementError::BlockedTerrain);
    }
    // The PLACE event's Unlimbo admits the building through its own
    // Can_Enter_Cell (`0x00449440`): BuildingTypeClass::CanPlaceAt at the
    // clicked cell for the owning house.
    if !crate::sim::build_site::can_place_building_at(
        sim,
        rules,
        overlay_registry,
        obj,
        (rx as i16, ry as i16),
        owner_id,
    ) {
        // The error variant only labels the refusal for presentation.
        let overlaps_structure = crate::rules::foundation::foundation_cell_offsets(&obj.foundation)
            .into_iter()
            .any(|(dx, dy)| {
                sim.cell_objects(
                    (rx.wrapping_add(dx as u16), ry.wrapping_add(dy as u16)),
                    MovementLayer::Ground,
                )
                .any(|member| {
                    matches!(member, crate::sim::occupancy::CellObjectMember::Entity(id)
                    if sim.substrate.entities.get(id).is_some_and(|e| {
                        e.category == EntityCategory::Structure
                    }))
                })
            });
        return Err(if overlaps_structure {
            BuildingPlacementError::OverlapsStructure
        } else {
            BuildingPlacementError::BlockedTerrain
        });
    }
    if is_within_build_area(sim, rules, owner, obj, rx, ry, width, height) {
        Ok(())
    } else {
        let providers: Vec<String> = sim
            .substrate
            .entities
            .values()
            .filter(|e| {
                e.category == EntityCategory::Structure
                    && sim.interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
            })
            .map(|e| {
                let type_str = sim.interner.resolve(e.type_ref());
                let bn = rules.object(type_str).map_or(false, |o| o.base_normal);
                format!(
                    "{}@({},{}) bn={}",
                    type_str, e.position.rx, e.position.ry, bn
                )
            })
            .collect();
        log::warn!(
            "Placement rejected: ({},{}) {}x{} outside build area for {} adj={} providers=[{}]",
            rx,
            ry,
            width,
            height,
            owner,
            obj.adjacent,
            providers.join(", "),
        );
        Err(BuildingPlacementError::OutOfBuildArea)
    }
}

/// One cell of the placement cursor: `CellClass::Is_Clear_To_Build` with the
/// type's own SpeedType and the placing house (`sim::build_site`).
fn placement_cell_clear(
    sim: &Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    obj: &ObjectType,
    owner: Option<InternedId>,
    cx: u16,
    cy: u16,
) -> bool {
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return false;
    };
    crate::sim::build_site::is_clear_to_build(
        sim,
        rules,
        registry,
        terrain.native_cell_identity((cx as i16, cy as i16)),
        crate::sim::build_site::building_speed_type(obj),
        Some(obj),
        owner,
    )
}

fn is_within_build_area(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    obj: &crate::rules::object_type::ObjectType,
    rx: u16,
    ry: u16,
    width: u16,
    height: u16,
) -> bool {
    let placed_adjacent = obj.adjacent;
    if placed_adjacent < 0 {
        return false;
    }
    for e in sim.substrate.entities.values() {
        // A dying structure provides no build-area adjacency during its window.
        if e.dying || e.lifecycle.in_limbo {
            continue;
        }
        if e.category != EntityCategory::Structure {
            continue;
        }
        let provider_owner = sim.interner.resolve(e.owner());
        let Some(existing) = sim.object_type(e.type_ref(), rules) else {
            continue;
        };
        if provider_owner.eq_ignore_ascii_case(owner) {
            if !existing.base_normal {
                continue;
            }
        } else if !(sim.session.game_options.build_off_ally
            && existing.eligibile_for_ally_building
            && are_houses_friendly(&sim.house_alliances, provider_owner, owner))
        {
            continue;
        }
        let (provider_width, provider_height) = foundation_dimensions(&existing.foundation);
        if provider_intersects_build_area_ring(
            (rx, ry, width, height),
            (
                e.position.rx,
                e.position.ry,
                provider_width,
                provider_height,
            ),
            placed_adjacent,
        ) {
            return true;
        }
    }
    false
}

fn provider_intersects_build_area_ring(
    placed: (u16, u16, u16, u16),
    provider: (u16, u16, u16, u16),
    adjacent: i32,
) -> bool {
    let (placed_rx, placed_ry, placed_width, placed_height) = placed;
    let (provider_rx, provider_ry, provider_width, provider_height) = provider;
    let expansion = adjacent.saturating_add(1);
    let placed_min_x = i32::from(placed_rx);
    let placed_min_y = i32::from(placed_ry);
    let placed_max_x = i32::from(placed_rx) + i32::from(placed_width) - 1;
    let placed_max_y = i32::from(placed_ry) + i32::from(placed_height) - 1;
    let min_x = placed_min_x - expansion;
    let min_y = placed_min_y - expansion;
    let max_x = placed_max_x + expansion;
    let max_y = placed_max_y + expansion;
    let provider_min_x = i32::from(provider_rx);
    let provider_min_y = i32::from(provider_ry);
    let provider_max_x = i32::from(provider_rx) + i32::from(provider_width) - 1;
    let provider_max_y = i32::from(provider_ry) + i32::from(provider_height) - 1;

    let intersects_expanded = provider_min_x <= max_x
        && provider_max_x >= min_x
        && provider_min_y <= max_y
        && provider_max_y >= min_y;
    let intersects_foundation = provider_min_x <= placed_max_x
        && provider_max_x >= placed_min_x
        && provider_min_y <= placed_max_y
        && provider_max_y >= placed_min_y;

    intersects_expanded && !intersects_foundation
}
