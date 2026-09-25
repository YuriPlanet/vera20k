//! Asset-enabled source checks for the hermetic INI contracts used by default CI.
//!
//! Default tests exercise behavior with narrow tracked fixtures. This ignored
//! test proves the consumed stock-shaped values against the user's own retail
//! archive without making public compilation depend on private game data.

use std::path::Path;

use vera20k::assets::asset_manager::AssetManager;
use vera20k::map::overlay_types::OverlayTypeRegistry;
use vera20k::map::rmg::tech_catalog;
use vera20k::map::rmg::tiles::TileIds;
use vera20k::map::theater::{RmgTileKeys, load_theater, parse_tileset_ini};
use vera20k::rules::art_data::ArtRegistry;
use vera20k::rules::ini_parser::IniFile;
use vera20k::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use vera20k::rules::ruleset::RuleSet;
use vera20k::rules::warhead_type::WarheadType;
use vera20k::sim::miner::MinerConfig;
use vera20k::skirmish_modes::{parse_mpmodes_ini, skirmish_modes_from_assets};
use vera20k::util::fixed_math::SimFixed;

fn asset_ini(assets: &AssetManager, name: &str) -> IniFile {
    let data = assets
        .get(name)
        .unwrap_or_else(|| panic!("{name} missing from retail archive stack"));
    IniFile::from_bytes(&data).unwrap_or_else(|error| panic!("parse {name}: {error}"))
}

fn merged_asset_ini(assets: &AssetManager, base: &str, patch: &str) -> IniFile {
    let mut ini = asset_ini(assets, base);
    ini.merge(&asset_ini(assets, patch));
    ini
}

fn parsed_rules(rules_ini: &IniFile, art_ini: &IniFile) -> RuleSet {
    let mut rules = RuleSet::from_ini(rules_ini).expect("parse merged retail rules");
    rules.merge_art_data(&ArtRegistry::from_ini(art_ini));
    rules
}

fn assert_contract_section(contract: &IniFile, retail: &IniFile, section_name: &str) {
    let contract_section = contract
        .section(section_name)
        .unwrap_or_else(|| panic!("contract section [{section_name}]"));
    let retail_section = retail
        .section(section_name)
        .unwrap_or_else(|| panic!("retail section [{section_name}]"));
    for key in contract_section.keys() {
        assert_eq!(
            retail_section.get(key).map(str::trim),
            contract_section.get(key).map(str::trim),
            "retail [{section_name}] {key}= must match the tracked contract"
        );
    }
}

fn assert_miner_object_contract(contract: &RuleSet, retail: &RuleSet, id: &str) {
    let expected = contract
        .object(id)
        .unwrap_or_else(|| panic!("contract {id}"));
    let actual = retail.object(id).unwrap_or_else(|| panic!("retail {id}"));

    assert_eq!(actual.strength, expected.strength, "{id} Strength");
    assert_eq!(actual.armor, expected.armor, "{id} Armor");
    assert_eq!(actual.harvester, expected.harvester, "{id} Harvester");
    assert_eq!(actual.speed, expected.speed, "{id} Speed");
    assert_eq!(actual.storage, expected.storage, "{id} Storage");
    assert_eq!(actual.turret_rot, expected.turret_rot, "{id} ROT");
    assert_eq!(actual.crusher, expected.crusher, "{id} Crusher");
    assert_eq!(actual.locomotor, expected.locomotor, "{id} Locomotor");
    assert_eq!(
        actual.movement_zone, expected.movement_zone,
        "{id} MovementZone"
    );
    assert_eq!(actual.speed_type, expected.speed_type, "{id} SpeedType");
    assert_eq!(actual.teleporter, expected.teleporter, "{id} Teleporter");
    assert_eq!(actual.accelerates, expected.accelerates, "{id} Accelerates");
    assert_eq!(
        actual.accel_factor, expected.accel_factor,
        "{id} acceleration"
    );
    assert_eq!(
        actual.decel_factor, expected.decel_factor,
        "{id} deceleration"
    );
    assert_eq!(
        actual.slowdown_distance, expected.slowdown_distance,
        "{id} slowdown distance"
    );
    assert_eq!(actual.dock, expected.dock, "{id} Dock");
}

fn rmg_keys_from_raw_theater(data: &[u8]) -> RmgTileKeys {
    let lookup = parse_tileset_ini(data, "tem").expect("parse retail temperate tilesets");
    let ini = IniFile::from_bytes(data).expect("parse retail temperate INI");
    let general = ini.section("General").expect("retail theater [General]");
    let start = |key: &str| -> Option<u16> {
        let ordinal = usize::try_from(general.get_i32(key)?).ok()?;
        lookup.bounds().get(ordinal).map(|bounds| bounds.start)
    };

    RmgTileKeys {
        clear_tile: start("ClearTile"),
        ramp_base: start("RampBase"),
        ramp_smooth: start("RampSmooth"),
        rough_tile: start("RoughTile"),
        sand_tile: start("SandTile"),
        green_tile: start("GreenTile"),
        clear_to_rough_lat: start("ClearToRoughLat"),
        clear_to_sand_lat: start("ClearToSandLat"),
        clear_to_green_lat: start("ClearToGreenLat"),
        clear_to_pave_lat: start("ClearToPaveLat"),
        pave_tile: start("PaveTile"),
        water_set: start("WaterSet"),
        shore_pieces: start("ShorePieces"),
        water_bridge: start("WaterBridge"),
        misc_pave_tile: start("MiscPaveTile"),
        paved_roads: start("PavedRoads"),
        paved_road_ends: start("PavedRoadEnds"),
        medians: start("Medians"),
    }
}

fn tile_id_projection(ids: TileIds) -> [i32; 18] {
    [
        ids.clear,
        ids.ramp_base,
        ids.ramp_smooth,
        ids.rough,
        ids.sand,
        ids.green,
        ids.rough_lat,
        ids.sand_lat,
        ids.green_lat,
        ids.pave_lat,
        ids.pave,
        ids.water_base,
        ids.shore,
        ids.water_bridge,
        ids.misc_pave,
        ids.paved_roads,
        ids.paved_road_ends,
        ids.medians,
    ]
}

#[test]
#[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
fn hermetic_ini_contracts_match_consumed_retail_values() {
    let root =
        std::env::var("RA2_DIR").expect("set RA2_DIR to the installed retail RA2/YR directory");
    let mut assets = AssetManager::new(Path::new(&root)).expect("load retail archive stack");
    let rules_ini = merged_asset_ini(&assets, "rules.ini", "rulesmd.ini");
    let art_ini = merged_asset_ini(&assets, "art.ini", "artmd.ini");
    let rules = parsed_rules(&rules_ini, &art_ini);
    let miner_contract_rules_ini = IniFile::from_str(include_str!(
        "fixtures/ini/miner_outbound_rules_contract.ini"
    ));
    let miner_contract_art_ini =
        IniFile::from_str(include_str!("fixtures/ini/miner_outbound_art_contract.ini"));
    let miner_contract_rules = parsed_rules(&miner_contract_rules_ini, &miner_contract_art_ini);

    let modes = skirmish_modes_from_assets(&assets).expect("load retail MPModes roster");
    let mut contract_modes = parse_mpmodes_ini(&IniFile::from_str(include_str!(
        "fixtures/ini/mpmodesmd_stock_contract.ini"
    )));
    for mode in &mut contract_modes {
        match mode.override_file.to_ascii_lowercase().as_str() {
            "mpteammd.ini" => mode.must_ally = true,
            "mpfreeforallmd.ini" | "mpcoopmd.ini" => mode.allies_allowed = false,
            _ => {}
        }
    }
    assert_eq!(
        modes, contract_modes,
        "every consumed MPModes roster and override field must match retail"
    );

    for section in [
        "General", "Riparius", "Clear", "Tiberium", "TIB01", "HARV", "CMIN", "GAREFN",
    ] {
        assert_contract_section(&miner_contract_rules_ini, &rules_ini, section);
    }
    assert_contract_section(&miner_contract_art_ini, &art_ini, "GAREFN");

    for (id, locomotor, teleporter) in [
        ("HARV", LocomotorKind::Drive, false),
        ("CMIN", LocomotorKind::Teleport, true),
    ] {
        assert_miner_object_contract(&miner_contract_rules, &rules, id);
        let object = rules.object(id).unwrap_or_else(|| panic!("retail {id}"));
        assert!(object.harvester);
        assert_eq!(object.speed, 4);
        assert_eq!(object.turret_rot, 5);
        assert!(object.crusher);
        assert_eq!(object.movement_zone, MovementZone::Crusher);
        assert_eq!(object.speed_type, SpeedType::Track);
        assert_eq!(object.locomotor, locomotor);
        assert_eq!(object.teleporter, teleporter);
        assert!(object.accelerates);
        assert_eq!(object.accel_factor, SimFixed::lit("0.03"));
        assert_eq!(object.decel_factor, SimFixed::lit("0.002"));
        assert_eq!(object.slowdown_distance, 500);
    }

    let contract_miner = MinerConfig::from_rules(&miner_contract_rules);
    let retail_miner = MinerConfig::from_rules(&rules);
    assert_eq!(retail_miner.ore_bale_value, contract_miner.ore_bale_value);
    assert_eq!(retail_miner.gem_bale_value, contract_miner.gem_bale_value);
    assert_eq!(
        retail_miner.war_miner_capacity,
        contract_miner.war_miner_capacity
    );
    assert_eq!(
        retail_miner.chrono_miner_capacity,
        contract_miner.chrono_miner_capacity
    );
    assert_eq!(
        retail_miner.harvest_tick_interval,
        contract_miner.harvest_tick_interval
    );
    assert_eq!(
        retail_miner.unload_tick_interval,
        contract_miner.unload_tick_interval
    );
    assert_eq!(
        retail_miner.local_continuation_radius,
        contract_miner.local_continuation_radius
    );
    assert_eq!(
        retail_miner.long_scan_radius,
        contract_miner.long_scan_radius
    );
    assert_eq!(
        rules.general.harvester_too_far_distance,
        miner_contract_rules.general.harvester_too_far_distance
    );
    assert_eq!(
        rules.general.chrono_harv_too_far_distance,
        miner_contract_rules.general.chrono_harv_too_far_distance
    );
    assert_eq!(
        retail_miner.rescan_cooldown_ticks,
        contract_miner.rescan_cooldown_ticks
    );

    let refinery = rules.object("GAREFN").expect("retail GAREFN");
    let contract_refinery = miner_contract_rules
        .object("GAREFN")
        .expect("contract GAREFN");
    assert_eq!(refinery.strength, contract_refinery.strength);
    assert_eq!(refinery.armor, contract_refinery.armor);
    assert_eq!(refinery.refinery, contract_refinery.refinery);
    assert_eq!(refinery.bib, contract_refinery.bib);
    assert_eq!(refinery.storage, contract_refinery.storage);
    assert_eq!(refinery.free_unit, contract_refinery.free_unit);
    assert_eq!(refinery.number_of_docks, contract_refinery.number_of_docks);
    assert_eq!(refinery.power, contract_refinery.power);
    assert_eq!(refinery.foundation, contract_refinery.foundation);
    assert_eq!(refinery.queueing_cell, contract_refinery.queueing_cell);
    assert!(refinery.refinery);
    assert!(refinery.bib);
    assert_eq!(refinery.foundation, "4x3");
    assert_eq!(refinery.queueing_cell, [4, 1]);

    let gacnst = rules.object("GACNST").expect("retail GACNST");
    assert_eq!(gacnst.strength, 1000);
    assert_eq!(gacnst.armor, "concrete");
    assert_eq!(gacnst.adjacent, 2);
    assert_eq!(gacnst.power, 0);
    let gapowr = rules.object("GAPOWR").expect("retail GAPOWR");
    assert_eq!(gapowr.strength, 750);
    assert_eq!(gapowr.armor, "wood");
    assert_eq!(gapowr.adjacent, 2);
    assert_eq!(gapowr.power, 200);
    let amradr = rules.object("AMRADR").expect("retail AMRADR");
    assert_eq!(amradr.strength, 600);
    assert_eq!(amradr.armor, "steel");
    assert_eq!(amradr.adjacent, 2);
    assert_eq!(amradr.power, -50);
    assert!(amradr.radar);

    let overlays = OverlayTypeRegistry::from_ini(&rules_ini, None);
    let tib01 = overlays.id_for_name("TIB01").expect("retail TIB01");
    assert!(overlays.flags(tib01).is_some_and(|flags| flags.tiberium));
    for terrain in ["Clear", "Tiberium"] {
        let expected = miner_contract_rules
            .terrain_rules
            .semantics_by_name(terrain)
            .unwrap_or_else(|| panic!("contract {terrain} terrain"));
        let actual = rules
            .terrain_rules
            .semantics_by_name(terrain)
            .unwrap_or_else(|| panic!("retail {terrain} terrain"));
        assert_eq!(actual.buildable, expected.buildable, "{terrain} Buildable");
        assert_eq!(
            actual.speed_costs, expected.speed_costs,
            "{terrain} speed-cost profile"
        );
    }

    let catalog = tech_catalog::resolve(&rules_ini, &art_ini);
    let contract_catalog = tech_catalog::resolve(
        &IniFile::from_str(include_str!(
            "fixtures/ini/rmg_neutral_tech_rules_contract.ini"
        )),
        &IniFile::from_str(include_str!(
            "fixtures/ini/rmg_neutral_tech_art_contract.ini"
        )),
    );
    assert_eq!(
        catalog
            .iter()
            .map(|entry| (entry.name.as_str(), entry.footprint.as_slice()))
            .collect::<Vec<_>>(),
        contract_catalog
            .iter()
            .map(|entry| (entry.name.as_str(), entry.footprint.as_slice()))
            .collect::<Vec<_>>(),
        "neutral-tech list and every footprint cell must match retail"
    );

    for id in ["GAWEAP", "NAWEAP", "GAYARD", "NAYARD", "YAWEAP", "YAYARD"] {
        assert!(
            rules
                .object(id)
                .is_some_and(|object| object.weapons_factory)
        );
    }
    for id in ["GAPILE", "GAPOWR"] {
        assert!(
            !rules
                .object(id)
                .is_some_and(|object| object.weapons_factory)
        );
    }

    assert_eq!(
        rules.object("APOC").expect("retail APOC").weight,
        SimFixed::lit("3.5")
    );
    assert_eq!(
        rules.object("MTNK").expect("retail MTNK").weight,
        SimFixed::lit("2")
    );
    assert_eq!(
        rules.general.direct_rocking_coefficient,
        SimFixed::lit("1.5")
    );
    assert_eq!(rules.general.fallback_coefficient, SimFixed::lit("0.1"));
    let v3wh_section = rules_ini.section("V3WH").expect("retail V3WH section");
    let v3wh = WarheadType::from_ini_section("V3WH", v3wh_section);
    assert!(v3wh.rocker);
    assert!(!v3wh.direct_rocker);

    for id in ["GHOST", "TANY", "PTROOP"] {
        assert!(rules.object(id).is_some_and(|object| object.c4));
    }
    for id in ["CAMISC01", "CAMISC02", "CAMISC06", "AMMOCRAT"] {
        assert!(!rules.object(id).is_some_and(|object| object.can_c4));
    }
    let napowr = rules.object("NAPOWR").expect("retail NAPOWR");
    assert_eq!(napowr.strength, 750);
    assert!(napowr.can_c4);
    assert_eq!(rules.c4_delay_ticks, 27);

    let temperate_ini = assets
        .get("temperatmd.ini")
        .expect("temperatmd.ini in retail archive stack");
    let expected_rmg_keys = rmg_keys_from_raw_theater(&temperate_ini);
    let theater = load_theater(&mut assets, "TEMPERATE").expect("load retail temperate theater");
    assert_eq!(
        theater.rmg_tiles, expected_rmg_keys,
        "production theater loading must resolve every RMG General ordinal"
    );
    assert_eq!(
        tile_id_projection(TileIds::resolve(&theater)),
        tile_id_projection(TileIds::from_keys(&expected_rmg_keys)),
        "resolved retail RMG keys must reach TileIds unchanged"
    );
}
