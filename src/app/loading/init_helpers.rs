//! App init helper functions — map file loading, atlas building, rules/art loading,
//! skirmish seeding, overlay atlas construction.
//!
//! Split from `loading::init` for file-size limits.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::map::basic::BridgeDestroyabilityMode;
use crate::map::houses::HouseColorMap;
use crate::map::map_file::MapFile;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, TerrainTileAnimation};
use crate::map::terrain::TerrainGrid;
use crate::map::theater::{self, TileImage, TileKey};
use crate::map::trigger_graph;
use crate::render::batch::BatchRenderer;
use crate::render::gpu::GpuContext;
use crate::render::sidebar_cameo_atlas::{self, SidebarCameoAtlas};
use crate::render::sprite_atlas::{self, SpriteAtlas};
use crate::render::tile_atlas::{self, TileAtlas};
use crate::render::unit_atlas::{self, UnitAtlas};
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::NativeRulesProcessOwner;
#[cfg(test)]
use crate::rules::retail_sources::load_startup_rules;
use crate::rules::ruleset::RuleSet;

use crate::sim::world::Simulation;

use crate::app::frontend::skirmish::deployable_building_types;

pub(crate) fn build_sidebar_cameo_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    asset_manager: &AssetManager,
    rules: Option<&RuleSet>,
) -> Option<SidebarCameoAtlas> {
    let rules = rules?;
    maybe_export_sidebar_cameo_debug(asset_manager, rules);
    let palette = load_sidebar_cameo_palette(asset_manager)?;
    sidebar_cameo_atlas::build_sidebar_cameo_atlas(gpu, batch, asset_manager, rules, &palette)
}

pub(crate) fn maybe_export_sidebar_cameo_debug(asset_manager: &AssetManager, rules: &RuleSet) {
    let enabled = std::env::var("RA2_DEBUG_CAMEO_PALETTES")
        .ok()
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes" || v == "on"
        })
        .unwrap_or(false);
    if !enabled {
        return;
    }

    let palette_names = [
        "cameo.pal",
        "cameomd.pal",
        "mousepal.pal",
        "anim.pal",
        "unittem.pal",
        "unit.pal",
        "temperat.pal",
        "isotem.pal",
    ];
    sidebar_cameo_atlas::export_debug_palette_sheet(
        asset_manager,
        rules,
        Path::new("debug_sidebar_cameo_palettes.png"),
        &palette_names,
    );
}

pub(crate) fn load_sidebar_cameo_palette(asset_manager: &AssetManager) -> Option<Palette> {
    let palette_names = [
        "cameo.pal",
        "cameomd.pal",
        "mousepal.pal",
        "anim.pal",
        "unittem.pal",
        "unit.pal",
        "temperat.pal",
    ];
    for name in palette_names {
        if let Some(data) = asset_manager.get_ref(name) {
            if let Ok(palette) = Palette::from_bytes(data) {
                log::info!("Sidebar cameos using palette {}", name);
                return Some(palette);
            }
        }
    }
    log::warn!("Sidebar cameo palette not found");
    None
}

pub(crate) fn log_trigger_graph_diagnostics(map_data: &MapFile) {
    let diag = trigger_graph::analyze_trigger_graph(
        &map_data.cell_tags,
        &map_data.tags,
        &map_data.triggers,
        &map_data.events,
        &map_data.actions,
    );
    if diag.cell_tags_total == 0
        && diag.tags_total == 0
        && diag.triggers_total == 0
        && map_data.events.is_empty()
        && map_data.actions.is_empty()
    {
        return;
    }

    log::info!(
        "Trigger graph: cell_tags={}/{} resolved, tags={}/{} trigger refs resolved, triggers={} events={} actions={}",
        diag.cell_tags_resolved,
        diag.cell_tags_total,
        diag.tags_resolved_to_triggers,
        diag.tags_with_trigger_ref,
        diag.triggers_total,
        diag.triggers_with_event,
        diag.triggers_with_action
    );
    if !diag.dangling_cell_tags.is_empty() {
        log::warn!(
            "Trigger graph dangling cell tags (first 8): {:?}",
            &diag.dangling_cell_tags[..diag.dangling_cell_tags.len().min(8)]
        );
    }
    if !diag.dangling_tag_trigger_refs.is_empty() {
        log::warn!(
            "Trigger graph dangling tag->trigger refs (first 8): {:?}",
            &diag.dangling_tag_trigger_refs[..diag.dangling_tag_trigger_refs.len().min(8)]
        );
    }
    if !diag.triggers_missing_event.is_empty() {
        log::warn!(
            "Trigger graph triggers missing events (first 8): {:?}",
            &diag.triggers_missing_event[..diag.triggers_missing_event.len().min(8)]
        );
    }
    if !diag.triggers_missing_action.is_empty() {
        log::warn!(
            "Trigger graph triggers missing actions (first 8): {:?}",
            &diag.triggers_missing_action[..diag.triggers_missing_action.len().min(8)]
        );
    }
}

pub(crate) fn parse_debug_spawn_units_env() -> Option<Vec<String>> {
    let raw = std::env::var("RA2_DEBUG_SPAWN_UNITS").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let enabled_tokens = ["1", "true", "yes", "on"];
    if enabled_tokens
        .iter()
        .any(|v| trimmed.eq_ignore_ascii_case(v))
    {
        return Some(vec![
            "HTNK".to_string(),
            "MTNK".to_string(),
            "E1".to_string(),
        ]);
    }
    let items: Vec<String> = trimmed
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if items.is_empty() { None } else { Some(items) }
}

/// Build a texture atlas from pre-loaded theater data and the terrain grid.
pub(crate) fn build_tile_atlas(
    asset_manager: &AssetManager,
    lookup: &theater::TilesetLookup,
    palette: &Palette,
    _ext: &str,
    grid: &TerrainGrid,
    gpu: &GpuContext,
    batch: &BatchRenderer,
) -> Option<TileAtlas> {
    let cell_pairs: Vec<(i32, u8)> = grid
        .cells
        .iter()
        .map(|c| (c.tile_id as i32, c.sub_tile))
        .collect();
    let mut needed: HashSet<TileKey> = theater::collect_used_tiles(&cell_pairs);
    // Always include tile_id 0 (clear ground) — used as fallback for missing tiles.
    needed.insert(TileKey {
        tile_id: 0,
        sub_tile: 0,
        variant: 0,
    });
    // Inject every bridge middle variant × all sub_tiles so the atlas holds
    // them before a live bridge publication rewrites a cell's tile; the
    // tactical draw keeps the load-time tile for anything not resident.
    if let Some(table) = &grid.bridge_middle_tiles {
        let before = needed.len();
        theater::inject_bridge_middle_tiles(&mut needed, table, lookup, asset_manager);
        log::info!(
            "Atlas pre-load: injected {} bridge middle TileKeys",
            needed.len() - before,
        );
    }
    log::info!("Map uses {} unique tile keys", needed.len());

    let images: HashMap<TileKey, TileImage> =
        theater::load_tile_images(asset_manager, lookup, palette, &needed);
    if images.is_empty() {
        log::warn!("No tile images loaded — falling back to single tile");
        return None;
    }

    let atlas: TileAtlas = tile_atlas::build_atlas(gpu, batch, &images);
    log::info!("Atlas built: {} tiles", atlas.tile_count());
    Some(atlas)
}

/// Fallback theater extension from theater name when load_theater fails.
pub(crate) fn theater_ext_for(theater_name: &str) -> &'static str {
    crate::map::theater::theater_extension(theater_name).unwrap_or("tem")
}

/// Match-load rules for a test, through the path a match load takes: the cold
/// startup selection (`load_startup_rules`), then the noncampaign scenario
/// rebuild on its process owner (`load_scenario`). Returns the
/// rules, the processed INI, the fixed ARTMD snapshot and the native receipt
/// that binds a stock-offline prefix plan.
///
/// Retail starts from RULESMD.INI, then processes optional LANGRULE.INI, the
/// selected mode INI, and finally the scenario/map INI. RA2 RULES.INI is not a
/// base layer in the active Yuri's Revenge path.
#[cfg(test)]
pub(crate) fn load_rules_with_merged_ini(
    asset_manager: &AssetManager,
    mode_rules_override: Option<&IniFile>,
    map_rules_overrides: Option<&IniFile>,
) -> Option<(
    RuleSet,
    IniFile,
    IniFile,
    crate::rules::process_owner::NativeScenarioRulesReceipt,
)> {
    let (_, _, mut native_owner) = load_startup_rules(
        asset_manager,
        std::sync::Arc::clone(
            crate::rules::audio_sources::AudioDefinitions::select(asset_manager).sounds(),
        ),
    )?
    .into_parts();
    let no_map = IniFile::from_str("");
    let (rules, processed_ini, fixed_art_ini, receipt) = native_owner
        .load_scenario(crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(mode_rules_override), map_rules_overrides.unwrap_or(&no_map))
        .map_err(|error| log::warn!("Failed native noncampaign rules rebuild: {error}"))
        .ok()?
        .into_parts();
    Some((rules, processed_ini, fixed_art_ini, receipt))
}

/// Resolve the neutral tech buildings the random map generator may place.
///
/// The type list lives in `[AI] NeutralTechBuildings` and each footprint in
/// `Foundation=`, so RULESMD/LANGRULE and fixed ARTMD are both needed.
/// An unavailable INI yields an empty catalog, which the placement phase
/// treats as "place nothing" rather than as an error.
pub(crate) fn load_neutral_tech_types(
    owner: Option<&NativeRulesProcessOwner>,
) -> Vec<crate::map::rmg::phases::tech_buildings::TechType> {
    let Some(owner) = owner else {
        log::warn!("random map: startup rules/art unavailable; placing no neutral tech buildings");
        return Vec::new();
    };
    let Ok(processed) = owner.startup_compatibility_projection() else {
        log::warn!(
            "random map: startup rules projection failed; placing no neutral tech buildings"
        );
        return Vec::new();
    };
    let types = crate::map::rmg::tech_catalog::resolve(processed.ini(), owner.fixed_art());
    log::info!(
        "random map: resolved {} neutral tech building type(s)",
        types.len()
    );
    types
}

/// AnimClass asset roots bound tolerantly: a type whose SHP is missing draws
/// nothing, as natively, instead of failing the load.
///
/// Building animations belong here, not in the strict scheduler closure. Art
/// defines building sections no rules list names and whose sprites retail never
/// shipped (`[CAARAY]` with `ActiveAnim=CAARAY_A`), and a few real buildings
/// name animations no archive holds (`GAGAP_A`, `GAREFNL4`), so requiring every
/// art building animation made every retail map fail to load. (Most of the
/// names that first failed were a separate resolver fault, since fixed in
/// `art_data::anim_shp_candidates`.)
pub(crate) fn tolerant_anim_class_roots(rules: &RuleSet) -> Vec<String> {
    let mut roots = crate::rules::effect_asset_catalog::anim_class_roots(rules);
    roots.extend(
        rules
            .art()
            .building_anim_roots()
            .into_iter()
            .filter(|name| rules.anim_type_names.contains(name)),
    );
    roots
}

/// Scheduler asset roots required by this map's surviving runtime objects.
///
/// Damage-fire roots remain part of the established closure. Terrain animation
/// names come exclusively from resolved theater `Tile##Anim` data, so custom
/// names take the same path without a WA/TUNTOP name table in Rust.
pub(crate) fn scheduler_anim_roots(
    rules: &RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    tile_animations: &[TerrainTileAnimation],
) -> Vec<String> {
    let mut roots = BTreeSet::new();
    roots.extend(
        rules
            .general
            .damage_fire_types
            .iter()
            .map(|anim| anim.name.trim().to_ascii_uppercase())
            .filter(|name| !name.is_empty()),
    );
    roots.extend(
        tile_animations
            .iter()
            .map(|anim| anim.anim_name.trim().to_ascii_uppercase())
            .filter(|name| !name.is_empty()),
    );
    // [General] Wake= is constructed as an ordinary AnimClass by the drive
    // locomotor's process (0x004B0823 region), so it needs the same loader
    // bounds as every other scheduler-owned anim or its constructor refuses it.
    {
        let wake = rules.general.wake.name.trim().to_ascii_uppercase();
        if !wake.is_empty() {
            roots.insert(wake);
        }
    }
    roots.extend(
        [
            rules.crate_rules.wood_crate_img.as_deref(),
            rules.crate_rules.crate_img.as_deref(),
            rules.crate_rules.water_crate_img.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter_map(|name| overlay_registry.id_for_name(name))
        .filter_map(|overlay_id| overlay_registry.flags(overlay_id))
        .filter_map(|flags| flags.cell_anim.as_deref())
        .map(|name| name.trim().to_ascii_uppercase())
        .filter(|name| !name.is_empty()),
    );
    roots.into_iter().collect()
}

/// Palette variants reachable from scenario-start crate CellAnim before the
/// post-map placer has constructed those AnimObjects. Initial atlases are built
/// earlier in the loading funnel, so live-object discovery alone is too late.
pub(crate) fn startup_crate_anim_remap_keys(
    rules: &RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
) -> HashSet<(String, crate::rules::house_colors::HouseColorIndex)> {
    [
        rules.crate_rules.wood_crate_img.as_deref(),
        rules.crate_rules.crate_img.as_deref(),
        rules.crate_rules.water_crate_img.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter_map(|name| overlay_registry.id_for_name(name))
    .filter_map(|overlay_id| {
        let flags = overlay_registry.flags(overlay_id)?;
        let anim = flags.cell_anim.as_deref()?;
        let tiberium = overlay_registry
            .tiberium_type_for_overlay(&rules.tiberium_types, overlay_id)
            .and_then(|type_id| rules.tiberium_types.get(type_id))?;
        let color = tiberium.color.as_deref()?;
        let entry = crate::rules::color_scheme::scheme_entry_by_name(&rules.color_schemes, color)?;
        let color = u8::try_from(entry)
            .ok()
            .map(crate::rules::house_colors::HouseColorIndex)?;
        Some((anim.trim().to_ascii_uppercase(), color))
    })
    .filter(|(anim, _)| !anim.is_empty())
    .collect()
}

/// The presentation-side atlas bundle the app builds AFTER GPU-free scenario
/// construction (F09): derived from immutable resources plus the constructed
/// simulation, never fed back into it.
pub(crate) struct PresentationManifest {
    pub(crate) unit_atlas: Option<UnitAtlas>,
    pub(crate) sprite_atlas: Option<SpriteAtlas>,
    pub(crate) palette_set: Option<crate::render::palette_textures::PaletteSet>,
}

/// Populate the already-staged gameplay owner with map object sections.
///
/// The caller creates this exact Simulation before Fill and keeps it through
/// authored overlay finalization.  This helper deliberately cannot construct a
/// replacement owner.
#[allow(clippy::too_many_arguments)]
pub(crate) fn populate_staged_app_scenario<F>(
    sim: &mut Simulation,
    map_data: &MapFile,
    resolved_terrain: &ResolvedTerrainGrid,
    theater_name: &str,
    rules: Option<&RuleSet>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    bridge_destroyability_mode: BridgeDestroyabilityMode,
    descriptor: &crate::sim::scenario_session::ScenarioDescriptor,
    generated_inits: Option<&crate::sim::world::GeneratedTechnoInitTable>,
    initialize_houses_before_objects: F,
) -> Result<(), crate::sim::world::GeneratedTechnoInitError>
where
    F: FnOnce(&mut Simulation),
{
    crate::sim::runtime::populate_staged_scenario_with_generated_inits(
        sim,
        map_data,
        resolved_terrain,
        theater_name,
        rules,
        overlay_registry,
        overlay_grid,
        bridge_destroyability_mode,
        descriptor,
        generated_inits,
        initialize_houses_before_objects,
    )
}

/// Attach app-only, GPU-free metadata after staged gameplay construction.
pub(crate) fn bind_staged_app_scenario_metadata(
    sim: &mut Simulation,
    asset_manager: &AssetManager,
    rules: Option<&RuleSet>,
) {
    // F09: HVA frame counts are sim metadata — parse them through the GPU-free
    // catalog before any atlas exists, so the renderer never writes into the
    // simulation. Atlas seeding derives its own copy from the same functions.
    let frame_catalog = crate::sim::voxel_frame_catalog::build_voxel_frame_catalog(
        sim.entities(),
        &sim.interner,
        asset_manager,
        rules,
    );
    sim.update_voxel_anim_frame_counts(&frame_catalog);
}

/// Build the voxel and SHP sprite atlases, and the palette set, for a fully
/// constructed simulation. A load builds them once; sprites the world needs
/// later go onto the atlases' growth pages.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_presentation_manifest(
    sim: &Simulation,
    asset_manager: &AssetManager,
    gpu: &GpuContext,
    batch: &BatchRenderer,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    house_colors: &HouseColorMap,
    theater_unit_palette: Option<&Palette>,
    theater_iso_palette: Option<&Palette>,
) -> PresentationManifest {
    let art = rules.map(RuleSet::art);
    // Use the theater-specific unit palette if provided, otherwise fall back to search.
    let palette: Option<Palette> = theater_unit_palette.cloned().or_else(|| {
        let pal_names: &[&str] = &["unittem.pal", "unit.pal", "temperat.pal"];
        pal_names.iter().find_map(|name| {
            let data: Vec<u8> = asset_manager.get(name)?;
            Palette::from_bytes(&data).ok()
        })
    });
    // Atlas build no longer needs the palette (tiles store palette indices,
    // not RGB). The palette load above is kept because downstream PaletteSet
    // construction (Task 1.9) will consume it. Skip the build if no palette
    // is available — it indicates a missing theater asset and rendering
    // wouldn't work anyway.
    let unit_atlas: Option<UnitAtlas> = if palette.is_some() {
        unit_atlas::build_unit_atlas(
            &gpu.device,
            &gpu.queue,
            batch,
            sim.entities(),
            asset_manager,
            rules,
            None, // initial build — no existing cache
            Some(&sim.interner),
        )
    } else {
        None
    };
    // Pre-load building types that can be spawned at runtime (e.g., ConYards from MCV deploy).
    let extra_buildings: Vec<&str> = deployable_building_types(rules);
    let mut anim_remap_keys = sprite_atlas::collect_anim_remap_base_keys(sim, house_colors);
    if let Some(rules) = rules {
        anim_remap_keys.extend(startup_crate_anim_remap_keys(rules, overlay_registry));
    }
    let cell_drawer_type_ids: HashSet<String> = sim
        .resolved_terrain
        .as_ref()
        .into_iter()
        .flat_map(|terrain| terrain.tile_animations())
        .map(|anim| anim.anim_name.to_ascii_uppercase())
        .collect();
    let loaded_iso_palette = theater_iso_palette.is_none().then(|| {
        asset_manager
            .get_ref(&format!("iso{}.pal", theater_ext.to_ascii_lowercase()))
            .and_then(|bytes| Palette::from_bytes(bytes).ok())
    });
    let cell_palette =
        theater_iso_palette.or_else(|| loaded_iso_palette.as_ref().and_then(Option::as_ref));
    let shp_atlas: Option<SpriteAtlas> = palette.as_ref().and_then(|pal| {
        sprite_atlas::build_sprite_atlas(
            &gpu.device,
            &gpu.queue,
            batch,
            sim.entities(),
            asset_manager,
            pal,
            theater_ext,
            theater_name,
            rules,
            art,
            house_colors,
            &extra_buildings,
            &anim_remap_keys,
            &cell_drawer_type_ids,
            cell_palette,
            None, // initial build — no existing cache
            Some(&sim.interner),
        )
    });
    // Build PaletteSet: theater palette + per-house RGB ramps for the voxel
    // sprite shader. Active houses are derived from the house_colors map
    // (deduplicated; row 0 of the ramp texture is the no-remap fallback).
    let default_ramps = crate::rules::house_colors::HouseColorRamps::default();
    let house_ramps: &crate::rules::house_colors::HouseColorRamps = rules
        .map(|r| &r.house_color_ramps)
        .unwrap_or(&default_ramps);
    let palette_set: Option<crate::render::palette_textures::PaletteSet> =
        palette.as_ref().map(|pal| {
            let mut active: Vec<crate::rules::house_colors::HouseColorIndex> =
                house_colors.values().copied().collect();
            active.sort_by_key(|h| h.0);
            active.dedup();
            crate::render::palette_textures::PaletteSet::new(gpu, pal, house_ramps, &active)
        });
    PresentationManifest {
        unit_atlas,
        sprite_atlas: shp_atlas,
        palette_set,
    }
}

#[cfg(test)]
#[path = "init_helpers_retail_placement_oracle_tests.rs"]
mod retail_placement_oracle_tests;

#[cfg(test)]
#[path = "init_helpers_retail_buildup_tests.rs"]
mod retail_buildup_tests;

#[cfg(test)]
mod tests {
    /// Building animations are tolerant roots, and only those the rules
    /// register as AnimTypes: art sections no rules list names (retail
    /// `[CAARAY]`) must not become required assets.
    #[test]
    fn building_anims_are_tolerant_roots_when_registered() {
        use crate::rules::art_data::ArtRegistry;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;

        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[Animations]\n0=KNOWN_A\n[General]\nWarpOut=MYWARP\n",
        ))
        .expect("rules");
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[BLDG]\nActiveAnim=KNOWN_A\n[GHOST]\nActiveAnim=GHOST_A\n",
        ));

        rules.install_art_data(art);
        let roots = super::tolerant_anim_class_roots(&rules);

        assert!(roots.iter().any(|root| root == "KNOWN_A"));
        assert!(roots.iter().any(|root| root == "MYWARP"));
        assert!(
            !roots.iter().any(|root| root == "GHOST_A"),
            "an unregistered art animation is no root at all: {roots:?}"
        );
        let strict = super::scheduler_anim_roots(
            &rules,
            &crate::rules::overlay_types::OverlayTypeRegistry::empty(),
            &[],
        );
        assert!(
            !strict.iter().any(|root| root == "KNOWN_A"),
            "building animations must not be required assets"
        );
    }

    use std::collections::HashSet;
    use std::path::PathBuf;

    use super::{load_rules_with_merged_ini, scheduler_anim_roots, startup_crate_anim_remap_keys};
    use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
    use crate::map::entities::EntityCategory;
    use crate::map::resolved_terrain::TerrainTileAnimation;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::RulesLayerStack;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::process_owner::NativeRulesProcessOwner;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile};
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::runtime::spawn_terrain_tile_animations;
    use crate::sim::world::{SimSoundEvent, Simulation};

    #[derive(Debug, PartialEq, Eq)]
    struct OverlayRegistryEntrySnapshot {
        id: u8,
        name: String,
        tiberium: bool,
        wall: bool,
        is_veins: bool,
        is_veinhole_monster: bool,
        is_gate: bool,
        crushable: bool,
        crate_type: bool,
        is_rubble: bool,
        is_a_rock: bool,
        land_wheel_speed_zero: bool,
        bridge_deck: bool,
        radar_color: Option<[u8; 3]>,
        track: bool,
        land: LandType,
        no_use_tile_land_type: bool,
        land_speed_costs: Option<SpeedCostProfile>,
        strength: u16,
        damage_levels: u16,
    }

    #[test]
    fn gsi_13_04_scheduler_roots_include_generic_resolved_tile_names() {
        let ini = IniFile::from_str(
            "[General]\nDamageFireTypes=FIRE_A,FIRE_B\n\
             [Animations]\n0=crate_spark\n\
             [OverlayTypes]\n0=STARTBOX\n[STARTBOX]\nCellAnim=crate_spark\n\
             [CrateRules]\nCrateImg=STARTBOX\nWoodCrateImg=STARTBOX\nWaterCrateImg=STARTBOX\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules");
        let overlay_registry =
            crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
        let tiles = vec![
            TerrainTileAnimation {
                rx: 4,
                ry: 1,
                anim_name: "custom_falls".to_string(),
                world_x: 0,
                world_y: 0,
                world_z: 0,
                z_adjust: 0,
            },
            TerrainTileAnimation {
                rx: 2,
                ry: 3,
                anim_name: "CUSTOM_MOUTH".to_string(),
                world_x: 0,
                world_y: 0,
                world_z: 0,
                z_adjust: 0,
            },
        ];

        assert_eq!(
            scheduler_anim_roots(&rules, &overlay_registry, &tiles),
            vec![
                "CRATE_SPARK",
                "CUSTOM_FALLS",
                "CUSTOM_MOUTH",
                "FIRE_A",
                "FIRE_B",
                // [General] Wake= default: constructed as a scheduler AnimClass
                // by the drive locomotor, so it is bound with the roots.
                "WAKE1"
            ]
        );
    }

    #[test]
    fn startup_crate_cell_anim_remap_is_rooted_before_post_map_construction() {
        let ini = IniFile::from_str(
            "[Colors]\nGold=43,239,255\nNeonGreen=104,241,195\n\
             [Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\nColor=NeonGreen\n\
             [Animations]\n0=crate_spark\n\
             [OverlayTypes]\n0=STARTBOX\n[STARTBOX]\nTiberium=yes\nCellAnim=crate_spark\n\
             [CrateRules]\nCrateImg=STARTBOX\nWoodCrateImg=STARTBOX\nWaterCrateImg=STARTBOX\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules");
        let overlay_registry =
            crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);

        assert_eq!(
            rules.crate_rules.crate_img.as_deref(),
            Some("STARTBOX"),
            "the processed CrateRules identity must drive pre-atlas discovery"
        );
        let overlay_id = overlay_registry
            .id_for_name("STARTBOX")
            .expect("startup crate overlay identity");
        let flags = overlay_registry
            .flags(overlay_id)
            .expect("startup crate overlay flags");
        assert_eq!(flags.cell_anim.as_deref(), Some("CRATE_SPARK"));
        let tiberium = overlay_registry
            .tiberium_type_for_overlay(&rules.tiberium_types, overlay_id)
            .and_then(|type_id| rules.tiberium_types.get(type_id))
            .expect("tiberium authority for startup crate CellAnim");
        assert_eq!(tiberium.color.as_deref(), Some("NeonGreen"));
        assert_eq!(
            crate::rules::color_scheme::scheme_entry_by_name(
                &rules.color_schemes,
                tiberium.color.as_deref().unwrap(),
            ),
            Some(1)
        );

        assert_eq!(
            startup_crate_anim_remap_keys(&rules, &overlay_registry),
            HashSet::from([(
                "CRATE_SPARK".to_string(),
                crate::rules::house_colors::HouseColorIndex(1),
            )])
        );
    }

    #[test]
    fn gsi_13_04_post_map_object_spawn_preserves_descriptor_order_state_and_sound() {
        let mut rules =
            RuleSet::from_ini(&IniFile::from_str("[General]\nDamageFireTypes=\n")).expect("rules");
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[CUSTOM_TOP]\nLoopCount=-1\nStartSound=WaterfallLoop\n\
             [CUSTOM_GROUND]\nLayer=ground\nLoopCount=-1\nYSortAdjust=1000\n",
        ));
        art.bind_anim_frame_count_for_test("CUSTOM_TOP", 16);
        art.bind_anim_frame_count_for_test("CUSTOM_GROUND", 2);
        rules.replace_art_registry_for_test(art);

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Neutral");
        let type_ref = sim.interner.intern("MAP_OBJECT");
        let map_object_id = sim.allocate_stable_id();
        sim.entities_mut()
            .insert(GameEntity::new_at_frame_zero_for_test(
                map_object_id,
                1,
                1,
                0,
                0,
                owner,
                Health { current: 100 },
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                false,
            ));
        sim.reveal(map_object_id);

        let tiles = vec![
            TerrainTileAnimation {
                rx: 1,
                ry: 2,
                anim_name: "CUSTOM_TOP".to_string(),
                world_x: 421,
                world_y: 702,
                world_z: 208,
                z_adjust: -17,
            },
            TerrainTileAnimation {
                rx: 0,
                ry: 4,
                anim_name: "CUSTOM_GROUND".to_string(),
                world_x: 101,
                world_y: 1_111,
                world_z: 312,
                z_adjust: 44,
            },
        ];
        let ids = spawn_terrain_tile_animations(&mut sim, &rules, &tiles);
        let expected_order = vec![map_object_id, ids[0], ids[1]];

        assert_eq!(sim.live_object_order_snapshot(), expected_order);
        for (id, tile) in ids.iter().zip(&tiles) {
            let anim = sim.anim(*id).expect("spawned terrain AnimClass");
            assert_eq!(
                anim.world_coord,
                crate::sim::anim_class::AnimWorldCoord {
                    x: tile.world_x,
                    y: tile.world_y,
                    z: tile.world_z,
                }
            );
            assert_eq!(anim.draw_flags, 0x1600);
            assert_eq!(anim.z_adjust, tile.z_adjust);
            assert_eq!(anim.runtime.loop_remaining, u8::MAX);
            assert!(!anim.runtime.constructor_reverse);
            assert!(anim.use_cell_drawer);
            assert!(anim.terrain_attached);
        }
        assert!(matches!(
            sim.sound_events.as_slice(),
            [SimSoundEvent::AnimationStarted { anim_id, .. }] if *anim_id == ids[0]
        ));
        assert_eq!(sim.sound_events.drain(..).count(), 1);
        assert!(sim.sound_events.is_empty());

        // Native load resets Scenario RNG to Seed(0); isolate persistence of
        // the AnimStore and authoritative LogicVector registration order.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        let bytes = bincode::serialize(&sim).expect("serialize terrain AnimClass state");
        let mut restored: Simulation = bincode::deserialize(&bytes).expect("deserialize state");
        restored.rebuild_logic_membership();
        assert_eq!(restored.live_object_order_snapshot(), expected_order);
        assert_eq!(restored.state_hash(), expected_hash);
        assert!(restored.anim(ids[0]).unwrap().use_cell_drawer);
        assert!(restored.anim(ids[0]).unwrap().terrain_attached);
        assert!(restored.sound_events.is_empty());
    }

    fn overlay_registry_snapshot(
        registry: &OverlayTypeRegistry,
    ) -> Vec<OverlayRegistryEntrySnapshot> {
        (0..registry.len())
            .map(|index| {
                let id = u8::try_from(index).expect("overlay registry fits u8 IDs");
                let name = registry.name(id).expect("registered overlay has a name");
                let flags = registry.flags(id).expect("registered overlay has flags");
                OverlayRegistryEntrySnapshot {
                    id,
                    name: name.to_string(),
                    tiberium: flags.tiberium,
                    wall: flags.wall,
                    is_veins: flags.is_veins,
                    is_veinhole_monster: flags.is_veinhole_monster,
                    is_gate: flags.is_gate,
                    crushable: flags.crushable,
                    crate_type: flags.crate_type,
                    is_rubble: flags.is_rubble,
                    is_a_rock: flags.is_a_rock,
                    land_wheel_speed_zero: flags.land_wheel_speed_zero,
                    bridge_deck: flags.bridge_deck,
                    radar_color: flags.radar_color,
                    track: flags.track,
                    land: flags.land,
                    no_use_tile_land_type: flags.no_use_tile_land_type,
                    land_speed_costs: flags.land_speed_costs,
                    strength: flags.strength,
                    damage_levels: flags.damage_levels,
                }
            })
            .collect()
    }

    fn retail_assets() -> AssetManager {
        let path = PathBuf::from(
            std::env::var_os("RA2_DIR")
                .expect("set RA2_DIR to the installed retail RA2/YR directory"),
        );
        assert!(
            path.is_dir(),
            "retail RA2/YR directory does not exist: {}",
            path.display()
        );
        AssetManager::new(&path, MediaArchiveMode::STOCK_DIGITAL)
            .expect("load retail RA2/YR assets")
    }

    const RULES_BASE: &str = "[InfantryTypes]\n0=E1\n[E1]\nStrength=125\n\
        [General]\nBuildSpeed=.7\n[CombatDamage]\nC4Delay=.03\n";

    /// AT-9: a map embedding [General]/[CombatDamage] overrides lands those
    /// values in RuleSet, including a sim-consumed path (C4 delay ticks).
    #[test]
    fn map_ini_overrides_rules_values() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(RULES_BASE));
        let map = IniFile::from_str(
            "[Basic]\nName=Fixture\n[General]\nBuildSpeed=1\n[CombatDamage]\nC4Delay=.06\n",
        );
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            map,
        );
        let rules = RuleSet::from_rules_layers(&layers).expect("ordered rules parse");
        // C4Delay is minutes: ticks = minutes * 60 * 15 => .06 -> 54.
        assert_eq!(rules.c4_delay_ticks, 54);
        // BuildSpeed: the map's 1 overrides the base .7.
        assert_eq!(
            rules.production.build_speed,
            crate::util::native_x87::NativeF64Bits::ONE
        );
    }

    /// AT-9 inverse: a map with no rules-shaped sections changes nothing.
    #[test]
    fn map_without_overrides_leaves_rules_unchanged() {
        let mut with_map = RulesLayerStack::new(IniFile::from_str(RULES_BASE));
        let map = IniFile::from_str("[Basic]\nName=Clean\n[Waypoints]\n0=45035\n");
        with_map.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            map,
        );
        let a = RuleSet::from_rules_layers(&with_map).expect("parse");
        let b = RuleSet::from_ini(&IniFile::from_str(RULES_BASE)).expect("parse");
        assert_eq!(a.c4_delay_ticks, b.c4_delay_ticks);
        assert_eq!(a.production.build_speed, b.production.build_speed);
        assert_eq!(
            a.object("E1").map(|o| o.strength),
            b.object("E1").map(|o| o.strength)
        );
    }

    /// The match load's rules path over in-memory layers: cold startup, then
    /// the noncampaign scenario rebuild.
    fn scenario_rules(
        rulesmd: IniFile,
        langrule: IniFile,
        mode: &IniFile,
        map: &IniFile,
    ) -> (RuleSet, IniFile, IniFile) {
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            rulesmd,
            Some(langrule),
            IniFile::from_str(""),
            Default::default(),
        )
        .expect("cold startup");
        let (rules, processed_ini, fixed_art, _receipt) = owner
            .load_scenario(crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(mode)), map)
            .expect("noncampaign scenario rebuild")
            .into_parts();
        (rules, processed_ini, fixed_art)
    }

    #[test]
    fn mode_override_processes_after_langrule_before_map() {
        let rulesmd = IniFile::from_str("[General]\nBuildSpeed=.7\nFlightLevel=1500\n");
        let langrule = IniFile::from_str("[General]\nBuildSpeed=.58\n");
        let mode = IniFile::from_str("[General]\nBuildSpeed=1\nFlightLevel=1200\n");
        let map = IniFile::from_str("[General]\nFlightLevel=900\n");
        let (_rules, ini, _fixed_art) = scenario_rules(rulesmd, langrule, &mode, &map);

        let general = ini.section("General").unwrap();
        assert_eq!(general.get_for_test("BuildSpeed"), Some("1"));
        assert_eq!(general.get_for_test("FlightLevel"), Some("900"));
    }

    #[test]
    fn map_overlay_flag_wins_after_langrule_and_mode() {
        let rulesmd = IniFile::from_str(
            "[OverlayTypes]\n1=TIB01\n\
             [Tiberiums]\n0=Riparius\n\
             [Riparius]\nImage=1\n\
             [TIB01]\nTiberium=no\n",
        );
        let langrule = IniFile::from_str("[Riparius]\nImage=2\n");
        let mode = IniFile::from_str("[Riparius]\nImage=3\n[TIB01]\nTiberium=no\n");
        let map = IniFile::from_str(
            "[OverlayTypes]\n1=GASAND\n\
             [Tiberiums]\n0=Cruentus\n\
             [Riparius]\nImage=4\n\
             [TIB01]\nTiberium=yes\n",
        );

        let (rules, merged_ini, _fixed_art_ini) = scenario_rules(rulesmd, langrule, &mode, &map);
        let registry = OverlayTypeRegistry::from_ini(&merged_ini, None);

        assert_eq!(rules.tiberium_types.types()[0].section, "Riparius");
        assert_eq!(rules.tiberium_types.types()[0].image, 4);
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.id_for_name("TIB01"), Some(0));
        assert_eq!(registry.name(0), Some("TIB01"));
        assert_eq!(registry.name(1), Some("GASAND"));
        assert!(registry.flags(0).is_some_and(|flags| flags.tiberium));
    }

    #[test]
    #[ignore = "requires retail RA2/YR assets"]
    fn retail_rules_plus_map_override_reaches_production_overlay_registry() {
        let assets = retail_assets();
        let (raw_bytes, _) = assets
            .get_with_source("rulesmd.ini")
            .expect("production raw rules selection");
        let raw_ini = IniFile::from_bytes(&raw_bytes).expect("parse raw retail rules");
        let raw_registry = OverlayTypeRegistry::from_ini(&raw_ini, None);
        assert_eq!(raw_registry.id_for_name("GASAND"), Some(0));
        assert_eq!(raw_registry.name(0), Some("GASAND"));
        assert!(!raw_registry.flags(0).expect("GASAND flags").tiberium);

        let map = IniFile::from_str("[GASAND]\nTiberium=yes\n");
        let loaded = load_rules_with_merged_ini(&assets, None, Some(&map))
            .expect("retail merged rules pair");
        let (rules, merged_ini, _fixed_art_ini, _receipt) = loaded;
        let merged_registry = OverlayTypeRegistry::from_ini(&merged_ini, None);

        assert_eq!(merged_registry.id_for_name("GASAND"), Some(0));
        assert_eq!(merged_registry.name(0), Some("GASAND"));
        assert!(merged_registry.flags(0).expect("GASAND flags").tiberium);
        assert_ne!(rules.source_ini_hash(), 0);
    }

    #[test]
    #[ignore = "requires retail RA2/YR assets"]
    fn retail_mount_moras_applies_rules_and_preserves_overlay_registry() {
        let assets = retail_assets();
        let (map_bytes, source) = assets
            .get_with_source("MountMoras.map")
            .expect("MountMoras.map");
        assert_eq!(source, "expandmd01.mix");
        assert_eq!(map_bytes.len(), 103_241);
        let map = IniFile::from_bytes(&map_bytes).expect("parse MountMoras.map");
        assert!(map.section("General").is_some());
        assert!(map.section("GAYARD").is_some());
        assert!(map.section("OverlayTypes").is_none());
        assert!(map.section("Tiberiums").is_none());

        let no_map =
            load_rules_with_merged_ini(&assets, None, None).expect("retail no-map rules pair");
        let with_map = load_rules_with_merged_ini(&assets, None, Some(&map))
            .expect("retail MountMoras rules pair");
        let (no_map_rules, no_map_ini, _no_map_fixed_art_ini, _no_map_receipt) = no_map;
        let (map_rules, map_ini, _map_fixed_art_ini, _map_receipt) = with_map;

        assert_eq!(
            no_map_rules
                .object("GAYARD")
                .map(|object| object.tech_level),
            Some(4)
        );
        assert_eq!(
            map_rules.object("GAYARD").map(|object| object.tech_level),
            Some(11)
        );
        assert_ne!(no_map_rules.source_ini_hash(), map_rules.source_ini_hash());

        let no_map_registry = OverlayTypeRegistry::from_ini(&no_map_ini, None);
        let map_registry = OverlayTypeRegistry::from_ini(&map_ini, None);
        assert_eq!(
            overlay_registry_snapshot(&no_map_registry),
            overlay_registry_snapshot(&map_registry)
        );
    }

    /// AT-12 (RC-4): type/weapon/warhead resolution reproduces the engine's
    /// outcomes for the three awkward cases.
    /// - **Forward reference:** the [HTNK] object names `Primary=120mm`, that
    ///   weapon names `Warhead=AP`, and both sections appear LATER in the file.
    ///   The engine resolves from a fully-parsed section table, so order is
    ///   irrelevant — both must resolve.
    /// - **Case-duplicate name:** [HTNK] is redefined as [htnk] further down.
    ///   One record per case-insensitive name, last definition wins
    ///   (Strength 200, not 100), and lookup is case-insensitive.
    /// - **Sectionless registry entry:** GHOST is listed in [VehicleTypes] but
    ///   has no [GHOST] section. The registry allocation pass still creates its
    ///   constructor-default record before the later body-read pass.
    #[test]
    fn resolution_order_matches_engine() {
        let ini = IniFile::from_str(
            "[General]\nBuildSpeed=.7\n\
             [VehicleTypes]\n0=HTNK\n1=GHOST\n\
             [HTNK]\nStrength=100\nPrimary=120mm\n\
             [120mm]\nDamage=50\nWarhead=AP\n\
             [AP]\nVerses=100%,100%,100%\n\
             [htnk]\nStrength=200\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("fixture rules parse");

        // Forward-referenced weapon + its warhead both resolved.
        assert!(rules.weapon("120mm").is_some(), "forward-referenced weapon");
        assert!(rules.warhead("AP").is_some(), "forward-referenced warhead");

        // Raw section lookup is exact. Type lookup is case-insensitive, but
        // only the exact [HTNK] body named by the registry is processed.
        assert!(rules.object("htnk").is_some());
        assert_eq!(rules.object("HTNK").map(|o| o.strength), Some(100));

        // Registry allocation precedes the body-read pass, so a sectionless
        // entry retains its constructor-default record.
        assert!(rules.object("GHOST").is_some());
    }

    /// AT-11 (RC-3): every ported scalar default that maps to a verified
    /// RulesClass constructor default falls back to THAT value when the key is
    /// absent from the INI. Constructor defaults verified from the binary
    /// (immediate stores inside the RulesClass ctor; doubles cross-checked
    /// against RULESCLASS_CONSTRUCTOR_DEFAULTS.csv): FlightLevel=500,
    /// GrowthRate=2.0 min, RepairStep=5, RepairPercent=25%, BuildSpeed=1.0,
    /// ParachuteMaxFallRate=-3, ParadropRadius=1024, URepairRate=.016 min
    /// (retained double), C4Delay=.03 min (→27 ticks).
    ///
    /// Retail rulesmd.ini always supplies its own value for each, so these
    /// fallbacks fire only for a non-retail INI missing the key — matching the
    /// ctor default is behaviour-neutral in real play and faithful to gamemd's
    /// key-absent path.
    ///
    /// EXCLUDED: `VeteranSight` (its constructor default is UNCHECKED; the
    /// field now reads the `double` retail authors) and `GapRadius` (no
    /// RulesClass ctor field in the verified offset map to flip to).
    #[test]
    fn ported_defaults_match_ctor_csv() {
        // Sections contain only an inert fixture key: every recognized key
        // below is absent, so each field takes its fallback default.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nFixtureOnly=1\n[CombatDamage]\nFixtureOnly=1\n",
        ))
        .expect("empty-section rules parse");

        // [General] scalar fallbacks == ctor defaults.
        assert_eq!(rules.general.flight_level, 500, "FlightLevel");
        assert_eq!(
            rules.general.parachute_max_fall_rate, -3,
            "ParachuteMaxFallRate"
        );
        assert_eq!(rules.general.paradrop_radius, 1024, "ParadropRadius");
        assert_eq!(rules.general.repair_step, 5, "RepairStep");
        assert_eq!(rules.general.repair_percent, 0.25, "RepairPercent (.25)");
        assert_eq!(
            rules.general.unit_repair_rate.to_bits(),
            0x3f90_624d_d2f1_a9fc,
            "URepairRate constructor double .016"
        );
        assert_eq!(rules.general.growth_rate_minutes, 2.0, "GrowthRate");

        assert_eq!(
            rules.production.build_speed,
            crate::util::native_x87::NativeF64Bits::ONE,
            "BuildSpeed 1.0"
        );

        // [CombatDamage] C4Delay .03 min -> 27 ticks.
        assert_eq!(rules.c4_delay_ticks, 27, "C4Delay .03 min -> 27 ticks");
    }

    /// The processed-rules component of the diagnostic/snapshot compatibility
    /// hash is sensitive to a map's *value* overrides — closing the gap
    /// where a registry-only hash let a map override [General]/[CombatDamage]
    /// values without changing the hash, so a diagnostic/snapshot recorded under
    /// the map could play back against base rules undetected.
    #[test]
    fn rules_hash_reflects_map_value_overrides() {
        let no_override = RuleSet::from_ini(&IniFile::from_str(RULES_BASE)).expect("parse");

        let mut with_override = RulesLayerStack::new(IniFile::from_str(RULES_BASE));
        with_override.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[General]\nBuildSpeed=2\n"),
        );
        let overridden = RuleSet::from_rules_layers(&with_override).expect("parse");

        assert_ne!(
            no_override.source_ini_hash(),
            overridden.source_ini_hash(),
            "a map BuildSpeed override must change the rules hash"
        );
    }
}
