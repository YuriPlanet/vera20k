//! Building placement validation, sell/repair, and producer focus management.
//!
//! Handles placement preview, build area checks, sell refunds with crew ejection,
//! repair tick, and producer cycling.

use std::collections::BTreeMap;

use crate::map::bridge_facts::BRIDGE_FLAG_DESTROYED_OR_RAMP;
use crate::map::entities::EntityCategory;
use crate::map::houses::are_houses_friendly;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::components::BuildingUp;
use crate::sim::entity_store::EntityStore;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding;
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
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    height_map: &BTreeMap<(u16, u16), u8>,
) -> Option<BuildingPlacementPreview> {
    placement_preview_for_owner_with_overlays(
        sim, rules, owner, type_id, rx, ry, path_grid, height_map, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn placement_preview_for_owner_with_overlays(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    height_map: &BTreeMap<(u16, u16), u8>,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<BuildingPlacementPreview> {
    let obj = rules.object(type_id)?;
    let (width, height) = foundation_dimensions(&obj.foundation);
    let reason = evaluate_building_placement(
        sim,
        rules,
        owner,
        type_id,
        rx,
        ry,
        path_grid,
        height_map,
        overlay_registry,
    )
    .err();
    let in_build_area = reason.as_ref().map_or(true, |r| {
        !matches!(r, BuildingPlacementError::OutOfBuildArea)
    });
    let owner_id = sim.interner.get(owner);
    let wall_overlay_id = overlay_registry.and_then(|registry| {
        obj.wall
            .then(|| wall_placement::linked_overlay_id(obj, registry))
            .flatten()
    });
    let mut cell_valid: Vec<bool> = Vec::with_capacity((width as usize) * (height as usize));
    for dy in 0..height {
        for dx in 0..width {
            let cx: u16 = rx.saturating_add(dx);
            let cy: u16 = ry.saturating_add(dy);
            let ok = if obj.wall {
                match (owner_id, wall_overlay_id, overlay_registry) {
                    (Some(owner_id), Some(overlay_id), Some(registry)) => wall_cell_placeable(
                        sim, rules, obj, path_grid, cx, cy, owner_id, overlay_id, registry,
                    ),
                    _ => false,
                }
            } else {
                can_this_exist_here(sim, &sim.substrate.entities, rules, obj, path_grid, cx, cy)
            };
            cell_valid.push(in_build_area && ok);
        }
    }
    let wall_autofill_cells = match (owner_id, wall_overlay_id, overlay_registry) {
        (Some(owner_id), Some(overlay_id), Some(_)) if obj.wall => wall_placement::autofill_cells(
            sim,
            rules,
            obj,
            path_grid,
            (rx, ry),
            owner_id,
            overlay_id,
        ),
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
    let active_sid = owner_id
        .and_then(|id| sim.production.active_producer_by_owner.get(&id))
        .and_then(|categories| categories.get(&category))
        .copied();
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

pub fn toggle_pause_for_owner_category(
    sim: &mut Simulation,
    owner: &str,
    category: ProductionCategory,
) -> bool {
    let owner_id = sim.interner.intern(owner);
    // P5d: pause is a registry flag on the active build (the retired `front.state` Paused
    // bridge). `step_all` skips a `manual` factory without losing progress; unpausing
    // auto-resumes via `set_rate`.
    sim.production
        .factory_shadow
        .toggle_pause(owner_id, category)
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
    let current = sim
        .production
        .active_producer_by_owner
        .get(&owner_id)
        .and_then(|categories| categories.get(&category))
        .copied();
    let next_sid =
        match current.and_then(|sid| candidates.iter().position(|candidate| candidate.0 == sid)) {
            Some(index) => candidates[(index + 1) % candidates.len()].0,
            None => candidates[0].0,
        };
    sim.production
        .active_producer_by_owner
        .entry(owner_id)
        .or_default()
        .insert(category, next_sid);
    true
}

/// Place a ready non-overlay building.
///
/// Wall callers must use `place_ready_building_with_overlays` so the
/// authoritative overlay registry cannot be discarded implicitly.
pub fn place_ready_building_without_overlays(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    height_map: &BTreeMap<(u16, u16), u8>,
) -> bool {
    place_ready_building_with_overlays(
        sim, rules, owner, type_id, rx, ry, path_grid, height_map, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn place_ready_building_with_overlays(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    height_map: &BTreeMap<(u16, u16), u8>,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
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
    if evaluate_building_placement(
        sim,
        rules,
        owner,
        type_id,
        rx,
        ry,
        path_grid,
        height_map,
        overlay_registry,
    )
    .is_err()
    {
        return false;
    }
    if obj.wall {
        let category = production_category_for_object(obj);
        let Some(held) =
            super::factory_lifecycle::ready_object(sim, owner_id, category, type_interned)
        else {
            return false;
        };
        let Some(registry) = overlay_registry else {
            return false;
        };
        let Some(overlay_id) = wall_placement::linked_overlay_id(obj, registry) else {
            return false;
        };
        if !wall_placement::stamp_wall(sim, registry, rx, ry, overlay_id, owner_id) {
            return false;
        }
        for direction in wall_placement::CARDINAL_DIRECTIONS {
            let gap = wall_placement::scan_autofill_direction(
                sim,
                rules,
                obj,
                path_grid,
                (rx, ry),
                owner_id,
                overlay_id,
                direction,
            );
            for (fill_rx, fill_ry) in gap {
                let stamped = wall_placement::stamp_wall(
                    sim, registry, fill_rx, fill_ry, overlay_id, owner_id,
                );
                debug_assert!(stamped, "scanned wall filler must remain stampable");
            }
        }
        // Wall placement consumes the factory-created BuildingClass into
        // overlay state; the constructor identity is destroyed, never
        // reconstructed at placement.
        return held.consume_after_wall_stamp(sim, rules);
    }
    let foundation_str: String = rules
        .object(type_id)
        .map(|o| o.foundation.clone())
        .unwrap_or_else(|| "?".to_string());
    let z: u8 = height_map.get(&(rx, ry)).copied().unwrap_or(0);
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
    let Some(new_sid) = sim.unlimbo_held_production_object(
        held.entity_id(),
        rx,
        ry,
        0,
        z,
        crate::sim::world::PlacementEvidence::EvaluateMark,
        rules,
    ) else {
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
    // Tag newly placed buildings with build-up animation (~1 second at 30Hz).
    if let Some(ge) = sim.substrate.entities.get_mut(new_sid) {
        ge.building_up = Some(BuildingUp {
            elapsed_ticks: 0,
            total_ticks: 30,
        });
    }
    // Refresh superweapon grants — newly placed building may provide a SW.
    if sim.session.game_options.super_weapons {
        crate::sim::superweapon::refresh_super_weapons_for_owner(sim, rules, owner_id);
    }

    held.release_after_placement(sim, rules)
}

fn evaluate_building_placement(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    rx: u16,
    ry: u16,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    _height_map: &BTreeMap<(u16, u16), u8>,
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
    let wall_overlay_id = if obj.wall {
        let Some(registry) = overlay_registry else {
            return Err(BuildingPlacementError::BlockedTerrain);
        };
        let Some(overlay_id) = wall_placement::linked_overlay_id(obj, registry) else {
            return Err(BuildingPlacementError::BlockedTerrain);
        };
        Some(overlay_id)
    } else {
        None
    };
    for dy in 0..height {
        for dx in 0..width {
            let cell_x = rx.saturating_add(dx);
            let cell_y = ry.saturating_add(dy);
            let placeable = match (wall_overlay_id, overlay_registry, owner_id) {
                (Some(overlay_id), Some(registry), Some(owner_id)) => wall_cell_placeable(
                    sim, rules, obj, path_grid, cell_x, cell_y, owner_id, overlay_id, registry,
                ),
                (Some(_), _, _) => false,
                (None, _, _) => can_this_exist_here(
                    sim,
                    &sim.substrate.entities,
                    rules,
                    obj,
                    path_grid,
                    cell_x,
                    cell_y,
                ),
            };
            if !placeable {
                // Distinguish overlap from terrain for the error variant.
                if structure_occupies_cell(
                    &sim.substrate.entities,
                    rules,
                    cell_x,
                    cell_y,
                    &sim.interner,
                ) {
                    return Err(BuildingPlacementError::OverlapsStructure);
                }
                return Err(BuildingPlacementError::BlockedTerrain);
            }
        }
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

/// Per-cell placement check shared by preview and validation.
///
/// When `water_bound` is true (naval yards), the cell MUST be ship-passable
/// water terrain, not merely `is_water=true`. Shore/beach cells can look watery
/// but are not valid for `MovementZone::Water` ships, which would trap produced
/// destroyers/cruisers while still allowing amphibious craft to move.
///
/// Normal walkability/build_blocked checks are skipped for WaterBound buildings
/// because water cells are intentionally blocked in those generic land-building
/// paths. Instead, we validate against the ship passability matrix plus static
/// overlay/terrain blockers.
#[allow(clippy::too_many_arguments)]
fn wall_cell_placeable(
    sim: &Simulation,
    rules: &RuleSet,
    object_type: &ObjectType,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    cx: u16,
    cy: u16,
    owner: crate::sim::intern::InternedId,
    overlay_id: u8,
    registry: &OverlayTypeRegistry,
) -> bool {
    if !registry.flags(overlay_id).is_some_and(|flags| flags.wall) {
        return false;
    }
    let Some(grid) = sim.overlay_grid.as_ref() else {
        return false;
    };
    if cx >= grid.width() || cy >= grid.height() {
        return false;
    }
    let existing = *grid.cell(cx, cy);
    if existing.overlay_id.is_none() {
        return can_this_exist_here(
            sim,
            &sim.substrate.entities,
            rules,
            object_type,
            path_grid,
            cx,
            cy,
        );
    }
    if existing.overlay_id != Some(overlay_id)
        || existing.wall_owner != Some(owner)
        || existing.overlay_data <= 0x0F
    {
        return false;
    }

    if structure_occupies_cell(&sim.substrate.entities, rules, cx, cy, &sim.interner)
        || ground_non_structure_occupies_cell(sim, cx, cy)
    {
        return false;
    }
    if let Some(terrain) = sim.resolved_terrain.as_ref() {
        return terrain.cell(cx, cy).is_some_and(|cell| {
            !cell.base_build_blocked
                && !cell.terrain_object_blocks
                && !cell.has_bridge_deck
                && !cell.bridge_walkable
                && !cell.bridge_facts.has_flag(BRIDGE_FLAG_DESTROYED_OR_RAMP)
                && cell.slope_type == 0
        });
    }
    path_grid.map_or(true, |grid| cx < grid.width() && cy < grid.height())
}

/// Live per-cell `CellClass::CanThisExistHere` projection used by both preview
/// and committed production placement.
// Native: CellClass::CanThisExistHere @ YR 0x0047C1D0. The available runtime
// inputs cover normal-list blockers, overlay absence, terrain/buildability,
// bridge/ramp/slope rejection, and the type SpeedType/WaterBound projection.
// The executable's editor/global bypass and unparsed +0xE58 exception remain
// explicit residuals; neither has a represented live input in this runtime.
pub(super) fn can_this_exist_here(
    sim: &Simulation,
    entities: &EntityStore,
    rules: &RuleSet,
    object_type: &ObjectType,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    cx: u16,
    cy: u16,
) -> bool {
    let no_overlap = !structure_occupies_cell(entities, rules, cx, cy, &sim.interner)
        && !ground_non_structure_occupies_cell(sim, cx, cy);
    let no_overlay = sim
        .overlay_grid
        .as_ref()
        .map_or(true, |grid| grid.cell(cx, cy).overlay_id.is_none());

    if object_type.water_bound {
        let cell_ok = if let Some(terrain) = sim.resolved_terrain.as_ref() {
            terrain.cell(cx, cy).is_some_and(|cell| {
                let ship_passable = pathfinding::passability::is_passable_for_zone(
                    cell.zone_type,
                    MovementZone::Water,
                );
                ship_passable
                    && !cell.overlay_blocks
                    && !cell.terrain_object_blocks
                    && !cell.has_bridge_deck
                    && !cell.bridge_walkable
                    && !cell.bridge_facts.has_flag(BRIDGE_FLAG_DESTROYED_OR_RAMP)
            })
        } else {
            path_grid.is_some_and(|grid| {
                pathfinding::is_cell_passable_for_mover(
                    grid,
                    cx,
                    cy,
                    Some(MovementZone::Water),
                    None,
                )
            })
        };
        cell_ok && no_overlap && no_overlay
    } else {
        let cell_ok = if let Some(terrain) = sim.resolved_terrain.as_ref() {
            terrain.cell(cx, cy).is_some_and(|cell| {
                !cell.build_blocked
                    && !cell.overlay_blocks
                    && !cell.terrain_object_blocks
                    && !cell.has_bridge_deck
                    && !cell.bridge_walkable
                    && !cell.bridge_facts.has_flag(BRIDGE_FLAG_DESTROYED_OR_RAMP)
                    && cell.slope_type == 0
            })
        } else {
            let walkable = path_grid.map_or(true, |g| g.is_walkable(cx, cy));
            let not_blocked = !sim.effective_build_blocked(cx, cy).unwrap_or(false);
            walkable && not_blocked
        };
        cell_ok && no_overlap && no_overlay
    }
}

fn ground_non_structure_occupies_cell(sim: &Simulation, rx: u16, ry: u16) -> bool {
    sim.substrate.occupancy.get(rx, ry).is_some_and(|cell| {
        cell.iter_layer(MovementLayer::Ground).any(|occupant| {
            match sim.substrate.entities.get(occupant.entity_id) {
                Some(entity) => entity.category != EntityCategory::Structure,
                None => {
                    debug_assert!(
                        false,
                        "occupancy cell ({rx},{ry}) references missing entity {}",
                        occupant.entity_id
                    );
                    true
                }
            }
        })
    })
}

pub(super) fn structure_occupies_cell(
    entities: &EntityStore,
    rules: &RuleSet,
    rx: u16,
    ry: u16,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    entities.values().any(|e| {
        // A dying structure is unmarked from cell lists synchronously in uninit;
        // it must not block placement during its deferred-delete window.
        if e.dying || e.lifecycle.in_limbo {
            return false;
        }
        if e.category != EntityCategory::Structure {
            return false;
        }
        let Some(existing) = rules.object(interner.resolve(e.type_ref())) else {
            return false;
        };
        // Wall entities render and behave as overlays — they don't block building
        // placement of other structures. A wall cell is only blocked to another wall
        // of the same type, which is handled by the overlay list, not the entity store.
        if existing.wall {
            return false;
        }
        let (width, height) = foundation_dimensions(&existing.foundation);
        rx >= e.position.rx
            && rx < e.position.rx.saturating_add(width)
            && ry >= e.position.ry
            && ry < e.position.ry.saturating_add(height)
    })
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
