//! Building placement validation, sell/repair, and producer focus management.
//!
//! Handles placement preview, build area checks, sell refunds with crew ejection,
//! repair tick, and producer cycling.

use crate::map::entities::EntityCategory;
use crate::map::houses::are_houses_friendly;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

use super::production_tech::{
    producer_candidates_for_owner_category, production_category_for_object,
};
use super::production_types::*;
use super::wall_placement;
use crate::rules::foundation::foundation_dimensions;

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
    // One fork covers admission, ground/shroud reads and wall filler queries;
    // none of the cursor's nested lookups may stamp simulation's live Dummy.
    let native_cells = sim
        .resolved_terrain
        .as_ref()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    let evaluation = evaluate_building_placement(
        sim,
        rules,
        owner,
        type_id,
        rx,
        ry,
        overlay_registry,
        native_cells.as_ref(),
    );
    let mut reason = evaluation.reason;
    // UpdateFoundationCenter4A9480/4A9485 stores proximity independently;
    // Cell47EE6C/47EE73 rejects every active cell before IsClearToBuild when
    // it is false. A terrain-refusal label cannot stand in for this result.
    let in_build_area = evaluation.in_build_area;
    let owner_id = sim.interner.get(owner);
    let wall_overlay_id = overlay_registry.and_then(|registry| {
        obj.wall
            .then(|| wall_placement::linked_overlay_id(obj, registry))
            .flatten()
    });
    // The cursor paints each cell with `CellClass::Is_Clear_To_Build`
    // (`BuildingPlacement_per_cell_draw`, `0x0047EE93`) for the placing house.
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    let mut all_unshrouded = true;
    if let Some(query) = native_cells.as_ref() {
        for dy in 0..height {
            for dx in 0..width {
                let cx = rx.saturating_add(dx);
                let cy = ry.saturating_add(dy);
                let (level, slope) = query.ground_fields(query.lookup((cx as i16, cy as i16)));
                // Cell47ED33..47ED79 queries the current player's mapped
                // ground via its actual height-corrected centre. The shared
                // vision owner handles odd levels and the retained Dummy.
                // A headless evaluation with no current viewer enrolls no
                // client visibility; active local gameplay always has one.
                let unshrouded = sim.session.current_house().is_none_or(|viewer| {
                    !crate::sim::vision::cell_is_shrouded(&sim.fog, query, viewer, (cx, cy))
                });
                all_unshrouded &= unshrouded;
                let clear = (!obj.wall || wall_overlay_id.is_some())
                    && crate::sim::build_site::is_clear_to_build(
                        sim,
                        rules,
                        overlay_registry,
                        query.lookup((cx as i16, cy as i16)),
                        crate::sim::build_site::building_speed_type(obj),
                        Some(obj),
                        owner_id,
                        Some(query),
                    );
                cells.push(BuildingPlacementCell {
                    coord: (cx, cy),
                    valid: in_build_area && unshrouded && clear,
                    level: level as i8,
                    slope,
                });
            }
        }
    }
    // Display4A9070/4A94DA publishes visibility independently of proximity;
    // local LeftUp4ABAB2 refuses a shrouded ordinary foundation. ToTile's
    // resolved pointer (+E58) alone bypasses that visibility refusal.
    let local_owner =
        sim.session.current_house().is_some() && sim.session.current_house() == owner_id;
    let replaces_tile = sim
        .resolved_terrain
        .as_ref()
        .is_some_and(|terrain| crate::sim::build_site::resolved_to_tile(terrain, obj));
    let foundation_visible = !local_owner || all_unshrouded || replaces_tile;
    if !foundation_visible && reason.is_none() {
        reason = Some(BuildingPlacementError::BlockedTerrain);
    }
    let wall_coords = match (owner_id, wall_overlay_id, overlay_registry) {
        (Some(owner_id), Some(overlay_id), Some(registry)) if obj.wall => {
            wall_placement::autofill_cells(
                sim,
                rules,
                registry,
                obj,
                (rx, ry),
                owner_id,
                overlay_id,
                native_cells.as_ref(),
            )
        }
        _ => Vec::new(),
    };
    // Wall filler is derived by the same production/terrain owners, refreshed
    // with the active preview. Rendering needs no cross-owner raw Cell access.
    let wall_autofill_cells = native_cells.as_ref().map_or_else(Vec::new, |query| {
        wall_coords
            .into_iter()
            .map(|coord| {
                let (level, slope) =
                    query.ground_fields(query.lookup((coord.0 as i16, coord.1 as i16)));
                BuildingPlacementCell {
                    coord,
                    valid: reason.is_none(),
                    level: level as i8,
                    slope,
                }
            })
            .collect()
    });
    Some(BuildingPlacementPreview {
        rx,
        ry,
        width,
        height,
        valid: reason.is_none(),
        in_build_area,
        foundation_visible,
        reason,
        cells,
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
    (!candidates.is_empty()).then_some(ProducerFocusView { category })
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

    let next_sid = match candidates
        .iter()
        .position(|candidate| sim.production.is_primary_factory(candidate.0))
    {
        Some(index) => candidates[(index + 1) % candidates.len()].0,
        None => candidates[0].0,
    };
    // Choosing the already-primary sole candidate keeps the VERA-only cycle
    // a no-op. Every actual change goes through the native manual owner.
    sim.production.is_primary_factory(next_sid)
        || super::can_build::toggle_factory_primary(sim, rules, next_sid)
}

/// One HouseClass::Place_Production4FB0E0 owner. A building PLACE uses
/// its clicked type/cell; mobile type_index=-1 resolves the current category
/// head when the queued event executes, not the object held when Strip issued it.
pub(crate) fn place_production_with_overlays(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    request: ProductionPlacement<'_>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    // House4FB197..4FB1BE sends the CRT-initialized empty cell (0,0) and
    // CellStruct(-1,-1) through the same producer exit as a mobile PLACE.
    // They never enter tactical Unlimbo or its retry/EVA tail. Source:
    // cameo_order.md, ordinary early-cell executed controls.
    let request = match request {
        ProductionPlacement::Building {
            type_id,
            cell: (0, 0) | (u16::MAX, u16::MAX),
        } => {
            let Some(object_type) = rules.object(type_id) else {
                return false;
            };
            if object_type.category != ObjectCategory::Building {
                return false;
            }
            ProductionPlacement::Mobile {
                category: production_category_for_object(object_type),
            }
        }
        request => request,
    };
    let (type_id, (rx, ry)) = match request {
        ProductionPlacement::Building { type_id, cell } => (type_id, cell),
        ProductionPlacement::Mobile { category } => {
            use crate::sim::ai_base_building::BuildingExit;
            let Some(owner_id) = sim.interner.get(owner) else {
                return false;
            };
            let Some(object) = sim
                .production
                .factories()
                .view(owner_id, category)
                .and_then(|factory| factory.complete_object())
            else {
                return false;
            };
            let Some(entity_id) = object.entity_id else {
                return false;
            };
            let Some(obj) = sim.object_type(object.type_id, rules) else {
                return false;
            };
            if production_category_for_object(obj) != category {
                return false;
            }
            let is_building = obj.category == ObjectCategory::Building;
            // Native wrapper5F5C20 -> TypeFindFactory5F7900 with (0,1); an
            // aircraft (`What_Am_I` 2) alone retries with the busy-airfield
            // gate skipped (1,1) (0x004FB524..0x004FB54C). A null producer
            // leaves the head and queue untouched.
            let producer =
                super::object_find_factory(sim, rules, entity_id, false, true).or_else(|| {
                    (obj.category == ObjectCategory::Aircraft)
                        .then(|| super::object_find_factory(sim, rules, entity_id, true, true))
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
                effects,
            );
            let accepted = exit == BuildingExit::Placed
                || exit == BuildingExit::TryLater
                    && sim
                        .production
                        .factories()
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
                if sim.owner_is_human(owner_id)
                    && let Some(factory_entity) = sim.substrate.entities.get(producer)
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
                super::factory_lifecycle::release_delivered_mobile(
                    sim, rules, owner_id, category, effects,
                );
            } else if is_building {
                // House4FB5BA returns immediately for a refused building
                // exit (RTTI6). The completed product stays paid and held;
                // only refused mobile products enter the refund tail.
                return false;
            } else {
                // Exit0 or Exit1 with producer+524zero: refund the actually
                // charged complete object, destroy it and start the queued head.
                super::factory_lifecycle::refund_failed_delivery(
                    sim, rules, owner_id, category, effects,
                );
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
    // The type's factory (Defenses for `BuildCat=Combat`, else Buildings)
    // must hold its completed object.
    let category = production_category_for_object(obj);
    let Some(held) = super::factory_lifecycle::ready_object(sim, owner_id, category, type_interned)
    else {
        return false;
    };
    // Original PLACE4FB1DA keeps Object::FindFactory(0,0)'s yard through
    // HELLO, Unlimbo (including the wall arm), the child's C message and the
    // yard's FirstContact BREAK. The House factory owns the completed object.
    let Some(yard) = super::object_find_factory(sim, rules, held.entity_id(), false, false) else {
        return false;
    };
    crate::sim::radio::transmit(
        sim,
        yard,
        held.entity_id(),
        crate::sim::radio::RadioMessage::Hello,
        crate::sim::radio::RadioPayload::default(),
        Some(rules),
        effects,
    );
    if obj.wall {
        if evaluate_building_placement(sim, rules, owner, type_id, rx, ry, overlay_registry, None)
            .reason
            .is_some()
        {
            failed_building_unlimbo(sim, rules, owner_id, yard, effects);
            return false;
        }
        let Some(registry) = overlay_registry else {
            failed_building_unlimbo(sim, rules, owner_id, yard, effects);
            return false;
        };
        //4FB1F7..4FB22E forms centered XY and literal Z0 for +D8. The
        //shared Building wall arm owns sale/Overlay/radio/sight/UnInit.
        if sim
            .reveal_constructed_object_at_coord_with_overlay_context(
                held.entity_id(),
                crate::sim::components::DriveCoord {
                    x: i32::from(rx) * 256 + 128,
                    y: i32::from(ry) * 256 + 128,
                    z: 0,
                },
                0,
                crate::sim::world::PlacementEvidence::EvaluateMark,
                rules,
                Some(registry),
                effects,
            )
            .is_none()
        {
            failed_building_unlimbo(sim, rules, owner_id, yard, effects);
            return false;
        }
        wall_placement::autofill_walls(sim, rules, registry, obj, (rx, ry), owner_id, effects);
        // UnInit already expired Factory's pointer; keep the admission token
        // captured before entry. Physical deletion belongs to725C70.
        let object = held.release_consumed_wall(sim, rules, effects);
        sim.sound_events
            .push(crate::sim::world::SimSoundEvent::BuildingPlaced { owner: owner_id });
        sim.lifecycle_outputs.push(
            crate::sim::world::LifecycleOutput::BuildingPlacementSucceeded { owner: owner_id },
        );
        crate::sim::radio::transmit_to_contact(
            sim,
            yard,
            crate::sim::radio::RadioMessage::Break,
            Some(rules),
            effects,
        );
        // A consumed wall skips selection, but still reaches BuildingSlam
        // (4FB2E6 ->4FB2FD..4FB314) and BREAK before Record4FB4B7.
        super::factory_lifecycle::record_last_built(sim, rules, owner_id, object);
        return true;
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
    // Event::Execute4C70E1..4C710B passes an admitted PLACE straight to
    // House4FB0E0. Its placement refusal occurs after HELLO4FB1F1, and
    // still reaches the yard's FirstContact BREAK4FB4A6.
    if evaluate_building_placement(sim, rules, owner, type_id, rx, ry, overlay_registry, None)
        .reason
        .is_some()
    {
        failed_building_unlimbo(sim, rules, owner_id, yard, effects);
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
        effects,
    ) else {
        // The failed placement reaches the same4FB4A6 FirstContact teardown;
        // its retained limbo object remains in the house factory for retry.
        failed_building_unlimbo(sim, rules, owner_id, yard, effects);
        return false;
    };
    // `0x004452FA`/`0x004FB252`: a placed building's slave manager (a Slave
    // Miner refinery's) takes the hand-off once its Unlimbo succeeds.
    sim.slave_manager_hand_off(new_sid, rules, effects);
    // Log screen position for debugging placement alignment.
    if let Some(ge) = sim.substrate.entities.get(new_sid) {
        let (fw, fh) = foundation_dimensions(&foundation_str);
        // The entity anchor — the projection of the building's own coordinate,
        // which is its north-west cell's diamond centre. The drawn art sits half
        // a tile above this (render's building render-coordinate lift); this is
        // a debug log, and sim/ cannot call render/ to apply it.
        let screen = crate::util::lepton::lepton_to_screen(
            ge.position.rx(),
            ge.position.ry(),
            ge.position.sub_x(),
            ge.position.sub_y(),
            ge.position.z(),
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
    // The placed building's Unlimbo queued Construction
    // (`Simulation::building_enter_idle_mode`); the HELLO above kept its
    // cell-put discovery from opening it. A computer house's yard places its
    // own through `sim::ai_base_building::exit_building`.
    // Shared Building Unlimbo requested the owner's native1FC House tail.

    held.release_after_placement(sim, rules, effects);
    crate::sim::radio::transmit(
        sim,
        new_sid,
        yard,
        crate::sim::radio::RadioMessage::DockArrived,
        crate::sim::radio::RadioPayload::default(),
        Some(rules),
        effects,
    );
    // `0x004FB2CC..0x004FB314`: BuildingSlam follows every successful
    // Unlimbo, before the yard's FirstContact BREAK, whatever the release.
    sim.sound_events
        .push(crate::sim::world::SimSoundEvent::BuildingPlaced { owner: owner_id });
    sim.lifecycle_outputs
        .push(crate::sim::world::LifecycleOutput::BuildingPlacementSucceeded { owner: owner_id });
    crate::sim::radio::transmit_to_contact(
        sim,
        yard,
        crate::sim::radio::RadioMessage::Break,
        Some(rules),
        effects,
    );
    // Original4FB4B7 records the successful build after FirstContact BREAK,
    // not during CompletedProduction4FB2A1 or its C message4FB2AD.
    let object = super::factory_lifecycle::BuiltObject::capture(sim, new_sid);
    super::factory_lifecycle::record_last_built(sim, rules, owner_id, object);
    true
}

/// The single failed-Unlimbo tail, HousePlace4FB35F..4FB4A6. Early null
/// factory/product/builder guards never enter it. The local-client half of
/// retry and EVA is consumed by existing app output owners.
/// Source: tools/sidebar_oracle/cameo_order.md, ready-building histories.
fn failed_building_unlimbo(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    yard: u64,
    effects: FrameEffects<'_>,
) {
    sim.sound_events
        .push(crate::sim::world::SimSoundEvent::CannotDeployHere { owner });
    sim.lifecycle_outputs
        .push(crate::sim::world::LifecycleOutput::BuildingPlacementRetry { owner });
    crate::sim::radio::transmit_to_contact(
        sim,
        yard,
        crate::sim::radio::RadioMessage::Break,
        Some(rules),
        effects,
    );
}

/// The admission result and the independent Display+1180 proximity gate,
/// derived together by the production owner for preview and PLACE consumers.
struct BuildingPlacementEvaluation {
    reason: Option<BuildingPlacementError>,
    in_build_area: bool,
}

fn evaluate_building_placement(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
    overlay_registry: Option<&OverlayTypeRegistry>,
    native_cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> BuildingPlacementEvaluation {
    let obj = rules.object(type_id);
    let in_build_area = obj.is_some_and(|obj| is_within_build_area(sim, rules, owner, obj, rx, ry));
    let reason = (|| {
        let Some(obj) = obj else {
            return Err(BuildingPlacementError::NotBuilding);
        };
        let (width, height) = foundation_dimensions(&obj.foundation);
        if obj.category != ObjectCategory::Building {
            return Err(BuildingPlacementError::NotBuilding);
        }
        let owner_id = sim.interner.get(owner);
        let ready = owner_id
            .zip(sim.interner.get(type_id))
            .is_some_and(|(owner_id, type_id)| {
                let category = production_category_for_object(obj);
                super::factory_lifecycle::ready_object(sim, owner_id, category, type_id).is_some()
            });
        if !ready {
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
            native_cells,
        ) {
            // The error variant only labels the refusal for presentation.
            let overlaps_structure =
                crate::rules::foundation::foundation_cell_offsets(&obj.foundation)
                    .into_iter()
                    .any(|(dx, dy)| {
                        sim.cell_objects(
                            (rx.wrapping_add(dx as u16), ry.wrapping_add(dy as u16)),
                            MovementLayer::Ground,
                        )
                        .any(|member| {
                            matches!(member, crate::sim::occupancy::CellObjectMember::Entity(id)
                            if sim.substrate.entities.get(id).is_some_and(|e| {
                                e.category() == EntityCategory::Structure
                            }))
                        })
                    });
            return Err(if overlaps_structure {
                BuildingPlacementError::OverlapsStructure
            } else {
                BuildingPlacementError::BlockedTerrain
            });
        }
        if in_build_area {
            Ok(())
        } else {
            let providers: Vec<String> = sim
                .substrate
                .entities
                .values()
                .filter(|e| {
                    e.category() == EntityCategory::Structure
                        && sim.interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
                })
                .map(|e| {
                    let type_str = sim.interner.resolve(e.type_ref());
                    let bn = rules.object(type_str).is_some_and(|o| o.base_normal);
                    format!(
                        "{}@({},{}) bn={}",
                        type_str,
                        e.position.rx(),
                        e.position.ry(),
                        bn
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
    })()
    .err();
    BuildingPlacementEvaluation {
        reason,
        in_build_area,
    }
}

/// Read-only Display4A8EB0 proximity query, shared by preview and the fresh
/// LeftUp event gate. Production owns provider lifecycle and adjacency policy.
pub fn is_within_build_area(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    obj: &ObjectType,
    rx: u16,
    ry: u16,
) -> bool {
    #[cfg(feature = "debug-tools")]
    if sim
        .interner
        .get(owner)
        .is_some_and(|id| sim.debug_flags_for(id).build_anywhere)
    {
        return true;
    }
    let (width, height) = foundation_dimensions(&obj.foundation);
    let placed_adjacent = obj.adjacent;
    if placed_adjacent < 0 {
        return false;
    }
    for e in sim.substrate.entities.values() {
        // A dying structure provides no build-area adjacency during its window.
        if e.death.is_dying() || e.lifecycle.in_limbo() {
            continue;
        }
        if e.category() != EntityCategory::Structure {
            continue;
        }
        if !e.unit_delivery_provides_base_space() {
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
        } else if !(sim.session.game_options().build_off_ally
            && existing.eligibile_for_ally_building
            && are_houses_friendly(&sim.house_alliances, provider_owner, owner))
        {
            continue;
        }
        let (provider_width, provider_height) = foundation_dimensions(&existing.foundation);
        if provider_intersects_build_area_ring(
            (rx, ry, width, height),
            (
                e.position.rx(),
                e.position.ry(),
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
