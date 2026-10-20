//! Launch house colours, deployable building detection, and overlay atlas construction.
//!
//! Split from `loading::init_helpers` for file-size limits.

use std::collections::{BTreeMap, HashMap};

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::map::houses::{HouseColorMap, HouseRoster};
#[cfg(test)]
use crate::map::map_file::MapFile;
use crate::map::overlay::{OverlayEntry, TerrainObject};
use crate::render::batch::BatchRenderer;
use crate::render::bridge_atlas::{self, BridgeAtlas};
use crate::render::gpu::GpuContext;
use crate::render::overlay_assets::{render_overlay_names, resolve_overlay_name_for_render};
use crate::render::overlay_atlas::{self, OverlayAtlas};
use crate::rules::color_scheme::{
    ColorSchemeEntry, scheme_entry_by_name, scheme_entry_for_priority,
};
use crate::rules::house_colors::{DEFAULT_SCHEME_ENTRY, HouseColorIndex};
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::scenario_bootstrap::normalized_launch_slots;
use crate::skirmish_launch::SkirmishLaunchSession;

pub(crate) fn house_color_map_for_launch_session(
    session: &SkirmishLaunchSession,
    house_roster: &HouseRoster,
    schemes: &[ColorSchemeEntry],
) -> HouseColorMap {
    // Active-retail CreateHouses @ 0x6882D1..0x68837D constructs Neutral and
    // Special even without [Houses]. Each looks up literal "LightGrey" with
    // 53 rows (0x688309/0x68835C -> 0x68CAB0), then calls InitColor (0x50B840).
    // A failed lookup falls back to runtime scheme 5, logical entry 2.
    let neutral_color = HouseColorIndex(
        scheme_entry_by_name(schemes, "LightGrey")
            .and_then(|entry| u8::try_from(entry).ok())
            .filter(|&entry| entry != crate::rules::house_colors::NO_REMAP.0)
            .unwrap_or(DEFAULT_SCHEME_ENTRY as u8),
    );
    let mut colors = HouseColorMap::new();
    colors.insert("Neutral".to_string(), neutral_color);
    colors.insert("Special".to_string(), neutral_color);
    for house in &house_roster.houses {
        if !is_playable_faction_name(&house.name) {
            // Preserve authored identity casing, but these two skirmish houses
            // use CreateHouses' explicit color rather than campaign Color=.
            let color = if house.name.eq_ignore_ascii_case("Neutral")
                || house.name.eq_ignore_ascii_case("Special")
            {
                neutral_color
            } else {
                house.color
            };
            colors.insert(house.name.clone(), color);
        }
    }
    for slot in normalized_launch_slots(session) {
        // `slot.color_index` is the gamemd color *priority* (lobby slot order);
        // resolve it to a `[Colors]` entry index via the priority LUT + /2 doubling.
        let entry = scheme_entry_for_priority(slot.color_index as i32) as u8;
        colors.insert(slot.owner_name, HouseColorIndex(entry));
    }
    colors
}

/// Returns true for faction names that represent real players (not neutral/civilian).
fn is_playable_faction_name(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    !matches!(
        up.as_str(),
        "NEUTRAL" | "SPECIAL" | "CIVILIAN" | "GOODGUY" | "BADGUY" | "JP"
    )
}

/// Collect building type IDs that can be spawned at runtime and need atlas pre-loading.
///
/// Scans all objects with `DeploysInto=` set in rules.ini to find deploy targets
/// (e.g., AMCV→GACNST). Data-driven — no hardcoded MCV/ConYard type pairs. It
/// covers every object type in the rules, since units with DeploysInto can
/// appear via production or scripted events even without being on the map yet.
pub fn deployable_building_types(rules: Option<&RuleSet>) -> Vec<&str> {
    let Some(rules) = rules else {
        return Vec::new();
    };
    let mut result: Vec<&str> = Vec::new();
    for obj in rules.all_objects() {
        if let Some(target_id) = &obj.deploys_into
            && let Some(target_obj) = rules.object(target_id)
            && !result
                .iter()
                .any(|r| r.eq_ignore_ascii_case(&target_obj.id))
        {
            result.push(&target_obj.id);
        }
    }
    result
}

/// Resident overlay, bridge and radar presentation derived from already-bound
/// registries and explicit display descriptors. It owns no gameplay frames.
pub(crate) struct OverlayPresentationAssets {
    pub(crate) overlay_atlas: Option<OverlayAtlas>,
    pub(crate) bridge_atlas: Option<BridgeAtlas>,
    pub(crate) overlay_names: BTreeMap<u8, String>,
    pub(crate) overlay_radar_colors: HashMap<(u8, u8), [u8; 3]>,
}

/// Build resident images from the current world descriptors. Fresh loading
/// supplies its connected authored overlays; saved loading supplies restored
/// damage/connectivity bytes unchanged. Neither registry readers nor wall
/// topology writers run at this asset-only boundary.
pub(crate) fn build_overlay_presentation(
    overlays: &[OverlayEntry],
    terrain_objects: &[TerrainObject],
    asset_manager: &AssetManager,
    gpu: &GpuContext,
    batch: &BatchRenderer,
    theater_ext: &str,
    theater_name: &str,
    rules_ini: &IniFile,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    theater_iso_palette: Option<&Palette>,
    theater_unit_palette: Option<&Palette>,
    theater_tiberium_palette: Option<&Palette>,
) -> OverlayPresentationAssets {
    let force_tib_remap_enabled: bool = std::env::var("RA2_FORCE_TIB3_TO_TIB01")
        .ok()
        .map(|v| {
            let n = v.trim().to_ascii_lowercase();
            n == "1" || n == "true" || n == "yes" || n == "on"
        })
        .unwrap_or(false);
    if force_tib_remap_enabled {
        log::warn!("Debug overlay remap enabled: TIB3_20 -> TIB01");
    }
    let tib_id_offset: isize = std::env::var("RA2_TIB_ID_OFFSET")
        .ok()
        .and_then(|s| s.parse::<isize>().ok())
        .unwrap_or(0);
    if tib_id_offset != 0 {
        log::warn!(
            "Debug resource ID offset enabled: RA2_TIB_ID_OFFSET={}",
            tib_id_offset
        );
    }

    let crate_rules = &rules.crate_rules;
    let art_registry = rules.art();
    let rubble_arts: Vec<_> = rules.building_rubble_arts().cloned().collect();

    // Log first 20 overlay types for diagnostic verification.
    let max_diag: usize = 20.min(overlay_registry.len());
    for i in 0..max_diag {
        if let Some(name) = overlay_registry.name(i as u8) {
            let mapped = resolve_overlay_name_for_render(overlay_registry, i as u8)
                .unwrap_or_else(|| name.to_string());
            let flags = overlay_registry.flags(i as u8);
            let tib: bool = flags.map(|f| f.tiberium).unwrap_or(false);
            let wall: bool = flags.map(|f| f.wall).unwrap_or(false);
            log::info!(
                "  OverlayType[{:3}] = {:20} mapped={:20} tib={} wall={}",
                i,
                name,
                mapped,
                tib,
                wall
            );
        }
    }

    // Every registered type is named: a cell draws whichever identity it holds,
    // whether the map authored it or the simulation wrote it after load.
    let overlay_names: BTreeMap<u8, String> = render_overlay_names(overlay_registry);
    let mut unmapped_ids: Vec<u8> = overlays
        .iter()
        .map(|entry| entry.overlay_id)
        .filter(|overlay_id| !overlay_names.contains_key(overlay_id))
        .collect();
    if !unmapped_ids.is_empty() {
        let unmapped_entries = unmapped_ids.len();
        unmapped_ids.sort_unstable();
        unmapped_ids.dedup();
        log::warn!(
            "Unmapped overlay IDs (not in registry): {unmapped_ids:?} ({unmapped_entries} entries)"
        );
    }

    // Log resource overlays for diagnostic visibility.
    for (id, name) in &overlay_names {
        let flags = overlay_registry.flags(*id);
        let tib: bool = flags.map(|f| f.tiberium).unwrap_or(false);
        if tib {
            log::info!("  Resource overlay: id={} name={}", id, name);
        }
    }

    // Use theater-provided palettes if available, otherwise fall back to search.
    let theater_palette: Option<Palette> = theater_iso_palette.cloned().or_else(|| {
        let pal_names: &[&str] = &["isotem.pal", "isosno.pal", "isourb.pal", "temperat.pal"];
        pal_names.iter().find_map(|name| {
            let data: Vec<u8> = asset_manager.get(name)?;
            Palette::from_bytes(&data).ok()
        })
    });
    let unit_palette: Option<Palette> = theater_unit_palette.cloned().or_else(|| {
        let pal_names: &[&str] = &["unittem.pal", "unitsno.pal", "uniturb.pal", "unit.pal"];
        pal_names.iter().find_map(|name| {
            let data: Vec<u8> = asset_manager.get(name)?;
            Palette::from_bytes(&data).ok()
        })
    });
    // Tiberium palette: the original engine uses a dedicated palette (e.g., temperat.pal) for
    // ore/gem overlays, distinct from both the iso palette and the unit palette.
    let tiberium_palette: Option<Palette> = theater_tiberium_palette.cloned().or_else(|| {
        let pal_names: &[&str] = &["temperat.pal", "snow.pal", "urban.pal"];
        pal_names.iter().find_map(|name| {
            let data: Vec<u8> = asset_manager.get(name)?;
            Palette::from_bytes(&data).ok()
        })
    });

    let overlay_radar_colors: HashMap<(u8, u8), [u8; 3]> =
        overlay_atlas::compute_overlay_radar_colors(
            asset_manager,
            overlay_registry,
            &overlay_names,
            theater_ext,
            theater_name,
            rules_ini,
            art_registry,
        );

    let atlas: Option<OverlayAtlas> = theater_palette.as_ref().and_then(|theater_pal| {
        // If no unit palette, fall back to theater palette for everything.
        let unit_pal: &Palette = unit_palette.as_ref().unwrap_or(theater_pal);
        let tib_pal: &Palette = tiberium_palette.as_ref().unwrap_or(theater_pal);
        overlay_atlas::build_overlay_atlas(
            gpu,
            batch,
            overlays,
            terrain_objects,
            &rules.general.default_mirage_disguises,
            asset_manager,
            theater_pal,
            unit_pal,
            tib_pal,
            theater_ext,
            theater_name,
            overlay_registry,
            &rules.tiberium_types,
            crate_rules,
            rules_ini,
            art_registry,
            Some(&rules.smudge_types),
            &rubble_arts,
        )
    });

    let bridge_atlas: Option<BridgeAtlas> = theater_palette.as_ref().and_then(|theater_pal| {
        let unit_pal: &Palette = unit_palette.as_ref().unwrap_or(theater_pal);
        bridge_atlas::build_bridge_atlas(
            &gpu.device,
            &gpu.queue,
            batch,
            &overlay_names,
            asset_manager,
            theater_pal,
            unit_pal,
            theater_ext,
            theater_name,
            overlay_registry,
            rules_ini,
            art_registry,
            &rubble_arts,
        )
    });

    OverlayPresentationAssets {
        overlay_atlas: atlas,
        bridge_atlas,
        overlay_names,
        overlay_radar_colors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::map::houses::HouseDefinition;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    use crate::map::waypoints::Waypoint;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::SpeedCostProfile;
    use crate::sim::cell_rect::{PlayfieldBounds, cell_is_in_playfield_height_aware};
    use crate::sim::house_state::HouseState;
    use crate::sim::mission::MissionType;
    use crate::sim::rng::SimRng;
    use crate::sim::scenario_bootstrap::*;
    use crate::sim::world::Simulation;
    use crate::skirmish_launch::{
        AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishAiSlot,
        SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLocalSlot,
    };

    fn launch_descriptor(session: &SkirmishLaunchSession) -> MatchLaunchDescriptor {
        MatchLaunchDescriptor::from_resolved(session.clone())
            .expect("test fixture session is fully resolved")
    }

    fn test_session() -> SkirmishLaunchSession {
        SkirmishLaunchSession {
            mode: test_battle_mode(),
            selected_map_file: Some("test.mmx".to_string()),
            humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: "Player".to_string(),
                slot: SkirmishLocalSlot {
                    country: LaunchCountry::America,
                    country_random: false,
                    color_index: 1,
                    color_random: false,
                    start_position: LaunchStartPosition::Position(3),
                    team: LaunchTeam::None,
                },
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
            }],
            local_player_index: 0,
            ai_player_count: 1,
            opponents: vec![SkirmishAiSlot {
                raw_slot_index: 1,
                country: LaunchCountry::Russia,
                country_random: false,
                color_index: 2,
                color_random: false,
                start_position: LaunchStartPosition::Auto,
                team: LaunchTeam::None,
                difficulty: Default::default(),
            }],
            pre_fill_house_roster:
                crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(1),
            options: SkirmishLaunchOptions::default(),
        }
    }

    #[test]
    fn gsi_04_18_shroud_no_reveals_only_the_local_human_plane_without_sight_or_rng() {
        let mut sim = Simulation::with_seed(0x418);
        sim.fog_mut_for_test().set_width_for_test(8);
        sim.fog_mut_for_test().set_height_for_test(6);
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .shroud = false;
        let local = sim.intern("Player");
        let ai = sim.intern("Computer");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, true, 10_000, 10));
        sim.houses_mut_for_test()
            .register(HouseState::new(ai, 1, None, false, 10_000, 10));
        let plane = crate::sim::vision::OwnerVisibility::new(sim.fog().width(), sim.fog().height());
        sim.fog_mut_for_test()
            .by_owner_mut_for_test()
            .insert(ai, plane);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        let scenario_rng_before = sim.scenario_rng().state();
        let main_rng_before = sim.main_rng().state();

        apply_launch_shroud_option(&mut sim, crate::sim::world::UninitContext::new(None, None));

        for (rx, ry) in [(0, 0), (7, 0), (0, 5), (7, 5), (3, 2)] {
            assert!(sim.fog().is_cell_revealed(local, rx, ry));
            assert!(!sim.fog().is_cell_visible(local, rx, ry));
            assert!(!sim.fog().is_cell_revealed(ai, rx, ry));
        }
        assert!(!sim.houses()[&local].map_is_clear());
        assert!(!sim.houses()[&ai].map_is_clear());
        assert_eq!(sim.scenario_rng().state(), scenario_rng_before);
        assert_eq!(sim.main_rng().state(), main_rng_before);
    }

    #[test]
    fn gsi_04_18_shroud_yes_and_a_missing_current_house_stay_unexplored() {
        let mut shrouded = Simulation::with_seed(0x418);
        shrouded.fog_mut_for_test().set_width_for_test(8);
        shrouded.fog_mut_for_test().set_height_for_test(6);
        let human = shrouded.intern("Player");
        shrouded
            .houses_mut_for_test()
            .register(HouseState::new(human, 0, None, true, 10_000, 10));
        let context = crate::sim::world::UninitContext::new(None, None);
        apply_launch_shroud_option(&mut shrouded, context);
        assert!(!shrouded.fog().is_cell_revealed(human, 7, 5));

        // RevealEntireMap(NULL) reveals to PlayerPtr alone.
        shrouded
            .session_mut_for_test()
            .game_options_mut_for_test()
            .shroud = false;
        apply_launch_shroud_option(&mut shrouded, context);
        assert!(!shrouded.fog().is_cell_revealed(human, 7, 5));
        assert!(!shrouded.fog().whole_map_revealed_for_owner(human));
    }

    #[test]
    fn gsi_04_18_explicit_launch_applies_shroud_no_with_bases_off() {
        let mut sim = Simulation::with_seed(0x418);
        // The map's Size 64x64 diamond spans rx and ry 1..=127.
        sim.fog_mut_for_test().set_width_for_test(128);
        sim.fog_mut_for_test().set_height_for_test(128);
        let mut session = test_session();
        session.options.shroud = false;
        session.options.bases = false;
        session.options.unit_count = 0;
        let terrain = test_terrain(128, 128);
        *sim.resolved_terrain_mut_for_test() = Some(terrain.clone());
        let starts = test_launch_starts();
        let map = test_map_with_starts(&starts);

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &test_standard_launch_rules(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 0);
        let local = sim.interner().get("Player").expect("local house");
        let ai = sim.interner().get("Computer1").expect("AI house");
        // The diamond's four corners and centre.
        for (rx, ry) in [(1, 64), (64, 1), (127, 64), (64, 127), (64, 64)] {
            assert!(sim.fog().is_cell_revealed(local, rx, ry));
            assert!(!sim.fog().is_cell_visible(local, rx, ry));
            assert!(!sim.fog().is_cell_revealed(ai, rx, ry));
        }
        for (rx, ry) in [(0, 0), (127, 0), (0, 127), (127, 127)] {
            assert!(
                !sim.fog().is_cell_revealed(local, rx, ry),
                "the CellIterator never walks storage outside the Size diamond"
            );
        }
        assert!(!sim.houses()[&local].map_is_clear());
    }

    fn test_battle_mode() -> SkirmishLaunchMode {
        SkirmishLaunchMode {
            class: crate::skirmish_modes::MpModeClass::Battle,
            id: 1,
            ui_name_key: "GUI:Battle".to_string(),
            tooltip_key: "STT:ModeBattle".to_string(),
            override_file: "MPBattleMD.ini".to_string(),
            map_filter: "standard".to_string(),
            random_maps_allowed: true,
            allies_allowed: true,
            must_ally: false,
        }
    }

    fn test_cooperative_mode() -> SkirmishLaunchMode {
        SkirmishLaunchMode {
            class: crate::skirmish_modes::MpModeClass::Cooperative,
            id: 3,
            ui_name_key: "GUI:Cooperative".to_string(),
            tooltip_key: "STT:ModeCooperative".to_string(),
            override_file: "MPCoopMD.ini".to_string(),
            map_filter: "cooperative".to_string(),
            random_maps_allowed: false,
            allies_allowed: true,
            must_ally: false,
        }
    }

    fn test_rules_with_base_units(
        base_units: &str,
        objects: &[(&str, &str, Option<&str>, Option<&str>)],
    ) -> RuleSet {
        let mut text = format!("[General]\nBaseUnit={base_units}\n\n[VehicleTypes]\n");
        for (idx, (id, _, _, _)) in objects.iter().enumerate() {
            text.push_str(&format!("{}={id}\n", idx + 1));
        }
        for (id, owner, required, forbidden) in objects {
            text.push_str(&format!("\n[{id}]\nOwner={owner}\n"));
            if let Some(required) = required {
                text.push_str(&format!("RequiredHouses={required}\n"));
            }
            if let Some(forbidden) = forbidden {
                text.push_str(&format!("ForbiddenHouses={forbidden}\n"));
            }
        }
        let ini = launch_fixture_ini(&text);
        RuleSet::from_ini(&ini).expect("test rules parse")
    }

    fn test_standard_launch_rules() -> RuleSet {
        test_rules_with_base_units(
            "AMCV,SMCV,PCV",
            &[
                (
                    "AMCV",
                    "British,French,Germans,Americans,Alliance",
                    None,
                    None,
                ),
                ("SMCV", "Russians,Confederation,Africans,Arabs", None, None),
                ("PCV", "YuriCountry", None, None),
            ],
        )
    }

    const ORDERED_COUNTRY_SIDE_INI: &str = "[Countries]\n\
             0=Americans\n1=Alliance\n2=French\n3=Germans\n4=British\n\
             5=Africans\n6=Arabs\n7=Confederation\n8=Russians\n9=YuriCountry\n\
             10=GDI\n11=Nod\n12=Neutral\n13=Special\n\
             [Sides]\n\
             GDI=British,French,Germans,Americans,Alliance\n\
             Nod=Russians,Africans,Confederation,Arabs\n\
             ThirdSide=YuriCountry\nCivilian=Neutral\nMutant=Special\n";

    fn launch_fixture_ini(source: &str) -> IniFile {
        IniFile::from_str(&format!("{ORDERED_COUNTRY_SIDE_INI}{source}"))
    }

    fn test_starting_unit_rules() -> RuleSet {
        let ini = launch_fixture_ini(
            "[General]\nBaseUnit=AMCV,SMCV,PCV\n\
             [VehicleTypes]\n1=AMCV\n2=SMCV\n3=MTNK\n4=HTNK\n5=HARV\n6=YTNK\n\
             [InfantryTypes]\n1=E1\n2=SHK\n\
             [AMCV]\nOwner=Americans\nCost=1000\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n\
             [SMCV]\nOwner=Russians\nCost=1000\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n\
             [MTNK]\nOwner=Americans\nCost=100\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n\
             [HTNK]\nOwner=Russians\nCost=100\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n\
             [HARV]\nOwner=Americans,Russians\nCost=500\nTechLevel=1\nAllowedToStartInMultiplayer=no\n\
             [YTNK]\nOwner=YuriCountry\nCost=700\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n\
             [E1]\nOwner=Americans\nCost=300\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n\
             [SHK]\nOwner=Russians\nCost=200\nTechLevel=11\nAllowedToStartInMultiplayer=yes\n",
        );
        RuleSet::from_ini(&ini).expect("starting unit rules parse")
    }

    fn test_map_with_starts(starts: &[Waypoint]) -> MapFile {
        let waypoints = starts
            .iter()
            .map(|waypoint| (waypoint.index, *waypoint))
            .collect();
        MapFile {
            header: crate::map::map_file::MapHeader {
                theater: "TEMPERATE".to_string(),
                fill: "Clear".to_string(),
                level: 0,
                width: 64,
                height: 64,
                local_left: 0,
                local_top: 0,
                local_width: 64,
                local_height: 64,
            },
            basic: crate::map::basic::BasicSection::default(),
            ranking: crate::map::ranking::RankingSection::default(),
            preview: crate::map::preview::PreviewSection::default(),
            cells: Vec::new(),
            iso_map_pack_lookups: Vec::new(),
            entities: Vec::new(),
            overlays: Vec::new(),
            authored_overlay_packs: crate::map::map_file::AuthoredOverlayPackSlot::empty(),
            smudges: Vec::new(),
            terrain_objects: Vec::new(),
            waypoints,
            tags: std::collections::HashMap::new(),
            triggers: std::collections::HashMap::new(),
            events: std::collections::HashMap::new(),
            actions: std::collections::HashMap::new(),
            local_variables: std::collections::HashMap::new(),
            special_flags: crate::map::basic::SpecialFlagsSection::default(),
            ini: IniFile::from_str(""),
        }
    }

    fn nearoref_starts() -> [Waypoint; 8] {
        [
            Waypoint {
                index: 0,
                rx: 38,
                ry: 63,
            },
            Waypoint {
                index: 1,
                rx: 53,
                ry: 48,
            },
            Waypoint {
                index: 2,
                rx: 71,
                ry: 32,
            },
            Waypoint {
                index: 3,
                rx: 99,
                ry: 32,
            },
            Waypoint {
                index: 4,
                rx: 39,
                ry: 106,
            },
            Waypoint {
                index: 5,
                rx: 68,
                ry: 107,
            },
            Waypoint {
                index: 6,
                rx: 85,
                ry: 90,
            },
            Waypoint {
                index: 7,
                rx: 100,
                ry: 75,
            },
        ]
    }

    fn configure_nearoref_geometry(sim: &mut Simulation, map: &mut MapFile) {
        map.header.width = 80;
        map.header.height = 58;
        map.header.local_left = 2;
        map.header.local_top = 4;
        map.header.local_width = 76;
        map.header.local_height = 48;
        sim.install_playfield_from_map_header(&map.header);
        sim.session_mut_for_test().set_map_width_for_test(138);
        sim.session_mut_for_test().set_map_height_for_test(138);
    }

    fn test_launch_starts() -> [Waypoint; 4] {
        [
            Waypoint {
                index: 0,
                rx: 10,
                ry: 10,
            },
            Waypoint {
                index: 1,
                rx: 30,
                ry: 10,
            },
            Waypoint {
                index: 2,
                rx: 10,
                ry: 30,
            },
            Waypoint {
                index: 3,
                rx: 30,
                ry: 30,
            },
        ]
    }

    /// Compact launch fixture whose four cells are all inside the normalized
    /// playfield derived from `test_map_with_starts`'s 64x64 header.
    fn test_in_playfield_launch_starts() -> [Waypoint; 4] {
        [
            Waypoint {
                index: 0,
                rx: 35,
                ry: 35,
            },
            Waypoint {
                index: 1,
                rx: 45,
                ry: 35,
            },
            Waypoint {
                index: 2,
                rx: 35,
                ry: 45,
            },
            Waypoint {
                index: 3,
                rx: 45,
                ry: 45,
            },
        ]
    }

    fn entity_position_for_owner(sim: &Simulation, owner: &str) -> Option<(u16, u16)> {
        sim.entities().values().find_map(|entity| {
            (sim.interner().resolve(entity.owner()) == owner)
                .then_some((entity.position.rx(), entity.position.ry()))
        })
    }

    /// The land rows a scenario load binds: the start footprint's off-map
    /// cells read the shared Dummy's Clear row.
    fn test_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let mut terrain = crate::map::resolved_terrain::test_grid(width, height, |rx, ry| {
            test_terrain_cell(rx, ry, Some(100))
        });
        terrain.set_land_speed_rules_for_test(
            &crate::map::resolved_terrain::test_retail_clear_rules(),
        );
        terrain
    }

    fn test_terrain_cell(rx: u16, ry: u16, track_cost: Option<u8>) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            speed_costs: SpeedCostProfile {
                track: track_cost,
                ..Default::default()
            },
            zone_type: zone_class::GROUND,
            base_speed_costs: SpeedCostProfile {
                track: track_cost,
                ..Default::default()
            },
            ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
        }
    }

    fn roster_with_neutral_and_playable() -> HouseRoster {
        HouseRoster {
            houses: vec![
                HouseDefinition {
                    name: "Neutral".to_string(),
                    color: HouseColorIndex(8),
                    country: None,
                    side: None,
                    player_control: None,
                    iq: 0,
                    allies: 0,
                    base_plan: Default::default(),
                },
                HouseDefinition {
                    name: "Americans".to_string(),
                    color: HouseColorIndex(4),
                    country: Some("Americans".to_string()),
                    side: Some("Allies".to_string()),
                    player_control: Some(true),
                    iq: 0,
                    allies: 0,
                    base_plan: Default::default(),
                },
            ],
        }
    }

    #[test]
    fn launch_color_map_initializes_neutral_houses_and_uses_session_slots() {
        let schemes = crate::rules::color_scheme::parse_color_schemes(&IniFile::from_str(
            crate::test_fixture::text("ini/rulesmd.ini"),
        ));
        let colors = house_color_map_for_launch_session(
            &test_session(),
            &roster_with_neutral_and_playable(),
            &schemes,
        );

        // CreateHouses overrides Neutral's authored color. Players resolve their
        // lobby priority through the [Colors] LUT (priority 1 → entry 5 DarkRed,
        // priority 2 → entry 10 DarkBlue).
        assert_eq!(schemes[2].name, "LightGrey");
        assert_eq!(colors.get("Neutral"), Some(&HouseColorIndex(2)));
        assert_eq!(colors.get("Special"), Some(&HouseColorIndex(2)));
        assert_eq!(colors.get("Player"), Some(&HouseColorIndex(5)));
        assert_eq!(colors.get("Computer1"), Some(&HouseColorIndex(10)));
        assert!(!colors.contains_key("Americans"));
    }

    #[test]
    fn launch_color_map_remaps_neutral_oil_without_authored_houses() {
        let schemes = crate::rules::color_scheme::parse_color_schemes(&IniFile::from_str(
            crate::test_fixture::text("ini/rulesmd.ini"),
        ));
        // Ordinary multiplayer maps, including Storm, have neutral structures
        // but no campaign-style [Houses] roster.
        let map = IniFile::from_str("[Structures]\n0=Neutral,CAOILD,256,168,78,64,None,0,0,0\n");
        let roster = crate::map::houses::parse_house_roster(&map, &schemes, None);
        assert!(roster.houses.is_empty());
        let colors = house_color_map_for_launch_session(&test_session(), &roster, &schemes);
        assert_eq!(colors.get("Neutral"), Some(&HouseColorIndex(2)));
        assert_eq!(colors.get("Special"), Some(&HouseColorIndex(2)));
        assert_eq!(colors.get("Player"), Some(&HouseColorIndex(5)));
    }

    #[test]
    fn launch_color_map_resolves_neutral_scheme_by_name_and_native_fallback() {
        let mut roster = roster_with_neutral_and_playable();
        roster.houses[0].name = "nEuTrAl".to_string();
        for (ini, expected) in [
            ("[Colors]\nlightgrey=0,0,240\nGold=25,255,255\n", 0),
            (
                "[Colors]\nGold=25,255,255\nGrey=0,0,100\nOther=0,255,255\n",
                2,
            ),
        ] {
            let schemes = crate::rules::color_scheme::parse_color_schemes(&IniFile::from_str(ini));
            let colors = house_color_map_for_launch_session(&test_session(), &roster, &schemes);
            for name in ["Neutral", "Special", "nEuTrAl"] {
                assert_eq!(colors.get(name), Some(&HouseColorIndex(expected)));
            }
        }
    }

    #[test]
    fn current_house_collision_preserves_launch_label_and_distinct_colors() {
        let schemes = crate::rules::color_scheme::parse_color_schemes(&IniFile::from_str(
            crate::test_fixture::text("ini/rulesmd.ini"),
        ));
        for (handle, expected_key) in [
            ("Neutral", "Player"),
            (" Neutral ", "Player"),
            ("SPECIAL", "Player"),
            (" Special ", "Player"),
            ("computer1", "Player"),
            (" Computer1 ", "Player"),
            ("Computer2", "Computer2"),
            (" Computer2 ", " Computer2 "),
            ("Computer01", "Computer01"),
            ("Commander", "Commander"),
            (" Commander ", " Commander "),
        ] {
            let mut session = test_session();
            session.local_player_mut().name = handle.to_string();
            let original = session.clone();
            let slots = normalized_launch_slots(&session);
            assert_eq!(slots[0].owner_name, expected_key);
            assert_eq!(slots[1].owner_name, "Computer1");
            assert_ne!(slots[0].owner_name, slots[1].owner_name);
            let colors = house_color_map_for_launch_session(
                &session,
                &roster_with_neutral_and_playable(),
                &schemes,
            );
            assert_eq!(colors.get("Neutral"), Some(&HouseColorIndex(2)));
            assert_eq!(colors.get(expected_key), Some(&HouseColorIndex(5)));
            assert_eq!(colors.get("Computer1"), Some(&HouseColorIndex(10)));
            let row =
                crate::app::loading::progress_row::LoadingProgressRowSnapshot::from_launch_session(
                    &session,
                );
            assert_eq!(row.local_label(), handle);
            assert_eq!(
                session, original,
                "normalization cannot rewrite display or launch inputs"
            );
        }
        let mut without_ai = test_session();
        without_ai.local_player_mut().name = "Computer1".to_string();
        without_ai.opponents.clear();
        assert_eq!(
            normalized_launch_slots(&without_ai)[0].owner_name,
            "Computer1"
        );
        let mut two_ai = test_session();
        two_ai.opponents.push(two_ai.opponents[0].clone());
        for handle in ["cOmPuTeR2", " Computer2 "] {
            two_ai.local_player_mut().name = handle.to_owned();
            assert_eq!(normalized_launch_slots(&two_ai)[0].owner_name, "Player");
        }
    }

    /// gamemd-derived: active YR `ScenarioClass__Gather_Start_Positions
    /// @ 0x00688380`, seed block `0x00688528..0x0068857C`, draws Y before X
    /// from the full cell-array bounds. Research:
    /// `docs/research/skirmish-ui/SKIRMISH_GATHER_START_POSITIONS_DEFICIENT_WAYPOINT_FALLBACK_00688380_GHIDRA_REPORT.md`.
    #[test]
    fn native_gather_generates_deficient_start_with_two_ranged_draws() {
        let authored = Waypoint {
            index: 0,
            rx: 30,
            ry: 30,
        };
        let map = test_map_with_starts(&[authored]);
        let playfield_bounds = PlayfieldBounds::from_map_header(&map.header);
        let terrain = test_terrain(64, 64);
        let bounds = NativeStartBounds {
            min_rx: 0,
            min_ry: 0,
            width: 64,
            height: 64,
        };
        let mut rng = SimRng::new(9);
        let mut expected = rng.clone();
        let _ = expected.next_range_u32_inclusive(0, 54);
        let _ = expected.next_range_u32_inclusive(10, 54);
        let occupancy = crate::sim::occupancy::OccupancyGrid::new();

        let starts = native_gather_start_positions(
            &map.waypoints,
            2,
            &terrain,
            &occupancy,
            bounds,
            Some(playfield_bounds),
            Some(map.header.height as i32),
            0,
            &mut rng,
        );

        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0], authored);
        assert_eq!(starts[1].index, 1);
        assert_eq!(
            (starts[1].rx, starts[1].ry),
            (54, 21),
            "first native ranged draw is Y, second is X"
        );
        assert_eq!(
            find_nearby_start_rect(
                &terrain,
                &occupancy,
                Some(playfield_bounds),
                Some(map.header.height as i32),
                0,
                starts[1].rx,
                starts[1].ry,
            ),
            Some((starts[1].rx, starts[1].ry)),
        );
        assert_eq!(rng.logical_state(), expected.logical_state());
    }

    #[test]
    fn native_gather_rejects_a_blocked_cell_inside_the_8x8_rect() {
        let mut terrain = test_terrain(32, 32);
        let blocked = terrain.cell_mut_for_test(11, 11).unwrap();
        blocked.speed_costs.track = Some(0);
        blocked.base_speed_costs.track = Some(0);
        blocked.zone_type = zone_class::IMPASSABLE;
        let occupancy = crate::sim::occupancy::OccupancyGrid::new();
        let diamond = PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        };

        let moved = find_nearby_start_rect(&terrain, &occupancy, Some(diamond), Some(32), 0, 4, 4)
            .expect("a ring-1 8x8 rectangle clears the blocked far corner");
        assert_ne!(moved, (4, 4), "all 64 footprint cells must pass");
        assert_eq!(
            find_nearby_start_rect(&terrain, &occupancy, Some(diamond), Some(32), 0, 16, 4,),
            Some((16, 4)),
            "an unobstructed 8x8 seed remains eligible",
        );
    }

    #[test]
    fn native_gather_rejects_live_occupation_inside_the_8x8_rect() {
        let terrain = test_terrain(32, 32);
        let mut occupancy = crate::sim::occupancy::OccupancyGrid::new();
        occupancy.add(
            11,
            11,
            1,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
        let diamond = PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        };

        assert_ne!(
            find_nearby_start_rect(&terrain, &occupancy, Some(diamond), Some(32), 0, 4, 4,),
            Some((4, 4)),
            "CellRect passability must read occupation across the 8x8 footprint",
        );
        assert_eq!(
            find_nearby_start_rect(&terrain, &occupancy, Some(diamond), Some(32), 0, 16, 4,),
            Some((16, 4)),
        );
    }

    /// gamemd-derived: `ScenarioClass::Gather_Start_Positions @ 0x00688380`
    /// reaches `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20`, whose null
    /// reference branch reads `g_CurrentFrameCounter` at `0x0056E6A8..0x0056E6ED`.
    #[test]
    fn simulation_gather_threads_live_binary_frame_into_deficient_start_selection() {
        let authored = Waypoint {
            index: 0,
            rx: 30,
            ry: 30,
        };
        let map = test_map_with_starts(&[authored]);
        let mut terrain = test_terrain(64, 64);
        let blocked = terrain.cell_mut_for_test(61, 28).unwrap();
        blocked.speed_costs.track = Some(0);
        blocked.base_speed_costs.track = Some(0);
        blocked.zone_type = zone_class::IMPASSABLE;
        let bounds = NativeStartBounds {
            min_rx: 0,
            min_ry: 0,
            width: 64,
            height: 64,
        };

        let gather = |frame| {
            let mut sim = Simulation::new();
            sim.install_playfield_from_map_header(&map.header);
            sim.session_mut_for_test().set_binary_frame_for_test(frame);
            *sim.scenario_rng_mut_for_test() = SimRng::new(9);
            sim.gather_native_start_positions(&map.waypoints, 2, &terrain, bounds)[1]
        };

        let frame_zero = gather(0);
        let frame_one = gather(1);
        assert_eq!((frame_zero.rx, frame_zero.ry), (53, 20));
        assert_eq!((frame_one.rx, frame_one.ry), (53, 22));
        assert_ne!(frame_zero, frame_one);
    }

    /// gamemd-derived: active YR `MapClass__Find_Nearby_Passable_Cell
    /// @ 0x0056DC20` gates the candidate anchor through
    /// `MapClass__Is_Cell_In_Playfield_CellClass @ 0x00578540` before the 8x8
    /// scan. Research:
    /// `docs/research/skirmish-ui/FOOTCLASS_FIND_NEARBY_PASSABLE_CELL_0056DC20_START_FALLBACK_GHIDRA_REPORT.md`.
    #[test]
    fn deficient_start_skips_passable_anchor_outside_diamond() {
        let terrain = test_terrain(32, 32);
        let occupancy = crate::sim::occupancy::OccupancyGrid::new();
        let diamond = PlayfieldBounds {
            base: 10,
            off_fc: 2,
            off_100: 1,
            off_104: 10,
            off_108: 6,
        };
        assert!(!cell_is_in_playfield_height_aware(
            (6, 6),
            Some(diamond),
            Some(&terrain),
        ));

        assert_eq!(
            find_nearby_start_rect(&terrain, &occupancy, Some(diamond), Some(32), 0, 6, 6,),
            Some((6, 7))
        );
    }

    /// gamemd-derived: `CellRect__CheckPassability @ 0x0056E7C0` scans the 8x8
    /// footprint after `0x00578540` gates only its anchor; retail does not
    /// diamond-test the other 63 cells. Research:
    /// `docs/research/skirmish-ui/FOOTCLASS_FIND_NEARBY_PASSABLE_CELL_0056DC20_START_FALLBACK_GHIDRA_REPORT.md`.
    #[test]
    fn deficient_start_gates_anchor_not_full_8x8_footprint() {
        let terrain = test_terrain(8, 8);
        let occupancy = crate::sim::occupancy::OccupancyGrid::new();
        let old_axis_bounds = NativeStartBounds {
            min_rx: 0,
            min_ry: 0,
            width: 8,
            height: 8,
        };
        let diamond = PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        };
        assert!(cell_is_in_playfield_height_aware(
            (7, 7),
            Some(diamond),
            Some(&terrain),
        ));
        assert!(
            7 + 8 - 1 > i32::from(old_axis_bounds.min_rx) + i32::from(old_axis_bounds.width) - 1,
            "the retired full-rectangle bounds gate would reject this anchor",
        );

        assert_eq!(
            find_nearby_start_rect(&terrain, &occupancy, Some(diamond), Some(8), 0, 7, 7,),
            Some((7, 7)),
            "only the anchor is diamond-gated; the remaining footprint uses real/dummy CellRect lookup",
        );
    }

    /// gamemd-derived: standard Battle dispatches start gathering through its
    /// `+0x80 @ 0x005D6BE0` and `+0x84 @ 0x005D6C70` phases, both reaching
    /// `ScenarioClass__Gather_Start_Positions @ 0x00688380`.
    #[test]
    fn gsi_04_16_standard_battle_gathers_deficient_starts_twice() {
        let authored = Waypoint {
            index: 0,
            rx: 30,
            ry: 30,
        };
        let map = test_map_with_starts(&[authored]);
        let terrain = test_terrain(64, 64);
        let mut session = test_session();
        session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
        session.opponents[0].start_position = LaunchStartPosition::Position(1);
        session.options.bases = false;
        session.options.unit_count = 0;
        let plan = prepare_stock_offline_scenario_prefix_plan(
            &launch_descriptor(&session),
            &map,
            &map.waypoints,
            9,
        )
        .expect("deficient starts are resolved before Fill");
        assert_ne!(
            plan.first_gathered_starts()[1],
            plan.final_gathered_starts()[1]
        );

        let mut bootstrap_rng = ScenarioBootstrapRng::new(9);
        let projection = bootstrap_rng
            .install_pre_fill_scenario_prefix_plan(plan)
            .expect("matching pre-Fill prefix");
        let mut sim =
            bootstrap_rng.into_simulation(&crate::sim::scenario_session::ScenarioDescriptor {
                seed: 9,
                ..Default::default()
            });
        // This synthetic fixture omits the production native Rules prefix.
        *sim.native_unique_ids_mut_for_test() =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        let mut expected_rng = sim.scenario_rng().clone();
        let _ = expected_rng.next_range_u32_inclusive(0, 0xffff);

        apply_pre_fill_scenario_prefix_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &test_standard_launch_rules(),
            &terrain,
            &launch_descriptor(&session),
            &projection,
        );

        assert_eq!(
            crate::sim::house_state::house_state_for_owner(sim.houses(), "Player", sim.interner())
                .and_then(|house| house.base_center()),
            Some((authored.rx, authored.ry))
        );
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(
                sim.houses(),
                "Computer1",
                sim.interner(),
            )
            .and_then(|house| house.base_center()),
            Some((
                projection.final_gathered_starts()[1].rx,
                projection.final_gathered_starts()[1].ry,
            ))
        );
        assert_eq!(
            sim.scenario_rng().logical_state(),
            expected_rng.logical_state()
        );
    }

    #[test]
    fn gsi_17_01_launch_registration_orders_participants_before_guaranteed_special_houses() {
        let session = test_session();
        let mut sim = Simulation::new();
        let rules = test_standard_launch_rules();

        initialize_skirmish_launch_houses(
            &mut sim,
            &HouseRoster::default(),
            &rules,
            &launch_descriptor(&session),
        );

        let order: Vec<_> = sim
            .houses()
            .order()
            .iter()
            .map(|id| sim.interner().resolve(*id))
            .collect();
        assert_eq!(order, ["Player", "Computer1", "Neutral", "Special"]);
        assert_eq!(sim.houses().len(), 4);
        let current_iq = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(sim.houses(), owner, sim.interner())
                .map(|house| house.current_iq())
        };
        assert_eq!(current_iq("Player"), Some(0));
        assert_eq!(current_iq("Computer1"), Some(rules.general.max_iq_levels));
        assert_eq!(current_iq("Neutral"), Some(0));
        assert_eq!(current_iq("Special"), Some(0));
    }

    #[test]
    fn skirmish_launch_houses_construct_every_super_weapon_type() {
        // `HouseClass::HouseClass` builds one Super per `[SuperWeaponTypes]`
        // entry (`0x004F620E..0x004F6290`) for every House: the launch
        // participants and Neutral/Special alike. The grant pass only walks
        // that list, so a House without it never receives a superweapon.
        let session = test_session();
        let mut sim = Simulation::new();
        let rules = RuleSet::from_ini(&launch_fixture_ini(
            "[SuperWeaponTypes]\n0=ParaDropSpecial\n1=NukeSpecial\n\
             [ParaDropSpecial]\n[NukeSpecial]\n",
        ))
        .expect("super weapon rules");
        assert_eq!(rules.super_weapon_order, ["ParaDropSpecial", "NukeSpecial"]);

        initialize_skirmish_launch_houses(
            &mut sim,
            &HouseRoster::default(),
            &rules,
            &launch_descriptor(&session),
        );

        for owner in ["Player", "Computer1", "Neutral", "Special"] {
            let id = sim.interner().get(owner).expect("launch house");
            let mut types: Vec<&str> =
                sim.super_weapons()
                    .get(&id)
                    .map_or_else(Vec::new, |supers| {
                        supers
                            .keys()
                            .map(|&type_id| sim.interner().resolve(type_id))
                            .collect()
                    });
            types.sort_unstable();
            assert_eq!(types, ["NukeSpecial", "ParaDropSpecial"], "{owner}");
        }
    }

    #[test]
    fn gsi_17_01_preplaced_structure_reveal_accounts_against_preexisting_house_once() {
        use crate::rules::art_data::ArtRegistry;

        let art = IniFile::from_str("[GACNST]\nFoundation=4x4\n");
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[AI]\nAIBaseSpacing=2\n\
             [Countries]\n0=Americans\n1=Russians\n2=Neutral\n3=Special\n\
             [BuildingTypes]\n0=GACNST\n\
             [GACNST]\nStrength=1000\nUndeploysInto=AMCV\n",
            ),
            &art,
        )
        .expect("structure fixture rules");
        rules.install_art_data(ArtRegistry::from_ini(&art));
        assert_eq!(
            rules.object("GACNST").unwrap().base_reservation_spacing,
            Some(2)
        );

        let mut session = test_session();
        session.local_slot_mut().country = LaunchCountry::from_country_index(u32::from(
            rules.country_index("Americans").unwrap().0,
        ));
        session.opponents[0].country = LaunchCountry::from_country_index(u32::from(
            rules.country_index("Russians").unwrap().0,
        ));
        let mut sim = Simulation::new();
        initialize_skirmish_launch_houses(
            &mut sim,
            &HouseRoster::default(),
            &rules,
            &launch_descriptor(&session),
        );
        assert!(sim.entities().is_empty());

        let spawned = sim.spawn_from_map(
            &[crate::map::entities::MapEntity {
                // Native offline Structures resolves the generated AI name.
                // The current human is excluded before the constructor.
                owner: "Computer".to_owned(),
                type_id: "GACNST".to_owned(),
                health: 256,
                cell_x: 30,
                cell_y: 40,
                facing: 0,
                category: EntityCategory::Structure,
                sub_cell: 0,
                veterancy: None,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
                structure_ai_sellable: false,
                structure_ai_repairable: false,
                structure_online: true,
                trigger_tag: None,
            }],
            Some(&rules),
        );

        assert_eq!(spawned, 1);
        let computer = sim
            .interner()
            .get("Computer1")
            .expect("pre-created first AI house");
        assert_eq!(sim.houses()[&computer].tracking().buildings(), 1);
        assert_eq!(
            sim.base_reservations().raw_mask(None, 28, 38),
            2,
            "Reveal writes the participant's house-index bit without a repair pass"
        );
    }

    /// gamemd-derived: `ScenarioClass__Post_Map_Init @ 0x006869BE..0x00686A7E`
    /// grants `ftol(MultiplayerAICM[difficulty] * 0.01 * Available_Money)` to
    /// every non-human, non-MultiplayPassive house. Stock `400,0,0` makes a
    /// Hard AI open with 5x its balance; Normal and Easy add 0.
    #[test]
    fn gsi_09_01_post_map_init_grants_multiplayer_ai_cm_percent_by_difficulty() {
        let mut session = test_session();
        session.opponents[0].difficulty = AiDifficulty::Hard;
        for (color_index, difficulty) in [
            (3, AiDifficulty::Hard),
            (4, AiDifficulty::Normal),
            (5, AiDifficulty::Easy),
        ] {
            session.opponents.push(SkirmishAiSlot {
                raw_slot_index: (session.opponents.len() + 1) as u8,
                color_index,
                difficulty,
                ..session.opponents[0].clone()
            });
        }
        let mut sim = Simulation::new();
        sim.session_mut_for_test().set_game_options_for_test(
            session
                .options
                .to_game_options(session.opponents.len() as i32),
        );
        let mut rules = test_standard_launch_rules();
        rules.general.multiplayer_ai_cm = vec![400, 0, 0];
        populate_launch_houses(&mut sim, &normalized_launch_slots(&session), &rules);
        populate_special_houses(&mut sim, &HouseRoster::default(), &rules);

        let passive_ai = sim.interner().get("Computer2").expect("passive AI");
        sim.houses_mut_for_test()
            .get_mut(&passive_ai)
            .expect("passive AI house")
            .set_multiplay_passive_for_test(true);
        for owner in ["Neutral", "Special"] {
            let owner = sim.interner().get(owner).expect("special house");
            sim.houses_mut_for_test()
                .get_mut(&owner)
                .expect("special HouseState")
                .set_multiplay_passive_for_test(true);
        }
        let observer = sim.intern("Observer");
        sim.houses_mut_for_test()
            .register(HouseState::new(observer, 0, None, false, 10_000, 10));

        apply_skirmish_ai_opening_credits(&mut sim, &rules);

        let credits = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(sim.houses(), owner, sim.interner())
                .map(|house| house.economy().credits())
        };
        assert_eq!(credits("Player"), Some(10_000));
        assert_eq!(credits("Computer1"), Some(50_000), "Hard: +400%");
        assert_eq!(
            credits("Computer2"),
            Some(10_000),
            "passive Hard AI skipped"
        );
        assert_eq!(credits("Computer3"), Some(10_000), "Normal: +0%");
        assert_eq!(credits("Computer4"), Some(10_000), "Easy: +0%");
        // Fixed-tail Houses retain ctor4F58D8 cash0; the passive grant
        // neither seeds them with participant cash nor adds a bonus.
        assert_eq!(credits("Neutral"), Some(0));
        assert_eq!(credits("Special"), Some(0));
        assert_eq!(credits("Observer"), Some(10_000));
    }

    /// The x87 product is chopped by `Math__ftol @ 0x007C5F00`; a missing list
    /// entry (empty constructor vector at `0x00667034`) grants nothing.
    #[test]
    fn gsi_09_01_ai_opening_grant_truncates_and_tolerates_missing_entries() {
        let mut session = test_session();
        session.opponents[0].difficulty = AiDifficulty::Hard;
        let mut sim = Simulation::new();
        sim.session_mut_for_test().set_game_options_for_test(
            session
                .options
                .to_game_options(session.opponents.len() as i32),
        );
        let mut rules = test_standard_launch_rules();
        populate_launch_houses(&mut sim, &normalized_launch_slots(&session), &rules);
        let ai = sim.interner().get("Computer1").expect("AI house");
        sim.houses_mut_for_test()
            .get_mut(&ai)
            .expect("AI house")
            .economy_mut_for_test()
            .set_credits_for_test(12_345);

        rules.general.multiplayer_ai_cm = Vec::new();
        apply_skirmish_ai_opening_credits(&mut sim, &rules);
        assert_eq!(sim.houses()[&ai].economy().credits(), 12_345);

        // 7 * 0.01 * 12345 = 864.15... -> 864 under chop.
        rules.general.multiplayer_ai_cm = vec![7, 0, 0];
        apply_skirmish_ai_opening_credits(&mut sim, &rules);
        assert_eq!(sim.houses()[&ai].economy().credits(), 12_345 + 864);
    }

    #[test]
    fn skirmish_launch_same_explicit_team_creates_mutual_alliance() {
        let mut session = test_session();
        session.local_slot_mut().team = LaunchTeam::Team(0);
        session.opponents[0].team = LaunchTeam::Team(0);
        session.opponents.push(SkirmishAiSlot {
            raw_slot_index: (session.opponents.len() + 1) as u8,
            country: LaunchCountry::Cuba,
            country_random: false,
            color_index: 3,
            color_random: false,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::Team(1),
            difficulty: Default::default(),
        });
        let slots = normalized_launch_slots(&session);
        let alliances =
            launch_alliance_map(&roster_with_neutral_and_playable(), &slots, &session.mode);

        assert!(
            alliances
                .get("PLAYER")
                .is_some_and(|allies| allies.contains("COMPUTER1"))
        );
        assert!(
            alliances
                .get("COMPUTER1")
                .is_some_and(|allies| allies.contains("PLAYER"))
        );
        assert!(
            !alliances
                .get("PLAYER")
                .is_some_and(|allies| allies.contains("COMPUTER2"))
        );
    }

    #[test]
    fn cooperative_mode_allies_nonspecial_houses_by_control_group() {
        let mut session = test_session();
        session.mode = test_cooperative_mode();
        session.opponents.push(SkirmishAiSlot {
            raw_slot_index: (session.opponents.len() + 1) as u8,
            country: LaunchCountry::Cuba,
            country_random: false,
            color_index: 3,
            color_random: false,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
            difficulty: Default::default(),
        });
        let slots = normalized_launch_slots(&session);

        let alliances =
            launch_alliance_map(&roster_with_neutral_and_playable(), &slots, &session.mode);

        assert!(
            alliances
                .get("COMPUTER1")
                .is_some_and(|allies| allies.contains("COMPUTER2"))
        );
        assert!(
            !alliances
                .get("PLAYER")
                .is_some_and(|allies| allies.contains("COMPUTER1"))
        );
    }

    #[test]
    fn automatic_start_distance_uses_the_retail_lut_tie() {
        let starts = [
            Waypoint {
                index: 0,
                rx: 100,
                ry: 100,
            },
            Waypoint {
                index: 1,
                rx: 100,
                ry: 229,
            },
            Waypoint {
                index: 2,
                rx: 228,
                ry: 100,
            },
        ];
        assert_eq!(native_start_distance(starts[1], starts[0]), 128);
        assert_eq!(native_start_distance(starts[2], starts[0]), 128);

        let mut rng = SimRng::new(7);
        assert_eq!(
            choose_battle_automatic_start(&starts, &[true, false, false], &mut rng),
            1
        );
    }

    #[test]
    fn cooperative_start_uses_human_prefix_then_ai_suffix() {
        let mut session = test_session();
        session.local_slot_mut().start_position = LaunchStartPosition::Auto;
        let starts = test_launch_starts();
        let mut expected_rng = SimRng::new(17);
        let expected_human = expected_rng.next_range_u32_inclusive(0, 1) as usize;
        let mut rng = SimRng::new(17);

        let assignments = native_assign_cooperative_launch_starts(&session, &starts, 2, &mut rng);

        assert_eq!(assignments.placements[0], (0, starts[expected_human]));
        assert_eq!(assignments.placements[1], (1, starts[2]));
        assert_eq!(rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn cooperative_explicit_table_is_reserved_before_house_iteration() {
        let mut session = test_session();
        session.local_slot_mut().start_position = LaunchStartPosition::Auto;
        session.opponents[0].start_position = LaunchStartPosition::Position(0);
        let starts = test_launch_starts();
        let mut rng = SimRng::new(17);
        let before = rng.logical_state();

        let assignments = native_assign_cooperative_launch_starts(&session, &starts, 2, &mut rng);

        assert_eq!(assignments.placements[0], (0, starts[2]));
        assert_eq!(assignments.placements[1], (1, starts[0]));
        assert_eq!(rng.logical_state(), before);
    }

    #[test]
    fn gsi_04_16_standard_ai_explicit_start_reserves_before_auto_human() {
        let mut sim = Simulation::new();
        *sim.scenario_rng_mut_for_test() = SimRng::new(8);
        let mut session = test_session();
        session.local_slot_mut().start_position = LaunchStartPosition::Auto;
        session.opponents[0].start_position = LaunchStartPosition::Position(0);
        session.options.bases = false;
        session.options.unit_count = 0;
        let all_starts = test_launch_starts();
        let starts = &all_starts[..3];
        let map = test_map_with_starts(starts);
        let terrain = test_terrain(64, 64);
        let rules = test_standard_launch_rules();
        let mut expected_rng = sim.scenario_rng().clone();
        // The full explicit table is already occupied before the House pass.
        // The Auto human therefore selects farthest with no draw, and the AI
        // later honors its explicit entry. Only the selected-mode tail draw
        // remains after assignment.
        let _ = expected_rng.next_range_u32_inclusive(0, 0xffff);

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.active_slots, 2);
        assert_eq!(result.spawned_mcvs, 0);
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(sim.houses(), "Player", sim.interner())
                .and_then(|house| house.base_center()),
            Some((30, 10)),
            "the Auto human takes the first farthest free tie, not the AI reservation"
        );
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(
                sim.houses(),
                "Computer1",
                sim.interner(),
            )
            .and_then(|house| house.base_center()),
            Some((10, 10)),
            "standard Battle honors the AI's preassigned table entry"
        );
        let table_owner = |start_idx| {
            sim.session()
                .start_slot_houses()
                .get(&start_idx)
                .map(|owner| sim.interner().resolve(*owner))
        };
        assert_eq!(table_owner(0), Some("Computer1"));
        assert_eq!(table_owner(1), Some("Player"));
        assert_eq!(table_owner(2), None);
        assert_eq!(sim.session().start_slot_houses().len(), 2);
        assert_eq!(
            sim.scenario_rng().logical_state(),
            expected_rng.logical_state()
        );
    }

    #[test]
    fn gsi_04_16_first_loading_markers_match_final_battle_start_table() {
        use crate::app::loading::composition::{LoadingParticipantId, LoadingStartAssignment};

        let launch_seed = 8;
        let mut session = test_session();
        session.local_slot_mut().start_position = LaunchStartPosition::Auto;
        session.opponents[0].start_position = LaunchStartPosition::Position(0);
        session.options.bases = false;
        session.options.unit_count = 0;
        let starts = test_launch_starts();
        let map = test_map_with_starts(&starts);
        let terrain = test_terrain(64, 64);
        let rules = test_standard_launch_rules();
        let plan = prepare_stock_offline_scenario_prefix_plan(
            &launch_descriptor(&session),
            &map,
            &map.waypoints,
            launch_seed,
        )
        .expect("complete stock-offline Scenario prefix");

        let loading_assignments = crate::app::loading::pump::selected_map_start_assignments(
            &session,
            Some(plan.projection()),
        );
        assert_eq!(
            loading_assignments,
            vec![
                LoadingStartAssignment {
                    start_index: 0,
                    participant: LoadingParticipantId::Opponent(0),
                    color_priority: session.opponents[0].color_index,
                },
                LoadingStartAssignment {
                    start_index: 3,
                    participant: LoadingParticipantId::Local,
                    color_priority: session.local_slot().color_index,
                },
            ]
        );

        let mut bootstrap_rng = ScenarioBootstrapRng::new(launch_seed);
        let projection = bootstrap_rng
            .install_pre_fill_scenario_prefix_plan(plan)
            .expect("fresh launch cursor matches plan prestate");
        let descriptor = crate::sim::scenario_session::ScenarioDescriptor {
            seed: launch_seed,
            ..Default::default()
        };
        let mut sim = bootstrap_rng.into_simulation(&descriptor);
        // This synthetic fixture omits the production native Rules prefix.
        *sim.native_unique_ids_mut_for_test() =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        let mut expected_rng = SimRng::new(u64::from(launch_seed));
        for _ in 0..8 {
            let _ = expected_rng
                .next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX);
        }
        assert_eq!(
            sim.scenario_rng().logical_state(),
            expected_rng.logical_state(),
            "one human, one AI, Neutral, and Special burn twice before loading"
        );

        let result = apply_pre_fill_scenario_prefix_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &terrain,
            &launch_descriptor(&session),
            &projection,
        );
        assert_eq!(result.active_slots, 2);
        let table_owner = |start_idx| {
            sim.session()
                .start_slot_houses()
                .get(&start_idx)
                .map(|owner| sim.interner().resolve(*owner))
        };
        for assignment in &loading_assignments {
            let expected_owner = match assignment.participant {
                LoadingParticipantId::Local => "Player",
                LoadingParticipantId::Human(index) => {
                    panic!("unexpected remote human marker {index}")
                }
                LoadingParticipantId::Opponent(0) => "Computer1",
                LoadingParticipantId::Opponent(index) => {
                    panic!("unexpected opponent marker {index}")
                }
            };
            assert_eq!(table_owner(assignment.start_index), Some(expected_owner));
        }
        assert_eq!(table_owner(0), Some("Computer1"));
        assert_eq!(table_owner(3), Some("Player"));
        assert_eq!(sim.session().start_slot_houses().len(), 2);

        // Auto local with an explicit AI reservation uses the farthest selector
        // without an assignment draw. Applying the stored plan must not replay
        // constructor or assignment RNG; only the starting-force tail remains.
        let _ = expected_rng.next_range_u32_inclusive(0, 0xffff);
        assert_eq!(
            sim.scenario_rng().logical_state(),
            expected_rng.logical_state()
        );
    }

    #[test]
    fn gsi_04_16_standard_all_auto_randomizes_first_then_chooses_farthest() {
        let mut session = test_session();
        session.local_slot_mut().start_position = LaunchStartPosition::Auto;
        session.opponents[0].start_position = LaunchStartPosition::Auto;
        let starts = [
            Waypoint {
                index: 0,
                rx: 10,
                ry: 10,
            },
            Waypoint {
                index: 1,
                rx: 11,
                ry: 10,
            },
            Waypoint {
                index: 2,
                rx: 50,
                ry: 50,
            },
        ];
        let mut expected_rng = SimRng::new(8);
        assert_eq!(expected_rng.next_range_u32_inclusive(0, 2), 0);
        let mut rng = SimRng::new(8);

        let assignment = native_assign_launch_starts(&session, &starts, &mut rng);

        assert_eq!(assignment.placements, vec![(0, starts[0]), (1, starts[2])]);
        assert_eq!(assignment.start_table, vec![Some(0), None, Some(1)]);
        assert_eq!(rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn gsi_04_16_standard_duplicate_explicit_start_is_last_writer_wins() {
        let mut session = test_session();
        session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
        session.opponents[0].start_position = LaunchStartPosition::Position(0);
        let all_starts = test_launch_starts();
        let starts = &all_starts[..3];
        let mut rng = SimRng::new(8);
        let before = rng.logical_state();

        let assignment = native_assign_launch_starts(&session, starts, &mut rng);

        assert_eq!(assignment.placements, vec![(0, starts[1]), (1, starts[0])]);
        assert_eq!(assignment.start_table, vec![Some(1), Some(0), None]);
        assert_eq!(rng.logical_state(), before);
    }

    #[test]
    fn gsi_04_16_standard_three_auto_houses_consume_only_first_draw() {
        let mut session = test_session();
        session.local_slot_mut().start_position = LaunchStartPosition::Auto;
        session.opponents[0].start_position = LaunchStartPosition::Auto;
        session.opponents.push(SkirmishAiSlot {
            raw_slot_index: (session.opponents.len() + 1) as u8,
            country: LaunchCountry::Cuba,
            country_random: false,
            color_index: 3,
            color_random: false,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
            difficulty: Default::default(),
        });
        let starts = test_launch_starts();
        let mut expected_rng = SimRng::new(8);
        assert_eq!(expected_rng.next_range_u32_inclusive(0, 3), 0);
        let mut rng = SimRng::new(8);

        let assignment = native_assign_launch_starts(&session, &starts, &mut rng);

        assert_eq!(
            assignment.placements,
            vec![(0, starts[0]), (1, starts[3]), (2, starts[1])]
        );
        assert_eq!(
            assignment.start_table,
            vec![Some(0), Some(2), None, Some(1)]
        );
        assert_eq!(rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn explicit_launch_local_owner_drives_production_sidebar_theme() {
        use crate::render::sidebar_chrome::SidebarTheme;

        let terrain = test_terrain(64, 64);
        let starts = test_launch_starts();
        let map = test_map_with_starts(&starts);
        let rules = test_standard_launch_rules();
        let roster = roster_with_neutral_and_playable();

        for (country, expected_theme) in [
            (LaunchCountry::America, SidebarTheme::Allied),
            (LaunchCountry::Russia, SidebarTheme::Soviet),
            (LaunchCountry::Yuri, SidebarTheme::Yuri),
        ] {
            let mut sim = Simulation::new();
            let mut session = test_session();
            session.local_player_mut().name = "Commander".to_string();
            session.local_slot_mut().country = country;
            session.options.bases = false;
            session.options.unit_count = 0;

            let result = apply_explicit_skirmish_launch_session(
                &mut sim,
                &map,
                &roster,
                &rules,
                &terrain,
                &launch_descriptor(&session),
            );
            let owner = result
                .local_owner
                .as_deref()
                .expect("explicit launch must return its pinned local owner");
            assert_eq!(owner, "Commander");

            let actual = crate::app::presentation::sidebar_render::sidebar_theme_for_owner(
                Some(&sim),
                owner,
            )
            .unwrap_or(SidebarTheme::Allied);
            assert_eq!(
                actual, expected_theme,
                "{country:?} launch owner must reach its live side theme"
            );
            // The map-install refresh and chrome selection use this same source
            // projection after the new roster, simulation and owner are installed.
            let assets = [
                "allied-frame-palette-insets",
                "soviet-frame-palette-insets",
                "yuri-frame-palette-insets",
            ];
            let selected = crate::app::presentation::sidebar_render::project_sidebar_source(
                Some(&sim),
                Some(owner),
                Some(&assets[0]),
                Some(&assets[1]),
                Some(&assets[2]),
            )
            .expect("all three source packages installed");
            assert_eq!((selected.0, selected.1), (expected_theme, expected_theme));
            let index = match expected_theme {
                SidebarTheme::Allied => 0,
                SidebarTheme::Soviet => 1,
                SidebarTheme::Yuri => 2,
            };
            assert_eq!(*selected.2, assets[index]);
            let fallback = crate::app::presentation::sidebar_render::project_sidebar_source(
                Some(&sim),
                Some(owner),
                Some(&assets[0]),
                None,
                None,
            )
            .unwrap();
            assert_eq!(
                (fallback.0, fallback.1),
                (expected_theme, SidebarTheme::Allied)
            );
            assert_eq!(
                *fallback.2, assets[0],
                "radar and chrome share the actual fallback package"
            );
        }
    }

    /// F09: the descriptor supersedes the old "explicit application bypasses
    /// the legacy resolver even with a poison random flag" guarantee — an
    /// unresolved random flag can no longer cross into sim at all, so the
    /// legacy resolver has nothing to bypass.
    #[test]
    fn unresolved_shell_random_flag_cannot_cross_into_sim() {
        let mut poisoned = test_session();
        poisoned.local_slot_mut().country_random = true;
        assert_eq!(
            MatchLaunchDescriptor::from_resolved(poisoned),
            Err(UnresolvedShellChoice {
                ai_slot: None,
                choice: "country",
            })
        );

        let mut poisoned_ai = test_session();
        poisoned_ai.opponents[0].color_random = true;
        assert_eq!(
            MatchLaunchDescriptor::from_resolved(poisoned_ai),
            Err(UnresolvedShellChoice {
                ai_slot: Some(0),
                choice: "color",
            })
        );
    }

    /// The RNG contract the poison test previously pinned: explicit
    /// application consumes exactly the start-assignment draw and nothing
    /// from any legacy random-resolution path.
    #[test]
    fn explicit_skirmish_application_consumes_only_the_start_assignment_draw() {
        let mut sim = Simulation::new();
        let mut session = test_session();
        session.opponents[0].start_position = LaunchStartPosition::Position(0);
        session.options.bases = false;
        session.options.unit_count = 0;
        let terrain = test_terrain(64, 64);
        let starts = test_launch_starts();
        let map = test_map_with_starts(&starts);
        let rules = test_standard_launch_rules();
        let mut expected_scenario = sim.scenario_rng().clone();
        let _ = expected_scenario.next_range_u32_inclusive(0, 0xffff);

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 0);
        assert_eq!(result.active_slots, 2);
        assert_eq!(sim.rng_state().scenario, expected_scenario.logical_state());
    }

    #[test]
    fn skirmish_bases_off_still_allows_unit_count_extra_units() {
        let mut sim = Simulation::new();
        let mut session = test_session();
        session.options.bases = false;
        session.options.unit_count = 1;
        let terrain = test_terrain(64, 64);
        let starts = test_in_playfield_launch_starts();
        let mut map = test_map_with_starts(&starts);
        map.special_flags.initial_veteran = Some(true);
        let rules = test_starting_unit_rules();

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 0);
        assert_eq!(sim.entities().len(), 4);
        assert_eq!(entity_position_for_owner(&sim, "Player"), Some((45, 45)));
        assert_eq!(entity_position_for_owner(&sim, "Computer1"), Some((35, 35)));
        // `0x005D73FD..0x005D740A`: SpecialFlags bit 9 → `SetElite(1)`, which
        // is the 2.0f accumulator, not just the rank projection — a unit
        // seeded only in the projection would demote on its first kill.
        assert!(sim.entities().values().all(|entity| {
            entity.veterancy.rank() == 200 && entity.veterancy.raw().bits() == 0x4000_0000
        }));
        assert!(sim.entities().values().all(|entity| {
            let expected = if sim.interner().resolve(entity.owner()) == "Player" {
                MissionType::Guard
            } else {
                MissionType::AreaGuard
            };
            entity.mission.current().known() == Some(expected)
        }));
    }

    #[test]
    fn skirmish_unit_count_zero_spawns_mcv_only_when_bases_enabled() {
        let mut sim = Simulation::new();
        let mut session = test_session();
        session.options.unit_count = 0;
        session.options.bases = true;
        let terrain = test_terrain(64, 64);
        let starts = test_launch_starts();
        let map = test_map_with_starts(&starts);
        let rules = test_standard_launch_rules();

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 2);
        assert_eq!(
            sim.playfield_bounds(),
            Some(PlayfieldBounds::from_map_header(&map.header)),
            "direct pre-funnel launch must install normalized MapClass fields"
        );
        assert_eq!(sim.session().game_options().unit_count, 0);
        assert_eq!(sim.entities().len(), 2);
    }

    /// gamemd-derived: active YR
    /// `MultiplayerGameMode__Create_Starting_Base_Unit @ 0x005D7030` falls
    /// through to `Try_Unlimbo_Object_At_Or_Near_Cell @ 0x00688ED0`; probes
    /// clamp through the `0x00565C10` full cell-array rect and then use the
    /// `0x00578460` diamond. Research:
    /// `docs/research/skirmish-ui/SKIRMISH_MCV_NEARBY_PLACEMENT_FALLBACK_00688ED0_GHIDRA_REPORT.md`.
    #[test]
    fn nearoref_blocked_start_fallback_uses_full_cell_array_clamp() {
        let mut sim = Simulation::with_seed(0);
        let mut session = test_session();
        session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
        session.opponents.clear();
        session.options.bases = true;
        session.options.unit_count = 0;
        let starts = [Waypoint {
            index: 0,
            rx: 100,
            ry: 75,
        }];
        let mut map = test_map_with_starts(&starts);
        configure_nearoref_geometry(&mut sim, &mut map);
        let terrain = test_terrain(138, 138);
        let rules = test_standard_launch_rules();
        let descriptor = launch_descriptor(&session);
        initialize_skirmish_launch_houses(
            &mut sim,
            &roster_with_neutral_and_playable(),
            &rules,
            &descriptor,
        );
        sim.spawn_object("AMCV", "Neutral", 100, 75, STARTING_MCV_FACING, &rules)
            .expect("authored start blocker");
        let mut expected_rng = sim.scenario_rng().clone();
        // TechnoClass::TechnoClass @ 0x006F2B90 consumes its raw Scenario
        // word at 0x006F3254 before Try_Unlimbo chooses a fallback spoke.
        let _ = expected_rng.next_u32();
        assert_eq!(
            expected_rng.next_range_u32_inclusive(0, 7),
            5,
            "after construction, seed zero chooses the southwest radius-one spoke"
        );
        let _ = expected_rng.next_range_u32_inclusive(0, 0xffff);

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &terrain,
            &descriptor,
        );

        assert_eq!(result.active_slots, 1);
        assert_eq!(result.spawned_mcvs, 1);
        assert_eq!(sim.entities().len(), 2);
        assert_eq!(entity_position_for_owner(&sim, "Neutral"), Some((100, 75)));
        assert_eq!(
            entity_position_for_owner(&sim, "Player"),
            Some((99, 76)),
            "the valid southwest spoke stays outside the old LocalSize box instead of clamping to (77,51)"
        );
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(sim.houses(), "Player", sim.interner())
                .and_then(|house| house.base_center()),
            Some((100, 75)),
            "fallback placement must not rewrite the assigned base center"
        );
        assert_eq!(
            sim.scenario_rng().logical_state(),
            expected_rng.logical_state()
        );
    }

    #[test]
    fn skirmish_start_unit_budget_is_global_but_house_pools_filter_tech_and_mask() {
        let rules = test_starting_unit_rules();
        let slots = normalized_launch_slots(&test_session());

        assert_eq!(starting_unit_budget(&rules, &slots, 10, 2), 334);

        let allied_candidates =
            starting_unit_candidates_for_country(&rules, LaunchCountry::America, 10);
        let allied_ids: Vec<&str> = allied_candidates
            .vehicles
            .iter()
            .chain(allied_candidates.infantry.iter())
            .map(|candidate| candidate.type_id.as_str())
            .collect();
        assert_eq!(allied_ids, vec!["MTNK", "E1"]);

        let soviet_candidates =
            starting_unit_candidates_for_country(&rules, LaunchCountry::Russia, 10);
        let soviet_ids: Vec<&str> = soviet_candidates
            .vehicles
            .iter()
            .chain(soviet_candidates.infantry.iter())
            .map(|candidate| candidate.type_id.as_str())
            .collect();
        assert_eq!(soviet_ids, vec!["HTNK"]);
    }

    #[test]
    fn skirmish_start_unit_vehicle_phase_preserves_native_third_rounding() {
        assert!(starting_unit_prefers_vehicle(0, 4));
        assert!(
            starting_unit_prefers_vehicle(2, 4),
            "remaining 2 is still greater than trunc(4 / 3)"
        );
        assert!(!starting_unit_prefers_vehicle(3, 4));
    }

    #[test]
    fn skirmish_start_unit_blocked_placement_stops_after_twenty_failures() {
        let mut sim = Simulation::new();
        let session = test_session();
        sim.session_mut_for_test()
            .set_game_options_for_test(session.options.to_game_options(0));
        let all_slots = normalized_launch_slots(&session);
        let slots = &all_slots[..1];
        let rules = test_starting_unit_rules();
        populate_launch_houses(&mut sim, slots, &rules);
        let owner = sim
            .interner()
            .get(&slots[0].owner_name)
            .expect("launch owner");
        sim.houses_mut_for_test()
            .get_mut(&owner)
            .expect("launch house")
            .set_base_center_for_test(Some((32, 32)));

        let mut terrain = test_terrain(64, 64);
        for ry in 0..terrain.height() {
            for rx in 0..terrain.width() {
                terrain
                    .cell_mut_for_test(rx, ry)
                    .expect("terrain cell")
                    .overlay_blocks = true;
            }
        }
        let bounds = NativeStartBounds::from_session(&sim, &terrain);
        let mut expected_rng = sim.scenario_rng().clone();
        assert_eq!(STARTING_EXTRA_UNIT_MAX_PLACEMENT_FAILURES, 20);
        for _ in 0..20 {
            // The only eligible vehicle has a one-entry pool. RandomRanged(0,0)
            // is still called but does not advance the native RNG.
            let _ = expected_rng.next_range_u32_inclusive(0, 0);
            // TechnoClass::TechnoClass @ 0x006F2B90 spends this word at
            // 0x006F3254 before each candidate's placement can fail.
            let _ = expected_rng.next_u32();
            for _radius in
                STARTING_EXTRA_UNIT_FALLBACK_START_RADIUS..=STARTING_MCV_FALLBACK_MAX_RADIUS
            {
                let _ = expected_rng.next_range_u32_inclusive(0, 7);
                for _direction in 0..8 {
                    let _ = expected_rng.next_range_u32_inclusive(0, 1);
                    let _ = expected_rng.next_range_u32_inclusive(0, 99);
                    let _ = expected_rng.next_range_u32_inclusive(0, 1);
                    let _ = expected_rng.next_range_u32_inclusive(0, 99);
                }
            }
        }

        let spawned =
            seed_starting_extra_units(&mut sim, slots, &rules, &terrain, bounds, 1, false);

        assert_eq!(spawned, 0);
        assert!(sim.entities().is_empty());
        assert_eq!(
            sim.rng_state().scenario,
            expected_rng.logical_state(),
            "candidate, constructor, and fallback draws stop after the twentieth failed placement"
        );
    }

    #[test]
    fn skirmish_positive_unit_count_spawns_extra_starting_units() {
        let mut sim = Simulation::new();
        let mut session = test_session();
        session.options.unit_count = 2;
        let terrain = test_terrain(64, 64);
        let starts = test_launch_starts();
        let map = test_map_with_starts(&starts);
        let rules = test_starting_unit_rules();

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 2);
        assert_eq!(sim.entities().len(), 9);
        let player_units = sim
            .entities()
            .values()
            .filter(|entity| sim.interner().resolve(entity.owner()) == "Player")
            .count();
        let ai_units = sim
            .entities()
            .values()
            .filter(|entity| sim.interner().resolve(entity.owner()) == "Computer1")
            .count();
        assert_eq!(player_units, 5);
        assert_eq!(ai_units, 4);

        // gamemd-derived leftover budget (`Generate_Starting_Units @
        // 0x005D6F91..0x005D6F9C`): budget = ((3/2 + 500) / 3) * 2 = 334.
        // Player spends 3xMTNK + E1 = 600 (remainder < 0, nothing credited);
        // Computer1 has no infantry, stops at 3xHTNK = 300 and is credited 34.
        let credits = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(sim.houses(), owner, sim.interner())
                .map(|house| house.economy().credits())
        };
        assert_eq!(credits("Player"), Some(10_000));
        assert_eq!(credits("Computer1"), Some(10_034));
    }

    /// A budget spent exactly credits nothing (`TEST EAX,EAX; JLE` at
    /// `0x005D6F95`): budget = ((3/2 + 300) / 3) * 3 = 300, and each house
    /// places 2 vehicles + 1 infantry = 300.
    #[test]
    fn gsi_09_01_exactly_spent_starting_budget_credits_nothing() {
        let mut sim = Simulation::new();
        let mut session = test_session();
        session.options.unit_count = 3;
        let terrain = test_terrain(64, 64);
        let starts = test_launch_starts();
        let map = test_map_with_starts(&starts);
        let ini = launch_fixture_ini(
            "[General]\nBaseUnit=AMCV,SMCV,PCV\n\
             [VehicleTypes]\n1=AMCV\n2=SMCV\n3=MTNK\n4=HTNK\n\
             [InfantryTypes]\n1=E1\n\
             [AMCV]\nOwner=Americans\nCost=1000\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n\
             [SMCV]\nOwner=Russians\nCost=1000\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n\
             [MTNK]\nOwner=Americans\nCost=100\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n\
             [HTNK]\nOwner=Russians\nCost=100\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n\
             [E1]\nOwner=Americans,Russians\nCost=100\nTechLevel=1\nAllowedToStartInMultiplayer=yes\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("exact-budget rules parse");

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 2);
        assert_eq!(sim.entities().len(), 8);
        let credits = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(sim.houses(), owner, sim.interner())
                .map(|house| house.economy().credits())
        };
        assert_eq!(credits("Player"), Some(10_000));
        assert_eq!(credits("Computer1"), Some(10_000));
    }

    /// `Post_Map_Init` calls `Generate_Starting_Units` at `0x00686937` before
    /// the AI grant loop at `0x006869BE`, so the Hard multiplier applies to the
    /// balance that already includes the credited leftover budget.
    #[test]
    fn gsi_09_01_ai_grant_multiplies_post_leftover_balance() {
        let mut sim = Simulation::new();
        let mut session = test_session();
        session.options.unit_count = 2;
        session.opponents[0].difficulty = AiDifficulty::Hard;
        let terrain = test_terrain(64, 64);
        let starts = test_launch_starts();
        let map = test_map_with_starts(&starts);
        let mut rules = test_starting_unit_rules();
        rules.general.multiplayer_ai_cm = vec![400, 0, 0];

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &terrain,
            &launch_descriptor(&session),
        );
        assert_eq!(result.spawned_mcvs, 2);

        apply_skirmish_ai_opening_credits(&mut sim, &rules);

        let credits = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(sim.houses(), owner, sim.interner())
                .map(|house| house.economy().credits())
        };
        assert_eq!(credits("Player"), Some(10_000));
        assert_eq!(credits("Computer1"), Some((10_000 + 34) * 5));
    }

    /// gamemd-derived: active YR
    /// `MultiplayerGameMode__Create_Starting_Base_Unit @ 0x005D7030` and
    /// `Try_Unlimbo_Object_At_Or_Near_Cell @ 0x00688ED0` use the independent
    /// full-array clamp (`0x00565C10`) and playfield diamond (`0x00578460`).
    /// NearOreF.MAP geometry is `Size=0,0,80,58`, `LocalSize=2,4,76,48`.
    /// GameMD unlimbos all eight starting MCVs exactly on their authored
    /// waypoints. The former Cartesian LocalSize test accepted only indices 1
    /// and 2, displaced the other six, and consumed fallback RNG.
    /// Research:
    /// `docs/research/skirmish-ui/SKIRMISH_MCV_NEARBY_PLACEMENT_FALLBACK_00688ED0_GHIDRA_REPORT.md`.
    #[test]
    fn all_eight_nearoref_mcvs_spawn_on_authored_waypoints_without_fallback_rng() {
        let mut sim = Simulation::with_seed(0);
        let starts = nearoref_starts();
        let mut map = test_map_with_starts(&starts);
        configure_nearoref_geometry(&mut sim, &mut map);
        let terrain = test_terrain(138, 138);
        let mut session = test_session();
        session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
        session.options.bases = true;
        session.options.unit_count = 0;
        let ai_template = session.opponents[0].clone();
        session.opponents = (1..8)
            .map(|index| SkirmishAiSlot {
                raw_slot_index: index as u8,
                color_index: (index + 1) as u8,
                start_position: LaunchStartPosition::Position(index as u8),
                ..ai_template.clone()
            })
            .collect();
        let mut expected_rng = sim.scenario_rng().clone();
        // MultiplayerGameMode__Create_Starting_Base_Unit @ 0x005D7030
        // constructs all eight MCVs, and TechnoClass::TechnoClass @ 0x006F2B90
        // consumes each raw Scenario word at 0x006F3254 before exact unlimbo.
        for _ in 0..8 {
            let _ = expected_rng.next_u32();
        }
        let _ = expected_rng.next_range_u32_inclusive(0, 0xffff);

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &test_standard_launch_rules(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.active_slots, 8);
        assert_eq!(result.spawned_mcvs, 8);
        assert_eq!(sim.entities().len(), 8);
        let cells: BTreeMap<String, (u16, u16)> = sim
            .entities()
            .values()
            .map(|entity| {
                (
                    sim.interner().resolve(entity.owner()).to_string(),
                    (entity.position.rx(), entity.position.ry()),
                )
            })
            .collect();
        let expected_cells: BTreeMap<String, (u16, u16)> = starts
            .iter()
            .enumerate()
            .map(|(index, start)| {
                let owner = if index == 0 {
                    "Player".to_string()
                } else {
                    format!("Computer{index}")
                };
                (owner, (start.rx, start.ry))
            })
            .collect();
        assert_eq!(
            cells, expected_cells,
            "starting MCVs must unlimbo exactly on their authored waypoints"
        );
        for (owner, expected_cell) in &expected_cells {
            assert_eq!(
                crate::sim::house_state::house_state_for_owner(sim.houses(), owner, sim.interner(),)
                    .and_then(|house| house.base_center()),
                Some(*expected_cell),
                "{owner} must retain its authored base center"
            );
        }
        assert_eq!(
            sim.scenario_rng().logical_state(),
            expected_rng.logical_state(),
            "exact placement consumes constructor words plus the final synchronization draw, with no fallback draws"
        );
    }

    #[test]
    fn missing_owner_compatible_baseunit_skips_constructor_rng_like_native() {
        let corpus: serde_json::Value =
            crate::test_fixture::json("tools/storage_oracle/lan_lobby.json").clone();
        for row in corpus["starting_mcv_no_match"].as_array().unwrap() {
            let mut ini = String::from("[General]\nBaseUnit=");
            let masks = row["owner_masks"].as_array().unwrap();
            ini.push_str(
                &(0..masks.len())
                    .map(|i| format!("MCV{i}"))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            ini.push_str("\n[VehicleTypes]\n");
            for i in 0..masks.len() {
                ini.push_str(&format!("{i}=MCV{i}\n"));
            }
            for (i, mask) in masks.iter().enumerate() {
                let owner = match mask.as_u64().unwrap() {
                    0 => "",
                    2 => "Alliance",
                    4 => "French",
                    _ => panic!("native control mask"),
                };
                ini.push_str(&format!("[MCV{i}]\nOwner={owner}\nStrength=100\n"));
            }
            let rules = RuleSet::from_ini(&launch_fixture_ini(&ini)).unwrap();
            assert_eq!(
                launch_mcv_type_for_country(LaunchCountry::America, &rules),
                None
            );
            assert_eq!(row["callback_result"], 0);
            assert_eq!(row["initial_rng_hex"], row["callback_rng_hex"]);
            assert!(row["callback_draws"].as_array().unwrap().is_empty());
            let mut session = test_session();
            session.opponents.clear();
            session.ai_player_count = 0;
            session.rebuild_house_roster();
            session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
            session.options.bases = true;
            session.options.unit_count = 0;
            let mut sim = Simulation::new();
            *sim.scenario_rng_mut_for_test() =
                crate::sim::rng::SimRng::from_native_state_hex_for_test(
                    row["initial_rng_hex"].as_str().unwrap(),
                );
            let terrain = test_terrain(64, 64);
            let map = test_map_with_starts(&test_launch_starts());
            let result = apply_explicit_skirmish_launch_session(
                &mut sim,
                &map,
                &roster_with_neutral_and_playable(),
                &rules,
                &terrain,
                &launch_descriptor(&session),
            );
            assert_eq!(result.spawned_mcvs, 0);
            assert!(sim.entities().is_empty());
            assert_eq!(result.local_owner.as_deref(), Some("Player"));
            assert_eq!(
                sim.scenario_rng().native_state_hex(),
                row["force_tail_rng_hex"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn skirmish_baseunit_vector_selects_side_matching_mcv() {
        let rules = test_rules_with_base_units(
            "SMCV,AMCV,PCV",
            &[
                ("SMCV", "Russians,Confederation,Africans,Arabs", None, None),
                (
                    "AMCV",
                    "British,French,Germans,Americans,Alliance",
                    None,
                    None,
                ),
                ("PCV", "YuriCountry", None, None),
            ],
        );

        assert_eq!(
            launch_mcv_type_for_country(LaunchCountry::America, &rules),
            Some("AMCV")
        );
        assert_eq!(
            launch_mcv_type_for_country(LaunchCountry::Russia, &rules),
            Some("SMCV")
        );
        assert_eq!(
            launch_mcv_type_for_country(LaunchCountry::Yuri, &rules),
            Some("PCV")
        );
    }

    #[test]
    fn skirmish_baseunit_selection_uses_rules_order() {
        let rules = test_rules_with_base_units(
            "ALTMCV,AMCV",
            &[
                ("AMCV", "Americans", None, None),
                ("ALTMCV", "Americans", None, None),
            ],
        );

        assert_eq!(
            launch_mcv_type_for_country(LaunchCountry::America, &rules),
            Some("ALTMCV")
        );
    }

    #[test]
    fn skirmish_baseunit_selection_ignores_production_required_and_forbidden_houses() {
        let rules = test_rules_with_base_units(
            "BLOCKED,REQUIRED,FORBIDDEN,AMCV",
            &[
                ("BLOCKED", "Americans", Some("French"), None),
                ("REQUIRED", "Americans", Some("Americans"), None),
                ("FORBIDDEN", "Americans", None, Some("Americans")),
                ("AMCV", "Americans", None, None),
            ],
        );

        assert_eq!(
            launch_mcv_type_for_country(LaunchCountry::America, &rules),
            Some("BLOCKED")
        );
    }
}
