//! Skirmish opening seeding, deployable building detection, and overlay atlas construction.
//!
//! Split from `loading::init_helpers` for file-size limits.

use std::collections::{BTreeMap, HashMap};

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::map::houses::{HouseColorMap, HouseRoster};
use crate::map::map_file::MapFile;
use crate::map::overlay::OverlayEntry;
use crate::map::overlay_types::{
    OverlayTypeRegistry, is_bridge_overlay_index, is_high_bridge_index,
};
use crate::map::waypoints;
use crate::render::batch::BatchRenderer;
use crate::render::bridge_atlas::{self, BridgeAtlas};
use crate::render::bridge_railing_atlas::{self, BridgeRailingAtlas, BridgeRailingTileBases};
use crate::render::gpu::GpuContext;
use crate::render::overlay_assets::resolve_overlay_name_for_render;
use crate::render::overlay_atlas::{self, OverlayAtlas};
use crate::rules::art_data::ArtRegistry;
use crate::rules::color_scheme::scheme_entry_for_priority;
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::tiberium_type::TiberiumTypeRegistry;
use crate::sim::house_state::determine_waypoint_edge;
use crate::sim::scenario_bootstrap::normalized_launch_slots;
use crate::sim::world::Simulation;
use crate::skirmish_launch::SkirmishLaunchSession;
use crate::ui::main_menu::{SkirmishSettings, StartPosition};

pub(crate) fn seed_skirmish_opening_if_needed(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    overlay_registry: &OverlayTypeRegistry,
    settings: &SkirmishSettings,
) -> Option<String> {
    // Seed MCVs whenever multiplayer start waypoints exist, even if the map
    // has pre-placed entities (e.g., oil derricks on Dustbowl). The waypoint
    // check is sufficient to distinguish multiplayer maps from campaign missions.
    let mut starts = waypoints::multiplayer_start_waypoints(&map_data.waypoints);
    if starts.len() < 2 {
        return None;
    }
    let houses = skirmish_house_candidates(house_roster);
    if houses.is_empty() {
        return None;
    }

    // If the player chose a specific start position, swap that waypoint to index 0
    // so the local player spawns there.
    if let StartPosition::Position(pos) = settings.start_position {
        let idx: usize = pos as usize;
        if idx < starts.len() && idx != 0 {
            starts.swap(0, idx);
        }
    }

    // Reorder houses so the player's chosen side is first (becomes local owner).
    let selected_side = settings.player_country.side();
    let houses = reorder_houses_for_side(houses, selected_side);

    let credits: i32 = settings.starting_credits;
    let pairings = starts.into_iter().zip(houses.into_iter());
    let mut spawned_mcvs: u32 = 0;
    let mut local_owner: Option<String> = None;
    for (start, house) in pairings.take(2) {
        if let Some(h) = crate::sim::house_state::house_state_for_owner_mut(
            &mut sim.houses,
            &house.name,
            &sim.interner,
        ) {
            h.economy.credits = credits;
        }
        let mcv_type: &str = skirmish_mcv_type_for_house(house, rules);
        if sim
            .spawn_object_with_overlay_registry(
                mcv_type,
                &house.name,
                start.rx,
                start.ry,
                64,
                rules,
                height_map,
                overlay_registry,
            )
            .is_some()
        {
            spawned_mcvs += 1;
            if local_owner.is_none() {
                local_owner = Some(house.name.clone());
            }
            let waypoint_edge = sim
                .playfield_bounds
                .map(|bounds| determine_waypoint_edge((start.rx, start.ry), bounds));
            if let Some(h) = crate::sim::house_state::house_state_for_owner_mut(
                &mut sim.houses,
                &house.name,
                &sim.interner,
            ) {
                h.base_center = Some((start.rx, start.ry));
                if let Some(waypoint_edge) = waypoint_edge {
                    h.waypoint_edge = waypoint_edge;
                }
            }
        } else {
            log::warn!(
                "Failed to seed opening MCV '{}' for {} at waypoint {} ({},{})",
                mcv_type,
                house.name,
                start.index,
                start.rx,
                start.ry
            );
        }
    }
    if spawned_mcvs > 0 {
        log::info!(
            "Seeded {} skirmish opening MCV(s) with {} credits each",
            spawned_mcvs,
            credits
        );
    }
    local_owner
}

pub(crate) fn house_color_map_for_launch_session(
    session: &SkirmishLaunchSession,
    house_roster: &HouseRoster,
) -> HouseColorMap {
    let mut colors = HouseColorMap::new();
    for house in &house_roster.houses {
        if !is_playable_faction_name(&house.name) {
            colors.insert(house.name.clone(), house.color);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::map::houses::HouseDefinition;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    use crate::map::waypoints::Waypoint;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::cell_rect::{PlayfieldBounds, cell_is_in_playfield_height_aware};
    use crate::sim::house_state::{HouseDifficulty, HouseState};
    use crate::sim::mission::MissionType;
    use crate::sim::rng::SimRng;
    use crate::sim::scenario_bootstrap::*;
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
            player_name: "Player".to_string(),
            local: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: false,
                color_index: 1,
                color_random: false,
                start_position: LaunchStartPosition::Position(3),
                team: LaunchTeam::None,
            },
            opponents: vec![SkirmishAiSlot {
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
        sim.fog.width = 8;
        sim.fog.height = 6;
        sim.session.game_options.shroud = false;
        let local = sim.interner.intern("Player");
        let ai = sim.interner.intern("Computer");
        sim.houses
            .insert(local, HouseState::new(local, 0, None, true, 10_000, 10));
        sim.houses
            .insert(ai, HouseState::new(ai, 1, None, false, 10_000, 10));
        sim.fog.by_owner.insert(
            ai,
            crate::sim::vision::OwnerVisibility::new(sim.fog.width, sim.fog.height),
        );
        let scenario_rng_before = sim.scenario_rng.state();
        let main_rng_before = sim.main_rng.state();

        apply_launch_shroud_option(&mut sim, Some("Player"));

        for (rx, ry) in [(0, 0), (7, 0), (0, 5), (7, 5), (3, 2)] {
            assert!(sim.fog.is_cell_revealed(local, rx, ry));
            assert!(!sim.fog.is_cell_visible(local, rx, ry));
            assert!(!sim.fog.is_cell_revealed(ai, rx, ry));
        }
        assert!(!sim.houses[&local].map_is_clear);
        assert!(!sim.houses[&ai].map_is_clear);
        assert_eq!(sim.scenario_rng.state(), scenario_rng_before);
        assert_eq!(sim.main_rng.state(), main_rng_before);
    }

    #[test]
    fn gsi_04_18_shroud_yes_and_nonhuman_local_candidates_stay_unexplored() {
        let mut shrouded = Simulation::with_seed(0x418);
        shrouded.fog.width = 8;
        shrouded.fog.height = 6;
        let human = shrouded.interner.intern("Player");
        shrouded
            .houses
            .insert(human, HouseState::new(human, 0, None, true, 10_000, 10));
        apply_launch_shroud_option(&mut shrouded, Some("Player"));
        assert!(!shrouded.fog.is_cell_revealed(human, 7, 5));

        shrouded.session.game_options.shroud = false;
        let ai = shrouded.interner.intern("Computer");
        shrouded
            .houses
            .insert(ai, HouseState::new(ai, 1, None, false, 10_000, 10));
        apply_launch_shroud_option(&mut shrouded, Some("Computer"));
        assert!(!shrouded.fog.is_cell_revealed(ai, 7, 5));
    }

    #[test]
    fn gsi_04_18_explicit_launch_applies_shroud_no_with_bases_off() {
        let mut sim = Simulation::with_seed(0x418);
        sim.fog.width = 64;
        sim.fog.height = 64;
        let mut session = test_session();
        session.options.shroud = false;
        session.options.bases = false;
        session.options.unit_count = 0;
        let mut terrain = test_terrain(64, 64);
        let allocated = (0..64u16)
            .flat_map(|ry| (0..64u16).map(move |rx| (rx, ry)))
            .filter(|cell| *cell != (5, 5))
            .collect::<Vec<_>>();
        terrain.test_set_native_allocated_cells(&allocated);
        sim.resolved_terrain = Some(terrain.clone());
        let starts = test_launch_starts();
        let map = test_map_with_starts(&starts);

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &test_standard_launch_rules(),
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 0);
        let local = sim.interner.get("Player").expect("local house");
        let ai = sim.interner.get("Computer1").expect("AI house");
        for (rx, ry) in [(0, 0), (63, 0), (0, 63), (63, 63), (32, 32)] {
            assert!(sim.fog.is_cell_revealed(local, rx, ry));
            assert!(!sim.fog.is_cell_visible(local, rx, ry));
            assert!(!sim.fog.is_cell_revealed(ai, rx, ry));
        }
        assert!(
            !sim.fog.is_cell_revealed(local, 5, 5),
            "the rectangular buffer hole outside the native Size diamond stays shrouded"
        );
        assert!(!sim.houses[&local].map_is_clear);
    }

    fn test_battle_mode() -> SkirmishLaunchMode {
        SkirmishLaunchMode {
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
        let ini = IniFile::from_str(&text);
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

    fn ordered_country_side_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[Countries]\n\
             0=Americans\n1=Alliance\n2=French\n3=Germans\n4=British\n\
             5=Africans\n6=Arabs\n7=Confederation\n8=Russians\n9=YuriCountry\n\
             10=GDI\n11=Nod\n12=Neutral\n13=Special\n\
             [Sides]\n\
             GDI=British,French,Germans,Americans,Alliance\n\
             Nod=Russians,Africans,Confederation,Arabs\n\
             ThirdSide=YuriCountry\nCivilian=Neutral\nMutant=Special\n",
        );
        RuleSet::from_ini(&ini).expect("ordered country/side rules parse")
    }

    fn test_starting_unit_rules() -> RuleSet {
        let ini = IniFile::from_str(
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
            briefing: crate::map::briefing::BriefingSection::default(),
            preview: crate::map::preview::PreviewSection::default(),
            cells: Vec::new(),
            iso_map_pack_lookups: Vec::new(),
            entities: Vec::new(),
            overlays: Vec::new(),
            authored_overlay_packs: crate::map::map_file::AuthoredOverlayPackSlot::empty(),
            smudges: Vec::new(),
            terrain_objects: Vec::new(),
            waypoints,
            cell_tags: std::collections::HashMap::new(),
            tags: std::collections::HashMap::new(),
            triggers: std::collections::HashMap::new(),
            events: std::collections::HashMap::new(),
            actions: std::collections::HashMap::new(),
            local_variables: std::collections::HashMap::new(),
            trigger_graph: crate::map::trigger_graph::TriggerGraph::default(),
            special_flags: crate::map::basic::SpecialFlagsSection::default(),
            explicit_tubes: Vec::new(),
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
        sim.session.map_width = 138;
        sim.session.map_height = 138;
        sim.session.local_left = 2;
        sim.session.local_top = 4;
        sim.session.local_width = 76;
        sim.session.local_height = 48;
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

    fn test_height_map() -> BTreeMap<(u16, u16), u8> {
        BTreeMap::new()
    }

    fn entity_position_for_owner(sim: &Simulation, owner: &str) -> Option<(u16, u16)> {
        sim.entities().values().find_map(|entity| {
            (sim.interner.resolve(entity.owner) == owner)
                .then_some((entity.position.rx, entity.position.ry))
        })
    }

    fn test_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let mut cells = Vec::with_capacity(width as usize * height as usize);
        for ry in 0..height {
            for rx in 0..width {
                cells.push(test_terrain_cell(rx, ry, Some(100)));
            }
        }
        ResolvedTerrainGrid::from_cells(width, height, cells)
    }

    fn test_terrain_cell(rx: u16, ry: u16, track_cost: Option<u8>) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs: SpeedCostProfile {
                track: track_cost,
                ..Default::default()
            },
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            height_in_pixels: 0,
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
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: SpeedCostProfile {
                track: track_cost,
                ..Default::default()
            },
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
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
                    iq: None,
                    allies: Vec::new(),
                    base_plan: Default::default(),
                },
                HouseDefinition {
                    name: "Americans".to_string(),
                    color: HouseColorIndex(4),
                    country: Some("Americans".to_string()),
                    side: Some("Allies".to_string()),
                    player_control: Some(true),
                    iq: None,
                    allies: Vec::new(),
                    base_plan: Default::default(),
                },
            ],
        }
    }

    #[test]
    fn launch_color_map_keeps_non_players_and_uses_session_slots() {
        let colors = house_color_map_for_launch_session(
            &test_session(),
            &roster_with_neutral_and_playable(),
        );

        // Neutral keeps the roster-assigned index; players resolve their lobby
        // priority through the [Colors] entry LUT (priority 1 → entry 5 DarkRed,
        // priority 2 → entry 10 DarkBlue).
        assert_eq!(colors.get("Neutral"), Some(&HouseColorIndex(8)));
        assert_eq!(colors.get("Player"), Some(&HouseColorIndex(5)));
        assert_eq!(colors.get("Computer1"), Some(&HouseColorIndex(10)));
        assert!(!colors.contains_key("Americans"));
    }

    #[test]
    fn launch_session_uses_player_name_for_local_owner() {
        let mut session = test_session();
        session.player_name = "Commander".to_string();

        let slots = normalized_launch_slots(&session);

        assert_eq!(slots[0].owner_name, "Commander");
        assert!(slots[0].is_human);
    }

    #[test]
    fn current_house_collision_preserves_launch_label_and_distinct_colors() {
        for (handle, expected_key) in [
            ("Neutral", "Player"),
            ("SPECIAL", "Player"),
            ("computer1", "Player"),
            ("Computer2", "Computer2"),
            ("Computer01", "Computer01"),
            ("Commander", "Commander"),
        ] {
            let mut session = test_session();
            session.player_name = handle.to_string();
            let original = session.clone();
            let slots = normalized_launch_slots(&session);
            assert_eq!(slots[0].owner_name, expected_key);
            assert_eq!(slots[1].owner_name, "Computer1");
            assert_ne!(slots[0].owner_name, slots[1].owner_name);
            let colors =
                house_color_map_for_launch_session(&session, &roster_with_neutral_and_playable());
            assert_eq!(colors.get("Neutral"), Some(&HouseColorIndex(8)));
            assert_eq!(colors.get(expected_key), Some(&HouseColorIndex(5)));
            assert_eq!(colors.get("Computer1"), Some(&HouseColorIndex(10)));
            let row =
                crate::app::loading::progress_row::LoadingProgressRowSnapshot::from_launch_session(
                    &session,
                );
            assert_eq!(row.label, handle);
            assert_eq!(
                session, original,
                "normalization cannot rewrite display or launch inputs"
            );
        }
        let mut without_ai = test_session();
        without_ai.player_name = "Computer1".to_string();
        without_ai.opponents.clear();
        assert_eq!(
            normalized_launch_slots(&without_ai)[0].owner_name,
            "Computer1"
        );
        let mut two_ai = test_session();
        two_ai.player_name = "cOmPuTeR2".to_string();
        two_ai.opponents.push(two_ai.opponents[0].clone());
        assert_eq!(normalized_launch_slots(&two_ai)[0].owner_name, "Player");
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
        let blocked = terrain.cell_mut(11, 11).unwrap();
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

    /// gamemd-derived: the null-reference start row selects
    /// `g_CurrentFrameCounter % direct_pool_count` after completing the first
    /// direct ring (`0x0056E5BF..0x0056E6ED`).
    #[test]
    fn deficient_start_uses_binary_frame_modulo_not_first_candidate() {
        let mut terrain = test_terrain(40, 40);
        let blocked = terrain.cell_mut(22, 22).unwrap();
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

        let find = |frame| {
            find_nearby_start_rect(&terrain, &occupancy, Some(diamond), Some(40), frame, 15, 15)
        };
        assert_eq!(find(0), Some((14, 14)));
        assert_eq!(find(1), Some((14, 16)));
        assert_eq!(find(2), Some((15, 14)));
        assert_ne!(
            find(0),
            find(1),
            "the adapter must not return accepted.first()"
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
        let blocked = terrain.cell_mut(61, 28).unwrap();
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
            sim.session.binary_frame = frame;
            sim.scenario_rng = SimRng::new(9);
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
        session.local.start_position = LaunchStartPosition::Position(0);
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
        let mut expected_rng = sim.scenario_rng.clone();
        let _ = expected_rng.next_range_u32_inclusive(0, 0xffff);

        apply_pre_fill_scenario_prefix_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &test_standard_launch_rules(),
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
            &projection,
        );

        assert_eq!(
            crate::sim::house_state::house_state_for_owner(&sim.houses, "Player", &sim.interner)
                .and_then(|house| house.base_center),
            Some((authored.rx, authored.ry))
        );
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(
                &sim.houses,
                "Computer1",
                &sim.interner,
            )
            .and_then(|house| house.base_center),
            Some((
                projection.final_gathered_starts()[1].rx,
                projection.final_gathered_starts()[1].ry,
            ))
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
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
            .session
            .house_order
            .iter()
            .map(|id| sim.interner.resolve(*id))
            .collect();
        assert_eq!(order, ["Player", "Computer1", "Neutral", "Special"]);
        assert_eq!(sim.houses.len(), 4);
        let current_iq = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(&sim.houses, owner, &sim.interner)
                .map(|house| house.current_iq)
        };
        assert_eq!(current_iq("Player"), Some(0));
        assert_eq!(current_iq("Computer1"), Some(rules.general.max_iq_levels));
        assert_eq!(current_iq("Neutral"), Some(0));
        assert_eq!(current_iq("Special"), Some(0));
    }

    #[test]
    fn gsi_17_01_preplaced_structure_reveal_accounts_against_preexisting_house_once() {
        use crate::rules::art_data::ArtRegistry;

        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[AI]\nAIBaseSpacing=2\n\
             [Countries]\n0=Americans\n1=Russians\n2=Neutral\n3=Special\n\
             [BuildingTypes]\n0=GACNST\n\
             [GACNST]\nStrength=1000\nUndeploysInto=AMCV\n",
        ))
        .expect("structure fixture rules");
        rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(
            "[GACNST]\nFoundation=4x4\n",
        )));
        assert_eq!(
            rules.object("GACNST").unwrap().base_reservation_spacing,
            Some(2)
        );

        let session = test_session();
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
                owner: "Player".to_owned(),
                type_id: "GACNST".to_owned(),
                health: 256,
                cell_x: 30,
                cell_y: 40,
                facing: 0,
                category: EntityCategory::Structure,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
                structure_ai_sellable: false,
            }],
            Some(&rules),
            &BTreeMap::new(),
        );

        assert_eq!(spawned, 1);
        let player = sim
            .interner
            .get("Player")
            .expect("pre-created player house");
        assert_eq!(sim.houses[&player].tracking.buildings_for_test(), 1);
        assert_eq!(
            sim.substrate.base_reservations.raw_mask(None, 28, 38),
            1,
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
                color_index,
                difficulty,
                ..session.opponents[0].clone()
            });
        }
        let mut sim = Simulation::new();
        sim.session.game_options = session
            .options
            .to_game_options(session.opponents.len() as i32);
        let mut rules = test_standard_launch_rules();
        rules.general.multiplayer_ai_cm = vec![400, 0, 0];
        populate_launch_houses(&mut sim, &normalized_launch_slots(&session), &rules);
        populate_special_houses(&mut sim, &HouseRoster::default(), &rules);

        let passive_ai = sim.interner.get("Computer2").expect("passive AI");
        sim.houses
            .get_mut(&passive_ai)
            .expect("passive AI house")
            .multiplay_passive = true;
        for owner in ["Neutral", "Special"] {
            let owner = sim.interner.get(owner).expect("special house");
            sim.houses
                .get_mut(&owner)
                .expect("special HouseState")
                .multiplay_passive = true;
        }
        let observer = sim.interner.intern("Observer");
        sim.houses.insert(
            observer,
            HouseState::new(observer, 0, None, false, 10_000, 10),
        );

        apply_skirmish_ai_opening_credits(&mut sim, &rules);

        let credits = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(&sim.houses, owner, &sim.interner)
                .map(|house| house.economy.credits)
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
        assert_eq!(credits("Neutral"), Some(10_000));
        assert_eq!(credits("Special"), Some(10_000));
        assert_eq!(credits("Observer"), Some(10_000));
    }

    /// The x87 product is chopped by `Math__ftol @ 0x007C5F00`; a missing list
    /// entry (empty constructor vector at `0x00667034`) grants nothing.
    #[test]
    fn gsi_09_01_ai_opening_grant_truncates_and_tolerates_missing_entries() {
        let mut session = test_session();
        session.opponents[0].difficulty = AiDifficulty::Hard;
        let mut sim = Simulation::new();
        sim.session.game_options = session
            .options
            .to_game_options(session.opponents.len() as i32);
        let mut rules = test_standard_launch_rules();
        populate_launch_houses(&mut sim, &normalized_launch_slots(&session), &rules);
        let ai = sim.interner.get("Computer1").expect("AI house");
        sim.houses.get_mut(&ai).expect("AI house").economy.credits = 12_345;

        rules.general.multiplayer_ai_cm = Vec::new();
        apply_skirmish_ai_opening_credits(&mut sim, &rules);
        assert_eq!(sim.houses[&ai].economy.credits, 12_345);

        // 7 * 0.01 * 12345 = 864.15... -> 864 under chop.
        rules.general.multiplayer_ai_cm = vec![7, 0, 0];
        apply_skirmish_ai_opening_credits(&mut sim, &rules);
        assert_eq!(sim.houses[&ai].economy.credits, 12_345 + 864);
    }

    #[test]
    fn ordered_country_side_stock_identities_reach_skirmish_house_state() {
        let mut session = test_session();
        session.opponents.push(SkirmishAiSlot {
            country: LaunchCountry::Yuri,
            country_random: false,
            color_index: 3,
            color_random: false,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
            difficulty: Default::default(),
        });
        let rules = ordered_country_side_rules();
        let mut sim = Simulation::new();

        populate_launch_houses(&mut sim, &normalized_launch_slots(&session), &rules);
        populate_special_houses(&mut sim, &HouseRoster::default(), &rules);

        let side = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(&sim.houses, owner, &sim.interner)
                .map(|house| house.side_index)
        };
        assert_eq!(side("Player"), Some(0));
        assert_eq!(side("Computer1"), Some(1));
        assert_eq!(side("Computer2"), Some(2));
        assert_eq!(side("Neutral"), Some(3));
        assert_eq!(side("Special"), Some(4));

        let country = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(&sim.houses, owner, &sim.interner)
                .and_then(|house| house.country)
                .map(|country| sim.interner.resolve(country).to_string())
        };
        assert_eq!(country("Neutral"), Some("Neutral".to_string()));
        assert_eq!(country("Special"), Some("Special".to_string()));
    }

    #[test]
    fn skirmish_launch_same_explicit_team_creates_mutual_alliance() {
        let mut session = test_session();
        session.local.team = LaunchTeam::Team(0);
        session.opponents[0].team = LaunchTeam::Team(0);
        session.opponents.push(SkirmishAiSlot {
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
    fn skirmish_launch_team_sentinels_do_not_auto_ally() {
        let session = test_session();
        let slots = normalized_launch_slots(&session);
        let alliances =
            launch_alliance_map(&roster_with_neutral_and_playable(), &slots, &session.mode);

        assert!(
            !alliances
                .get("PLAYER")
                .is_some_and(|allies| allies.contains("COMPUTER1"))
        );
        assert!(
            !alliances
                .get("COMPUTER1")
                .is_some_and(|allies| allies.contains("PLAYER"))
        );
    }

    #[test]
    fn cooperative_mode_allies_nonspecial_houses_by_control_group() {
        let mut session = test_session();
        session.mode = test_cooperative_mode();
        session.opponents.push(SkirmishAiSlot {
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
        session.local.start_position = LaunchStartPosition::Auto;
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
        session.local.start_position = LaunchStartPosition::Auto;
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
        sim.scenario_rng = SimRng::new(8);
        let mut session = test_session();
        session.local.start_position = LaunchStartPosition::Auto;
        session.opponents[0].start_position = LaunchStartPosition::Position(0);
        session.options.bases = false;
        session.options.unit_count = 0;
        let all_starts = test_launch_starts();
        let starts = &all_starts[..3];
        let map = test_map_with_starts(starts);
        let terrain = test_terrain(64, 64);
        let rules = test_standard_launch_rules();
        let mut expected_rng = sim.scenario_rng.clone();
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
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.active_slots, 2);
        assert_eq!(result.spawned_mcvs, 0);
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(&sim.houses, "Player", &sim.interner)
                .and_then(|house| house.base_center),
            Some((30, 10)),
            "the Auto human takes the first farthest free tie, not the AI reservation"
        );
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(
                &sim.houses,
                "Computer1",
                &sim.interner,
            )
            .and_then(|house| house.base_center),
            Some((10, 10)),
            "standard Battle honors the AI's preassigned table entry"
        );
        let table_owner = |start_idx| {
            sim.session
                .start_slot_houses
                .get(&start_idx)
                .map(|owner| sim.interner.resolve(*owner))
        };
        assert_eq!(table_owner(0), Some("Computer1"));
        assert_eq!(table_owner(1), Some("Player"));
        assert_eq!(table_owner(2), None);
        assert_eq!(sim.session.start_slot_houses.len(), 2);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
    }

    #[test]
    fn gsi_04_16_first_loading_markers_match_final_battle_start_table() {
        use crate::app::loading::composition::{LoadingParticipantId, LoadingStartAssignment};

        let launch_seed = 8;
        let mut session = test_session();
        session.local.start_position = LaunchStartPosition::Auto;
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
                    color_priority: session.local.color_index,
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
        let mut expected_rng = SimRng::new(u64::from(launch_seed));
        for _ in 0..8 {
            let _ = expected_rng
                .next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX);
        }
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "one human, one AI, Neutral, and Special burn twice before loading"
        );

        let result = apply_pre_fill_scenario_prefix_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
            &projection,
        );
        assert_eq!(result.active_slots, 2);
        let table_owner = |start_idx| {
            sim.session
                .start_slot_houses
                .get(&start_idx)
                .map(|owner| sim.interner.resolve(*owner))
        };
        for assignment in &loading_assignments {
            let expected_owner = match assignment.participant {
                LoadingParticipantId::Local => "Player",
                LoadingParticipantId::Opponent(0) => "Computer1",
                LoadingParticipantId::Opponent(index) => {
                    panic!("unexpected opponent marker {index}")
                }
            };
            assert_eq!(table_owner(assignment.start_index), Some(expected_owner));
        }
        assert_eq!(table_owner(0), Some("Computer1"));
        assert_eq!(table_owner(3), Some("Player"));
        assert_eq!(sim.session.start_slot_houses.len(), 2);

        // Auto local with an explicit AI reservation uses the farthest selector
        // without an assignment draw. Applying the stored plan must not replay
        // constructor or assignment RNG; only the starting-force tail remains.
        let _ = expected_rng.next_range_u32_inclusive(0, 0xffff);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
    }

    #[test]
    fn gsi_04_16_standard_all_auto_randomizes_first_then_chooses_farthest() {
        let mut session = test_session();
        session.local.start_position = LaunchStartPosition::Auto;
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
        session.local.start_position = LaunchStartPosition::Position(0);
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
        session.local.start_position = LaunchStartPosition::Auto;
        session.opponents[0].start_position = LaunchStartPosition::Auto;
        session.opponents.push(SkirmishAiSlot {
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
    fn skirmish_launch_start_position_and_team_are_independent() {
        let mut session = test_session();
        session.local.start_position = LaunchStartPosition::Position(3);
        session.local.team = LaunchTeam::Team(0);
        session.opponents[0].start_position = LaunchStartPosition::Auto;
        session.opponents[0].team = LaunchTeam::Team(0);
        let slots = normalized_launch_slots(&session);
        let starts = test_launch_starts();
        let mut rng = SimRng::new(17);
        let before = rng.logical_state();
        let assignments = native_assign_launch_starts(&session, &starts, &mut rng);
        let alliances =
            launch_alliance_map(&roster_with_neutral_and_playable(), &slots, &session.mode);

        assert_eq!(assignments.placements[0], (0, starts[3]));
        assert_eq!(assignments.placements[1], (1, starts[0]));
        assert_eq!(rng.logical_state(), before);
        assert!(
            alliances
                .get("PLAYER")
                .is_some_and(|allies| allies.contains("COMPUTER1"))
        );
    }

    #[test]
    fn skirmish_bases_off_skips_standard_mcv_callback() {
        let mut sim = Simulation::new();
        let mut session = test_session();
        session.options.bases = false;
        session.options.unit_count = 0;
        let terrain = test_terrain(64, 64);
        let starts = test_launch_starts();
        let map = test_map_with_starts(&starts);
        let rules = test_standard_launch_rules();

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 0);
        assert_eq!(result.active_slots, 2);
        assert_eq!(sim.entities().len(), 0);
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(&sim.houses, "Player", &sim.interner)
                .and_then(|house| house.base_center),
            Some((30, 30))
        );
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(&sim.houses, "Computer1", &sim.interner)
                .and_then(|house| house.base_center),
            Some((10, 10))
        );
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
            session.player_name = "Commander".to_string();
            session.local.country = country;
            session.options.bases = false;
            session.options.unit_count = 0;

            let result = apply_explicit_skirmish_launch_session(
                &mut sim,
                &map,
                &roster,
                &rules,
                &test_height_map(),
                &terrain,
                &launch_descriptor(&session),
            );
            let owner = result
                .local_owner
                .as_deref()
                .expect("explicit launch must return its pinned local owner");
            assert_eq!(owner, "Commander");

            let actual = crate::app::presentation::sidebar_render::sidebar_theme_for_owner_sources(
                Some(&sim),
                &roster,
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
                &roster,
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
                &roster,
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

    #[test]
    fn sidebar_theme_sources_preserve_roster_path_on_owner_name_collision() {
        use crate::render::sidebar_chrome::SidebarTheme;

        let roster = HouseRoster {
            houses: vec![HouseDefinition {
                name: "MapPlayer".to_string(),
                color: HouseColorIndex(4),
                country: Some("Russians".to_string()),
                side: Some("Soviet".to_string()),
                player_control: Some(true),
                iq: None,
                allies: Vec::new(),
                base_plan: Default::default(),
            }],
        };

        assert_eq!(
            crate::app::presentation::sidebar_render::sidebar_theme_for_owner_sources(
                None,
                &roster,
                "MapPlayer",
            ),
            Some(SidebarTheme::Soviet),
            "an absent live owner must preserve the existing roster resolver"
        );

        let mut sim = Simulation::new();
        let owner_id = sim.interner.intern("MapPlayer");
        sim.houses
            .insert(owner_id, HouseState::new(owner_id, 2, None, true, 0, 10));
        assert_eq!(
            crate::app::presentation::sidebar_render::sidebar_theme_for_owner_sources(
                Some(&sim),
                &roster,
                "MapPlayer",
            ),
            Some(SidebarTheme::Soviet),
            "an explicit-name collision must preserve the roster-resolved map path"
        );
    }

    #[test]
    fn sidebar_theme_sources_reject_unknown_live_side() {
        let mut sim = Simulation::new();
        let owner_id = sim.interner.intern("DynamicOwner");
        sim.houses
            .insert(owner_id, HouseState::new(owner_id, 7, None, true, 0, 10));

        assert_eq!(
            crate::app::presentation::sidebar_render::sidebar_theme_for_owner_sources(
                Some(&sim),
                &HouseRoster::default(),
                "DynamicOwner",
            ),
            None,
            "unknown live side must defer to the caller's explicit fallback"
        );
    }

    /// F09: the descriptor supersedes the old "explicit application bypasses
    /// the legacy resolver even with a poison random flag" guarantee — an
    /// unresolved random flag can no longer cross into sim at all, so the
    /// legacy resolver has nothing to bypass.
    #[test]
    fn unresolved_shell_random_flag_cannot_cross_into_sim() {
        let mut poisoned = test_session();
        poisoned.local.country_random = true;
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
        let mut expected_scenario = sim.scenario_rng.clone();
        let _ = expected_scenario.next_range_u32_inclusive(0, 0xffff);

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 0);
        assert_eq!(result.active_slots, 2);
        assert_eq!(sim.rng_state().scenario, expected_scenario.logical_state());
    }

    #[test]
    fn launch_population_copies_each_ai_row_difficulty_to_its_house() {
        let mut session = test_session();
        let template = session.opponents[0].clone();
        session.opponents = [AiDifficulty::Hard, AiDifficulty::Normal, AiDifficulty::Easy]
            .into_iter()
            .enumerate()
            .map(|(index, difficulty)| SkirmishAiSlot {
                color_index: (index + 2) as u8,
                difficulty,
                ..template.clone()
            })
            .collect();

        let mut sim = Simulation::new();
        let rules = test_standard_launch_rules();
        populate_launch_houses(&mut sim, &normalized_launch_slots(&session), &rules);

        let difficulty = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(&sim.houses, owner, &sim.interner)
                .map(|house| house.difficulty)
        };
        assert_eq!(difficulty("Player"), Some(HouseDifficulty::Normal));
        assert_eq!(difficulty("Computer1"), Some(HouseDifficulty::Hard));
        assert_eq!(difficulty("Computer2"), Some(HouseDifficulty::Normal));
        assert_eq!(difficulty("Computer3"), Some(HouseDifficulty::Easy));
        let current_iq = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(&sim.houses, owner, &sim.interner)
                .map(|house| house.current_iq)
        };
        assert_eq!(current_iq("Player"), Some(0));
        assert_eq!(current_iq("Computer1"), Some(rules.general.max_iq_levels));
        assert_eq!(current_iq("Computer2"), Some(rules.general.max_iq_levels));
        assert_eq!(current_iq("Computer3"), Some(rules.general.max_iq_levels));
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
            &test_height_map(),
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
            entity.veterancy == 200 && entity.veterancy_raw.bits() == 0x4000_0000
        }));
        assert!(sim.entities().values().all(|entity| {
            let expected = if sim.interner.resolve(entity.owner) == "Player" {
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
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 2);
        assert_eq!(
            sim.playfield_bounds,
            Some(PlayfieldBounds::from_map_header(&map.header)),
            "direct pre-funnel launch must install normalized MapClass fields"
        );
        assert_eq!(sim.session.game_options.unit_count, 0);
        assert_eq!(sim.entities().len(), 2);
    }

    #[test]
    fn skirmish_assigned_start_sets_house_base_cell_before_mcv_spawn() {
        let mut sim = Simulation::new();
        let session = test_session();
        let terrain = test_terrain(64, 64);
        let starts = test_in_playfield_launch_starts();
        let map = test_map_with_starts(&starts);
        let rules = test_standard_launch_rules();

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 2);
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(&sim.houses, "Player", &sim.interner)
                .and_then(|house| house.base_center),
            Some((45, 45))
        );
        assert_eq!(entity_position_for_owner(&sim, "Player"), Some((45, 45)));
    }

    #[test]
    fn skirmish_mcv_start_uses_radius_fallback_when_start_cell_blocked() {
        let mut sim = Simulation::new();
        let session = test_session();
        let terrain = test_terrain(64, 64);
        let starts = test_in_playfield_launch_starts();
        let map = test_map_with_starts(&starts);
        let rules = test_standard_launch_rules();
        initialize_skirmish_launch_houses(
            &mut sim,
            &roster_with_neutral_and_playable(),
            &rules,
            &launch_descriptor(&session),
        );
        sim.spawn_object(
            "AMCV",
            "Neutral",
            45,
            45,
            STARTING_MCV_FACING,
            &rules,
            &test_height_map(),
        )
        .expect("blocker");
        let mut expected_rng = sim.scenario_rng.clone();
        // gamemd-derived: TechnoClass::TechnoClass @ 0x006F2B90 consumes and
        // stores one raw Scenario word at 0x006F3254 before placement.
        let _ = expected_rng.next_u32();
        let direction = expected_rng.next_range_u32_inclusive(0, 7) as usize;
        let (dx, dy) = STARTING_MCV_FALLBACK_DIRECTIONS[direction];
        let expected_position = ((45i32 + dx) as u16, (45i32 + dy) as u16);
        let _ = expected_rng.next_u32();
        let _ = expected_rng.next_range_u32_inclusive(0, 0xffff);

        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &roster_with_neutral_and_playable(),
            &rules,
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 2);
        assert_eq!(
            crate::sim::house_state::house_state_for_owner(&sim.houses, "Player", &sim.interner)
                .and_then(|house| house.base_center),
            Some((45, 45))
        );
        assert_eq!(
            entity_position_for_owner(&sim, "Player"),
            Some(expected_position)
        );
        assert_eq!(
            sim.rng_state().scenario,
            expected_rng.logical_state(),
            "two constructor draws plus the fallback direction and final sync draws"
        );
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
        session.local.start_position = LaunchStartPosition::Position(0);
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
        sim.spawn_object(
            "AMCV",
            "Neutral",
            100,
            75,
            STARTING_MCV_FACING,
            &rules,
            &test_height_map(),
        )
        .expect("authored start blocker");
        let mut expected_rng = sim.scenario_rng.clone();
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
            &test_height_map(),
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
            crate::sim::house_state::house_state_for_owner(&sim.houses, "Player", &sim.interner)
                .and_then(|house| house.base_center),
            Some((100, 75)),
            "fallback placement must not rewrite the assigned base center"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
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
    fn skirmish_start_unit_budget_excludes_baseunit_entries() {
        let rules = test_starting_unit_rules();
        let slots = normalized_launch_slots(&test_session());

        let allied_candidates =
            starting_unit_candidates_for_country(&rules, LaunchCountry::America, 10);
        assert_eq!(starting_unit_budget(&rules, &slots, 10, 1), 167);

        assert!(
            !allied_candidates
                .vehicles
                .iter()
                .any(|candidate| candidate.type_id == "AMCV")
        );
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
        sim.session.game_options = session.options.to_game_options(0);
        let all_slots = normalized_launch_slots(&session);
        let slots = &all_slots[..1];
        let rules = test_starting_unit_rules();
        populate_launch_houses(&mut sim, slots, &rules);
        crate::sim::house_state::house_state_for_owner_mut(
            &mut sim.houses,
            &slots[0].owner_name,
            &sim.interner,
        )
        .expect("launch house")
        .base_center = Some((32, 32));

        let mut terrain = test_terrain(64, 64);
        for ry in 0..terrain.height() {
            for rx in 0..terrain.width() {
                terrain
                    .cell_mut(rx, ry)
                    .expect("terrain cell")
                    .overlay_blocks = true;
            }
        }
        let bounds = NativeStartBounds::from_session(&sim, &terrain);
        let mut expected_rng = sim.scenario_rng.clone();
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

        let spawned = seed_starting_extra_units(
            &mut sim,
            slots,
            &rules,
            &test_height_map(),
            &terrain,
            bounds,
            1,
            false,
        );

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
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 2);
        assert_eq!(sim.entities().len(), 9);
        let player_units = sim
            .entities()
            .values()
            .filter(|entity| sim.interner.resolve(entity.owner) == "Player")
            .count();
        let ai_units = sim
            .entities()
            .values()
            .filter(|entity| sim.interner.resolve(entity.owner) == "Computer1")
            .count();
        assert_eq!(player_units, 5);
        assert_eq!(ai_units, 4);

        // gamemd-derived leftover budget (`Generate_Starting_Units @
        // 0x005D6F91..0x005D6F9C`): budget = ((3/2 + 500) / 3) * 2 = 334.
        // Player spends 3xMTNK + E1 = 600 (remainder < 0, nothing credited);
        // Computer1 has no infantry, stops at 3xHTNK = 300 and is credited 34.
        let credits = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(&sim.houses, owner, &sim.interner)
                .map(|house| house.economy.credits)
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
        let ini = IniFile::from_str(
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
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
        );

        assert_eq!(result.spawned_mcvs, 2);
        assert_eq!(sim.entities().len(), 8);
        let credits = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(&sim.houses, owner, &sim.interner)
                .map(|house| house.economy.credits)
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
            &test_height_map(),
            &terrain,
            &launch_descriptor(&session),
        );
        assert_eq!(result.spawned_mcvs, 2);

        apply_skirmish_ai_opening_credits(&mut sim, &rules);

        let credits = |owner: &str| {
            crate::sim::house_state::house_state_for_owner(&sim.houses, owner, &sim.interner)
                .map(|house| house.economy.credits)
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
        session.local.start_position = LaunchStartPosition::Position(0);
        session.options.bases = true;
        session.options.unit_count = 0;
        let ai_template = session.opponents[0].clone();
        session.opponents = (1..8)
            .map(|index| SkirmishAiSlot {
                color_index: (index + 1) as u8,
                start_position: LaunchStartPosition::Position(index as u8),
                ..ai_template.clone()
            })
            .collect();
        let mut expected_rng = sim.scenario_rng.clone();
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
            &test_height_map(),
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
                    sim.interner.resolve(entity.owner).to_string(),
                    (entity.position.rx, entity.position.ry),
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
                crate::sim::house_state::house_state_for_owner(&sim.houses, owner, &sim.interner,)
                    .and_then(|house| house.base_center),
                Some(*expected_cell),
                "{owner} must retain its authored base center"
            );
        }
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "exact placement consumes constructor words plus the final synchronization draw, with no fallback draws"
        );
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
            "AMCV"
        );
        assert_eq!(
            launch_mcv_type_for_country(LaunchCountry::Russia, &rules),
            "SMCV"
        );
        assert_eq!(
            launch_mcv_type_for_country(LaunchCountry::Yuri, &rules),
            "PCV"
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
            "ALTMCV"
        );
    }

    #[test]
    fn skirmish_baseunit_selection_respects_required_and_forbidden_houses() {
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
            "REQUIRED"
        );
    }

    #[test]
    fn skirmish_launch_does_not_use_country_hardcoded_mcv_for_parity_path() {
        let rules =
            test_rules_with_base_units("CUSTOMMCV", &[("CUSTOMMCV", "Americans", None, None)]);

        assert_eq!(
            launch_mcv_type_for_country(LaunchCountry::America, &rules),
            "CUSTOMMCV"
        );
    }

    #[test]
    fn gsi_04_13_overlay_names_include_every_runtime_low_bridge_variant() {
        let mut text = String::from("[OverlayTypes]\n");
        for overlay_id in 0u16..=238 {
            let name = match overlay_id {
                24 => "BRIDGE1".to_string(),
                25 => "BRIDGE2".to_string(),
                74..=101 => format!("LOBRDG{:02}", overlay_id - 73),
                122..=125 => format!("LOBRDGE{}", overlay_id - 121),
                205..=232 => format!("LOBRDB{:02}", overlay_id - 204),
                233..=236 => format!("LOBRDGB{}", overlay_id - 232),
                237 => "BRIDGEB1".to_string(),
                238 => "BRIDGEB2".to_string(),
                _ => format!("DUMMY{overlay_id}"),
            };
            text.push_str(&format!("{overlay_id}={name}\n"));
        }
        let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(&text), None);
        let mut names = BTreeMap::new();

        let (wall_count, low_bridge_count, crate_count) = preregister_runtime_overlay_names(
            &registry,
            &crate::rules::crate_rules::CrateRules::default(),
            &mut names,
        );

        assert_eq!(wall_count, 0);
        assert_eq!(low_bridge_count, 64);
        assert_eq!(crate_count, 0);
        assert_eq!(names.get(&0x50).map(String::as_str), Some("LOBRDG07"));
        assert_eq!(names.get(&0x64).map(String::as_str), Some("LOBRDG27"));
        assert_eq!(names.get(&0xE7).map(String::as_str), Some("LOBRDB27"));
        assert!(
            !names.contains_key(&24) && !names.contains_key(&237),
            "high bridges remain owned by the dedicated bridge renderer"
        );
    }

    #[test]
    fn runtime_overlay_names_include_every_crate_identity_before_startup_scatter() {
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=CRATE\n1=WCRATE\n2=PLAIN\n\
                 [CRATE]\nCrate=yes\n[WCRATE]\nCrate=yes\n[PLAIN]\n",
            ),
            None,
        );
        let mut names = BTreeMap::new();

        let crate_rules = crate::rules::crate_rules::CrateRules {
            wood_crate_img: Some("CRATE".to_owned()),
            crate_img: Some("PLAIN".to_owned()),
            water_crate_img: Some("WCRATE".to_owned()),
            ..crate::rules::crate_rules::CrateRules::default()
        };
        let counts = preregister_runtime_overlay_names(&registry, &crate_rules, &mut names);

        assert_eq!(counts, (0, 0, 3));
        assert_eq!(names.get(&0).map(String::as_str), Some("CRATE"));
        assert_eq!(names.get(&1).map(String::as_str), Some("WCRATE"));
        assert_eq!(
            names.get(&2).map(String::as_str),
            Some("PLAIN"),
            "late-allocated configured identity registers even with Crate=false"
        );
    }
}

pub(crate) fn skirmish_house_candidates(
    house_roster: &HouseRoster,
) -> Vec<&crate::map::houses::HouseDefinition> {
    // First pass: prefer houses without explicit PlayerControl=no.
    let preferred: Vec<&crate::map::houses::HouseDefinition> = house_roster
        .houses
        .iter()
        .filter(|house| {
            is_playable_faction_name(&house.name) && house.player_control != Some(false)
        })
        .collect();
    if preferred.len() >= 2 {
        return preferred;
    }
    // Second pass: include all playable factions (even PlayerControl=no)
    // so skirmish maps can seed at least 2 MCVs for AI opponents.
    house_roster
        .houses
        .iter()
        .filter(|house| is_playable_faction_name(&house.name))
        .collect()
}

/// Reorder house candidates so the player's chosen side appears first.
///
/// Matches houses by their Side= field (Allies/Soviet). If no exact match,
/// falls back to original order.
fn reorder_houses_for_side<'a>(
    houses: Vec<&'a crate::map::houses::HouseDefinition>,
    side: crate::ui::main_menu::SkirmishSide,
) -> Vec<&'a crate::map::houses::HouseDefinition> {
    use crate::ui::main_menu::SkirmishSide;

    let target_side: &str = match side {
        SkirmishSide::Allied => "ALLIES",
        SkirmishSide::Soviet => "SOVIET",
    };

    // Find index of a house matching the player's chosen side.
    let matching_idx = houses.iter().position(|h| {
        h.side
            .as_deref()
            .is_some_and(|s| s.to_ascii_uppercase().contains(target_side))
    });

    let Some(idx) = matching_idx else {
        return houses;
    };
    if idx == 0 {
        return houses;
    }

    // Swap the matching house to position 0 (local player slot).
    let mut reordered = houses;
    reordered.swap(0, idx);
    reordered
}

/// Returns true for faction names that represent real players (not neutral/civilian).
fn is_playable_faction_name(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    !matches!(
        up.as_str(),
        "NEUTRAL" | "SPECIAL" | "CIVILIAN" | "GOODGUY" | "BADGUY" | "JP"
    )
}

pub(crate) fn skirmish_mcv_type_for_house(
    house: &crate::map::houses::HouseDefinition,
    rules: &RuleSet,
) -> &'static str {
    let mut candidates = Vec::new();
    if let Some(country) = house.country.as_deref() {
        let upper = country.to_ascii_uppercase();
        if upper.contains("YURI") {
            candidates.push("PCV");
        } else if upper.contains("RUSS")
            || upper.contains("CONFED")
            || upper.contains("IRAQ")
            || upper.contains("CUBA")
            || upper.contains("LIBYA")
        {
            candidates.push("SMCV");
        } else {
            candidates.push("AMCV");
        }
    }
    if let Some(side) = house.side.as_deref() {
        let upper = side.to_ascii_uppercase();
        if upper.contains("YURI") {
            candidates.push("PCV");
        } else if upper.contains("SOV") {
            candidates.push("SMCV");
        } else if upper.contains("ALL") {
            candidates.push("AMCV");
        }
    }
    candidates.extend(["AMCV", "SMCV", "PCV"]);
    candidates
        .into_iter()
        .find(|id| rules.object(id).is_some())
        .unwrap_or("AMCV")
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

pub(crate) fn preregister_runtime_overlay_names(
    overlay_registry: &OverlayTypeRegistry,
    crate_rules: &crate::rules::crate_rules::CrateRules,
    overlay_names: &mut BTreeMap<u8, String>,
) -> (u32, u32, u32) {
    let selected_crate_ids = [
        crate_rules
            .wood_crate_img
            .as_deref()
            .and_then(|name| overlay_registry.id_for_name(name)),
        crate_rules
            .crate_img
            .as_deref()
            .and_then(|name| overlay_registry.id_for_name(name)),
        crate_rules
            .water_crate_img
            .as_deref()
            .and_then(|name| overlay_registry.id_for_name(name)),
    ];
    let mut wall_ids_added: u32 = 0;
    let mut low_bridge_ids_added: u32 = 0;
    let mut crate_ids_added: u32 = 0;
    for overlay_id in 0u8..=u8::MAX {
        let flags = overlay_registry.flags(overlay_id);
        let is_wall = flags.is_some_and(|flags| flags.wall);
        let is_crate = flags.is_some_and(|flags| flags.crate_type)
            || selected_crate_ids.contains(&Some(overlay_id));
        let is_low_bridge =
            is_bridge_overlay_index(overlay_id) && !is_high_bridge_index(overlay_id);
        if !is_wall && !is_low_bridge && !is_crate {
            continue;
        }
        let Some(name) = resolve_overlay_name_for_render(overlay_registry, overlay_id) else {
            continue;
        };
        if let std::collections::btree_map::Entry::Vacant(entry) = overlay_names.entry(overlay_id) {
            entry.insert(name);
            if is_wall {
                wall_ids_added += 1;
            }
            if is_low_bridge {
                low_bridge_ids_added += 1;
            }
            if is_crate {
                crate_ids_added += 1;
            }
        }
    }
    (wall_ids_added, low_bridge_ids_added, crate_ids_added)
}

/// Build overlay sprite atlas and name mapping from map data + rules.ini.
pub(crate) fn build_overlay_atlas_from_map(
    map_data: &MapFile,
    asset_manager: &AssetManager,
    gpu: &GpuContext,
    batch: &BatchRenderer,
    theater_ext: &str,
    rules_ini: &IniFile,
    crate_rules: &crate::rules::crate_rules::CrateRules,
    art_registry: &ArtRegistry,
    theater_iso_palette: Option<&Palette>,
    theater_unit_palette: Option<&Palette>,
    theater_tiberium_palette: Option<&Palette>,
    smudge_types: Option<&crate::rules::smudge_type::SmudgeTypeRegistry>,
    bridge_railing_tile_bases: Option<BridgeRailingTileBases>,
) -> (
    Option<OverlayAtlas>,
    Option<BridgeAtlas>,
    Option<BridgeRailingAtlas>,
    BTreeMap<u8, String>,
    Vec<OverlayEntry>,
    HashMap<(u8, u8), [u8; 3]>,
) {
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

    let overlay_registry: OverlayTypeRegistry = OverlayTypeRegistry::from_ini(rules_ini, None);
    let tiberium_types = TiberiumTypeRegistry::from_ini(rules_ini);

    // Compute wall connectivity bitmasks on a mutable clone so the atlas
    // and AppState see correct auto-tiled frames (0–15 per wall type).
    let mut wall_overlays: Vec<OverlayEntry> = map_data.overlays.clone();
    let walls_updated: u32 =
        crate::map::overlay::compute_wall_connectivity(&mut wall_overlays, &overlay_registry);
    if walls_updated > 0 {
        log::info!("Wall connectivity: {} wall entries updated", walls_updated);
    }

    // Log first 20 overlay types for diagnostic verification.
    let max_diag: usize = 20.min(overlay_registry.len());
    for i in 0..max_diag {
        if let Some(name) = overlay_registry.name(i as u8) {
            let mapped = resolve_overlay_name_for_render(&overlay_registry, i as u8)
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

    // Build ID → name mapping for render-time lookups.
    let mut overlay_names: BTreeMap<u8, String> = BTreeMap::new();
    let mut unmapped_count: u32 = 0;
    let mut unmapped_ids: std::collections::HashSet<u8> = std::collections::HashSet::new();
    for entry in &map_data.overlays {
        if let Some(mapped_name) =
            resolve_overlay_name_for_render(&overlay_registry, entry.overlay_id)
        {
            overlay_names.entry(entry.overlay_id).or_insert(mapped_name);
        } else {
            unmapped_count += 1;
            unmapped_ids.insert(entry.overlay_id);
        }
    }
    if !unmapped_ids.is_empty() {
        let mut ids: Vec<u8> = unmapped_ids.into_iter().collect();
        ids.sort();
        log::warn!("Unmapped overlay IDs (not in registry): {:?}", ids,);
    }
    log::info!(
        "Overlay name mapping: {} IDs mapped, {} unmapped entries",
        overlay_names.len(),
        unmapped_count,
    );
    // Register overlay identities the sim can create after map load. Walls can
    // be placed by production; low bridges replace their CellClass identity as
    // damage/collapse/repair advances while the map-pack entry stays fixed.
    let (wall_ids_added, low_bridge_ids_added, crate_ids_added) =
        preregister_runtime_overlay_names(&overlay_registry, crate_rules, &mut overlay_names);
    if wall_ids_added > 0 {
        log::info!(
            "Pre-registered {} wall overlay type(s) in overlay_names for player placement",
            wall_ids_added
        );
    }
    if low_bridge_ids_added > 0 {
        log::info!(
            "Pre-registered {} low-bridge overlay variant(s) in overlay_names",
            low_bridge_ids_added
        );
    }
    if crate_ids_added > 0 {
        log::info!(
            "Pre-registered {} crate overlay type(s) in overlay_names for startup/runtime placement",
            crate_ids_added
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
            &overlay_registry,
            &overlay_names,
            theater_ext,
            &map_data.header.theater,
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
            &wall_overlays,
            &map_data.terrain_objects,
            asset_manager,
            theater_pal,
            unit_pal,
            tib_pal,
            theater_ext,
            &map_data.header.theater,
            &overlay_registry,
            &tiberium_types,
            crate_rules,
            rules_ini,
            art_registry,
            smudge_types,
        )
    });

    let bridge_atlas: Option<BridgeAtlas> = theater_palette.as_ref().and_then(|theater_pal| {
        let unit_pal: &Palette = unit_palette.as_ref().unwrap_or(theater_pal);
        bridge_atlas::build_bridge_atlas(
            gpu,
            batch,
            &wall_overlays,
            &overlay_names,
            asset_manager,
            theater_pal,
            unit_pal,
            theater_ext,
            &map_data.header.theater,
            &overlay_registry,
            crate_rules,
            rules_ini,
            art_registry,
        )
    });

    let bridge_railing_atlas: Option<BridgeRailingAtlas> =
        theater_palette.as_ref().and_then(|theater_pal| {
            bridge_railing_atlas::build_bridge_railing_atlas(
                gpu,
                batch,
                asset_manager,
                theater_pal,
                theater_ext,
                bridge_railing_tile_bases,
            )
        });

    (
        atlas,
        bridge_atlas,
        bridge_railing_atlas,
        overlay_names,
        wall_overlays,
        overlay_radar_colors,
    )
}
