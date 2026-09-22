//! App initialization helpers — map loading, entity spawning, asset loading.
//!
//! Extracted from app.rs to keep the main orchestrator under the 400-line limit.
//! These functions run once at startup (not per-frame).
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::app::frontend::list_maps::{
    LoadedMap, LoadedMapSource, load_map_by_name_or_path_with_assets, try_load_mmx,
};
use crate::app::frontend::skirmish::{
    build_overlay_atlas_from_map, house_color_map_for_launch_session,
};
use crate::app::loading::fresh_scenario::{
    FreshMapMaterialization, FreshScenarioLoadContextDescriptor,
};
#[cfg(test)]
use crate::app::loading::init_helpers::load_rules_with_merged_ini;
use crate::app::loading::init_helpers::{
    build_entity_atlases, build_sidebar_cameo_atlas, build_tile_atlas,
    log_trigger_graph_diagnostics, parse_debug_spawn_units_env, scheduler_anim_roots,
    theater_ext_for,
};
use crate::match_bootstrap::LoadingStartup;
use crate::sim::scenario_bootstrap::{
    ScenarioBootstrapRng, StockOfflinePrefixProjection,
    apply_pre_fill_scenario_prefix_launch_session_with_overlay_registry,
    initialize_skirmish_launch_houses,
};

use crate::app::presentation::lighting::rebuild_lighting_grid_from_sim;
#[cfg(test)]
use crate::app::presentation::lighting::{build_lighting_grid_from_view, derive_lighting_view};
use crate::assets::asset_manager::AssetManager;
use crate::assets::shp_file::ShpFile;
use crate::map::actions::ActionMap;
use crate::map::basic::{BasicSection, BridgeDestroyabilityMode};
use crate::map::cell_tags::CellTagMap;
use crate::map::events::EventMap;
use crate::map::houses::{self, HouseColorMap, HouseRoster};
use crate::map::lighting::{self, CellLightGrid, LightingConfig};
use crate::map::map_file::MapFile;
use crate::map::overlay::{OverlayEntry, TerrainObject};
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::tags::TagMap;
use crate::map::terrain::{self, LocalBounds, TerrainGrid};
use crate::map::theater;
use crate::map::trigger_graph::TriggerGraph;
use crate::map::triggers::TriggerMap;
use crate::map::waypoints::{self, Waypoint};
use crate::render::batch::BatchRenderer;
use crate::render::bridge_atlas::BridgeAtlas;
use crate::render::bridge_railing_atlas::{BridgeRailingAtlas, BridgeRailingTileBases};
use crate::render::cursor_atlas;
use crate::render::gpu::GpuContext;
use crate::render::overlay_atlas::OverlayAtlas;
use crate::render::sidebar_cameo_atlas::SidebarCameoAtlas;
use crate::render::sidebar_chrome::SidebarChromeSet;
use crate::render::sprite_atlas::SpriteAtlas;
use crate::render::tile_atlas::TileAtlas;
use crate::render::unit_atlas::UnitAtlas;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

pub(crate) fn resolved_overlay_shp_ids(
    registry: &OverlayTypeRegistry,
    rules_ini: &IniFile,
    art: &ArtRegistry,
    asset_manager: &AssetManager,
    theater_ext: &str,
    theater_name: &str,
) -> BTreeSet<u8> {
    let mut available = BTreeSet::new();
    for raw_id in 0..registry.len().min(usize::from(u8::MAX) + 1) {
        let overlay_id = raw_id as u8;
        let Some(name) = registry.name(overlay_id) else {
            continue;
        };
        let image_id = art.resolve_overlay_image_id(name, rules_ini);
        let native_names = crate::rules::art_data::native_overlay_shp_names(
            art,
            &image_id,
            theater_ext,
            theater_name,
        );
        if native_names
            .iter()
            .filter_map(|candidate| asset_manager.load_file_from_mix(candidate))
            .any(|loaded| ShpFile::from_bytes(&loaded.bytes).is_ok())
        {
            available.insert(overlay_id);
        }
    }
    available
}

/// Append every visible real-cell write made by startup crate Mark to the
/// initial presentation index. Ghost slots have no live overlay identity, while
/// low-bridge Mark may write many cells for one timed slot.
fn connect_startup_crate_overlays(
    overlays: &mut Vec<OverlayEntry>,
    simulation: &Simulation,
) -> usize {
    let Some(grid) = simulation.overlay_grid.as_ref() else {
        return 0;
    };
    let mut seen = BTreeSet::new();
    let candidates = grid
        .pending_dirty_cells()
        .iter()
        .copied()
        .filter(|cell| seen.insert(*cell))
        .filter_map(|(rx, ry)| {
            let cell = grid.cell(rx, ry);
            Some(OverlayEntry {
                rx,
                ry,
                overlay_id: cell.overlay_id?,
                frame: cell.overlay_data,
            })
        })
        .collect();
    let mut index = crate::app::presentation::overlay_index::OverlayRenderIndex::default();
    index.replace_from_source(std::mem::take(overlays));
    let synced = index.upsert_occupied(candidates);
    *overlays = index.as_slice().to_vec();
    synced
}

/// Refresh the legacy generated presentation entries whose overlay resolves
/// to a TiberiumClass from the live grid. The generator tail's final
/// `InitCellAttributes(1)` rewrote every ore density after those entries were
/// built from the raw packs; non-resource identities keep their source frame.
fn refresh_generated_tiberium_presentation_frames(
    overlays_connected: &mut [OverlayEntry],
    overlay_grid: &crate::sim::overlay_grid::OverlayGrid,
    overlay_registry: &OverlayTypeRegistry,
    tiberium_types: &crate::rules::tiberium_type::TiberiumTypeRegistry,
) -> usize {
    let mut refreshed = 0;
    for entry in overlays_connected {
        if overlay_registry
            .tiberium_type_for_overlay(tiberium_types, entry.overlay_id)
            .is_none()
        {
            continue;
        }
        let cell = overlay_grid.cell(entry.rx, entry.ry);
        if cell.overlay_id == Some(entry.overlay_id) && cell.overlay_data != entry.frame {
            entry.frame = cell.overlay_data;
            refreshed += 1;
        }
    }
    refreshed
}

#[cfg(test)]
mod startup_crate_presentation_tests {
    use super::*;
    use crate::sim::crates::CrateSlot;
    use crate::sim::overlay_grid::OverlayGrid;

    #[test]
    fn generated_presentation_frames_follow_germinated_ore_densities_only() {
        let ini = IniFile::from_str(
            "[Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\n\
             [OverlayTypes]\n0=TIBCELL\n1=BRIDGE\n\
             [TIBCELL]\nTiberium=yes\n[BRIDGE]\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("presentation rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let mut grid = OverlayGrid::new(8, 8);
        grid.place_overlay(2, 3, 0, 11);
        grid.place_overlay(4, 4, 1, 9);
        grid.place_overlay(5, 5, 0, 4);
        let mut entries = vec![
            OverlayEntry {
                rx: 2,
                ry: 3,
                overlay_id: 0,
                frame: 5,
            },
            OverlayEntry {
                rx: 4,
                ry: 4,
                overlay_id: 1,
                frame: 2,
            },
            OverlayEntry {
                rx: 5,
                ry: 5,
                overlay_id: 0,
                frame: 4,
            },
        ];

        assert_eq!(
            refresh_generated_tiberium_presentation_frames(
                &mut entries,
                &grid,
                &registry,
                &rules.tiberium_types,
            ),
            1
        );
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.rx, entry.ry, entry.overlay_id, entry.frame))
                .collect::<Vec<_>>(),
            vec![(2, 3, 0, 11), (4, 4, 1, 2), (5, 5, 0, 4)],
            "only the resource identity with a changed density is rewritten"
        );
    }

    #[test]
    fn startup_crate_presentation_appends_visible_slots_and_excludes_ghosts() {
        let mut simulation = Simulation::new();
        simulation.overlay_grid = Some(OverlayGrid::new(16, 16));
        *simulation.crate_authority.slot_mut(0) = CrateSlot {
            start_frame: 0,
            aux: 1,
            duration: 2,
            cell_x: 9,
            cell_y: 4,
        };
        *simulation.crate_authority.slot_mut(1) = CrateSlot {
            start_frame: 0,
            aux: 3,
            duration: 4,
            cell_x: 7,
            cell_y: 8,
        };
        simulation
            .overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(9, 4, 33, u8::MAX);
        let mut source = vec![OverlayEntry {
            rx: 2,
            ry: 3,
            overlay_id: 5,
            frame: 6,
        }];

        assert_eq!(connect_startup_crate_overlays(&mut source, &simulation), 1);
        assert_eq!(
            source
                .iter()
                .map(|entry| (entry.rx, entry.ry, entry.overlay_id, entry.frame))
                .collect::<Vec<_>>(),
            vec![(2, 3, 5, 6), (9, 4, 33, u8::MAX),]
        );
        assert!(
            source.iter().all(|entry| (entry.rx, entry.ry) != (7, 8)),
            "a timed ghost has no initial render entry"
        );
    }

    #[test]
    fn startup_crate_presentation_includes_low_bridge_extension_without_consuming_dirty_receipt() {
        let mut simulation = Simulation::new();
        simulation.overlay_grid = Some(OverlayGrid::new(32, 32));
        *simulation.crate_authority.slot_mut(0) = CrateSlot {
            start_frame: 10,
            aux: 20,
            duration: 30,
            cell_x: 12,
            cell_y: 12,
        };
        let grid = simulation.overlay_grid.as_mut().unwrap();
        grid.place_overlay(12, 11, 0x5C, 0);
        grid.place_overlay(12, 12, 0x4A, 1);
        grid.place_overlay(12, 13, 0x4B, 2);
        let dirty_before = grid.pending_dirty_cells().to_vec();
        let mut source = Vec::new();

        assert_eq!(connect_startup_crate_overlays(&mut source, &simulation), 3);
        assert_eq!(
            source
                .iter()
                .map(|entry| (entry.rx, entry.ry, entry.overlay_id, entry.frame))
                .collect::<Vec<_>>(),
            vec![(12, 11, 0x5C, 0), (12, 12, 0x4A, 1), (12, 13, 0x4B, 2)]
        );
        assert_eq!(
            simulation
                .overlay_grid
                .as_ref()
                .unwrap()
                .pending_dirty_cells(),
            dirty_before,
            "initial presentation must not consume the sim/navigation receipt"
        );
    }
}

#[cfg(test)]
mod native_overlay_shp_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn with_asset(label: &str, filename: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "vera20k-overlay-get-shp-{label}-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create overlay SHP test directory");
            std::fs::write(path.join(filename), valid_test_shp())
                .expect("write overlay SHP test asset");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn valid_test_shp() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());

        let data_offset = 32u32;
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.push(0);
        data.extend_from_slice(&[0u8; 11]);
        data.extend_from_slice(&data_offset.to_le_bytes());
        data.extend_from_slice(&[1, 2, 3, 0]);
        data
    }

    fn theater_overlay_fixture() -> (OverlayTypeRegistry, IniFile, ArtRegistry) {
        let rules_ini = IniFile::from_str("[OverlayTypes]\n0=NAME\n");
        let art_ini = IniFile::from_str("[NAME]\nTheater=yes\n");
        let registry = OverlayTypeRegistry::from_ini(&rules_ini, Some(&art_ini));
        let art = ArtRegistry::from_ini(&art_ini);
        (registry, rules_ini, art)
    }

    #[test]
    fn gsi_04_07_placement_map_pack_get_shp_uses_native_primary_and_generic_retry() {
        let (registry, rules_ini, art) = theater_overlay_fixture();
        assert_eq!(
            crate::rules::art_data::native_overlay_shp_names(&art, "NAME", "TEM", "TEMPERATE"),
            ["NAME.TEM".to_string(), "NGME.TEM".to_string()]
        );

        for (label, filename, expected_available) in [
            ("native-extension", "NAME.TEM", true),
            ("ineligible-shp-alternative", "NAME.SHP", false),
            ("generic-letter-retry", "NGME.TEM", true),
        ] {
            let directory = TestDirectory::with_asset(label, filename);
            let asset_manager = AssetManager::from_loose_root_for_test(directory.path());
            let available = resolved_overlay_shp_ids(
                &registry,
                &rules_ini,
                &art,
                &asset_manager,
                "TEM",
                "TEMPERATE",
            );

            assert_eq!(
                available.contains(&0),
                expected_available,
                "only {filename} is present"
            );
        }
    }
}

#[cfg(test)]
mod map_wall_owner_candidate_tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, zone_class};
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
    use crate::sim::components::{BuildingUp, Health};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::overlay_grid::{MapWallOwnerCandidate, OverlayGrid};
    use crate::sim::power_system::PowerState;
    use crate::sim::radiation::RadDetonation;
    use crate::sim::runtime::map_wall_owner_candidate_from_building;

    fn flat_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        let land = LandType::Clear.as_index();
        let speed_costs = SpeedCostProfile::default();
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: true,
            tileset_index: None,
            land_type: land,
            yr_cell_land_type: land,
            slope_type: 0,
            template_height: 0,
            height_in_pixels: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs,
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: true,
            allows_tiberium: false,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: zone_class::GROUND,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: land,
            base_yr_cell_land_type: land,
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: speed_costs,
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0; 3],
            radar_right: [0; 3],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let cells = (0..height)
            .flat_map(|ry| (0..width).map(move |rx| flat_cell(rx, ry)))
            .collect();
        ResolvedTerrainGrid::from_cells(width, height, cells)
    }

    fn building(
        stable_id: u64,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        foundation: &str,
    ) -> GameEntity {
        let mut entity = GameEntity::test_default(stable_id, type_id, owner, rx, ry);
        entity.category = EntityCategory::Structure;
        entity.foundation = foundation.to_string();
        entity.lifecycle.object_alive = true;
        entity.lifecycle.cell_marked = true;
        entity
    }

    #[test]
    fn gsi_04_07_placement_production_wall_owner_uses_building_get_coords_center() {
        let terrain = flat_terrain(15, 10);
        let gacnst = building(1, "GACNST", "GDI", 8, 8, "4x4");
        let gaspot = building(2, "GASPOT", "Neutral", 14, 9, "1x1");
        let gacnst_candidate = map_wall_owner_candidate_from_building(&gacnst, &terrain, true);
        let gaspot_candidate = map_wall_owner_candidate_from_building(&gaspot, &terrain, true);

        assert_eq!(
            (gacnst_candidate.world_x, gacnst_candidate.world_y),
            (8 * 256 + 128 + 3 * 128, 8 * 256 + 128 + 3 * 128)
        );
        assert_eq!(
            (gaspot_candidate.world_x, gaspot_candidate.world_y),
            (14 * 256 + 128, 9 * 256 + 128),
            "a 1x1 foundation has zero GetCoords projection"
        );
        assert_eq!(
            (
                gacnst_candidate.foundation_width,
                gacnst_candidate.foundation_height
            ),
            (4, 4),
            "candidate uses the dimensions stamped on the entity"
        );

        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str("[OverlayTypes]\n0=GAWALL\n[GAWALL]\nWall=yes\n"),
            None,
        );
        let mut projected_grid = OverlayGrid::new(15, 10);
        projected_grid.place_overlay(12, 9, 0, 0);
        projected_grid.reconstruct_map_wall_owners(
            &terrain,
            &registry,
            &[gacnst_candidate, gaspot_candidate],
        );
        assert_eq!(projected_grid.cell(12, 9).wall_owner, Some(gacnst.owner));

        let mut northwest_grid = OverlayGrid::new(15, 10);
        northwest_grid.place_overlay(12, 9, 0, 0);
        northwest_grid.reconstruct_map_wall_owners(
            &terrain,
            &registry,
            &[
                MapWallOwnerCandidate {
                    world_x: 8 * 256 + 128,
                    world_y: 8 * 256 + 128,
                    ..gacnst_candidate
                },
                MapWallOwnerCandidate {
                    world_x: 14 * 256 + 128,
                    world_y: 9 * 256 + 128,
                    ..gaspot_candidate
                },
            ],
        );
        assert_eq!(
            northwest_grid.cell(12, 9).wall_owner,
            Some(gaspot.owner),
            "the former north-west candidate coordinates select the wrong owner"
        );
    }

    #[test]
    fn gsi_04_10_all_terrain_clears_same_cell_tiberium_before_entity_admission() {
        let ini = IniFile::from_str(
            "[General]\nTreeStrength=200\n\
             [TerrainTypes]\n0=TREE01\n1=TIBTRE01\n\
             [TREE01]\nSpawnsTiberium=no\n\
             [TIBTRE01]\nSpawnsTiberium=yes\n\
             [OverlayTypes]\n0=ORE\n1=ROCK\n\
             [ORE]\nTiberium=yes\n\
             [ROCK]\nIsARock=yes\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        assert!(
            !rules
                .terrain_object_type_case_insensitive("TREE01")
                .expect("ordinary tree type")
                .spawns_tiberium
        );
        let mut sim = Simulation::with_seed(0x0410);
        let rng_before = sim.scenario_rng.state();

        let mut terrain = flat_terrain(6, 3);
        let mut overlays = OverlayGrid::new(6, 3);
        overlays.place_overlay(1, 1, 0, 7);
        overlays.place_overlay(2, 1, 0, 8);
        overlays.place_overlay(3, 1, 1, 9);
        overlays.place_overlay(4, 1, 0, 10);
        for rx in 1..=4 {
            crate::sim::overlay_grid::recalc_overlay_passability(
                &mut overlays,
                &mut terrain,
                &registry,
                rx,
                1,
            );
        }
        assert_eq!(
            terrain.cell(1, 1).unwrap().terrain_class,
            TerrainClass::Tiberium
        );
        assert_eq!(
            terrain.cell(2, 1).unwrap().terrain_class,
            TerrainClass::Tiberium
        );
        assert_eq!(
            terrain.cell(4, 1).unwrap().terrain_class,
            TerrainClass::Tiberium,
            "the unknown Terrain fixture starts with the same projected ore state"
        );
        overlays.take_dirty_cells();
        let terrain_objects = [
            TerrainObject {
                rx: 1,
                ry: 1,
                name: "TREE01".to_string(),
            },
            TerrainObject {
                rx: 2,
                ry: 1,
                name: "TIBTRE01".to_string(),
            },
            TerrainObject {
                rx: 3,
                ry: 1,
                name: "TREE01".to_string(),
            },
            TerrainObject {
                rx: 4,
                ry: 1,
                name: "UNKNOWN".to_string(),
            },
        ];

        let cleared = crate::sim::terrain_spawn::clear_tiberium_source_cells_for_terrain(
            &mut overlays,
            &mut terrain,
            &terrain_objects,
            &rules,
            &registry,
        );

        assert_eq!(cleared, BTreeSet::from([(1, 1), (2, 1)]));
        assert_eq!(overlays.cell(1, 1).overlay_id, None);
        assert_eq!(overlays.cell(1, 1).overlay_data, 0);
        assert_eq!(overlays.cell(2, 1).overlay_id, None);
        assert_eq!(overlays.cell(2, 1).overlay_data, 0);
        assert_eq!(overlays.cell(3, 1).overlay_id, Some(1));
        assert_eq!(overlays.cell(3, 1).overlay_data, 9);
        assert_eq!(overlays.cell(4, 1).overlay_id, Some(0));
        assert_eq!(overlays.cell(4, 1).overlay_data, 10);
        assert_eq!(
            terrain.cell(1, 1).unwrap().terrain_class,
            TerrainClass::Clear
        );
        assert_eq!(
            terrain.cell(2, 1).unwrap().terrain_class,
            TerrainClass::Clear
        );
        assert_eq!(
            terrain.cell(4, 1).unwrap().terrain_class,
            TerrainClass::Tiberium,
            "an unknown TerrainType leaves the resolved ore projection intact"
        );
        assert!(
            overlays.take_dirty_cells().is_empty(),
            "map-load TerrainClass::Unlimbo clearing must not emit runtime dirtiness"
        );

        let constructed = crate::sim::terrain_spawn::construct_terrain_objects(
            &mut sim,
            &terrain_objects,
            &rules,
            false,
        );
        assert_eq!(constructed, 3);
        assert_eq!(
            sim.production.terrain_object_cells,
            BTreeMap::from([((1, 1), 1), ((2, 1), 2), ((3, 1), 3)]),
            "recognized Terrain entries construct in source order while UNKNOWN is skipped"
        );
        assert_eq!(sim.scenario_rng.state(), rng_before);
    }

    #[test]
    fn stock_lamp_removal_publishes_before_next_draw_with_many_other_lamps() {
        use crate::app::presentation::lighting::MatchLighting;
        use crate::map::lighting::point_light_area_cells;
        // Native 554A80 -> 554AF0(mode0): every affected Cell483E30 call occurs
        // before return. The original-byte fixture binds the stock radius/area;
        // this scene exceeds the old 8192-cell app budget using unchanged lamps.
        let native: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/light_publication.json"
        ))
        .unwrap();
        let terrain = flat_terrain(128, 128);
        // Active rulesmd GALITE: Powered=yes, Power=0, radius5000, intensity.2.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=GALITE\n[GALITE]\nStrength=100\nPowered=yes\nPower=0\nLightVisibility=5000\nLightIntensity=.2\n",
        ))
        .unwrap();
        let mut sim = Simulation::with_seed(0x420);
        sim.session.lighting.current_ambient = 30;
        let owner = sim.interner.intern("Neutral");
        let kind = sim.interner.intern("GALITE");
        let mut id = 100;
        for ry in [24, 64, 104] {
            for rx in [24, 64, 104] {
                let mut lamp = GameEntity::new_at_frame_zero_for_test(
                    id,
                    rx,
                    ry,
                    0,
                    0,
                    owner,
                    Health { current: 100 },
                    kind,
                    EntityCategory::Structure,
                    0,
                    0,
                    false,
                );
                lamp.lifecycle.object_alive = true;
                lamp.lifecycle.in_limbo = false;
                lamp.lifecycle.cell_marked = true;
                sim.entities_mut().insert(lamp);
                sim.allocate_building_light(id, &rules);
                id += 1;
            }
        }
        let view = derive_lighting_view(&LightingConfig::default(), Some(&sim), Some(&rules), 2);
        assert_eq!(view.point_lights.len(), 9);
        let mut union = std::collections::BTreeSet::new();
        for source in &view.point_lights {
            let area = point_light_area_cells(source, 128, 128, |x, y| {
                terrain.cell(x, y).map(|c| c.level)
            });
            if (source.rx, source.ry) == (24, 24) {
                let actual: Vec<[u16; 2]> = area.iter().map(|((x, y), _)| [*x, *y]).collect();
                let expected: Vec<[u16; 2]> =
                    serde_json::from_value(native["area"].clone()).unwrap();
                assert_eq!(actual, expected);
                assert_eq!(native["events"][2]["name"], "disable");
                assert_eq!(
                    native["events"][2]["recomputes_before_return"],
                    actual.len()
                );
                assert_eq!(native["events"][2]["queued"], 0);
            }
            union.extend(area.into_iter().map(|(cell, _)| cell));
        }
        assert!(
            union.len() > 8192,
            "regression must exceed the former frame budget"
        );
        let mut lighting = MatchLighting::default();
        lighting.install(
            CellLightGrid::new(),
            LightingConfig::default(),
            2,
            Some((&terrain, &sim, &rules)),
        );
        let before =
            crate::render::palette_light::PaletteLight::cell(lighting.grid(), (24, 24), false);
        let unaffected = lighting.grid().cell_light_at((104, 104)).unwrap().clone();
        assert!(
            lighting
                .grid()
                .cell_light_at((24, 24))
                .unwrap()
                .raw_additive_intensity
                > 0
        );
        sim.discard_lighting_events();
        fatal_lamp_stage(&mut sim, &rules, 100);
        apply_lighting_events(&mut lighting, &terrain, &mut sim);
        lighting.refresh(&terrain, &sim, &rules, 2);
        assert_eq!(
            lighting
                .grid()
                .cell_light_at((24, 24))
                .unwrap()
                .raw_additive_intensity,
            0,
            "native mode-zero removal must publish in this refresh, before the draw consumer"
        );
        let after =
            crate::render::palette_light::PaletteLight::cell(lighting.grid(), (24, 24), false);
        assert!(after.brightness() < before.brightness());
        assert_eq!(
            lighting.grid().cell_light_at((104, 104)).unwrap(),
            &unaffected
        );
    }

    // Exercise the exact owner called by handoff, restore and the frame driver.
    // Reuse the live-world fixtures below rather than testing forwarded fields.
    #[test]
    fn match_lighting_global_refresh_publishes_before_the_next_draw() {
        use crate::app::presentation::lighting::MatchLighting;
        let terrain = flat_terrain(128, 128);
        let rules = lighting_rules();
        let mut sim = Simulation::with_seed(0x1a);
        sim.session.lighting.current_ambient = 30;
        let mut lights = MatchLighting::default();
        lights.install(
            CellLightGrid::new(),
            LightingConfig::default(),
            2,
            Some((&terrain, &sim, &rules)),
        );
        let tint_a = lights.grid().terrain_tile_tint_at((1, 1));
        lights.refresh(&terrain, &sim, &rules, 2);
        assert_eq!(lights.grid().terrain_tile_tint_at((1, 1)), tint_a);
        sim.session.lighting.current_ambient = 60;
        let grid_b = rebuild_lighting_grid_from_sim(
            &terrain,
            &LightingConfig::default(),
            Some(&sim),
            Some(&rules),
            2,
        );
        lights.refresh(&terrain, &sim, &rules, 2);
        for cell in [(1, 1), (127, 127)] {
            assert_eq!(
                lights.grid().terrain_tile_tint_at(cell),
                grid_b.terrain_tile_tint_at(cell)
            );
        }
        sim.session.lighting.current_ambient = 90;
        let grid_c = rebuild_lighting_grid_from_sim(
            &terrain,
            &LightingConfig::default(),
            Some(&sim),
            Some(&rules),
            2,
        );
        lights.refresh(&terrain, &sim, &rules, 2);
        assert_ne!(tint_a, grid_b.terrain_tile_tint_at((1, 1)));
        assert_ne!(
            grid_b.terrain_tile_tint_at((1, 1)),
            grid_c.terrain_tile_tint_at((1, 1))
        );
        for cell in [(1, 1), (127, 127)] {
            assert_eq!(
                lights.grid().terrain_tile_tint_at(cell),
                grid_c.terrain_tile_tint_at(cell)
            );
        }
        // An unchanged view must not replay an obsolete earlier global state.
        lights.refresh(&terrain, &sim, &rules, 2);
        for cell in [(1, 1), (127, 127)] {
            assert_eq!(
                lights.grid().terrain_tile_tint_at(cell),
                grid_c.terrain_tile_tint_at(cell)
            );
        }
    }

    #[test]
    fn match_lighting_restore_discards_outgoing_pending_samples() {
        use crate::app::presentation::lighting::MatchLighting;
        let terrain = flat_terrain(128, 128);
        let rules = lighting_rules();
        let mut sim = Simulation::with_seed(0x1b);
        seed_live_lamp(&mut sim, &rules);
        sim.session.lighting.current_ambient = 30;
        let mut lights = MatchLighting::default();
        lights.install(
            CellLightGrid::new(),
            LightingConfig::default(),
            2,
            Some((&terrain, &sim, &rules)),
        );
        let restored_tint = lights.grid().terrain_tile_tint_at((1, 1));
        sim.session.lighting.current_ambient = 90;
        lights.refresh(&terrain, &sim, &rules, 2);
        sim.set_building_light_active(41, false);
        sim.publish_global_lighting();
        assert!(!sim.lighting_sources.pending.is_empty());
        sim.radiation.apply_detonation(
            RadDetonation {
                rx: 120,
                ry: 120,
                rad_level: 500,
                spread: 1,
            },
            0,
            &rules.radiation,
            None,
        );
        assert_eq!(sim.radiation.clone().take_lighting_events().len(), 1);
        sim.session.lighting.current_ambient = 30;
        sim.rebuild_lighting_sources_after_load(&rules);
        assert!(sim.lighting_sources.pending.is_empty());
        assert!(sim.radiation.take_lighting_events().is_empty());
        assert!(sim.lighting_sources.buildings[&41].active);
        lights.restore(&terrain, &sim, &rules, 2);
        for _ in 0..3 {
            assert_eq!(lights.grid().terrain_tile_tint_at((1, 1)), restored_tint);
            lights.refresh(&terrain, &sim, &rules, 2);
        }
        assert_eq!(lights.grid().terrain_tile_tint_at((1, 1)), restored_tint);
    }

    #[test]
    fn match_lighting_installs_selected_detail_and_removes_live_source_area() {
        use crate::app::presentation::lighting::MatchLighting;
        let terrain = flat_terrain(32, 32);
        let rules = lighting_rules();
        let mut sim = Simulation::with_seed(0x1c);
        seed_live_lamp(&mut sim, &rules);
        let mut lights = MatchLighting::default();
        lights.install(
            CellLightGrid::new(),
            LightingConfig::default(),
            1,
            Some((&terrain, &sim, &rules)),
        );
        assert_eq!(
            lights
                .grid()
                .cell_light_at((4, 5))
                .unwrap()
                .raw_additive_intensity,
            0
        );
        lights.refresh(&terrain, &sim, &rules, 2);
        assert!(
            lights
                .grid()
                .cell_light_at((4, 5))
                .unwrap()
                .raw_additive_intensity
                > 0
        );
        let distant = lights.grid().cell_light_at((31, 31)).unwrap().clone();
        sim.destroy_building_light(41);
        sim.entities_mut().remove(41).expect("remove live lamp");
        lights.refresh(&terrain, &sim, &rules, 2);
        let rebuilt = rebuild_lighting_grid_from_sim(
            &terrain,
            &LightingConfig::default(),
            Some(&sim),
            Some(&rules),
            2,
        );
        for (cell, light) in rebuilt.cells() {
            let actual = lights.grid().cell_light_at(cell).unwrap();
            assert_eq!(actual.raw_additive_intensity, light.raw_additive_intensity);
            assert_eq!(actual.raw_rgb, light.raw_rgb);
            assert_eq!(
                lights.grid().terrain_tile_tint_at(cell),
                rebuilt.terrain_tile_tint_at(cell)
            );
        }
        assert_eq!(lights.grid().cell_light_at((31, 31)).unwrap(), &distant);
    }

    #[test]
    fn match_lighting_fatal_then_global_survives_production_frame_output() {
        use crate::app::presentation::lighting::MatchLighting;
        use crate::sim::light_sources::LightingEvent;
        let rules = lighting_rules();
        let terrain = flat_terrain(16, 16);
        let mut sim = Simulation::with_seed(0x1d);
        sim.session.lighting.normal.red_percent = 0;
        sim.session.lighting.normal.green_percent = 0;
        sim.session.lighting.normal.blue_percent = 0;
        sim.session.lighting.normal.ground_units = 0;
        sim.session.lighting.normal.level_units = 0;
        seed_live_lamp(&mut sim, &rules);
        let mut lights = MatchLighting::default();
        lights.install(
            CellLightGrid::new(),
            LightingConfig::default(),
            2,
            Some((&terrain, &sim, &rules)),
        );
        sim.discard_lighting_events();

        // Actual BeforeDeathEffects callback must publish disable before a
        // later global operation; the frame transaction must retain both.
        fatal_lamp_stage(&mut sim, &rules, 41);
        sim.session.lighting.current_ambient = 30;
        sim.session.lighting.target_ambient = 30;
        sim.publish_global_lighting();
        let mut rt = crate::sim::runtime::SimRuntime::from_simulation(sim);
        rt.resources.rules = rules;
        let frame = rt
            .advance_frame(&[], 16, crate::sim::world::TickLane::Ordinary)
            .expect("fixture frame must complete");
        assert!(matches!(frame.lighting_events.as_slice(),
            [LightingEvent::Building { id: 41, source: Some(source) },
             LightingEvent::Global(_)] if !source.active));
        lights.apply_events(&terrain, &frame.lighting_events);
        let cell = lights.grid().cell_light_at((4, 5)).unwrap();
        assert_eq!(cell.raw_rgb, [0; 3]);
        assert_eq!(cell.additive_intensity, 0);
        // Near-black full sampling clears common, whereas native484680
        // retains identity scale and publishes the new ambient (oracle case7).
        assert_eq!(cell.common_scalar, 300);
        let final_rebuild = rebuild_lighting_grid_from_sim(
            &terrain,
            &LightingConfig::default(),
            Some(&rt.simulation),
            Some(&rt.resources.rules),
            2,
        );
        assert_eq!(
            final_rebuild.cell_light_at((4, 5)).unwrap().common_scalar,
            0
        );
        let next = rt
            .advance_frame(&[], 16, crate::sim::world::TickLane::Ordinary)
            .expect("fixture frame must complete");
        assert!(
            next.lighting_events.is_empty(),
            "outgoing history is consumed once"
        );

        // An on/off pair has the same final owner state but MUST resample its
        // area, replacing the retained-global common with full-sampling zero.
        rt.simulation.set_building_light_active(41, true);
        rt.simulation.set_building_light_active(41, false);
        apply_lighting_events(&mut lights, &terrain, &mut rt.simulation);
        assert_eq!(
            lights.grid().cell_light_at((4, 5)).unwrap().common_scalar,
            0
        );
        assert_eq!(
            lights.grid().cell_light_at((15, 15)).unwrap().common_scalar,
            300,
            "a source refresh must not rebuild distant retained cells"
        );
    }

    #[test]
    fn match_lighting_source_global_source_uses_the_new_sampling_ambient() {
        use crate::app::presentation::lighting::MatchLighting;
        use crate::sim::light_sources::LightingEvent;
        let rules = lighting_rules();
        let terrain = flat_terrain(16, 16);
        let mut sim = Simulation::with_seed(0x1e);
        sim.session.lighting.normal.red_percent = 20;
        sim.session.lighting.normal.green_percent = 20;
        sim.session.lighting.normal.blue_percent = 20;
        seed_live_lamp(&mut sim, &rules);
        sim.session.lighting.current_ambient = 30;
        let mut lights = MatchLighting::default();
        lights.install(
            CellLightGrid::new(),
            LightingConfig::default(),
            2,
            Some((&terrain, &sim, &rules)),
        );
        sim.discard_lighting_events();
        sim.set_building_light_active(41, false);
        let radiation = RadDetonation {
            rx: 15,
            ry: 15,
            rad_level: 500,
            spread: 1,
        };
        sim.radiation
            .apply_detonation(radiation, 0, &rules.radiation, None);
        sim.session.lighting.current_ambient = 250;
        sim.publish_global_lighting();
        sim.set_building_light_active(41, true);
        sim.radiation
            .apply_detonation(radiation, 0, &rules.radiation, None);
        sim.flush_radiation_lighting();
        assert!(matches!(
            sim.lighting_sources.pending.as_slice(),
            [
                LightingEvent::Building { .. },
                LightingEvent::Radiation { .. },
                LightingEvent::Global(_),
                LightingEvent::Building { .. },
                LightingEvent::Radiation { .. }
            ]
        ));
        apply_lighting_events(&mut lights, &terrain, &mut sim);
        let expected = rebuild_lighting_grid_from_sim(
            &terrain,
            &LightingConfig::default(),
            Some(&sim),
            Some(&rules),
            2,
        );
        let actual = lights.grid().cell_light_at((4, 5)).unwrap();
        let expected = expected.cell_light_at((4, 5)).unwrap();
        assert_eq!(actual.raw_rgb, expected.raw_rgb);
        assert_eq!(actual.scale16, expected.scale16);
        assert_eq!(actual.raw_rgb, [300, 400, 500]);
        assert_eq!(actual.scale16, 32768);
        assert_eq!(actual.common_scalar, expected.common_scalar);
        assert_eq!(actual.common_scalar, 1000);
        assert_eq!(actual.raw_top_scalar, expected.raw_top_scalar);
        assert!(actual.raw_top_scalar > 2000);
        let mut incorrectly_global_last = actual.clone();
        incorrectly_global_last.refresh_retained_scalars(
            derive_lighting_view(&LightingConfig::default(), Some(&sim), Some(&rules), 2).profile,
            0,
        );
        assert_eq!(incorrectly_global_last.common_scalar, 1325);
        assert_ne!(
            actual.common_scalar, incorrectly_global_last.common_scalar,
            "this fixture distinguishes full-sample cap-before-scale from retained refresh"
        );
    }

    #[test]
    fn match_lighting_ion_new_source_keeps_normal_profile_and_palette_rows() {
        use crate::app::presentation::lighting::MatchLighting;
        use crate::render::palette_light::PaletteLight;
        use crate::sim::scenario_session::ScenarioLightingProfile;
        let rules = lighting_rules();
        let terrain = flat_terrain(16, 16);
        let mut sim = Simulation::with_seed(0x1f);
        sim.session.lighting.current_ambient = 30;
        sim.session.lighting.ion.red_percent = 20;
        sim.session.lighting.ion.green_percent = 90;
        sim.session.lighting.ion.blue_percent = 40;
        let mut lights = MatchLighting::default();
        lights.install(
            CellLightGrid::new(),
            LightingConfig::default(),
            2,
            Some((&terrain, &sim, &rules)),
        );
        let original_key = lights.grid().cell_light_at((4, 5)).unwrap().rgb_key;
        sim.select_lighting_profile(ScenarioLightingProfile::Ion);
        seed_live_lamp(&mut sim, &rules);
        apply_lighting_events(&mut lights, &terrain, &mut sim);
        let normal_cell = lights.grid().cell_light_at((4, 5)).unwrap().clone();
        assert_eq!(
            normal_cell.raw_rgb,
            [1100, 1200, 1300],
            "full source sampling uses normal scenario RGB even during Ion"
        );
        assert_ne!(
            normal_cell.rgb_key, original_key,
            "new Convert identity is created during Ion"
        );
        let rows = if normal_cell.rgb_key.iter().sum::<i32>() < 2000 {
            27
        } else {
            53
        };
        assert_eq!(
            PaletteLight::cell(lights.grid(), (4, 5), false),
            PaletteLight::new([200, 900, 400], rows, normal_cell.common_scalar, false)
        );
        let ion_tint = lights.grid().unit_tint_at((4, 5), 0);
        sim.select_lighting_profile(ScenarioLightingProfile::Normal);
        apply_lighting_events(&mut lights, &terrain, &mut sim);
        let restored = lights.grid().cell_light_at((4, 5)).unwrap();
        assert_eq!(restored.rgb_key, normal_cell.rgb_key);
        assert_eq!(restored.profile_id, normal_cell.profile_id);
        assert_eq!(restored.scale16, normal_cell.scale16);
        assert_eq!(
            PaletteLight::cell(lights.grid(), (4, 5), false),
            PaletteLight::new(normal_cell.rgb_key, rows, restored.common_scalar, false)
        );
        assert_ne!(lights.grid().unit_tint_at((4, 5), 0), ion_tint);
    }

    fn fatal_lamp_stage(sim: &mut Simulation, rules: &RuleSet, id: u64) {
        sim.apply_fatal_lifecycle_stage(
            rules,
            crate::sim::combat::FatalLifecycleStage::BeforeDeathEffects,
            id,
            EntityCategory::Structure,
            crate::sim::world::UninitContext::with_rules(rules),
        );
    }

    fn apply_lighting_events(
        lighting: &mut crate::app::presentation::lighting::MatchLighting,
        terrain: &ResolvedTerrainGrid,
        sim: &mut Simulation,
    ) {
        sim.flush_radiation_lighting();
        let events = std::mem::take(&mut sim.lighting_sources.pending);
        lighting.apply_events(terrain, &events);
    }

    fn lighting_rules() -> RuleSet {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=LAMP\n\
             [LAMP]\nStrength=100\nPowered=yes\nPower=-100\n\
             LightVisibility=2048\nLightIntensity=.2\n\
             LightRedTint=.1\nLightGreenTint=.2\nLightBlueTint=.3\n",
        ))
        .expect("lighting rules");
        rules.radiation = crate::rules::ruleset::RadiationRules {
            duration_multiple: 1,
            application_delay: 16,
            level_max: 500,
            level_delay: 90,
            light_delay: 90,
            level_factor: 0.2,
            light_factor: crate::util::fixed_math::sim_from_f32(0.1),
            tint_factor: crate::util::fixed_math::sim_from_f32(1.0),
            color: (0, 255, 0),
            site_warhead: "RadSite".to_string(),
        };
        rules
    }

    fn seed_live_lamp(sim: &mut Simulation, rules: &RuleSet) -> crate::sim::intern::InternedId {
        let owner = sim.interner.intern("House");
        let type_ref = sim.interner.intern("LAMP");
        let mut lamp = GameEntity::new_at_frame_zero_for_test(
            41,
            4,
            5,
            0,
            0,
            owner,
            Health { current: 100 },
            type_ref,
            EntityCategory::Structure,
            0,
            0,
            false,
        );
        lamp.lifecycle.object_alive = true;
        lamp.lifecycle.in_limbo = false;
        lamp.lifecycle.cell_marked = false;
        lamp.building_up = Some(BuildingUp {
            elapsed_ticks: 1,
            total_ticks: 10,
        });
        sim.entities_mut().insert(lamp);
        sim.add_entity_occupancy(41);
        sim.allocate_building_light(41, rules);
        owner
    }

    #[test]
    fn gsi_04_20_building_lamp_tracks_explicit_activity_and_detail() {
        let rules = lighting_rules();
        let mut sim = Simulation::with_seed(0x420);
        let owner = seed_live_lamp(&mut sim, &rules);
        let config = LightingConfig::default();

        let lit = derive_lighting_view(&config, Some(&sim), Some(&rules), 2);
        assert_eq!(
            lit.point_lights.len(),
            1,
            "BuildingUp does not suppress LightSource"
        );
        let low_detail = derive_lighting_view(&config, Some(&sim), Some(&rules), 1);
        assert!(low_detail.point_lights.is_empty());
        assert_ne!(lit.fingerprint, low_detail.fingerprint);

        sim.power_states.insert(
            owner,
            PowerState {
                total_output: 0,
                total_drain: 100,
                is_low_power: true,
                ..PowerState::default()
            },
        );
        let offline = derive_lighting_view(&config, Some(&sim), Some(&rules), 2);
        // House508C30->454CE0 RET and Building4549B0 animation-only
        // effects do not call LightSource disable on ordinary power loss.
        assert_eq!(offline.point_lights, lit.point_lights);
        assert_eq!(lit.fingerprint, offline.fingerprint);

        let power = sim.power_states.get_mut(&owner).expect("power state");
        power.total_output = 100;
        power.is_low_power = false;
        let restored = derive_lighting_view(&config, Some(&sim), Some(&rules), 2);
        assert_eq!(restored.point_lights.len(), 1);
        assert_eq!(lit.fingerprint, restored.fingerprint);

        sim.set_building_light_active(41, false);
        let disabled = derive_lighting_view(&config, Some(&sim), Some(&rules), 2);
        assert!(disabled.point_lights.is_empty());
        // Repeated allocation/construction must not reactivate an existing614.
        sim.allocate_building_light(41, &rules);
        assert!(
            derive_lighting_view(&config, Some(&sim), Some(&rules), 2)
                .point_lights
                .is_empty()
        );
        sim.set_building_light_active(41, true);
        fatal_lamp_stage(&mut sim, &rules, 41);
        assert!(
            derive_lighting_view(&config, Some(&sim), Some(&rules), 2)
                .point_lights
                .is_empty()
        );
    }

    #[test]
    fn gsi_04_20_building_lamp_tracks_capture_power_and_sale_lifecycle() {
        let rules = lighting_rules();
        let mut sim = Simulation::with_seed(0x4201);
        let _original_owner = seed_live_lamp(&mut sim, &rules);
        let captured_owner = sim.interner.intern("Captured");
        let config = LightingConfig::default();

        let before_capture = derive_lighting_view(&config, Some(&sim), Some(&rules), 2);
        assert_eq!(before_capture.point_lights.len(), 1);

        sim.power_states.insert(
            captured_owner,
            PowerState {
                total_output: 0,
                total_drain: 100,
                is_low_power: true,
                ..PowerState::default()
            },
        );
        sim.change_owner(41, captured_owner);
        let captured_offline = derive_lighting_view(&config, Some(&sim), Some(&rules), 2);
        assert_eq!(captured_offline.point_lights, before_capture.point_lights);
        assert_eq!(before_capture.fingerprint, captured_offline.fingerprint);

        let power = sim
            .power_states
            .get_mut(&captured_owner)
            .expect("captured owner power state");
        power.total_output = 100;
        power.is_low_power = false;
        let captured_online = derive_lighting_view(&config, Some(&sim), Some(&rules), 2);
        assert_eq!(captured_online.point_lights.len(), 1);
        assert_eq!(before_capture.fingerprint, captured_online.fingerprint);

        assert!(crate::sim::production::sell_building(&mut sim, &rules, 41));
        let sold = derive_lighting_view(&config, Some(&sim), Some(&rules), 2);
        assert!(sold.point_lights.is_empty());
        assert_ne!(captured_online.fingerprint, sold.fingerprint);
    }

    #[test]
    fn gsi_04_20_composed_scenario_lamp_and_radiation_reach_world_tint_consumer() {
        let rules = lighting_rules();
        let mut sim = Simulation::with_seed(0x4202);
        seed_live_lamp(&mut sim, &rules);
        sim.session.lighting.current_ambient = 80;
        sim.session.lighting.target_ambient = sim.session.lighting.ion.ambient_percent;
        sim.session.lighting.selected_profile =
            crate::sim::scenario_session::ScenarioLightingProfile::Ion;
        sim.radiation.apply_detonation(
            RadDetonation {
                rx: 4,
                ry: 5,
                rad_level: 500,
                spread: 10,
            },
            0,
            &rules.radiation,
            None,
        );

        let terrain = flat_terrain(10, 10);
        let view = derive_lighting_view(&LightingConfig::default(), Some(&sim), Some(&rules), 2);
        assert_eq!(view.profile.ambient_percent, 80);
        assert_eq!(
            view.point_lights.len(),
            2,
            "lamp and RadSite are both present"
        );

        let base = lighting::build_cell_light_grid_from_heights_and_units_with_detail(
            terrain.iter().map(|cell| ((cell.rx, cell.ry), cell.level)),
            view.profile,
            view.detail_level,
        );
        let composed = build_lighting_grid_from_view(&terrain, &view);
        let center = composed.cell_light_at((4, 5)).expect("composed cell light");
        assert_eq!(
            center.raw_additive_intensity,
            view.point_lights
                .iter()
                .map(|light| light.intensity)
                .sum::<i32>(),
            "both centered sources accumulate over the selected scenario profile"
        );
        assert_ne!(
            composed.unit_tint_at((4, 5), 0),
            base.unit_tint_at((4, 5), 0),
            "the world-instance unit tint consumer observes the composed grid"
        );
    }

    #[test]
    fn gsi_04_20_same_cell_radiation_merge_changes_complete_light_fingerprint() {
        let rules = lighting_rules();
        let mut sim = Simulation::with_seed(0x421);
        let detonation = RadDetonation {
            rx: 10,
            ry: 11,
            rad_level: 500,
            spread: 10,
        };
        sim.radiation
            .apply_detonation(detonation, 0, &rules.radiation, None);
        let first = derive_lighting_view(&LightingConfig::default(), Some(&sim), Some(&rules), 2);
        assert_eq!(first.point_lights.len(), 1);

        sim.radiation
            .apply_detonation(detonation, 0, &rules.radiation, None);
        let merged = derive_lighting_view(&LightingConfig::default(), Some(&sim), Some(&rules), 2);
        assert_eq!(merged.point_lights.len(), 1);
        assert_ne!(first.fingerprint, merged.fingerprint);
        assert_ne!(
            first.point_lights[0].intensity,
            merged.point_lights[0].intensity
        );
    }
}

/// Scenario/runtime inputs produced by loading a map (F11): everything the
/// sim runtime, trigger machinery, and match view facts consume.
pub struct ScenarioLoadInputs {
    pub(crate) startup: LoadingStartup,
    pub(crate) map_source: LoadedMapSource,
    /// Digest of the parsed source map INI used for strict save compatibility.
    pub(crate) map_hash: Option<u64>,
    pub basic: BasicSection,
    pub terrain_grid: Option<TerrainGrid>,
    pub resolved_terrain: Option<ResolvedTerrainGrid>,
    pub simulation: Option<Simulation>,
    /// Overlay entries for per-frame instance generation.
    pub overlays: Vec<OverlayEntry>,
    /// Terrain objects for per-frame instance generation.
    pub terrain_objects: Vec<TerrainObject>,
    pub waypoints: HashMap<u32, Waypoint>,
    pub cell_tags: CellTagMap,
    pub tags: TagMap,
    pub triggers: TriggerMap,
    pub events: EventMap,
    pub actions: ActionMap,
    pub trigger_graph: TriggerGraph,
    /// Overlay type registry — kept so wall placement can look up overlay_id by name.
    pub overlay_registry: OverlayTypeRegistry,
    pub house_roster: HouseRoster,
    /// Cell (rx, ry) → terrain elevation z for overlay/entity height lookup.
    pub height_map: BTreeMap<(u16, u16), u8>,
    /// Cell (rx, ry) → bridge deck elevation z. Only bridge cells present.
    pub bridge_height_map: BTreeMap<(u16, u16), u8>,
    pub tactical_bridge_inverse_map: BTreeMap<(u16, u16), crate::map::terrain::TacticalBridgeCell>,
    /// Parsed rules.ini data — kept for combat system weapon/warhead lookups.
    pub rules: Option<RuleSet>,
    /// Parsed [Lighting] config used for transient lighting rebuilds.
    pub map_lighting_config: LightingConfig,
    /// Current map theater name (e.g., "DESERT", "TEMPERATE").
    pub theater_name: String,
    /// Current theater extension (e.g., "des", "tem").
    pub theater_ext: String,
    /// Preferred initial local owner when the loader seeded a sandbox opening.
    pub initial_local_owner: Option<String>,
    /// Keep full map visibility for the empty-map sandbox opening.
    pub sandbox_full_visibility: bool,
    /// True when MCV seeding was deferred for spawn-pick phase.
    /// The map has 2+ multiplayer start waypoints and the player should pick one.
    pub spawn_pick_pending: bool,
    /// World point the camera should be centred on, in the frame
    /// `terrain::iso_to_screen` produces. Converted to a camera top-left by the
    /// transition, which knows the scaled sidebar width and the live zoom.
    pub camera_anchor_x: f32,
    pub camera_anchor_y: f32,
}

/// Presentation assets produced by loading a map (F11): atlases, chrome,
/// fonts, and derived display tables. Built FROM the constructed scenario;
/// nothing here feeds back into it.
pub struct PresentationLoadAssets {
    pub tile_atlas: Option<TileAtlas>,
    /// BUILDNGZ.SHA z-shape every building body writes its depth through.
    /// `None` falls back to the neutral z-shape (flat building depth).
    pub building_zshape: Option<crate::render::building_zshape::BuildingZShape>,
    pub unit_atlas: Option<UnitAtlas>,
    /// Palette + per-house RGB ramp GPU resources for the voxel sprite shader.
    /// None when no theater palette is available (rare).
    pub palette_set: Option<crate::render::palette_textures::PaletteSet>,
    pub sprite_atlas: Option<SpriteAtlas>,
    pub overlay_atlas: Option<OverlayAtlas>,
    pub bridge_atlas: Option<BridgeAtlas>,
    pub bridge_railing_atlas: Option<BridgeRailingAtlas>,
    pub sidebar_cameo_atlas: Option<SidebarCameoAtlas>,
    pub sidebar_chrome: Option<SidebarChromeSet>,
    pub(crate) software_cursor: Option<crate::app::presentation::render::SoftwareCursor>,
    /// Overlay ID → type name mapping (from rules.ini [OverlayTypes]).
    pub overlay_names: BTreeMap<u8, String>,
    /// Exact SHP frame-header radar RGB for each native-selected overlay frame.
    pub overlay_radar_colors: HashMap<(u8, u8), [u8; 3]>,
    /// Owner name → house color index mapping (from map [Houses] sections).
    pub house_color_map: HouseColorMap,
    /// Per-cell RGB tint from map [Lighting] section.
    pub lighting_grid: CellLightGrid,
    /// CSF string table — localized display names loaded from language MIX.
    pub csf: Option<crate::assets::csf_file::CsfFile>,
    /// Parsed GAME.FNT bitmap font for authentic sidebar text rendering.
    pub fnt_file: Option<crate::assets::fnt_file::FntFile>,
}

/// All data produced by loading a map (F11): concrete scenario/runtime inputs
/// and presentation assets, no longer one cross-domain bag, plus the leased
/// process asset manager on its way home.
pub struct MapLoadResult {
    pub(crate) scenario: ScenarioLoadInputs,
    pub(crate) presentation: PresentationLoadAssets,
    /// The leased process manager returning to `ProcessAssets` (F11 slot).
    pub asset_manager: Option<AssetManager>,
}

pub(crate) struct MapLoadInitial {
    map_data: MapFile,
    map_source: LoadedMapSource,
    /// Move-only generated-map authority; fixed maps never synthesize one.
    mapgen_rng_continuation: Option<crate::rng_continuation::MapGenRngContinuation>,
    /// Ordered launch-generation Building constructor effects. Preview traces
    /// never reach this owner; fixed maps carry none.
    generated_construction_trace: Option<crate::map::rmg::RmgConstructionTrace>,
}

impl MapLoadInitial {
    pub(crate) fn theater_name(&self) -> &str {
        &self.map_data.header.theater
    }

    pub(crate) fn map_data(&self) -> &MapFile {
        &self.map_data
    }

    pub(crate) fn map_source(&self) -> &LoadedMapSource {
        &self.map_source
    }

    #[cfg(test)]
    pub(crate) fn from_test_map_source(map_data: MapFile, map_source: LoadedMapSource) -> Self {
        Self {
            map_data,
            map_source,
            mapgen_rng_continuation: None,
            generated_construction_trace: None,
        }
    }
}

/// Bind the admitted physical source to the transport produced by the same
/// fresh-read transaction before the staged Scenario owner can advance.
///
/// gamemd provenance: `Read_Scenario @ 0x00684620` dispatches `.SED`
/// generation before `Read_Scenario_INI @ 0x00686730`; authored pack execution
/// belongs only to `ReadMapOverlayPacks @ 0x005FD2E0`, reached from fresh
/// `ScenarioClass::Full_Init @ 0x00686B20` after the family prefix and Fill.
fn validate_fresh_transport_before_effects(
    materialization: FreshMapMaterialization,
    has_mapgen_continuation: bool,
    has_construction_journal: bool,
) -> anyhow::Result<()> {
    match materialization {
        FreshMapMaterialization::Authored => {
            if has_mapgen_continuation || has_construction_journal {
                anyhow::bail!(
                    "authored fresh source carried generated continuation or construction transport"
                );
            }
        }
        FreshMapMaterialization::AcceptedGenerated => {
            if !has_mapgen_continuation {
                anyhow::bail!("accepted generated fresh source has no MapGen continuation");
            }
        }
    }
    Ok(())
}

fn construction_trace_after_fill<'a>(
    materialization: FreshMapMaterialization,
    trace: Option<&'a crate::map::rmg::RmgConstructionTrace>,
) -> anyhow::Result<Option<&'a crate::map::rmg::RmgConstructionTrace>> {
    match materialization {
        FreshMapMaterialization::Authored => Ok(None),
        FreshMapMaterialization::AcceptedGenerated => trace.map(Some).ok_or_else(|| {
            anyhow::anyhow!(
                "accepted generated materialization has no required construction journal"
            )
        }),
    }
}

#[cfg(test)]
mod fresh_transport_tests {
    use super::*;

    #[test]
    fn generated_missing_journal_stays_generated_until_its_post_fill_error() {
        validate_fresh_transport_before_effects(
            FreshMapMaterialization::AcceptedGenerated,
            true,
            false,
        )
        .expect("missing journal is not an authored-classification signal");
        let err = construction_trace_after_fill(FreshMapMaterialization::AcceptedGenerated, None)
            .unwrap_err();
        assert!(format!("{err:#}").contains("no required construction journal"));
    }

    #[test]
    fn source_materialization_rejects_crossed_generated_transport_before_effects() {
        assert!(
            validate_fresh_transport_before_effects(
                FreshMapMaterialization::Authored,
                true,
                false,
            )
            .is_err()
        );
        assert!(
            validate_fresh_transport_before_effects(
                FreshMapMaterialization::Authored,
                false,
                true,
            )
            .is_err()
        );
        assert!(
            validate_fresh_transport_before_effects(
                FreshMapMaterialization::AcceptedGenerated,
                false,
                true,
            )
            .is_err()
        );
    }
}

/// Snapshot the active Scenario multiplayer-start table into the live session.
///
/// Authored loads retain their parsed waypoint table. Accepted RMG loads have
/// already copied setup staging into the active Scenario array before `.SED`
/// regeneration, so the prefix plan—not regenerated map content—owns these
/// eight entries for loading markers, save/restore, and deterministic hashing.
pub(crate) fn scenario_start_waypoints_for_load(
    map_data: &MapFile,
    projection: Option<&StockOfflinePrefixProjection>,
) -> BTreeMap<u32, (u16, u16)> {
    let active_waypoints = projection
        .map(StockOfflinePrefixProjection::active_scenario_waypoints)
        .unwrap_or(&map_data.waypoints);
    waypoints::multiplayer_start_waypoints(active_waypoints)
        .into_iter()
        .map(|waypoint| (waypoint.index, (waypoint.rx, waypoint.ry)))
        .collect()
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RandomMapEntityProjection {
    owner: String,
    type_id: String,
    health: i32,
    cell: (u16, u16),
    facing: u8,
    category: crate::map::entities::EntityCategory,
    sub_cell: u8,
    veterancy: u16,
    high: bool,
    mission: Option<crate::rules::mission_data::MissionType>,
    recruitable: (bool, bool),
    structure_upgrades: [Option<String>; 3],
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RandomMapProjection {
    source_seed: String,
    pub(crate) header: (String, String, i32, u32, u32, u32, u32, u32, u32),
    cells: Vec<(u16, u16, i32, u8, u8)>,
    entities: Vec<RandomMapEntityProjection>,
    overlays: Vec<(u16, u16, u8, u8, u8)>,
    smudges: Vec<(String, u16, u16)>,
    terrain_objects: Vec<(String, u16, u16)>,
    waypoints: Vec<(u32, u16, u16)>,
    explicit_tubes: Vec<crate::map::tube_facts::TubeFact>,
    ini_content_hash: u64,
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RandomMapStartupCrateProjection {
    pub(crate) crates_enabled: bool,
    pub(crate) minimum: i32,
    pub(crate) maximum: i32,
    pub(crate) regen_bits: u64,
    pub(crate) wood_name: Option<String>,
    pub(crate) common_name: Option<String>,
    pub(crate) water_name: Option<String>,
    pub(crate) wood_id: Option<u8>,
    pub(crate) common_id: Option<u8>,
    pub(crate) water_id: Option<u8>,
    pub(crate) runtime_names: Vec<(u8, String)>,
    pub(crate) presented: Vec<(u16, u16, u8, u8)>,
    pub(crate) body_frame: u8,
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RandomMapLaunchSnapshot {
    pub(crate) map: RandomMapProjection,
    pub(crate) trace: crate::map::rmg::RmgConstructionTrace,
    pub(crate) emitted_constructor_words: Vec<(usize, u16)>,
    pub(crate) installed_constructor_words: Vec<(usize, u16, u16)>,
    pub(crate) scenario_after_trace: crate::sim::rng::SimRngLogicalState,
    pub(crate) mapgen_continuation: crate::sim::rng::SimRngLogicalState,
    pub(crate) final_rng: crate::sim::world::SimulationRngState,
    pub(crate) post_map_output: crate::sim::scenario_post_map::ScenarioPostMapOutput,
    pub(crate) startup_crate_slots: Vec<(usize, crate::sim::crates::CrateSlot, Option<(u8, u8)>)>,
    pub(crate) startup_crate: RandomMapStartupCrateProjection,
}

#[cfg(test)]
impl MapLoadInitial {
    /// Consume the same initial-map receipt as the match load and build the
    /// accepted-generated scenario in its order, with its functions, without a
    /// GPU, projecting the generated gameplay surfaces for lifecycle
    /// convergence tests.
    ///
    /// It is a second sequencing of the generated arm of `load_map_from_initial`,
    /// which cannot run here because it takes the GPU context, and it leaves
    /// out more than rendering: the Team AI registry, the shared cell dummy's
    /// Resize reconstruction and the theater registry publication. `final_rng` and `post_map_output` are what this sequence
    /// yields, which the tests compare between two launch routes; they are not
    /// certified equal to a match load's.
    pub(crate) fn into_random_map_launch_snapshot(
        self,
        asset_manager: &mut AssetManager,
        fresh_scenario_context: FreshScenarioLoadContextDescriptor,
    ) -> RandomMapLaunchSnapshot {
        let MapLoadInitial {
            map_data,
            map_source,
            mapgen_rng_continuation,
            generated_construction_trace,
        } = self;
        assert_eq!(fresh_scenario_context.physical_source(), &map_source);
        assert_eq!(
            fresh_scenario_context.signed_new_ini_format(),
            map_data.basic.new_ini_format.unwrap_or(0)
        );
        assert_eq!(
            fresh_scenario_context.materialization(),
            FreshMapMaterialization::AcceptedGenerated
        );
        let fresh_parts = fresh_scenario_context.into_stock_offline_parts();
        let match_seed = fresh_parts.match_seed;
        let match_launch_descriptor = fresh_parts.launch;
        let scenario_prefix_plan = fresh_parts.scenario_prefix;
        let source_seed = match &map_source {
            LoadedMapSource::Generated { seed_name } => seed_name.clone(),
            other => panic!("random-map lifecycle fixture loaded {other:?}"),
        };
        let mut waypoints: Vec<_> = map_data
            .waypoints
            .iter()
            .map(|(&index, waypoint)| (index, waypoint.rx, waypoint.ry))
            .collect();
        waypoints.sort_unstable();
        let raw_overlay_data = map_data
            .overlay_data_pack()
            .expect("generated projection must precede authored pack consumption");
        let map = RandomMapProjection {
            source_seed,
            header: (
                map_data.header.theater.clone(),
                map_data.header.fill.clone(),
                map_data.header.level,
                map_data.header.width,
                map_data.header.height,
                map_data.header.local_left,
                map_data.header.local_top,
                map_data.header.local_width,
                map_data.header.local_height,
            ),
            cells: map_data
                .cells
                .iter()
                .map(|cell| (cell.rx, cell.ry, cell.tile_index, cell.sub_tile, cell.z))
                .collect(),
            entities: map_data
                .entities
                .iter()
                .map(|entity| RandomMapEntityProjection {
                    owner: entity.owner.clone(),
                    type_id: entity.type_id.clone(),
                    health: entity.health,
                    cell: (entity.cell_x, entity.cell_y),
                    facing: entity.facing,
                    category: entity.category,
                    sub_cell: entity.sub_cell,
                    veterancy: entity.veterancy,
                    high: entity.high,
                    mission: entity.mission,
                    recruitable: (entity.recruitable_a, entity.recruitable_b),
                    structure_upgrades: entity.structure_upgrades.clone(),
                })
                .collect(),
            overlays: map_data
                .overlays
                .iter()
                .map(|overlay| {
                    (
                        overlay.rx,
                        overlay.ry,
                        overlay.overlay_id,
                        overlay.frame,
                        raw_overlay_data.byte_at(overlay.rx, overlay.ry),
                    )
                })
                .collect(),
            smudges: map_data
                .smudges
                .iter()
                .map(|smudge| (smudge.type_name.clone(), smudge.rx, smudge.ry))
                .collect(),
            terrain_objects: map_data
                .terrain_objects
                .iter()
                .map(|object| (object.name.clone(), object.rx, object.ry))
                .collect(),
            waypoints,
            explicit_tubes: map_data.explicit_tubes.clone(),
            ini_content_hash: map_data.ini.content_hash(),
        };
        let trace = generated_construction_trace
            .expect("random-map initial receipt carries a construction trace");
        let mut bootstrap_rng = ScenarioBootstrapRng::new(match_seed);
        bootstrap_rng.install_generated_mapgen_continuation(
            mapgen_rng_continuation.expect("random-map initial receipt carries MapGen"),
        );

        // Drive the production load inputs in the match load's order: rules,
        // the one Simulation staged before terrain Fill, Fill and generated
        // constructor replay on that owner, the GPU-free population funnel,
        // standard Battle launch application, and the shared Post_Map_Init
        // finalizer.
        let theater_result = theater::load_theater(asset_manager, &map_data.header.theater);
        let mode_override_ini = asset_manager
            .get_ref(&match_launch_descriptor.session().mode.override_file)
            .and_then(|bytes| IniFile::from_bytes(bytes).ok());
        let (mut rules, rules_ini, art_ini, native_rules_receipt) = load_rules_with_merged_ini(
            asset_manager,
            mode_override_ini.as_ref(),
            Some(&map_data.ini),
        )
        .expect("retail generated-map rules");
        let mut art = ArtRegistry::from_ini(&art_ini);
        art.apply_anim_type_read_states(&rules.anim_type_art_read_states);
        rules.merge_art_data(&mut art);
        rules.art_registry = art.clone();
        rules.general.resolve_art_rates(&art_ini);
        let infantry_sequences =
            crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art_ini);
        let overlay_registry = OverlayTypeRegistry::from_ini(&rules_ini, Some(&art_ini));
        // Stage the one Simulation before terrain Fill, as the match load does:
        // the prefix cursors, Fill and every later load constructor mutate the
        // identity that reaches gameplay.
        let bound_scenario_prefix =
            scenario_prefix_plan.bind_native_rules_receipt(native_rules_receipt);
        let pixel_conversion_bounds =
            crate::util::pixel_conversion::PixelConversionBounds::default();
        let native_start_bounds =
            crate::sim::scenario_bootstrap::NativeStartBounds::from_map_header(&map_data.header)
                .expect("map Size produces a fresh cell array");
        let scenario_cell_extent = native_start_bounds.min_rx + native_start_bounds.width;
        let lighting_profiles = lighting::parse_lighting_profiles(&map_data.ini);
        let scenario_descriptor = crate::sim::scenario_session::ScenarioDescriptor {
            seed: match_seed,
            map_name: match_launch_descriptor
                .session()
                .selected_map_file
                .clone()
                .or_else(|| map_data.basic.name.clone())
                .unwrap_or_default(),
            theater: map_data.header.theater.clone(),
            game_mode_nonzero: true,
            no_damage: false,
            free_radar: map_data.basic.free_radar.unwrap_or(false),
            // Skirmish start forces `TiberiumGrows|TiberiumSpreads` (`OR 0xC0`
            // at `0x005E74CD`), copied into the scenario at `0x00687C23`.
            tiberium_grows_flag: true,
            tiberium_spreads_flag: true,
            // Native Resize constructs a square cell-array extent of SizeW+SizeH.
            map_width: scenario_cell_extent,
            map_height: scenario_cell_extent,
            local_left: map_data.header.local_left as u16,
            local_top: map_data.header.local_top as u16,
            local_width: map_data.header.local_width as u16,
            local_height: map_data.header.local_height as u16,
            mp_start_waypoints: scenario_start_waypoints_for_load(
                &map_data,
                Some(bound_scenario_prefix.projection()),
            ),
            pixel_conversion_bounds,
            lighting: crate::sim::scenario_session::ScenarioLightingState::new(
                crate::sim::scenario_session::ScenarioLightProfileUnits {
                    ambient_percent: lighting_profiles.normal.ambient_percent,
                    red_percent: lighting_profiles.normal.red_percent,
                    green_percent: lighting_profiles.normal.green_percent,
                    blue_percent: lighting_profiles.normal.blue_percent,
                    ground_units: lighting_profiles.normal.ground_units,
                    level_units: lighting_profiles.normal.level_units,
                },
                crate::sim::scenario_session::ScenarioLightProfileUnits {
                    ambient_percent: lighting_profiles.ion.ambient_percent,
                    red_percent: lighting_profiles.ion.red_percent,
                    green_percent: lighting_profiles.ion.green_percent,
                    blue_percent: lighting_profiles.ion.blue_percent,
                    ground_units: lighting_profiles.ion.ground_units,
                    level_units: lighting_profiles.ion.level_units,
                },
            ),
        };
        let (mut simulation, scenario_prefix_projection) = bootstrap_rng
            .into_stock_offline_staged_simulation(&scenario_descriptor, bound_scenario_prefix)
            .expect("matching stock-offline Scenario prefix");
        let mapgen_continuation = simulation.rng_state().mapgen;
        let mut selector_cache =
            crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let (mut scenario_fill_rng, mut variant_main_rng) = simulation.terrain_load_draws();
        let mut scenario_fill_ranged =
            |low, high| scenario_fill_rng.next_range_u32_inclusive(low, high);
        let mut variant_draw = || variant_main_rng.next_u32();
        let mut variant_selector = selector_cache.begin_load(&mut variant_draw);
        let mut resolved_terrain =
            ResolvedTerrainGrid::build_with_variant_selector_and_shared_dummy(
                &map_data,
                theater_result.as_ref(),
                Some(asset_manager),
                Some(&rules.terrain_rules),
                Some(&overlay_registry),
                Some(&rules.terrain_object_types),
                true,
                rules.general.cliff_back_impassability,
                &mut scenario_fill_ranged,
                &mut variant_selector,
                crate::map::resolved_terrain::SharedCellDummy::fresh(),
                crate::map::resolved_terrain::OverlayLoadSource::GeneratedMaterialized,
                pixel_conversion_bounds,
            );
        let theater_ext = theater_result.as_ref().map_or_else(
            || theater_ext_for(&map_data.header.theater),
            |td| td.extension,
        );
        let scheduler_roots = scheduler_anim_roots(
            &rules,
            &overlay_registry,
            resolved_terrain.tile_animations(),
        );
        art.bind_scheduler_anim_assets(
            &scheduler_roots,
            asset_manager,
            theater_ext,
            &map_data.header.theater,
        )
        .expect("retail scheduler animation closure");
        // Every other AnimClass producer and the building animations: tolerant
        // pass, after the strict one, which rewrites the scheduler-owned set
        // wholesale.
        let unbound_explosion_roots = art.bind_anim_class_assets(
            &crate::app::loading::init_helpers::tolerant_anim_class_roots(&rules, &art),
            asset_manager,
            theater_ext,
            &map_data.header.theater,
        );
        crate::rules::effect_asset_catalog::log_unbound_combat_explosion_roots(
            unbound_explosion_roots,
        );
        rules.art_registry = art.clone();
        rules.bind_effect_assets(asset_manager, theater_ext, &map_data.header.theater);
        rules.bind_terrain_spawner_assets(
            &rules_ini,
            asset_manager,
            theater_ext,
            &map_data.header.theater,
        );
        rules.bind_animation_sequences(&infantry_sequences);
        drop(variant_selector);
        drop(variant_draw);
        drop(scenario_fill_ranged);
        drop(variant_main_rng);
        drop(scenario_fill_rng);
        // The native-id reservations the match load makes at this point: the
        // `[Tubes]` rows, then the launch branch's post-load particle system.
        simulation
            .construct_native_map_tubes(&map_data.ini)
            .expect("native [Tubes] construction");
        simulation.construct_post_load_particle_system_id();
        let table = simulation
            .replay_staged_generated_construction_trace(&trace)
            .expect("valid generated construction trace");
        let emitted_constructor_words = trace
            .events
            .iter()
            .filter_map(|event| match &event.outcome {
                crate::map::rmg::RmgConstructionOutcome::Discarded => None,
                crate::map::rmg::RmgConstructionOutcome::Emitted { entity_index, .. } => Some((
                    *entity_index,
                    table
                        .entry(*entity_index)
                        .expect("emitted trace row has an exact binding")
                        .techno_ctor_random_word,
                )),
            })
            .collect();
        let scenario_after_trace = simulation.rng_state().scenario;

        let overlay_shp_ids = resolved_overlay_shp_ids(
            &overlay_registry,
            &rules_ini,
            &art,
            asset_manager,
            theater_ext,
            &map_data.header.theater,
        );
        let mut overlay_grid = crate::sim::overlay_grid::OverlayGrid::from_native_overlay_packs(
            &map_data.overlays,
            map_data
                .overlay_data_pack()
                .expect("random-map snapshot precedes authored pack consumption"),
            &mut resolved_terrain,
            &overlay_registry,
            &overlay_shp_ids,
            true,
        );
        let _ = crate::sim::terrain_spawn::clear_tiberium_source_cells_for_terrain(
            &mut overlay_grid,
            &mut resolved_terrain,
            &map_data.terrain_objects,
            &rules,
            &overlay_registry,
        );
        let house_roster =
            houses::parse_house_roster(&map_data.ini, &rules.color_schemes, Some(&rules));
        let height_map = resolved_terrain.build_height_map();
        let bridge_destroyability_mode =
            crate::map::basic::BridgeDestroyabilityMode::SkirmishOrMultiplayer {
                bridge_destruction: match_launch_descriptor
                    .session()
                    .options
                    .bridges_destroyable,
            };
        assert_eq!(
            (resolved_terrain.width(), resolved_terrain.height()),
            (scenario_cell_extent, scenario_cell_extent),
            "Fill allocates the extent the descriptor announced"
        );
        crate::app::loading::init_helpers::populate_staged_app_scenario(
            &mut simulation,
            &map_data,
            &resolved_terrain,
            &map_data.header.theater,
            Some(&rules),
            &height_map,
            Some(&overlay_registry),
            Some(&overlay_grid),
            bridge_destroyability_mode,
            &scenario_descriptor,
            Some(&table),
            |simulation| {
                initialize_skirmish_launch_houses(
                    simulation,
                    &house_roster,
                    &rules,
                    &match_launch_descriptor,
                );
            },
        )
        .expect("production generated-map construction funnel");
        // The generator tail, where the match load runs it: growth and spread
        // queues from the painted densities, then the final germination
        // (`RandomMapGenerator::Generate @ 0x00598960` tail). The post-map
        // finalizer below leaves the queues alone.
        let _ = crate::sim::runtime::initialize_native_tiberium_queues(
            &mut simulation,
            &map_data.basic,
            &map_data.special_flags,
            &rules,
            &overlay_registry,
            Some(&overlay_grid),
            (map_data.header.width as u16, map_data.header.height as u16),
        );
        let _ = crate::sim::tiberium_germinate::run_generated_final_cell_attributes(
            &resolved_terrain,
            &mut overlay_grid,
            &rules.tiberium_types,
            &overlay_registry,
            map_data.header.width as u16,
            map_data.header.height as u16,
        );
        crate::app::loading::init_helpers::bind_staged_app_scenario_metadata(
            &mut simulation,
            asset_manager,
            Some(&rules),
            Some(&art),
        );
        let installed_constructor_words = map_data
            .entities
            .iter()
            .enumerate()
            .map(|(entity_index, map_entity)| {
                let expected = table
                    .entry(entity_index)
                    .expect("generated map entity has an exact constructor binding")
                    .techno_ctor_random_word;
                let actual = simulation
                    .entities()
                    .values()
                    .find(|entity| {
                        entity.position.rx == map_entity.cell_x
                            && entity.position.ry == map_entity.cell_y
                            && simulation
                                .interner
                                .resolve(entity.type_ref)
                                .eq_ignore_ascii_case(&map_entity.type_id)
                    })
                    .expect("generated map entity reached Simulation")
                    .techno_ctor_random_word;
                (entity_index, expected, actual)
            })
            .collect();
        simulation.intern_rule_type_ids(&rules);
        simulation.resolve_type_handles(&rules);
        let _ = apply_pre_fill_scenario_prefix_launch_session_with_overlay_registry(
            &mut simulation,
            &map_data,
            &house_roster,
            &rules,
            &height_map,
            &resolved_terrain,
            &match_launch_descriptor,
            &overlay_registry,
            &scenario_prefix_projection,
        );
        let post_map_output = crate::sim::runtime::finalize_constructed_scenario(
            &mut simulation,
            &map_data,
            &rules,
            &overlay_registry,
            overlay_grid,
            &house_roster,
            Some(&match_launch_descriptor),
        );
        let crate_name_id =
            |name: Option<&str>| name.and_then(|name| overlay_registry.id_for_name(name));
        let mut runtime_names = BTreeMap::new();
        crate::app::frontend::skirmish::preregister_runtime_overlay_names(
            &overlay_registry,
            &rules.crate_rules,
            &mut runtime_names,
        );
        let selected_ids = [
            crate_name_id(rules.crate_rules.wood_crate_img.as_deref()),
            crate_name_id(rules.crate_rules.crate_img.as_deref()),
            crate_name_id(rules.crate_rules.water_crate_img.as_deref()),
        ];
        runtime_names.retain(|id, _| {
            overlay_registry
                .flags(*id)
                .is_some_and(|flags| flags.crate_type)
                || selected_ids.contains(&Some(*id))
        });
        let mut presented = Vec::new();
        connect_startup_crate_overlays(&mut presented, &simulation);
        let startup_crate = RandomMapStartupCrateProjection {
            crates_enabled: simulation.session.game_options.crates,
            minimum: rules.crate_rules.minimum,
            maximum: rules.crate_rules.maximum,
            regen_bits: rules.crate_rules.regen.bits(),
            wood_id: crate_name_id(rules.crate_rules.wood_crate_img.as_deref()),
            common_id: crate_name_id(rules.crate_rules.crate_img.as_deref()),
            water_id: crate_name_id(rules.crate_rules.water_crate_img.as_deref()),
            wood_name: rules.crate_rules.wood_crate_img.clone(),
            common_name: rules.crate_rules.crate_img.clone(),
            water_name: rules.crate_rules.water_crate_img.clone(),
            runtime_names: runtime_names.into_iter().collect(),
            presented: presented
                .into_iter()
                .map(|entry| (entry.rx, entry.ry, entry.overlay_id, entry.frame))
                .collect(),
            body_frame: crate::render::overlay_atlas::CRATE_BODY_FRAME,
        };
        let startup_crate_slots = simulation
            .crate_authority
            .slots()
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, slot)| !slot.is_empty())
            .map(|(index, slot)| {
                let live_overlay = u16::try_from(slot.cell_x)
                    .ok()
                    .zip(u16::try_from(slot.cell_y).ok())
                    .and_then(|(rx, ry)| {
                        let cell = simulation.overlay_grid.as_ref()?.cell(rx, ry);
                        Some((cell.overlay_id?, cell.overlay_data))
                    });
                (index, slot, live_overlay)
            })
            .collect();
        let final_rng = simulation.rng_state();
        RandomMapLaunchSnapshot {
            map,
            trace,
            emitted_constructor_words,
            installed_constructor_words,
            scenario_after_trace,
            mapgen_continuation,
            final_rng,
            post_map_output,
            startup_crate_slots,
            startup_crate,
        }
    }
}

pub(crate) fn load_csf(
    asset_manager: &AssetManager,
) -> anyhow::Result<crate::assets::csf_file::CsfFile> {
    let bytes = asset_manager
        .get_ref("ra2md.csf")
        .ok_or_else(|| anyhow::anyhow!("required retail string table ra2md.csf is missing"))?;
    let csf = crate::assets::csf_file::CsfFile::from_bytes(bytes)
        .map_err(|err| anyhow::anyhow!("could not parse required ra2md.csf: {err:#}"))?;
    log::info!("Loaded CSF string table: ra2md.csf");
    Ok(csf)
}

// Scenario-menu metadata is map-owned (F06); re-exported for app callers.
pub use crate::map::scenario_menu::MapMenuEntry;

pub(crate) fn load_map_initial_with_assets(
    ra2_dir: PathBuf,
    asset_manager: &mut AssetManager,
    requested_map: Option<&str>,
    progress: &mut dyn crate::app::loading::pump::LoadingProgressSink,
) -> Result<MapLoadInitial> {
    // TubeMovement and random-map geometry share the retail executable's
    // sine/cosine table.  Install it before selecting the map source: explicit
    // `[Tubes]` are valid in ordinary .map/.mpr/.mmx content, not just .SED
    // random maps.  OnceLock keeps repeated map loads read-only and cheap.
    crate::map::rmg::trig::install_from_dir(&ra2_dir);
    if !crate::map::retail_trig::wave_tables_available() {
        anyhow::bail!(
            "{} does not provide the verified gamemd sine/Acos tables required by stock Sonic Wave simulation",
            ra2_dir.join("gamemd.exe").display()
        );
    }

    // Check RA2_QUICKPLAY env var: if it names a .map/.mpr file, load that directly.
    // UI-selected map name/path (requested_map) takes precedence.
    // Default: try testmap1.map in the project directory first, then fall back to .mmx files.
    let quickplay_map: Option<String> = std::env::var("RA2_QUICKPLAY")
        .ok()
        .filter(|v| v.ends_with(".map") || v.ends_with(".mpr") || v.ends_with(".mmx"));
    // `RA2_QUICKPLAY=<name>.sed` forces a random-map generation without going
    // through the skirmish UI — a dev shortcut for exercising the generator.
    let quickplay_seed: Option<String> = std::env::var("RA2_QUICKPLAY")
        .ok()
        .filter(|v| crate::map::rmg::is_seed_selection(v));

    // A `.SED` selection names a random-map seed, not a map file: the map is
    // generated in memory from its options. Handled before the file-loading
    // branches below, which would look for a map file that does not exist.
    let seed_selection: Option<String> = requested_map
        .filter(|name| crate::map::rmg::is_seed_selection(name))
        .map(str::to_string)
        .or(quickplay_seed);
    if let Some(seed_name) = seed_selection.as_deref() {
        let mut options = crate::map::rmg::RmgOptions::default();
        match std::fs::read(ra2_dir.join(seed_name)) {
            Ok(bytes) => match crate::rules::ini_parser::IniFile::from_bytes(&bytes) {
                Ok(ini) => options.apply_sed(&ini),
                Err(err) => {
                    log::warn!("random map: {seed_name} is not valid INI ({err}); using defaults")
                }
            },
            // Missing seed file is not fatal: the original's options object
            // keeps its constructor defaults when a key (or the file) is absent.
            Err(err) => log::warn!("random map: cannot read {seed_name} ({err}); using defaults"),
        }
        options.normalize();

        let settings = crate::map::rmg::RmgSettings::load(asset_manager);
        let theater_name = crate::map::rmg::emit::theater_name(options.theater);
        let theater = crate::map::theater::load_theater(asset_manager, theater_name)
            .ok_or_else(|| anyhow::anyhow!("random map: theater {theater_name} unavailable"))?;

        // Terrain rules feed the zone classifier's wheel-impassable table. The
        // table is observably inert during generation (the classifier runs at
        // start-placement time, before any rock/rough/cliff terrain exists), but
        // it is resolved faithfully from `rulesmd.ini` when present; a missing
        // file falls back to the passable defaults.
        let terrain_rules = asset_manager
            .get_ref("rulesmd.ini")
            .and_then(|bytes| crate::rules::ini_parser::IniFile::from_bytes(bytes).ok())
            .map(|ini| crate::rules::terrain_rules::TerrainRules::from_ini(&ini))
            .unwrap_or_default();
        let resolved = crate::map::rmg::build::ResolvedTheaterInputs::from_theater(
            &theater,
            &terrain_rules,
            crate::map::rmg::trig::global().cloned(),
        );

        // Tile-block layouts (sub-cell height/terrain grids) from the theater's
        // real TMP data — the shore tiler and zone classifier read these.
        let blocks =
            crate::map::rmg::theater_blocks::TheaterTileBlocks::build(&theater.lookup, |name| {
                asset_manager.get(name)
            });

        // `[AI] NeutralTechBuildings` plus each type's `Foundation=`. The phase
        // runs for every map type except 0, so an empty list here would both
        // strip the buildings and skip the draws the original consumes.
        let tech_types = crate::app::loading::init_helpers::load_neutral_tech_types(asset_manager);
        let generated = crate::map::rmg::build::generate_map(
            &options,
            &settings,
            &resolved,
            &blocks,
            &tech_types,
        );
        log::info!(
            "Random map generated: theater={}, {}x{}, seed={}, players={}",
            generated.map_file.header.theater,
            generated.map_file.header.width,
            generated.map_file.header.height,
            options.seed,
            options.num_players
        );
        if generated.unfilled_start_slots > 0 {
            log::warn!(
                "Random map is short of spawns: {} start slot(s) could not be \
                 filled (seed={}, players={}); those players have no start position",
                generated.unfilled_start_slots,
                options.seed,
                options.num_players
            );
        }

        progress.milestone(8);
        let mapgen_rng_continuation = generated.mapgen_continuation;
        let generated_construction_trace = generated.construction_trace;
        return Ok(MapLoadInitial {
            map_data: generated.map_file,
            map_source: LoadedMapSource::Generated {
                seed_name: seed_name.to_string(),
            },
            mapgen_rng_continuation: Some(mapgen_rng_continuation),
            generated_construction_trace: Some(generated_construction_trace),
        });
    }

    let loaded_map: LoadedMap =
        if let Some(map_name) = requested_map.filter(|m| !m.eq_ignore_ascii_case("auto")) {
            load_map_by_name_or_path_with_assets(&ra2_dir, map_name, &asset_manager)?
        } else if let Some(ref map_name) = quickplay_map {
            load_map_by_name_or_path_with_assets(&ra2_dir, map_name, &asset_manager)?
        } else if Path::new("testmap1.map").exists() {
            let bytes: Vec<u8> = std::fs::read("testmap1.map")?;
            log::info!("Loading default map: testmap1.map");
            LoadedMap {
                map: MapFile::from_bytes(&bytes)?,
                source: LoadedMapSource::Loose {
                    path: PathBuf::from("testmap1.map"),
                    payload_len: bytes.len(),
                },
            }
        } else {
            let mmx_names: &[&str] = &[
                "Dustbowl.mmx",
                "Barrel.mmx",
                "GoldSt.mmx",
                "Kaliforn.mmx",
                "Hills.mmx",
                "Grinder.mmx",
                "Break.mmx",
                "Potomac.mmx",
                "Arena.mmx",
                "Lostlake.mmx",
                "Oceansid.mmx",
                "Pacific.mmx",
            ];
            try_load_mmx(&ra2_dir, mmx_names)?
        };
    let LoadedMap {
        map: map_data,
        source: map_source,
    } = loaded_map;
    log::info!(
        "Map loaded: title={:?}, theater={}, {}x{}, {} cells, {} entities",
        map_data.basic.name,
        map_data.header.theater,
        map_data.header.width,
        map_data.header.height,
        map_data.cells.len(),
        map_data.entities.len()
    );
    log_trigger_graph_diagnostics(&map_data);

    // First post-render-handoff milestone: the selected map has been opened and
    // parsed. (gamemd emits 8 at theater-init entry; in our pipeline the map is
    // parsed first, so 8 marks the end of this initial phase.)
    progress.milestone(8);

    Ok(MapLoadInitial {
        map_data,
        map_source,
        mapgen_rng_continuation: None,
        generated_construction_trace: None,
    })
}

pub(crate) fn load_map_from_initial(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    asset_manager: &mut AssetManager,
    initial: MapLoadInitial,
    startup: LoadingStartup,
    fresh_scenario_context: FreshScenarioLoadContextDescriptor,
    _skirmish_settings: &crate::ui::main_menu::SkirmishSettings,
    theater_cache_mismatch: bool,
    runtime_color_scheme_count: usize,
    mut vxl_compute: Option<&mut crate::render::vxl_compute::VxlComputeRenderer>,
    native_rules_owner: &mut crate::rules::process_owner::NativeRulesProcessOwner,
    shared_cell_dummy: crate::map::resolved_terrain::SharedCellDummy,
    tile_variant_selector_cache: &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
    progress: &mut dyn crate::app::loading::pump::LoadingProgressSink,
) -> Result<MapLoadResult> {
    let MapLoadInitial {
        mut map_data,
        map_source,
        mapgen_rng_continuation,
        generated_construction_trace,
    } = initial;
    let signed_new_ini_format = map_data.basic.new_ini_format.unwrap_or(0);
    fresh_scenario_context.validate_terminal_transfer(
        &startup,
        &map_source,
        signed_new_ini_format,
    )?;
    let fresh_parts = fresh_scenario_context.into_stock_offline_parts();
    debug_assert_eq!(&fresh_parts.physical_source, &map_source);
    debug_assert_eq!(fresh_parts.signed_new_ini_format, signed_new_ini_format);
    let _startup_provenance = fresh_parts.startup_provenance;
    let materialization = fresh_parts.materialization;
    validate_fresh_transport_before_effects(
        materialization,
        mapgen_rng_continuation.is_some(),
        generated_construction_trace.is_some(),
    )?;
    let match_seed = fresh_parts.match_seed;
    let match_launch_descriptor = fresh_parts.launch;
    let scenario_prefix_plan = fresh_parts.scenario_prefix;
    let skirmish_launch_session = match_launch_descriptor.session();
    let map_hash = match &map_source {
        LoadedMapSource::Loose { .. }
        | LoadedMapSource::Mix { .. }
        | LoadedMapSource::Generated { .. } => Some(map_data.ini.content_hash()),
        LoadedMapSource::LegacyFallback { .. } => None,
    };
    // Load theater INI for tileset lookup, palette, and LAT configuration.
    // Also loads theater-specific MIX archives (e.g., isotemmd.mix) at highest priority.
    let theater_result: Option<theater::TheaterData> =
        theater::load_theater(asset_manager, &map_data.header.theater);
    if theater_cache_mismatch {
        progress.milestone(12);
        // Native advances while rebuilding each color scheme. Rust's theater
        // loader is monolithic, so present the verified pre-load-count sequence
        // synchronously after that work instead of faking per-item callbacks.
        for value in
            crate::app::loading::pump::theater_ramp_changed_values(runtime_color_scheme_count)
        {
            progress.milestone(value);
        }
        progress.milestone(25);
    }
    progress.milestone(30);

    let theater_ext: &'static str = match &theater_result {
        Some(td) => td.extension,
        None => theater_ext_for(&map_data.header.theater),
    };

    // Native map loading always runs the LAT half. The terrain builder no-ops
    // when theater data or a complete LAT group configuration is unavailable.
    let lat_enabled = true;

    // Load rules.ini and art.ini before building resolved terrain so overlay
    // semantics and art-foundation data are available to the pipeline.
    // The selected game mode's override INI (MPModes roster row) applies its
    // rules payload between rulesmd and the map overrides — without it every
    // non-Battle mode silently plays with Battle rules.
    let mode_override_ini: Option<IniFile> = {
        let override_file = skirmish_launch_session.mode.override_file.trim();
        if override_file.is_empty() {
            None
        } else {
            let (data, source) = asset_manager
                .get_with_source(override_file)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "selected game-mode rules override {override_file} is unavailable"
                    )
                })?;
            log::info!(
                "Loading game-mode rules override {} ({} bytes) from {}",
                override_file,
                data.len(),
                source
            );
            Some(IniFile::from_bytes(&data).map_err(|error| {
                anyhow::anyhow!(
                    "failed to parse selected game-mode override {override_file}: {error}"
                )
            })?)
        }
    };
    let (loaded_rules, rules_ini, fixed_art_ini, native_rules_receipt) = native_rules_owner
        .load_noncampaign_scenario(mode_override_ini.as_ref(), &map_data.ini)
        .map_err(|error| anyhow::anyhow!("failed native noncampaign rules rebuild: {error}"))?
        .into_parts();
    let bound_scenario_prefix =
        scenario_prefix_plan.bind_native_rules_receipt(native_rules_receipt);
    let fixed_team_ai_ini =
        crate::app::loading::init_helpers::load_retail_team_ai_source(&asset_manager)
            .ok_or_else(|| anyhow::anyhow!("failed to load active YR aimd.ini"))?;
    let team_ai_registry = crate::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
        &fixed_team_ai_ini,
        &map_data.ini,
        true,
    );
    if !team_ai_registry.fixed_source_is_complete() {
        anyhow::bail!(
            "active YR aimd.ini failed structural validation: fixed_counts={:?}, diagnostics={:?}",
            team_ai_registry.fixed_counts,
            team_ai_registry.diagnostics
        );
    }
    for diagnostic in &team_ai_registry.diagnostics {
        log::warn!("Team AI INI diagnostic: {diagnostic:?}");
    }
    let mut rules: Option<RuleSet> = Some(loaded_rules);
    let mut art = Some(ArtRegistry::from_ini(&fixed_art_ini));
    let art_ini = Some(fixed_art_ini);
    if let (Some(r), Some(a)) = (rules.as_mut(), art.as_mut()) {
        a.apply_anim_type_read_states(&r.anim_type_art_read_states);
        r.merge_art_data(a);
        // Eagerly populate per-anim SHP frame dimensions so the smudge
        // dispatcher can size-filter without falling back to the (30, 30)
        // default that always loses the threshold check.
        let (populated, fallback) =
            a.populate_anim_frame_dims(&asset_manager, theater_ext, &map_data.header.theater);
        log::info!(
            "Anim frame dims: {} populated, {} fallback (defaults to 30x30)",
            populated,
            fallback,
        );
        // Retain the art registry on RuleSet so dispatchers (e.g. smudge
        // spawning) can read per-anim spawn flags via &RuleSet alone.
        // Cloned because downstream consumers in this function still read
        // through the `art` Option (lighting, sidebar, sim spawn, etc.).
        r.art_registry = a.clone();
    }
    // Resolve warp animation rates from art.ini sections (e.g., [WARPOUT] Rate=120).
    if let (Some(r), Some(art_ini_file)) = (&mut rules, &art_ini) {
        r.general.resolve_art_rates(art_ini_file);
    }
    // Parse infantry animation sequence definitions from art.ini [*Sequence] sections.
    let infantry_sequences = if let Some(ref art_ini_file) = art_ini {
        crate::rules::infantry_sequence::parse_infantry_sequence_registry(art_ini_file)
    } else {
        HashMap::new()
    };
    // Rules + art parsed, merged, and processed (gamemd command-bar/CD/rules
    // milestones).
    progress.milestone(31);
    progress.milestone(35);
    progress.milestone(45);
    let csf = Some(load_csf(&asset_manager)?);
    let overlay_registry: OverlayTypeRegistry =
        OverlayTypeRegistry::from_ini(&rules_ini, art_ini.as_ref());

    // Compute playable area bounds from LocalSize (border filler hidden by shroud).
    let local_bounds: Option<LocalBounds> = Some(LocalBounds::from_header(&map_data.header));

    let cliff_back = rules
        .as_ref()
        .map(|r| r.general.cliff_back_impassability)
        .unwrap_or(2);

    // Parse Scenario-owned lighting before Fill so the one authoritative
    // Simulation can be constructed and receive the prefix cursors first.
    // Presentation milestones remain at their native-shaped point below.
    let lighting_config = lighting::parse_lighting(&map_data.ini);
    let lighting_profiles = lighting::parse_lighting_profiles(&map_data.ini);
    let native_start_bounds =
        crate::sim::scenario_bootstrap::NativeStartBounds::from_map_header(&map_data.header)
            .ok_or_else(|| anyhow::anyhow!("map Size does not produce a valid fresh cell array"))?;
    let scenario_cell_extent = native_start_bounds
        .min_rx
        .checked_add(native_start_bounds.width)
        .ok_or_else(|| anyhow::anyhow!("fresh cell-array extent overflow"))?;
    let mut bootstrap_rng = ScenarioBootstrapRng::new(match_seed);
    if let Some(continuation) = mapgen_rng_continuation {
        bootstrap_rng.install_generated_mapgen_continuation(continuation);
    }
    let scenario_descriptor = crate::sim::scenario_session::ScenarioDescriptor {
        seed: match_seed,
        map_name: skirmish_launch_session
            .selected_map_file
            .clone()
            .or_else(|| map_data.basic.name.clone())
            .unwrap_or_default(),
        theater: map_data.header.theater.clone(),
        game_mode_nonzero: true,
        // Campaign/editor reads `[SpecialFlags] Inert=`. Nonzero game modes
        // replace active SpecialFlags from session staging.
        no_damage: false,
        free_radar: map_data.basic.free_radar.unwrap_or(false),
        // Skirmish start forces `TiberiumGrows|TiberiumSpreads` (`OR 0xC0`
        // at `0x005E74CD`), copied into the scenario at `0x00687C23`.
        tiberium_grows_flag: true,
        tiberium_spreads_flag: true,
        // Native Resize constructs a square cell-array extent of SizeW+SizeH.
        map_width: scenario_cell_extent,
        map_height: scenario_cell_extent,
        local_left: map_data.header.local_left as u16,
        local_top: map_data.header.local_top as u16,
        local_width: map_data.header.local_width as u16,
        local_height: map_data.header.local_height as u16,
        mp_start_waypoints: scenario_start_waypoints_for_load(
            &map_data,
            Some(bound_scenario_prefix.projection()),
        ),
        pixel_conversion_bounds: Default::default(),
        lighting: crate::sim::scenario_session::ScenarioLightingState::new(
            crate::sim::scenario_session::ScenarioLightProfileUnits {
                ambient_percent: lighting_profiles.normal.ambient_percent,
                red_percent: lighting_profiles.normal.red_percent,
                green_percent: lighting_profiles.normal.green_percent,
                blue_percent: lighting_profiles.normal.blue_percent,
                ground_units: lighting_profiles.normal.ground_units,
                level_units: lighting_profiles.normal.level_units,
            },
            crate::sim::scenario_session::ScenarioLightProfileUnits {
                ambient_percent: lighting_profiles.ion.ambient_percent,
                red_percent: lighting_profiles.ion.red_percent,
                green_percent: lighting_profiles.ion.green_percent,
                blue_percent: lighting_profiles.ion.blue_percent,
                ground_units: lighting_profiles.ion.ground_units,
                level_units: lighting_profiles.ion.level_units,
            },
        ),
    };
    log::info!("Match seed: 0x{:08X}", scenario_descriptor.seed);
    // Consume the paired RNG/native-ID prefix here: Fill and every later load
    // constructor now mutate the same Simulation identity that reaches gameplay.
    let (mut staged_simulation, scenario_prefix_projection) = bootstrap_rng
        .into_stock_offline_staged_simulation(&scenario_descriptor, bound_scenario_prefix)?;
    staged_simulation.bind_shared_cell_dummy(shared_cell_dummy.clone());
    let (mut scenario_fill_rng, mut variant_main_rng) = staged_simulation.terrain_load_draws();
    let mut scenario_fill_ranged =
        |low, high| scenario_fill_rng.next_range_u32_inclusive(low, high);
    let mut variant_draw = || variant_main_rng.next_u32();
    let mut variant_selector = tile_variant_selector_cache.begin_load(&mut variant_draw);
    // Native `MapClass::Resize @ 0x00565C10` reconstructs the fixed fallback
    // CellClass through `CellClass::Constructor @ 0x0047BBF0` before Fill and
    // IsoMapPack materialize the new map. Reset its modeled bytes in place;
    // replacing this handle would break process-global pointer identity.
    shared_cell_dummy.reconstruct_for_map_resize();
    let (mut resolved_terrain, mut authored_terrain_fill) = match materialization {
        FreshMapMaterialization::Authored => {
            let fill =
                ResolvedTerrainGrid::build_pending_authored_with_variant_selector_and_shared_dummy(
                    &map_data,
                    theater_result.as_ref(),
                    Some(&asset_manager),
                    rules.as_ref().map(|r| &r.terrain_rules),
                    Some(&overlay_registry),
                    lat_enabled,
                    cliff_back,
                    &mut scenario_fill_ranged,
                    &mut variant_selector,
                    shared_cell_dummy,
                );
            (None, Some(fill))
        }
        FreshMapMaterialization::AcceptedGenerated => (
            Some(
                ResolvedTerrainGrid::build_with_variant_selector_and_shared_dummy(
                    &map_data,
                    theater_result.as_ref(),
                    Some(&asset_manager),
                    rules.as_ref().map(|r| &r.terrain_rules),
                    Some(&overlay_registry),
                    rules.as_ref().map(|r| &r.terrain_object_types),
                    lat_enabled,
                    cliff_back,
                    &mut scenario_fill_ranged,
                    &mut variant_selector,
                    shared_cell_dummy,
                    materialization.overlay_load_source(),
                    scenario_descriptor.pixel_conversion_bounds,
                ),
            ),
            None,
        ),
    };
    let materialized_dimensions = resolved_terrain
        .as_ref()
        .map(|terrain| (terrain.width(), terrain.height()))
        .or_else(|| authored_terrain_fill.as_ref().map(|fill| fill.dimensions()))
        .expect("fresh materialization produced one terrain owner");
    if materialized_dimensions.0 != scenario_descriptor.map_width
        || materialized_dimensions.1 != scenario_descriptor.map_height
    {
        anyhow::bail!(
            "fresh Resize extent {}x{} disagrees with materialized terrain {}x{}",
            scenario_descriptor.map_width,
            scenario_descriptor.map_height,
            materialized_dimensions.0,
            materialized_dimensions.1,
        );
    }
    let variant_table_generated = variant_selector.generated_table();
    let map_fill_scenario_advances = variant_selector.map_fill_scenario_advance_count();
    let variant_table_draws = variant_selector.raw_draw_count();
    drop(variant_selector);
    drop(variant_draw);
    drop(scenario_fill_ranged);
    drop(variant_main_rng);
    drop(scenario_fill_rng);
    // Bind the complete scheduler closure only after theater Tile##Anim rows
    // have resolved, but before any atlas or AnimClass construction. Missing
    // tile art is a load error rather than a silently invisible map feature.
    if let (Some(r), Some(a)) = (rules.as_mut(), art.as_mut()) {
        let roots = scheduler_anim_roots(
            r,
            &overlay_registry,
            resolved_terrain
                .as_ref()
                .map_or(&[], |terrain| terrain.tile_animations()),
        );
        a.bind_scheduler_anim_assets(
            &roots,
            &asset_manager,
            theater_ext,
            &map_data.header.theater,
        )?;
        // Every other AnimClass producer and the building animations need the
        // same loader-derived End/LoopEnd. Tolerant by design; must follow the
        // strict pass, which rewrites the scheduler-owned set wholesale.
        let unbound_explosion_roots = a.bind_anim_class_assets(
            &crate::app::loading::init_helpers::tolerant_anim_class_roots(r, a),
            &asset_manager,
            theater_ext,
            &map_data.header.theater,
        );
        crate::rules::effect_asset_catalog::log_unbound_combat_explosion_roots(
            unbound_explosion_roots,
        );
        r.art_registry = a.clone();
        r.bind_effect_assets(&asset_manager, theater_ext, &map_data.header.theater);
        r.bind_terrain_spawner_assets(
            &rules_ini,
            &asset_manager,
            theater_ext,
            &map_data.header.theater,
        );
        r.bind_animation_sequences(&infantry_sequences);
    }
    // `Read_Map_Section_And_IsoMapPacks @ 0x004ACE70` snapshots the fresh
    // prefix before theater Tile##Anim resolution, then sets the cursor from
    // that snapshot plus 0x2710. Only after that reservation does
    // `MapClass::ReadTubesINI @ 0x007283C0` allocate, assign, and parse each
    // raw row. Keep these successful bindings separate from the existing
    // convenience topology; its later owning transaction consumes the IDs
    // without allocating again.
    staged_simulation
        .construct_native_map_tubes(&map_data.ini)
        .map_err(|error| anyhow::anyhow!("failed native [Tubes] construction: {error}"))?;
    if materialization == FreshMapMaterialization::AcceptedGenerated {
        // `RandomMapGenerator::InitMapFromSyntheticINI @ 0x00599650` (launch
        // branch `0x00599A3A..0x00599A5B`) runs `Full_Init` with DL=1, whose
        // `Clear_Scene` nulls `DAT_00A8ED78`, then calls the post-load setup
        // `FUN_00684C30`, which constructs the GasCloudSys ParticleSystem before
        // any generator constructor. The later post-`Post_Map_Init` setup finds
        // it constructed and spends nothing.
        staged_simulation.construct_post_load_particle_system_id();
    }
    // Launch-time `.SED` generation already chose all geometry. Replay only
    // its Techno constructor effects now, after the Full-Init stock-offline
    // prefix and terrain Fill, on the one Scenario owner later moved into Simulation.
    let generated_techno_inits =
        construction_trace_after_fill(materialization, generated_construction_trace.as_ref())?
            .map(|trace| staged_simulation.replay_staged_generated_construction_trace(trace))
            .transpose()?;
    // Native Fill snapshots prior process-global ClearTile/WaterSet values
    // before the current theater registry reload. Rust loads assets earlier,
    // so defer publishing current results until materialization is complete.
    if let Some(theater) = theater_result.as_ref() {
        tile_variant_selector_cache.complete_theater_registry_load(
            theater.rmg_tiles.clear_tile,
            theater.rmg_tiles.water_set,
        );
    }
    log::info!(
        "Map terrain load: {} Scenario Fill cursor advances; TMP variant table {} this load, {} raw Main draws",
        map_fill_scenario_advances,
        if variant_table_generated {
            "generated"
        } else if tile_variant_selector_cache.is_initialized() {
            "reused"
        } else {
            "not reached"
        },
        variant_table_draws,
    );
    let art_fallback: ArtRegistry = ArtRegistry::empty();
    let overlay_shp_ids = resolved_overlay_shp_ids(
        &overlay_registry,
        &rules_ini,
        art.as_ref().unwrap_or(&art_fallback),
        &asset_manager,
        theater_ext,
        &map_data.header.theater,
    );
    // Parse house color assignments from map INI ([Houses] + per-house Color=).
    // Color=<name> resolves against the rules `[Colors]` list (entry index).
    let color_schemes: &[crate::rules::color_scheme::ColorSchemeEntry] = rules
        .as_ref()
        .map(|r| r.color_schemes.as_slice())
        .unwrap_or(&[]);
    let house_roster: HouseRoster =
        houses::parse_house_roster(&map_data.ini, color_schemes, rules.as_ref());
    let house_color_map: HouseColorMap =
        house_color_map_for_launch_session(skirmish_launch_session, &house_roster);
    progress.milestone(67);
    let bridge_destroyability_mode = BridgeDestroyabilityMode::SkirmishOrMultiplayer {
        bridge_destruction: skirmish_launch_session.options.bridges_destroyable,
    };
    let overlay_grid = match materialization {
        FreshMapMaterialization::Authored => {
            let theater = theater_result
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("authored map load requires theater data"))?;
            let ruleset = rules
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("authored map load requires merged rules"))?;
            let art_registry = art
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("authored map load requires art data"))?;
            let fill = authored_terrain_fill
                .take()
                .expect("authored materialization retained its pending Fill owner");
            let output = crate::sim::runtime::finalize_and_populate_staged_authored_scenario(
                &mut staged_simulation,
                &mut map_data,
                fill,
                theater,
                &asset_manager,
                ruleset,
                art_registry,
                &overlay_registry,
                &overlay_shp_ids,
                signed_new_ini_format,
                lat_enabled,
                cliff_back,
                theater_ext,
                &scenario_descriptor.theater,
                bridge_destroyability_mode,
                &scenario_descriptor,
                |sim| {
                    initialize_skirmish_launch_houses(
                        sim,
                        &house_roster,
                        ruleset,
                        &match_launch_descriptor,
                    );
                },
            )?;
            resolved_terrain = Some(output.resolved_terrain);
            if let (Some(ruleset), Some(art_registry)) = (rules.as_mut(), art.as_ref()) {
                ruleset.art_registry = art_registry.clone();
            }
            output.overlay_grid
        }
        FreshMapMaterialization::AcceptedGenerated => {
            let resolved_terrain = resolved_terrain
                .as_mut()
                .expect("generated materialization retained its eager terrain");
            let mut overlay_grid = crate::sim::overlay_grid::OverlayGrid::from_native_overlay_packs(
                &map_data.overlays,
                map_data.overlay_data_pack().map_err(|_| {
                    anyhow::anyhow!("generated overlay packs were already consumed")
                })?,
                resolved_terrain,
                &overlay_registry,
                &overlay_shp_ids,
                true,
            );
            let cleared_terrain_overlay_cells =
                rules.as_ref().map_or_else(BTreeSet::new, |rules| {
                    crate::sim::terrain_spawn::clear_tiberium_source_cells_for_terrain(
                        &mut overlay_grid,
                        resolved_terrain,
                        &map_data.terrain_objects,
                        rules,
                        &overlay_registry,
                    )
                });
            if !cleared_terrain_overlay_cells.is_empty() {
                log::info!(
                    "Cleared {} same-cell tiberium overlay cell(s) for recognized terrain",
                    cleared_terrain_overlay_cells.len(),
                );
            }
            let construction_height_map = resolved_terrain.build_height_map();
            crate::app::loading::init_helpers::populate_staged_app_scenario(
                &mut staged_simulation,
                &map_data,
                resolved_terrain,
                &map_data.header.theater,
                rules.as_ref(),
                &construction_height_map,
                Some(&overlay_registry),
                Some(&overlay_grid),
                bridge_destroyability_mode,
                &scenario_descriptor,
                generated_techno_inits.as_ref(),
                |sim| {
                    let ruleset = rules
                        .as_ref()
                        .expect("offline skirmish requires rules before House construction");
                    initialize_skirmish_launch_houses(
                        sim,
                        &house_roster,
                        ruleset,
                        &match_launch_descriptor,
                    );
                },
            )?;
            // `RandomMapGenerator::Generate @ 0x00598960` tail (`0x00599370..
            // 0x0059945B`): after the generator constructors and its final
            // whole-map Recalc (`0x0059937D`), `TiberiumClass::InitGrowthQueues_All
            // @ 0x00722D00` then `InitSpreadQueues_All @ 0x00722240` scan the
            // then-current painted densities; only afterwards does
            // `MapClass::InitCellAttributes(1)` (`push 1` at `0x0059943F`, call
            // at `0x0059944C`) rewrite every ore cell's density from its
            // same-class neighbours. The post-map tail must not rebuild those
            // queues from the germinated state.
            let ruleset = rules
                .as_ref()
                .expect("offline skirmish requires rules before the generator tail");
            let queue_stats = crate::sim::runtime::initialize_native_tiberium_queues(
                &mut staged_simulation,
                &map_data.basic,
                &map_data.special_flags,
                ruleset,
                &overlay_registry,
                Some(&overlay_grid),
                (map_data.header.width as u16, map_data.header.height as u16),
            );
            if let Some(stats) = queue_stats {
                log::info!(
                    "Initialized generated native tiberium queues before germination: {} growth, {} spread",
                    stats.growth_entries,
                    stats.spread_entries,
                );
            }
            let germination = crate::sim::tiberium_germinate::run_generated_final_cell_attributes(
                &*resolved_terrain,
                &mut overlay_grid,
                &ruleset.tiberium_types,
                &overlay_registry,
                map_data.header.width as u16,
                map_data.header.height as u16,
            );
            log::info!(
                "Generated final InitCellAttributes(1): {} of {} real cells germinated (value total {})",
                germination.germinated_cells,
                germination.real_cells,
                germination.tiberium_value_total,
            );
            if germination.unallocated_cells != 0 {
                log::error!(
                    "Generated final InitCellAttributes(1) reached {} unallocated iterator cells",
                    germination.unallocated_cells,
                );
            }
            overlay_grid
        }
    };
    let resolved_terrain =
        resolved_terrain.expect("authored or generated materialization produced final terrain");

    // Every exported terrain/presentation surface is derived only after the
    // authored final sweep. Generated materialization reaches this same seam
    // with its existing eager grid unchanged.
    let height_map: BTreeMap<(u16, u16), u8> = resolved_terrain.build_height_map();
    let bridge_height_map: BTreeMap<(u16, u16), u8> = resolved_terrain.build_bridge_height_map();
    let tactical_bridge_inverse_map: BTreeMap<(u16, u16), crate::map::terrain::TacticalBridgeCell> =
        resolved_terrain.build_tactical_bridge_inverse_map();
    let anchor_variant_table = theater_result
        .as_ref()
        .and_then(crate::map::theater::BridgeAnchorVariantTable::from_theater);
    let grid: TerrainGrid = terrain::build_terrain_grid_from_resolved(
        &resolved_terrain,
        local_bounds,
        anchor_variant_table,
    );
    progress.milestone(50);
    progress.milestone(55);
    progress.milestone(58);
    progress.milestone(60);
    let tile_atlas: Option<TileAtlas> = match &theater_result {
        Some(td) => build_tile_atlas(
            &asset_manager,
            &td.lookup,
            &td.iso_palette,
            td.extension,
            &grid,
            gpu,
            batch,
        ),
        None => None,
    };
    progress.milestone(63);
    progress.milestone(65);
    let bridge_railing_tile_bases = theater_result
        .as_ref()
        .and_then(|td| td.bridge_railing_slope_starts())
        .map(
            |(slope_set_pieces_start, slope_set_pieces2_start)| BridgeRailingTileBases {
                slope_set_pieces_start,
                slope_set_pieces2_start,
            },
        );
    let (unit_palette, overlay_iso_palette, overlay_tiberium_palette) = match theater_result {
        Some(td) => (
            Some(td.unit_palette),
            Some(td.iso_palette),
            Some(td.tiberium_palette),
        ),
        None => (None, None, None),
    };
    progress.milestone(68);
    progress.milestone(69);
    progress.milestone(70);

    crate::app::loading::init_helpers::bind_staged_app_scenario_metadata(
        &mut staged_simulation,
        &asset_manager,
        rules.as_ref(),
        art.as_ref(),
    );
    // F09 seam: presentation derives from the one staged Simulation and never
    // feeds state back into it.
    let manifest = crate::app::loading::init_helpers::build_presentation_manifest(
        &staged_simulation,
        &asset_manager,
        gpu,
        batch,
        theater_ext,
        &map_data.header.theater,
        rules.as_ref(),
        art.as_ref(),
        &overlay_registry,
        &house_color_map,
        unit_palette.as_ref(),
        overlay_iso_palette.as_ref(),
        vxl_compute.as_deref_mut(),
    );
    let (mut unit_atlas, mut sprite_atlas, mut palette_set) = (
        manifest.unit_atlas,
        manifest.sprite_atlas,
        manifest.palette_set,
    );
    // Terrain/tiberium + units/infantry/buildings created from the map
    // (gamemd terrain/units/objects/buildings milestones).
    progress.milestone(72);
    progress.milestone(74);
    progress.milestone(76);
    progress.milestone(78);
    let mut simulation = Some(staged_simulation);
    // Pre-intern all rule type IDs so that build_option_for_owner can resolve
    // InternedIds for types that haven't been spawned yet (e.g. GAPOWR).
    // Without this, sidebar cameo lookups fail because unspawned types get
    // InternedId(0) and resolve to the wrong string.
    if let (Some(sim), Some(ruleset)) = (&mut simulation, rules.as_ref()) {
        sim.intern_rule_type_ids(ruleset);
        // One-hop type resolution: build the handle table now that every type
        // id is interned. This also pre-resolves the `[CombatDamage]` bridge
        // warhead handles combat compares during the bridge-damage path;
        // resolution must happen before any combat tick.
        sim.resolve_type_handles(ruleset);
        let diagnostics = sim.install_team_ai_registry(&team_ai_registry, ruleset);
        if diagnostics
            .iter()
            .any(crate::sim::team_script_vm::TeamAiInstallDiagnostic::is_fixed_source_refusal)
        {
            anyhow::bail!(
                "active YR aimd.ini failed RuleSet resolution: diagnostics={diagnostics:?}"
            );
        }
        for diagnostic in diagnostics {
            log::warn!("Team AI install diagnostic: {diagnostic:?}");
        }
    }

    // SpawnPick phase is disabled — MCV always spawns directly at the chosen position.
    let spawn_pick_pending: bool = false;

    let mut initial_local_owner: Option<String> = None;
    if !spawn_pick_pending {
        if let (Some(sim), Some(ruleset)) = (&mut simulation, rules.as_ref()) {
            let result = apply_pre_fill_scenario_prefix_launch_session_with_overlay_registry(
                sim,
                &map_data,
                &house_roster,
                ruleset,
                &height_map,
                &resolved_terrain,
                &match_launch_descriptor,
                &overlay_registry,
                &scenario_prefix_projection,
            );
            initial_local_owner = result.local_owner;
            let should_rebuild_entity_atlases = result.spawned_mcvs > 0;

            if should_rebuild_entity_atlases {
                let (new_unit_atlas, new_sprite_atlas, new_palette_set) = build_entity_atlases(
                    sim,
                    &asset_manager,
                    gpu,
                    batch,
                    theater_ext,
                    &map_data.header.theater,
                    rules.as_ref(),
                    art.as_ref(),
                    &overlay_registry,
                    &house_color_map,
                    unit_palette.as_ref(),
                    overlay_iso_palette.as_ref(),
                    vxl_compute.as_deref_mut(),
                );
                unit_atlas = new_unit_atlas;
                sprite_atlas = new_sprite_atlas;
                palette_set = new_palette_set;
            }
        }
    }

    // Optional debug spawn list for render testing.
    // Examples:
    //   RA2_DEBUG_SPAWN_UNITS=1                  -> default list (HTNK,MTNK,E1)
    //   RA2_DEBUG_SPAWN_UNITS=HTNK,MTNK,APOC
    if let (Some(sim), Some(ruleset), Some(debug_units)) = (
        &mut simulation,
        rules.as_ref(),
        parse_debug_spawn_units_env(),
    ) {
        let owner: String = house_color_map
            .keys()
            .find(|h| {
                let up = h.to_ascii_uppercase();
                up != "NEUTRAL" && up != "SPECIAL"
            })
            .cloned()
            .unwrap_or_else(|| "Americans".to_string());

        let (anchor_rx, anchor_ry): (u16, u16) = map_data
            .entities
            .iter()
            .find(|e| {
                e.category == crate::map::entities::EntityCategory::Structure
                    && e.owner.eq_ignore_ascii_case(&owner)
            })
            .map(|e| (e.cell_x, e.cell_y))
            .or_else(|| {
                waypoints::first_multiplayer_start(&map_data.waypoints).map(|wp| (wp.rx, wp.ry))
            })
            .or_else(|| map_data.cells.first().map(|c| (c.rx, c.ry)))
            .unwrap_or((50, 50));

        let offsets: &[(i32, i32)] = &[
            (2, 2),
            (4, 2),
            (6, 2),
            (2, 4),
            (4, 4),
            (6, 4),
            (2, 6),
            (4, 6),
        ];
        let mut spawned: u32 = 0;
        for (i, type_id) in debug_units.iter().enumerate() {
            let (ox, oy) = offsets[i % offsets.len()];
            let rx = (anchor_rx as i32 + ox).max(0) as u16;
            let ry = (anchor_ry as i32 + oy).max(0) as u16;
            if sim
                .spawn_object_with_overlay_registry(
                    type_id,
                    &owner,
                    rx,
                    ry,
                    64,
                    ruleset,
                    &height_map,
                    &overlay_registry,
                )
                .is_some()
            {
                spawned += 1;
            } else {
                log::warn!("Debug spawn failed for '{}'", type_id);
            }
        }
        if spawned > 0 {
            log::info!(
                "Debug-spawned {} unit(s) for owner={} near ({},{}): {:?}",
                spawned,
                owner,
                anchor_rx,
                anchor_ry,
                debug_units
            );
        }
    }

    let (
        overlay_atlas,
        bridge_atlas,
        bridge_railing_atlas,
        overlay_names,
        mut overlays_connected,
        overlay_radar_colors,
    ) = build_overlay_atlas_from_map(
        &map_data,
        &asset_manager,
        gpu,
        batch,
        theater_ext,
        &rules_ini,
        &rules
            .as_ref()
            .expect("merged rules were installed before atlas construction")
            .crate_rules,
        art.as_ref().unwrap_or(&art_fallback),
        overlay_iso_palette.as_ref(),
        unit_palette.as_ref(),
        overlay_tiberium_palette.as_ref(),
        rules.as_ref().map(|r| &r.smudge_types),
        bridge_railing_tile_bases,
    );

    // F09: the shared post-funnel finalization — spawner seed, map-wall owner
    // reconstruction, overlay-grid installation, smudge-grid seeding, and the
    // authoritative post-map tail — runs through the same sim-owned function
    // the headless loader uses.
    //
    // Authored presentation is rebuilt from the completed second-sweep grid;
    // generated presentation retains its legacy source entries and only drops
    // identities cleared during eager projection. Do this before the shared
    // post-map tail can place scenario-start crates, because those runtime
    // overlays are not map-authored presentation entries.
    if materialization == FreshMapMaterialization::Authored {
        overlays_connected = overlay_grid
            .iter_occupied()
            .filter_map(|(rx, ry, cell)| {
                cell.overlay_id.map(|overlay_id| OverlayEntry {
                    rx,
                    ry,
                    overlay_id,
                    frame: cell.overlay_data,
                })
            })
            .collect();
    } else {
        overlays_connected.retain(|entry| {
            overlay_grid.cell(entry.rx, entry.ry).overlay_id == Some(entry.overlay_id)
        });
        let refreshed = refresh_generated_tiberium_presentation_frames(
            &mut overlays_connected,
            &overlay_grid,
            &overlay_registry,
            &rules
                .as_ref()
                .expect("merged rules were installed before atlas construction")
                .tiberium_types,
        );
        if refreshed != 0 {
            log::info!(
                "Refreshed {refreshed} generated ore presentation frame(s) from the germinated grid"
            );
        }
    }
    let rules_for_post_map = rules
        .as_ref()
        .expect("merged rules were installed before post-map finalization");
    if let Some(sim) = &mut simulation {
        let output = crate::sim::runtime::finalize_constructed_scenario(
            sim,
            &map_data,
            rules_for_post_map,
            &overlay_registry,
            overlay_grid,
            &house_roster,
            Some(&match_launch_descriptor),
        );
        if !output.navigation_published {
            log::error!("Initial navigation rebuild failed: resolved terrain is unavailable");
        }
        let connected_crates = connect_startup_crate_overlays(&mut overlays_connected, sim);
        if connected_crates != 0 {
            log::info!(
                "Connected {connected_crates} visible startup crate cell(s) to initial overlay presentation"
            );
        }
    }

    // Anchor the opening view on the LOCAL player's start — retail opens a
    // skirmish looking at your own MCV, and anything else strands the player
    // staring at shroud. The local house's units are already spawned at the
    // assigned start slot by this point, so the MCV's actual cell is the
    // authoritative anchor. Falling back to the first multiplayer start
    // waypoint is wrong on any map with more than one start slot (it is some
    // OTHER player's corner unless you happened to draw slot one); it remains
    // only as the no-local-spawn fallback, then the middle of the playable
    // area for maps with no start waypoints at all.
    //
    // This is a **world point**, not a camera position: the sidebar's real width
    // depends on the UI scale, which only exists once `AppState` is built, so the
    // conversion to a camera top-left happens in `loading::transitions` where the
    // scaled layout spec is available.
    let local_start_cell: Option<(u16, u16)> = initial_local_owner
        .as_deref()
        .zip(simulation.as_ref())
        .and_then(|(owner, sim)| {
            let owner_id = sim.interner.get(owner)?;
            sim.substrate
                .entities
                .keys_sorted()
                .into_iter()
                .filter_map(|id| sim.substrate.entities.get(id))
                .find(|e| e.owner() == owner_id)
                .map(|e| (e.position.rx, e.position.ry))
        });
    let (camera_anchor_x, camera_anchor_y): (f32, f32) = if let Some((rx, ry)) = local_start_cell {
        let z = height_map.get(&(rx, ry)).copied().unwrap_or(0);
        crate::app::input::camera::cell_centre_world_point(rx, ry, z)
    } else if let Some(start_wp) = waypoints::first_multiplayer_start(&map_data.waypoints) {
        let wp_z = height_map
            .get(&(start_wp.rx, start_wp.ry))
            .copied()
            .unwrap_or(0);
        crate::app::input::camera::cell_centre_world_point(start_wp.rx, start_wp.ry, wp_z)
    } else {
        let (area_x, area_y, area_w, area_h) = match local_bounds {
            Some(b) => (b.pixel_x, b.pixel_y, b.pixel_w, b.pixel_h),
            None => (
                grid.origin_x,
                grid.origin_y,
                grid.world_width,
                grid.world_height,
            ),
        };
        (area_x + area_w / 2.0, area_y + area_h / 2.0)
    };
    // Load cameo MIX archives so that *ICON.SHP files are findable.
    // These nested MIXes live inside local.mix/localmd.mix and aren't
    // auto-extracted by the two-level brute-force pass.
    for cameo_mix in ["cameomd.mix", "cameo.mix"] {
        match asset_manager.load_nested(cameo_mix) {
            Ok(()) => log::info!("Loaded nested {cameo_mix} for sidebar cameo icons"),
            Err(_) => log::debug!("{cameo_mix} not found (optional)"),
        }
    }
    let sidebar_cameo_atlas =
        build_sidebar_cameo_atlas(gpu, batch, &asset_manager, rules.as_ref(), art.as_ref());
    let sidebar_chrome =
        crate::render::sidebar_chrome::build_sidebar_chrome_set(gpu, batch, &asset_manager);
    let fnt_file = asset_manager.get_ref("GAME.FNT").and_then(|data| {
        crate::assets::fnt_file::FntFile::from_bytes(data)
            .map_err(|e| log::warn!("Failed to parse GAME.FNT: {e}"))
            .ok()
    });
    let building_zshape =
        crate::render::building_zshape::BuildingZShape::load(gpu, batch, &asset_manager);
    if building_zshape.is_none() {
        log::warn!("BUILDNGZ.SHA not loaded: building bodies write flat depth");
    }
    let software_cursor = cursor_atlas::build_software_cursor(gpu, batch, &asset_manager);
    if software_cursor.is_some() {
        log::info!("Software cursor loaded from mouse.sha — OS cursor will be hidden");
    } else {
        log::warn!("Software cursor NOT loaded (mouse.sha missing?) — using OS cursor");
    }
    let lighting_grid = rebuild_lighting_grid_from_sim(
        &resolved_terrain,
        &lighting_config,
        simulation.as_ref(),
        rules.as_ref(),
        2,
    );
    // Final post-map-init milestones (cell attributes, beacon art, post-map
    // init, tactical cleanup, final pre-render refresh). 100 is emitted by the
    // pump at Finished.
    progress.milestone(82);
    progress.milestone(86);
    progress.milestone(90);
    progress.milestone(93);
    progress.milestone(96);
    progress.milestone(98);

    // Move fields out of map_data (last use) instead of cloning.
    let theater_name = map_data.header.theater;
    Ok(MapLoadResult {
        scenario: ScenarioLoadInputs {
            startup,
            map_source,
            map_hash,
            basic: map_data.basic,
            terrain_grid: Some(grid),
            resolved_terrain: Some(resolved_terrain),
            simulation,
            overlays: overlays_connected,
            terrain_objects: map_data.terrain_objects,
            waypoints: map_data.waypoints,
            cell_tags: map_data.cell_tags,
            tags: map_data.tags,
            triggers: map_data.triggers,
            events: map_data.events,
            actions: map_data.actions,
            trigger_graph: map_data.trigger_graph,
            overlay_registry,
            house_roster,
            height_map,
            bridge_height_map,
            tactical_bridge_inverse_map,
            rules,
            map_lighting_config: lighting_config,
            theater_name,
            theater_ext: theater_ext.to_string(),
            sandbox_full_visibility: false,
            spawn_pick_pending,
            initial_local_owner,
            camera_anchor_x,
            camera_anchor_y,
        },
        presentation: PresentationLoadAssets {
            tile_atlas,
            building_zshape,
            unit_atlas,
            palette_set,
            sprite_atlas,
            overlay_atlas,
            bridge_atlas,
            bridge_railing_atlas,
            sidebar_cameo_atlas,
            sidebar_chrome,
            software_cursor,
            overlay_names,
            overlay_radar_colors,
            house_color_map,
            lighting_grid,
            csf,
            fnt_file,
        },
        // The app loading job retains the one process-lifetime manager while
        // this borrowed phase runs, then moves it into the completed result.
        asset_manager: None,
    })
}

/// Register non-local playable houses as AI opponents.
#[cfg(test)]
mod random_map_retail_tests {
    //! Retail-input pass over the real `.SED` load path.
    //!
    //! This is the only place retail theater data meets the generator's
    //! emitted tile indices. The in-crate matrix cannot reach that clause at
    //! all: its synthetic tile-block stub answers every id, so resolvability
    //! is true there by construction rather than by generation being correct.
    //!
    //! It drives `load_map_initial_with_assets` — the production loop — instead
    //! of reassembling that function's resolution steps in test code. A
    //! mirrored sequence keeps passing after the production callsite drifts,
    //! which is exactly how every generated map shipped without a single
    //! neutral tech building while the phase itself was fully implemented and
    //! tested.
    //!
    //! Still not parity evidence. It shows the emitted surface is *loadable*,
    //! never that it matches the original.

    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::map::rmg::RmgOptions;

    struct SilentProgress;
    impl crate::app::loading::pump::LoadingProgressSink for SilentProgress {
        fn milestone(&mut self, _percent: u32) {}
    }

    /// The configurations the dialog itself can emit.
    const RETAIL_MAP_TYPES: [i32; 4] = [1, 2, 3, 4];
    /// Comfortably above the river gate of 20, so the carved map types seed a
    /// lake *and* a river and both get their tiles resolved against retail
    /// theater data.
    const RETAIL_WATER_AMOUNT: i32 = 50;
    const RETAIL_THEATERS: [i32; 2] = [0, 1];

    /// A generated map must carry far more than this; the bar exists only to
    /// catch a run that emitted nothing real and would therefore pass the
    /// resolvability check without testing anything.
    const MIN_REAL_TILES_PER_MAP: usize = 1_000;

    fn retail_dir() -> Option<PathBuf> {
        let dir = PathBuf::from(std::env::var("RA2_DIR").ok()?);
        dir.is_dir().then_some(dir)
    }

    /// Write a `.SED` for `options` into a scratch dir, returning the dir the
    /// loader should read from and the seed file's name.
    ///
    /// The scratch dir is deliberately *not* the retail install: the loader
    /// takes `ra2_dir` and the asset manager as separate parameters, so the
    /// seed can come from a temp dir while the assets come from the real game.
    fn write_seed(options: &RmgOptions, tag: &str) -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("vera20k-rmg-retail-{tag}"));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let name = format!("{tag}.sed");
        std::fs::write(dir.join(&name), options.to_sed_bytes()).expect("write .SED");
        (dir, name)
    }

    #[test]
    #[ignore] // Requires RA2_DIR (retail game files).
    fn generated_maps_resolve_every_tile_against_retail_theaters() {
        let Some(ra2) = retail_dir() else {
            panic!("set RA2_DIR to the retail RA2/YR install directory");
        };

        let mut structures_placed = 0usize;
        let mut configurations = 0usize;
        for map_type in RETAIL_MAP_TYPES {
            for theater in RETAIL_THEATERS {
                configurations += 1;
                let tag = format!("t{map_type}-th{theater}");
                let options = RmgOptions {
                    map_type,
                    theater,
                    num_players: 4,
                    seed: 4242,
                    // Above the river gate, so the carved map types actually
                    // seed water. The default is zero, which skips their whole
                    // seeder — this pass validated neither lakes nor rivers
                    // until that was noticed.
                    water_amount: RETAIL_WATER_AMOUNT,
                    ..Default::default()
                };
                let (seed_dir, seed_name) = write_seed(&options, &tag);

                let mut asset_manager = AssetManager::new(&ra2).expect("AssetManager::new");
                let initial = load_map_initial_with_assets(
                    seed_dir,
                    &mut asset_manager,
                    Some(&seed_name),
                    &mut SilentProgress,
                )
                .unwrap_or_else(|err| panic!("{tag}: the .SED branch failed: {err}"));

                assert!(
                    !initial.map_data.cells.is_empty(),
                    "{tag}: the generated map has no cells"
                );

                let theater_name = crate::map::rmg::emit::theater_name(theater);
                let data = crate::map::theater::load_theater(&mut asset_manager, theater_name)
                    .unwrap_or_else(|| panic!("{tag}: theater {theater_name} unavailable"));

                // Guard against a vacuous pass: if every cell were NO_TILE the
                // resolvability filter below would empty and the assertion
                // would hold without ever having looked at a tile.
                let real_tiles: Vec<i32> = initial
                    .map_data
                    .cells
                    .iter()
                    .map(|cell| cell.tile_index)
                    .filter(|index| *index >= 0)
                    .collect();
                assert!(
                    real_tiles.len() > MIN_REAL_TILES_PER_MAP,
                    "{tag}: only {} cell(s) carry a real tile — the resolvability \
                     check below would be vacuous",
                    real_tiles.len()
                );

                // The completion-gate clause. `filename` returns None both for
                // out-of-range ids and for blank tileset slots, and either one
                // renders as a hole in the map, so both count as unresolved.
                let mut unresolved: Vec<i32> = real_tiles
                    .iter()
                    .copied()
                    .filter(|index| !data.lookup.filename(*index).is_some_and(|f| !f.is_empty()))
                    .collect();
                unresolved.sort_unstable();
                unresolved.dedup();
                let mut distinct = real_tiles.clone();
                distinct.sort_unstable();
                distinct.dedup();
                println!(
                    "{tag}: {} cells, {} real tiles ({} distinct), {} unresolved",
                    initial.map_data.cells.len(),
                    real_tiles.len(),
                    distinct.len(),
                    unresolved.len()
                );
                assert!(
                    unresolved.is_empty(),
                    "{tag}: {} distinct tile index(es) do not resolve against {theater_name}: {:?}",
                    unresolved.len(),
                    unresolved
                );

                structures_placed += initial
                    .map_data
                    .entities
                    .iter()
                    .filter(|entity| entity.category == EntityCategory::Structure)
                    .count();
            }
        }

        // The first check that the real `NeutralTechBuildings` catalog reaches
        // the generator through the app's own resolution rather than a test
        // stub. Asserted over the tier: how many a given map places is
        // configuration-dependent, but a starved catalog places none anywhere.
        assert!(
            structures_placed > 0,
            "no neutral tech building was placed on any retail configuration"
        );
        assert_eq!(
            configurations,
            RETAIL_MAP_TYPES.len() * RETAIL_THEATERS.len(),
            "the matrix did not visit every configuration"
        );
        println!(
            "{configurations} retail configurations checked, {structures_placed} neutral \
             structure(s) placed"
        );
    }
}
