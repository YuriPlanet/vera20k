//! Ignored retail production-load oracle for stock building placement.
//!
//! This test stays headless: it loads sealed retail rules, art, theater, and a
//! real MMX map, then drives the ordinary placement command through the sim.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::load_rules_with_merged_ini;
use crate::assets::asset_manager::AssetManager;
use crate::map::entities::EntityCategory;
use crate::map::map_file::{self, MapFile};
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::theater;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::power_system::tick_power_states;
use crate::sim::production::{
    foundation_dimensions, placement_preview_for_owner_without_overlays, ready_buildings_for_owner,
};
use crate::sim::world::Simulation;

const DUSTBOWL_FILE: &str = "Dustbowl.mmx";
const DUSTBOWL_SIZE: u64 = 125_288;
const DUSTBOWL_CRC32: u32 = 0x75B7_3654;
const OWNER: &str = "Americans";
const CONYARD: &str = "GACNST";
const POWER_PLANT: &str = "GAPOWR";

#[derive(Debug, Clone, Copy)]
struct PlacementFixture {
    provider: (u16, u16),
    blocked: (u16, u16),
    valid: (u16, u16),
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn rect_cells(origin: (u16, u16), width: u16, height: u16) -> Vec<(u16, u16)> {
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for dy in 0..height {
        for dx in 0..width {
            cells.push((
                origin.0.checked_add(dx).expect("fixture x fits u16"),
                origin.1.checked_add(dy).expect("fixture y fits u16"),
            ));
        }
    }
    cells
}

fn ordinary_surface_cell(resolved: &ResolvedTerrainGrid, rx: u16, ry: u16) -> bool {
    resolved.cell(rx, ry).is_some_and(|cell| {
        !cell.build_blocked
            && !cell.overlay_blocks
            && !cell.terrain_object_blocks
            && !cell.has_bridge_deck
            && !cell.bridge_walkable
            && !cell.bridge_facts.has_structural_bridge()
            && cell.slope_type == 0
    })
}

fn rect_is_clear(
    resolved: &ResolvedTerrainGrid,
    overlays: &OverlayGrid,
    map_entity_cells: &BTreeSet<(u16, u16)>,
    origin: (u16, u16),
    width: u16,
    height: u16,
) -> bool {
    rect_cells(origin, width, height).into_iter().all(|cell| {
        ordinary_surface_cell(resolved, cell.0, cell.1)
            && overlays.cell(cell.0, cell.1).overlay_id.is_none()
            && !map_entity_cells.contains(&cell)
    })
}

fn rect_is_nonblocking_tiberium(
    resolved: &ResolvedTerrainGrid,
    overlays: &OverlayGrid,
    registry: &OverlayTypeRegistry,
    map_entity_cells: &BTreeSet<(u16, u16)>,
    origin: (u16, u16),
) -> bool {
    let mut saw_tiberium = false;
    for cell in rect_cells(origin, 2, 2) {
        if !ordinary_surface_cell(resolved, cell.0, cell.1) || map_entity_cells.contains(&cell) {
            return false;
        }
        let Some(overlay_id) = overlays.cell(cell.0, cell.1).overlay_id else {
            continue;
        };
        if !registry
            .flags(overlay_id)
            .is_some_and(|flags| flags.tiberium)
        {
            return false;
        }
        saw_tiberium = true;
    }
    saw_tiberium
}

fn rects_intersect(left: (u16, u16, u16, u16), right: (u16, u16, u16, u16)) -> bool {
    let left_max_x = u32::from(left.0) + u32::from(left.2) - 1;
    let left_max_y = u32::from(left.1) + u32::from(left.3) - 1;
    let right_max_x = u32::from(right.0) + u32::from(right.2) - 1;
    let right_max_y = u32::from(right.1) + u32::from(right.3) - 1;
    u32::from(left.0) <= right_max_x
        && left_max_x >= u32::from(right.0)
        && u32::from(left.1) <= right_max_y
        && left_max_y >= u32::from(right.1)
}

fn provider_intersects_ring(
    placed: (u16, u16, u16, u16),
    provider: (u16, u16, u16, u16),
    adjacent: i32,
) -> bool {
    let expansion = adjacent.saturating_add(1);
    let placed_min_x = i32::from(placed.0);
    let placed_min_y = i32::from(placed.1);
    let placed_max_x = placed_min_x + i32::from(placed.2) - 1;
    let placed_max_y = placed_min_y + i32::from(placed.3) - 1;
    let provider_min_x = i32::from(provider.0);
    let provider_min_y = i32::from(provider.1);
    let provider_max_x = provider_min_x + i32::from(provider.2) - 1;
    let provider_max_y = provider_min_y + i32::from(provider.3) - 1;

    provider_min_x <= placed_max_x + expansion
        && provider_max_x >= placed_min_x - expansion
        && provider_min_y <= placed_max_y + expansion
        && provider_max_y >= placed_min_y - expansion
        && !rects_intersect(placed, provider)
}

fn bounded_origins(
    center: (u16, u16),
    radius: u16,
    footprint: (u16, u16),
    grid: (u16, u16),
) -> Vec<(u16, u16)> {
    let max_x = grid.0.saturating_sub(footprint.0);
    let max_y = grid.1.saturating_sub(footprint.1);
    let start_x = center.0.saturating_sub(radius);
    let start_y = center.1.saturating_sub(radius);
    let end_x = center.0.saturating_add(radius).min(max_x);
    let end_y = center.1.saturating_add(radius).min(max_y);
    let mut origins = Vec::new();
    for ry in start_y..=end_y {
        for rx in start_x..=end_x {
            origins.push((rx, ry));
        }
    }
    origins
}

fn find_fixture(
    map: &MapFile,
    resolved: &ResolvedTerrainGrid,
    overlays: &OverlayGrid,
    registry: &OverlayTypeRegistry,
    adjacent: i32,
) -> Option<PlacementFixture> {
    let grid = (resolved.width(), resolved.height());
    let map_entity_cells: BTreeSet<(u16, u16)> = map
        .entities
        .iter()
        .map(|entity| (entity.cell_x, entity.cell_y))
        .collect();
    let mut ore_cells: Vec<(u16, u16)> = map
        .overlays
        .iter()
        .filter(|entry| {
            registry
                .flags(entry.overlay_id)
                .is_some_and(|flags| flags.tiberium)
        })
        .map(|entry| (entry.rx, entry.ry))
        .collect();
    ore_cells.sort_by_key(|&(rx, ry)| (ry, rx));
    ore_cells.dedup();

    for ore in ore_cells {
        for offset_y in 0..2u16 {
            for offset_x in 0..2u16 {
                let Some(blocked_x) = ore.0.checked_sub(offset_x) else {
                    continue;
                };
                let Some(blocked_y) = ore.1.checked_sub(offset_y) else {
                    continue;
                };
                let blocked = (blocked_x, blocked_y);
                if !rect_is_nonblocking_tiberium(
                    resolved,
                    overlays,
                    registry,
                    &map_entity_cells,
                    blocked,
                ) {
                    continue;
                }

                for provider in bounded_origins(blocked, 10, (4, 4), grid) {
                    if !rect_is_clear(resolved, overlays, &map_entity_cells, provider, 4, 4)
                        || !provider_intersects_ring(
                            (blocked.0, blocked.1, 2, 2),
                            (provider.0, provider.1, 4, 4),
                            adjacent,
                        )
                    {
                        continue;
                    }

                    for valid in bounded_origins(provider, 10, (2, 2), grid) {
                        if valid == blocked
                            || !rect_is_clear(resolved, overlays, &map_entity_cells, valid, 2, 2)
                            || !provider_intersects_ring(
                                (valid.0, valid.1, 2, 2),
                                (provider.0, provider.1, 4, 4),
                                adjacent,
                            )
                        {
                            continue;
                        }
                        return Some(PlacementFixture {
                            provider,
                            blocked,
                            valid,
                        });
                    }
                }
            }
        }
    }
    None
}

#[test]
#[ignore = "requires the sealed retail RA2/YR install and Dustbowl.mmx"]
fn retail_dustbowl_gapowr_blocked_then_valid_placement_oracle() {
    let retail_dir = PathBuf::from(
        std::env::var("RA2_DIR").expect("RA2_DIR must name the sealed retail RA2/YR install"),
    );
    let map_path = retail_dir.join(DUSTBOWL_FILE);
    let map_bytes = std::fs::read(&map_path).expect("read sealed Dustbowl.mmx");
    assert_eq!(map_bytes.len() as u64, DUSTBOWL_SIZE);
    assert_eq!(crc32(&map_bytes), DUSTBOWL_CRC32);

    let map = map_file::load_from_path(&map_path).expect("parse sealed Dustbowl.mmx");
    assert_eq!(map.header.theater, "TEMPERATE");
    assert_eq!((map.header.width, map.header.height), (70, 76));

    let mut assets = AssetManager::new(&retail_dir).expect("open retail MIX archives");
    let theater =
        theater::load_theater(&mut assets, &map.header.theater).expect("load retail theater");
    let (mut rules, rules_ini, _native_type_construction_trace, art_ini) =
        load_rules_with_merged_ini(&assets, None, Some(&map.ini))
            .expect("load production merged rules")
            .into_parts();
    let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    rules.merge_art_data(&art);
    let overlay_registry = OverlayTypeRegistry::from_ini(&rules_ini, Some(&art_ini));

    let conyard = rules.object(CONYARD).expect("stock GACNST");
    let gapowr = rules.object(POWER_PLANT).expect("stock GAPOWR");
    assert_eq!(foundation_dimensions(&conyard.foundation), (4, 4));
    assert!(conyard.base_normal);
    assert_eq!(foundation_dimensions(&gapowr.foundation), (2, 2));
    assert_eq!(gapowr.adjacent, 2);
    assert_eq!(gapowr.strength, 750);
    assert_eq!(gapowr.power, 200);

    let resolved = ResolvedTerrainGrid::build(
        &map,
        Some(&theater),
        Some(&assets),
        Some(&rules.terrain_rules),
        Some(&overlay_registry),
        false,
        rules.general.cliff_back_impassability,
    );
    let height_map = resolved.build_height_map();
    let path_grid = PathGrid::from_resolved_terrain(&resolved);
    let overlay_grid =
        OverlayGrid::from_overlay_entries(&map.overlays, resolved.width(), resolved.height());
    let fixture = find_fixture(
        &map,
        &resolved,
        &overlay_grid,
        &overlay_registry,
        gapowr.adjacent,
    )
    .expect("Dustbowl must provide clear GACNST/valid GAPOWR cells beside nonblocking ore");

    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(resolved);
    sim.overlay_grid = Some(overlay_grid);
    let provider_id = sim
        .spawn_object(
            CONYARD,
            OWNER,
            fixture.provider.0,
            fixture.provider.1,
            0,
            &rules,
            &height_map,
        )
        .expect("spawn stock GACNST through lifecycle authority");
    let provider = sim
        .substrate
        .entities
        .get(provider_id)
        .expect("spawned provider retained");
    assert!(provider.lifecycle.cell_marked);
    assert!(provider.in_logic_vector);
    assert_eq!(provider.foundation, "4x4");

    let owner_id = sim.interner.intern(OWNER);
    let gapowr_id = sim.interner.intern(POWER_PLANT);
    assert!(crate::sim::production::enqueue_by_type(
        &mut sim,
        &rules,
        OWNER,
        POWER_PLANT,
    ));
    let category = crate::sim::production::category_for_object(
        rules.object(POWER_PLANT).expect("stock GAPOWR profile"),
    );
    let held_id = sim
        .production
        .factory_shadow
        .view(owner_id, category)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .expect("enqueue constructs the held GAPOWR identity");
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(owner_id, category)
    );
    // The factory owns completion accounting and its ready projection.
    assert!(
        !crate::sim::production::tick_production_with_overlay_registry(
            &mut sim,
            &rules,
            &height_map,
            Some(&path_grid),
            Some(&overlay_registry),
        )
    );
    let completed = sim
        .production
        .factory_shadow
        .view(owner_id, category)
        .unwrap()
        .object
        .unwrap();
    assert!(completed.completion_accounted);
    assert_eq!(completed.entity_id, Some(held_id));
    assert_eq!(ready_buildings_for_owner(&sim, &rules, OWNER).len(), 1);
    let held = sim.substrate.entities.get(held_id).unwrap();
    assert!(held.lifecycle.in_limbo && !held.lifecycle.cell_marked);

    let blocked_cells = rect_cells(fixture.blocked, 2, 2);
    let overlay_before: BTreeMap<(u16, u16), (Option<u8>, u8)> = blocked_cells
        .iter()
        .map(|cell| {
            let overlay = sim
                .overlay_grid
                .as_ref()
                .expect("overlay grid")
                .cell(cell.0, cell.1);
            (*cell, (overlay.overlay_id, overlay.overlay_data))
        })
        .collect();
    assert!(
        overlay_before
            .values()
            .any(|(overlay_id, _)| overlay_id.is_some()),
        "blocked retail footprint must contain a map ore overlay"
    );
    let preview = placement_preview_for_owner_without_overlays(
        &sim,
        &rules,
        OWNER,
        POWER_PLANT,
        fixture.blocked.0,
        fixture.blocked.1,
        Some(&path_grid),
        &height_map,
    )
    .expect("stock GAPOWR preview");
    assert!(!preview.valid);
    assert!(
        preview.cell_valid.iter().any(|valid| !valid),
        "the ore-bearing foundation cell must be red"
    );

    let entities_before = sim.substrate.entities.len();
    let next_id_before = sim.substrate.next_stable_object_id;
    let occupancy_generation_before = sim.substrate.occupancy.generation();
    let rejected = CommandEnvelope::new(
        owner_id,
        sim.session.tick + 1,
        Command::PlaceReadyBuilding {
            owner: owner_id,
            type_id: gapowr_id,
            rx: fixture.blocked.0,
            ry: fixture.blocked.1,
        },
    );
    let tick = sim.advance_tick(
        &[rejected],
        Some(&rules),
        &height_map,
        Some(&path_grid),
        None,
        67,
    );
    assert_eq!(tick.executed_commands, 1);
    assert!(!tick.spawned_entities);
    assert_eq!(sim.substrate.entities.len(), entities_before);
    assert_eq!(sim.substrate.next_stable_object_id, next_id_before);
    assert_eq!(
        sim.substrate.occupancy.generation(),
        occupancy_generation_before
    );
    assert_eq!(ready_buildings_for_owner(&sim, &rules, OWNER).len(), 1);
    assert_eq!(
        sim.production
            .factory_shadow
            .view(owner_id, category)
            .unwrap()
            .object
            .unwrap()
            .entity_id,
        Some(held_id),
        "rejected placement keeps the original factory identity"
    );
    assert_eq!(
        blocked_cells
            .iter()
            .map(|cell| {
                let overlay = sim
                    .overlay_grid
                    .as_ref()
                    .expect("overlay grid")
                    .cell(cell.0, cell.1);
                (*cell, (overlay.overlay_id, overlay.overlay_data))
            })
            .collect::<BTreeMap<_, _>>(),
        overlay_before
    );

    let preview = placement_preview_for_owner_without_overlays(
        &sim,
        &rules,
        OWNER,
        POWER_PLANT,
        fixture.valid.0,
        fixture.valid.1,
        Some(&path_grid),
        &height_map,
    )
    .expect("stock GAPOWR valid preview");
    assert!(preview.valid);
    assert_eq!(preview.cell_valid, vec![true; 4]);
    let generation_before_valid = sim.substrate.occupancy.generation();
    let accepted = CommandEnvelope::new(
        owner_id,
        sim.session.tick + 1,
        Command::PlaceReadyBuilding {
            owner: owner_id,
            type_id: gapowr_id,
            rx: fixture.valid.0,
            ry: fixture.valid.1,
        },
    );
    let tick = sim.advance_tick(
        &[accepted],
        Some(&rules),
        &height_map,
        Some(&path_grid),
        None,
        67,
    );
    assert_eq!(tick.executed_commands, 1);
    assert!(tick.spawned_entities);
    assert!(ready_buildings_for_owner(&sim, &rules, OWNER).is_empty());

    let placed = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.type_ref == gapowr_id
                && entity.position.rx == fixture.valid.0
                && entity.position.ry == fixture.valid.1
        })
        .expect("accepted command spawned stock GAPOWR");
    assert_eq!(placed.category, EntityCategory::Structure);
    assert_eq!(placed.foundation, "2x2");
    assert_eq!(placed.health.current, 750);
    assert_eq!(rules.object("GAPOWR").unwrap().strength, 750);
    assert!(placed.lifecycle.cell_marked);
    assert!(placed.in_logic_vector);
    assert!(placed.building_up.is_some());
    let placed_id = placed.stable_id;
    assert_eq!(
        placed_id, held_id,
        "placement reuses the completed identity"
    );
    assert_eq!(
        sim.substrate.occupancy.generation(),
        generation_before_valid + 4
    );
    for cell in rect_cells(fixture.valid, 2, 2) {
        assert!(
            sim.substrate
                .occupancy
                .get(cell.0, cell.1)
                .is_some_and(|occupancy| {
                    occupancy
                        .iter_layer(MovementLayer::Ground)
                        .any(|occupant| occupant.entity_id == placed_id)
                }),
            "placed GAPOWR must own retail foundation cell {cell:?}"
        );
    }

    let _ = tick_power_states(
        &mut sim.power_states,
        &mut sim.substrate.entities,
        &rules,
        &sim.interner,
    );
    let power = sim
        .power_states
        .get(&owner_id)
        .expect("placed GAPOWR contributes live power");
    assert_eq!(power.total_output, 200);
    assert_eq!(power.total_drain, 0);
    assert!(!power.is_low_power);
    assert!(
        sim.substrate
            .entities
            .get(placed_id)
            .is_some_and(|entity| entity.building_up.is_some()),
        "power must be live during the visible buildup"
    );
}
