//! Original campaign House5009B0/500B40/4F54A0/4F6EC0 comparisons.
//! The native numeric-ID prior covers selected Country/Side/SuperType
//! constructors only. Tests replay that segment; it is not full Rules Pcount.

use serde_json::Value;

use super::{
    FreshScenarioPrefix, FreshScenarioPrefixError, ScenarioBootstrapRng,
    initialize_campaign_current_house, initialize_map_roster_houses,
};
use crate::map::houses::{HouseRoster, is_allied_with, parse_house_roster};
use crate::map::map_file::MapFile;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioDescriptor;
use crate::sim::world::Simulation;

fn corpus() -> Value {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/campaign_start_houses.json",
    ))
    .unwrap();
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    native
}

fn cached_sections(sections: &Value) -> IniFile {
    let mut ini_sections = Vec::new();
    for (name, entries) in sections.as_object().unwrap() {
        let mut section = IniSection::new(name.clone());
        // These fixtures record contiguous numbered House entries. Preserve
        // their input registration order despite JSON's sorted object keys.
        if name == "Houses" {
            let entries = entries.as_object().unwrap();
            for index in 0..entries.len() {
                let key = index.to_string();
                section.set(&key, entries[&key].as_str().unwrap());
            }
        } else {
            for (key, value) in entries.as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
        }
        ini_sections.push(section);
    }
    IniFile::from_sections_for_test(ini_sections)
}

#[test]
fn campaign_country_color_reader_matches_original_full_country_controls() {
    let native = corpus();
    let controls = &native["color_controls"];
    let mut colors = IniSection::new("Colors".to_string());
    for scheme in controls["registry"].as_array().unwrap() {
        if scheme["shade_count"] != 1 {
            continue;
        }
        let name = scheme["name"].as_str().unwrap();
        let value = controls["physical_colors"]
            .get(name)
            .or_else(|| controls["synthetic_colors"].get(name))
            .unwrap()
            .as_str()
            .unwrap();
        colors.set(name, value);
    }
    let mut countries = IniSection::new("Countries".to_string());
    countries.set("0", "Control");
    let catalog = IniFile::from_sections_for_test([colors, countries]);
    let rows = controls["country_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 24);
    for row in rows {
        let mut root = catalog.clone();
        root.merge(&cached_sections(&row["setup_sections"]));
        let before = RulesLayerStack::new(root).process().unwrap();
        assert_eq!(
            before.country_color_scheme("Control"),
            Some(i32_at(row, "before")),
            "original511850 setup {row}",
        );
        let (_, trace) = before.into_ini_and_native_type_construction_trace();
        let input = if let Some(physical) = row["physical_ini"].as_str() {
            IniFile::from_str(physical)
        } else {
            cached_sections(&row["sections"])
        };
        let mut continuation = RulesLayerStack::new(IniFile::empty());
        continuation.push(RulesLayerKind::Scenario, input);
        let after = continuation
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let rules = crate::rules::ruleset::RuleSet::from_processed_rules(&after).unwrap();
        assert_eq!(
            rules.country_color_scheme("Control"),
            i32_at(row, "after"),
            "original511850 field+C0 {row}",
        );
    }
}

fn i32_at(row: &Value, key: &str) -> i32 {
    row[key].as_i64().unwrap() as i32
}

fn le_bits(value: &Value) -> u64 {
    let hex = value.as_str().unwrap();
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    u64::from_le_bytes(bytes.try_into().unwrap())
}

fn assert_houses(
    sim: &Simulation,
    roster: &HouseRoster,
    rules: &crate::rules::ruleset::RuleSet,
    rows: &Value,
    id_offset: i32,
) {
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), sim.session.house_order.len());
    for (index, expected) in rows.iter().enumerate() {
        let owner = sim.session.house_order[index];
        let actual = &sim.houses[&owner];
        assert_eq!(
            sim.interner.resolve(owner),
            expected["name"].as_str().unwrap()
        );
        assert_eq!(
            actual.native_unique_id(),
            Some(i32_at(expected, "id").wrapping_add(id_offset))
        );
        assert_eq!(actual.side_index as i32, i32_at(expected, "side"));
        assert_eq!(
            actual.country.map(|id| sim.interner.resolve(id)),
            expected["country"].as_str()
        );
        assert_eq!(actual.is_human, expected["human"].as_bool().unwrap());
        assert_eq!(
            actual.player_control,
            expected["player_control"].as_bool().unwrap()
        );
        assert_eq!(actual.difficulty as i32, i32_at(expected, "difficulty"));
        assert_eq!(actual.tech_level, i32_at(expected, "tech_level"));
        assert_eq!(actual.current_iq, i32_at(expected, "iq"));
        assert_eq!(actual.authored_iq, i32_at(expected, "iq"));
        assert_eq!(
            actual.economy.scenario_credits(),
            i32_at(expected, "scenario_credits")
        );
        assert_eq!(actual.economy.credits(), i32_at(expected, "balance"));
        assert_eq!(actual.authored_edge(), i32_at(expected, "edge"));
        assert_eq!(
            actual.team_creation.ratio(),
            i32_at(expected, "ratio_ai_trigger_team")
        );
        assert_eq!(
            actual.scenario_team_ratios(),
            ["ratio_aircraft", "ratio_infantry", "ratio_units"].map(|key| i32_at(expected, key))
        );
        for (name, value) in [
            "firepower",
            "groundspeed",
            "airspeed",
            "armor",
            "rof",
            "cost",
            "build_time",
            "repair_delay",
            "build_delay",
        ]
        .into_iter()
        .zip(actual.scalar_difficulty_biases())
        {
            assert_eq!(
                value.bits(),
                le_bits(&expected["doubles"][name]),
                "House{index} {name}"
            );
        }
        let (timer, initial_delay) = actual.native_attack_timer().unwrap();
        assert_eq!(
            [timer.start_frame(), timer.duration(), initial_delay],
            ["start", "duration", "initial_delay"]
                .map(|key| i32_at(&expected["attack_timer"], key))
        );
        let timer = actual.team_creation.timer();
        assert_eq!(
            [timer.start_frame(), timer.duration()],
            ["start", "duration"].map(|key| i32_at(&expected["team_timer"], key))
        );
        // The palette owner exposes logical Colors indexes; native registers
        // paired1/53-shade schemes and ReadColor selects the odd member.
        assert_eq!(
            u32::from(roster.houses[index].color.0),
            expected["initial_color_index"].as_u64().unwrap() as u32 / 2,
            "House{index} {} inherited/authored Color",
            roster.houses[index].name
        );
        let mask = expected["alliance_mask"].as_u64().unwrap() as u32;
        for (target_index, target) in roster.houses.iter().enumerate() {
            if index != target_index {
                assert_eq!(
                    is_allied_with(
                        &sim.house_alliances,
                        &roster.houses[index].name,
                        &target.name
                    ),
                    mask & 1u32.wrapping_shl(target_index as u32) != 0,
                    "House{index} -> House{target_index}"
                );
            }
        }
        assert_eq!(
            sim.super_weapons[&owner].len(),
            rules.super_weapon_order.len()
        );
        let supers = expected["supers"].as_array().unwrap();
        assert_eq!(supers.len(), rules.super_weapon_order.len());
        for (name, expected) in rules.super_weapon_order.iter().zip(supers) {
            assert_eq!(name, expected["type_name"].as_str().unwrap());
            let actual = &sim.super_weapons[&owner][&sim.interner.get(name).unwrap()];
            assert_eq!(
                actual.native_unique_id(),
                Some(i32_at(expected, "id").wrapping_add(id_offset))
            );
            assert_eq!(
                actual.charge_start_tick,
                i32_at(&expected["charge_timer"], "start")
            );
            assert_eq!(
                actual.charge_duration,
                i32_at(&expected["charge_timer"], "duration")
            );
            assert_eq!(actual.is_active, expected["granted"].as_bool().unwrap());
            assert_eq!(actual.is_ready, expected["charged"].as_bool().unwrap());
            assert_eq!(actual.is_suspended, expected["on_hold"].as_bool().unwrap());
        }
    }
}

#[test]
fn campaign_house_segment_matches_six_stock_rows_and_nine_startup_controls() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let root = IniFile::from_bytes(assets.get_ref("RULESMD.INI").unwrap()).unwrap();
    let art = IniFile::from_bytes(assets.get_ref("ARTMD.INI").unwrap()).unwrap();
    let native = corpus();
    assert_eq!(native["stock_rows"].as_array().unwrap().len(), 6);
    let mut compared = 0;
    for row in native["stock_rows"]
        .as_array()
        .unwrap()
        .iter()
        .chain(native["controls"].as_array().unwrap().iter())
    {
        if row["scenario_init"].as_i64() != Some(2) || row["name"] == "missing_houses_fallback" {
            // Native fallback is outside the authored stock-roster admission;
            // counter-zero live CanAlly is outside ScenarioInit's bypass.
            continue;
        }
        let ini = if let Some(filename) = row["filename"].as_str() {
            IniFile::from_bytes(assets.get_ref(filename).unwrap()).unwrap()
        } else {
            cached_sections(&row["sections"])
        };
        let mut source = root.clone();
        source.merge(&cached_sections(&row["rule_sections_override"]));
        let mut process = NativeRulesProcessOwner::from_cold_start_sources(
            source,
            None,
            art.clone(),
            std::sync::Arc::default(),
        )
        .unwrap();
        let (rules, _, _, receipt) = process
            .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &ini)
            .unwrap()
            .into_parts();
        let expected_types: Vec<_> = row["superweapon_types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value["name"].as_str().unwrap())
            .collect();
        assert_eq!(rules.super_weapon_order, expected_types);
        assert_eq!(rules.super_weapon_order.len(), 12);
        assert_eq!(
            rules.general.attack_delay.to_bits(),
            le_bits(&row["rules"]["ai_attack_delay_bits"])
        );
        assert_eq!(
            rules.general.game_speed_bias.to_bits(),
            le_bits(&row["rules"]["game_speed_bias_bits"])
        );
        let roster = parse_house_roster(&ini, &rules.color_schemes, Some(&rules));
        let mut sim = Simulation::from_descriptor(&ScenarioDescriptor {
            seed: row["seed"].as_u64().unwrap() as u32,
            ..Default::default()
        });
        sim.session
            .initialize_campaign_startup(i32_at(row, "option"), i32_at(row, "mission_number"))
            .unwrap();
        sim.native_unique_ids = Some(NativeUniqueIdCursor::begin_campaign_prefix(
            0,
            row["id_before"].as_u64().unwrap() as usize - 1_000_000,
        ));
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                .into();
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
        initialize_map_roster_houses(&mut sim, &roster, Some(&rules), Some(&ini));
        assert_houses(&sim, &roster, &rules, &row["houses"], 0);
        initialize_campaign_current_house(&mut sim, &roster, &ini);
        assert_houses(&sim, &roster, &rules, &row["final_houses"], 0);
        let current = sim.session.current_house().unwrap();
        assert_eq!(
            sim.interner.resolve(current),
            row["current_house_name"].as_str().unwrap()
        );
        assert_eq!(
            sim.houses[&current].native_unique_id(),
            Some(i32_at(row, "current_house_id"))
        );
        assert_eq!(
            sim.native_unique_ids.as_ref().unwrap().current_raw(),
            row["id_after"].as_u64().unwrap() as u32
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"]["scenario"].as_str().unwrap()
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            row["rng_after"]["main"].as_str().unwrap()
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            row["rng_after"]["mapgen"].as_str().unwrap()
        );
        let saved = serde_json::to_vec(&sim.houses[&current]).unwrap();
        let restored: crate::sim::house_state::HouseState = serde_json::from_slice(&saved).unwrap();
        assert_eq!(
            restored.native_unique_id(),
            sim.houses[&current].native_unique_id()
        );
        assert_eq!(
            restored.scalar_difficulty_biases(),
            sim.houses[&current].scalar_difficulty_biases()
        );
        assert_eq!(
            restored.economy.scenario_credits(),
            sim.houses[&current].economy.scenario_credits()
        );
        if let Some(filename) = row["filename"].as_str() {
            let map = MapFile::from_bytes(assets.get_ref(filename).unwrap()).unwrap();
            let descriptor = ScenarioDescriptor {
                seed: row["seed"].as_u64().unwrap() as u32,
                ..Default::default()
            };
            let mut bootstrap = ScenarioBootstrapRng::new(descriptor.seed);
            bootstrap.install_process_mapgen_continuation(
                SimRng::from_native_state_hex_for_test(
                    row["rng_before"]["mapgen"].as_str().unwrap(),
                )
                .mapgen_continuation(),
            );
            let (fresh, projection) = bootstrap
                .into_fresh_staged_simulation(
                    &descriptor,
                    FreshScenarioPrefix::Campaign {
                        native_rules_receipt: receipt,
                        rules: &rules,
                        map_data: &map,
                        house_roster: &roster,
                        difficulty: i32_at(row, "option"),
                        mission_counter: i32_at(row, "mission_number"),
                    },
                )
                .unwrap();
            assert!(
                projection.is_none(),
                "campaigns do not synthesize an offline start plan"
            );
            let first = fresh.session.house_order[0];
            // Rebase the bounded native House segment against the actual
            // Rules receipt. This validates House allocation order without
            // asserting that the native fixture established full Pcount.
            let offset = fresh.houses[&first]
                .native_unique_id()
                .unwrap()
                .wrapping_sub(i32_at(&row["final_houses"][0], "id"));
            assert_houses(&fresh, &roster, &rules, &row["final_houses"], offset);
            assert_eq!(
                fresh.scenario_rng.native_state_hex(),
                row["rng_after"]["scenario"].as_str().unwrap()
            );
            assert_eq!(
                fresh.main_rng.native_state_hex(),
                row["rng_after"]["main"].as_str().unwrap()
            );
            assert_eq!(
                fresh.mapgen_rng.native_state_hex(),
                row["rng_after"]["mapgen"].as_str().unwrap()
            );
            assert!(
                fresh.entities().is_empty() && fresh.production.terrain_objects.is_empty(),
                "the actual House generation completes before Fill and every object section"
            );
        }
        compared += 1;
    }
    assert_eq!(compared, 15);
}

#[test]
fn fresh_campaign_rejects_an_absent_stock_roster_instead_of_fabricating_offline_houses() {
    let ini =
        IniFile::from_str("[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n");
    let mut process = NativeRulesProcessOwner::from_cold_start_sources(
        ini,
        None,
        IniFile::empty(),
        std::sync::Arc::default(),
    )
    .unwrap();
    let map =
        MapFile::from_bytes(b"[Map]\nSize=0,0,4,4\n[IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n").unwrap();
    let (rules, _, _, receipt) = process
        .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &map.ini)
        .unwrap()
        .into_parts();
    let roster = HouseRoster::default();
    let result = ScenarioBootstrapRng::new(31).into_fresh_staged_simulation(
        &ScenarioDescriptor::default(),
        FreshScenarioPrefix::Campaign {
            native_rules_receipt: receipt,
            rules: &rules,
            map_data: &map,
            house_roster: &roster,
            difficulty: 1,
            mission_counter: 1,
        },
    );
    assert!(matches!(
        result,
        Err(FreshScenarioPrefixError::MissingCampaignHouseRoster)
    ));
}

#[test]
fn fresh_campaign_rejects_unknown_country_factory_before_house_construction() {
    let root = IniFile::from_str(
        "[Colors]\nLightGold=25,255,255\nGold=43,239,255\n\
         [Countries]\n0=Americans\n[Americans]\nColor=Gold\n\
         [General]\nTeamDelays=1,1,1\n",
    );
    let mut process = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        IniFile::empty(),
        std::sync::Arc::default(),
    )
    .unwrap();
    let map = MapFile::from_bytes(
        b"[Map]\nSize=0,0,4,4\n[IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n\
          [Houses]\n0=KnownHouse\n1=UnknownHouse\n\
          [KnownHouse]\nCountry=Americans\n\
          [UnknownHouse]\nCountry=UnregisteredCountry\n",
    )
    .unwrap();
    let (rules, _, _, receipt) = process
        .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &map.ini)
        .unwrap()
        .into_parts();
    let roster = parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let result = ScenarioBootstrapRng::new(31).into_fresh_staged_simulation(
        &ScenarioDescriptor::default(),
        FreshScenarioPrefix::Campaign {
            native_rules_receipt: receipt,
            rules: &rules,
            map_data: &map,
            house_roster: &roster,
            difficulty: 1,
            mission_counter: 1,
        },
    );
    assert!(matches!(
        result,
        Err(FreshScenarioPrefixError::UnsupportedCampaignCountryConstructor { house, country })
            if house == "UnknownHouse" && country == "UnregisteredCountry"
    ));
}
