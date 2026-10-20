//! Native comparisons for a computer house's superweapon use
//! (`tools/superweapon_oracle.py` sections `ai_best_rally_target`,
//! `ai_try_fire`, `ai_ground_rally_point` and `ai_genetic_mutator`; `--check`
//! regenerates them), then the chain on retail rules.

use super::*;
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::test_grid;
use crate::rules::ini_parser::IniFile;
use crate::sim::production::ProductionCategory;
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::superweapon::cell_receiver_tests::{test_playfield_bounds, test_terrain_cell};
use crate::sim::superweapon::lightning_storm::LightningStorm;
use crate::util::lepton::lepton_to_cell_packed;
use serde_json::{Value, json};

const COMPUTER: &str = "Computer";
const ENEMY: &str = "Enemy";
const ALLY: &str = "Ally";

/// `SuperWeaponTypeClass +0xB4` values in the Type= name table's order.
const TYPE_NAMES: [&str; 12] = [
    "MultiMissile",
    "IronCurtain",
    "LightningStorm",
    "ChronoSphere",
    "ChronoWarp",
    "ParaDrop",
    "AmerParaDrop",
    "PsychicDominator",
    "SpyPlane",
    "GeneticConverter",
    "ForceShield",
    "PsychicReveal",
];

fn oracle() -> &'static Value {
    crate::test_fixture::json("tools/superweapon_oracle.json")
}

fn scripted_preference(kind: i32) -> (u16, u16) {
    let data = crate::test_fixture::json("tools/spatial_oracle/trigger_superweapon_state.json");
    let control = data["consumer_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == kind)
        .unwrap();
    let rows = control["rows"].as_array().unwrap();
    let preferred = rows
        .iter()
        .find(|row| row["stage"] == "set_preferred")
        .unwrap();
    let suppressed = rows
        .iter()
        .find(|row| row["stage"] == "preferred_consumer")
        .unwrap();
    assert!(
        suppressed["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| event["call"] != "fire")
    );
    wide(cell(&preferred["preferred"]))
}

#[test]
fn scripted_rally_preference_and_clear_match_original_targets_and_rng() {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let data = crate::test_fixture::json("tools/spatial_oracle/trigger_superweapon_state.json");
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map =
        crate::map::map_file::MapFile::from_bytes(assets.get_ref("ALL03UMD.MAP").unwrap()).unwrap();
    let named = assets.resolve_ref("ALL03UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL03UMD.INI")
            .unwrap()
            .ini
    });
    let mut reader = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = reader
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let preference = &data["retail_inputs"]["waypoints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|wp| wp["index"] == 364)
        .unwrap()["cell"];
    let seeds = &data["rng_seed_states"];
    let rng_hashes = |sim: &Simulation| {
        let hash = |hex: String| {
            let bytes: Vec<u8> = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            crate::util::sha256::sha256_hex(&bytes)
        };
        json!({"main": hash(sim.main_rng.native_state_hex()), "scenario": hash(sim.scenario_rng.native_state_hex()), "mapgen": hash(sim.mapgen_rng.native_state_hex())})
    };
    for control in data["consumer_controls"].as_array().unwrap() {
        let kind = int(&control["kind"]);
        if !matches!(kind, 0 | 2 | 5) {
            continue;
        }
        let mut sim = Simulation::with_seed(0);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let computer = sim.interner.intern(COMPUTER);
        let enemy = sim.interner.intern(ENEMY);
        for owner in [computer, enemy] {
            sim.houses
                .register(HouseState::new(owner, 0, None, false, 0, 10));
        }
        let input = &control["input"];
        sim.houses
            .get_mut(&computer)
            .unwrap()
            .set_difficulty_for_test(
                HouseDifficulty::from_native(int(&input["difficulty"])).unwrap(),
            );
        sim.houses
            .get_mut(&computer)
            .unwrap()
            .set_enemy_house_for_test(Some(enemy));
        sim.houses
            .get_mut(&enemy)
            .unwrap()
            .set_base_center_for_test(Some(wide(cell(&input["enemy_base"]))));
        sim.houses
            .get_mut(&enemy)
            .unwrap()
            .set_preferred_super_weapon_target(wide(cell(&input["initial_other_preferred"])));
        sim.session
            .set_binary_frame_for_test(int(&control["frame"]) as u32);
        sim.resolved_terrain = Some(test_grid(64, 64, test_terrain_cell));
        // These are the five admitted MapClass predicate inputs in the
        // generator, not a terrain-height fit to a captured target.
        sim.set_playfield_bounds_for_test(Some(PlayfieldBounds {
            base: 40,
            off_fc: 2,
            off_100: 4,
            off_104: 36,
            off_108: 36,
        }));
        crate::sim::arena_fixture::supply_native_map_keyed(&mut sim, "scripted rally flat64");
        for object in input["buildings"].as_array().unwrap() {
            let at = wide(cell(&object["cell"]));
            sim.spawn_object(
                object["type"].as_str().unwrap(),
                ENEMY,
                at.0,
                at.1,
                0,
                &rules,
            )
            .unwrap();
        }
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(seeds["main"].as_str().unwrap()).into();
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(seeds["scenario"].as_str().unwrap());
        sim.mapgen_rng = SimRng::from_native_state_hex_for_test(seeds["mapgen"].as_str().unwrap());
        assert_eq!(rng_hashes(&sim), control["rng_before"]);
        for row in control["rows"].as_array().unwrap() {
            let stage = row["stage"].as_str().unwrap();
            let expected = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["call"] == "fire")
                .map(|event| wide(cell(&event["cell"])));
            match stage {
                "set_preferred" => sim
                    .houses
                    .get_mut(&computer)
                    .unwrap()
                    .set_preferred_super_weapon_target(wide(cell(preference))),
                "clear_preferred" => sim
                    .houses
                    .get_mut(&computer)
                    .unwrap()
                    .clear_preferred_super_weapon_target(),
                "preferred_consumer" | "cleared_consumer" if matches!(kind, 0 | 2) => {
                    let (actual, draws) = crate::sim::rng::trace_draws(|| {
                        rally_target(&mut sim, &rules, computer, None)
                    });
                    assert_eq!(actual, expected, "{} {stage}", control["name"]);
                    let requests = row["rng_calls"].as_array().unwrap();
                    assert_eq!(draws.len(), requests.len());
                    for request in requests {
                        assert_eq!(request["callee"], "0065C7E0");
                        assert_eq!(request["stream"], "scenario");
                        assert_eq!(request["bounds"], json!([0, 1]));
                    }
                }
                "preferred_consumer" => assert_eq!(
                    ground_rally_point(&mut sim, &rules, computer, None),
                    expected
                ),
                "cleared_consumer" => {
                    // The native fixture has zero cell-array bounds; its real
                    // FNPC fails with (0,0). Compare that existing query seam's
                    // downstream scalar here. Ordinary FNPC options/returns
                    // retain the earlier ground-rally native tests' coverage.
                    assert_eq!(input["map_bounds"], json!([0, 0, 0, 0]));
                    assert!(
                        row["events"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|event| event["call"] == "nearby_passable")
                    );
                    assert_eq!(ground_rally_cell((0, 0)), expected);
                }
                _ => panic!("unhandled native consumer stage {stage}"),
            }
            assert_eq!(
                json!(sim.houses[&computer].preferred_super_weapon_target()),
                row["preferred"]
            );
            assert_eq!(
                json!(sim.houses[&enemy].preferred_super_weapon_target()),
                row["other_preferred"]
            );
            assert_eq!(
                rng_hashes(&sim),
                row["rng_after"],
                "{} {stage}",
                control["name"]
            );
        }
    }
}

fn rows<'a>(oracle: &'a Value, section: &str) -> &'a [Value] {
    oracle[section].as_array().unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn flag(value: &Value) -> bool {
    value.as_bool().unwrap()
}

fn cell(value: &Value) -> (i16, i16) {
    (
        i16::try_from(int(&value[0])).unwrap(),
        i16::try_from(int(&value[1])).unwrap(),
    )
}

fn wide(cell: (i16, i16)) -> (u16, u16) {
    (cell.0 as u16, cell.1 as u16)
}

/// The fixture's playfield (`ai_catalog.playfield`, MapClass
/// `+0xF4..+0x108`).
fn playfield(oracle: &Value) -> PlayfieldBounds {
    let field = &oracle["ai_catalog"]["playfield"];
    PlayfieldBounds {
        base: int(&field["base"]),
        off_fc: int(&field["off_fc"]),
        off_100: int(&field["off_100"]),
        off_104: int(&field["off_104"]),
        off_108: int(&field["off_108"]),
    }
}

/// The fixture's object types by their INI keys (`ai_catalog`), its `[AI]
/// BuildConst=` and `BuildTech=`, the row's `[General] AIIonCannon*Value=`
/// lists and `extra`, through the production reader.
fn catalog_rules(oracle: &Value, values: Option<&Value>, extra: &str) -> RuleSet {
    let catalog = &oracle["ai_catalog"];
    let mut lists = [
        ("unit", "VehicleTypes", String::new()),
        ("infantry", "InfantryTypes", String::new()),
        ("aircraft", "AircraftTypes", String::new()),
        ("building", "BuildingTypes", String::new()),
    ];
    let mut sections = String::new();
    let mut art = String::new();
    for (index, ty) in catalog["types"].as_array().unwrap().iter().enumerate() {
        let name = ty[0].as_str().unwrap();
        let what = ty[1].as_str().unwrap();
        let list = lists.iter_mut().find(|(kind, ..)| *kind == what).unwrap();
        list.2 += &format!("{index}={name}\n");
        sections += &format!("[{name}]\nStrength=100\nSpeed=4\n");
        for (key, value) in ty[2].as_object().unwrap() {
            sections += &format!("{key}={}\n", value.as_str().unwrap());
        }
        if what == "building" {
            art += &format!("[{name}]\nFoundation=1x1\n");
        }
    }
    let names = |key: &str| {
        catalog[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut general = String::from("[General]\n");
    for (key, list) in values
        .into_iter()
        .flat_map(|values| values.as_object().unwrap())
    {
        let list: Vec<String> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|value| int(value).to_string())
            .collect();
        general += &format!("{key}={}\n", list.join(","));
    }
    let mut text = String::new();
    for (_, section, entries) in &lists {
        text += &format!("[{section}]\n{entries}");
    }
    text += &format!(
        "[AI]\nBuildConst={}\nBuildTech={}\n{general}{sections}{extra}",
        names("build_const"),
        names("build_tech")
    );
    let art = IniFile::from_str(&art);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&text),
        &art,
    )
    .unwrap();
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art));
    rules
}

fn category(what: &str) -> EntityCategory {
    match what {
        "unit" => EntityCategory::Unit,
        "infantry" => EntityCategory::Infantry,
        "aircraft" => EntityCategory::Aircraft,
        "building" => EntityCategory::Structure,
        other => panic!("{other}"),
    }
}

/// AI_FindBestRallyTarget over each row's TechnoClass::Array: the row's
/// object facts become the [`RallyEntry`] the walk reads (`rally_entry` reads
/// the same facts of a live object; the retail test below covers it), then
/// the production value, cell, playfield test and pick. The Scenario stream
/// is seeded natively in each row; the target and the stream after it match.
#[test]
fn the_best_rally_target_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "ai_best_rally_target");
    assert_eq!(rows.len(), 161);
    let bounds = playfield(&oracle);
    let whats: Vec<(String, String)> = oracle["ai_catalog"]["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ty| {
            (
                ty[0].as_str().unwrap().into(),
                ty[1].as_str().unwrap().into(),
            )
        })
        .collect();
    let what = |name: &str| {
        &whats
            .iter()
            .find(|(type_name, _)| type_name == name)
            .unwrap()
            .1
    };
    let mut interner = crate::sim::intern::StringInterner::default();
    let owner = interner.intern(COMPUTER);
    for row in rows {
        let rules = catalog_rules(&oracle, Some(&row["values"]), "");
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.set_difficulty_for_test(
            HouseDifficulty::from_native(int(&row["difficulty"])).unwrap(),
        );
        let entries: Vec<RallyEntry> = row["objects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|object| {
                let name = object["type"].as_str().unwrap();
                let category = category(what(name));
                let enemy = object["owner"] == "enemy";
                let standing =
                    int(&object["layer"]) == 2 && flag(&object["alive"]) && !flag(&object["limbo"]);
                let factory = &object["factory"];
                let being_built = !factory.is_null()
                    && int(&factory["rate"]) != 0
                    && !flag(&factory["suspended"]);
                let coords = &object["coords"];
                let cell = (
                    lepton_to_cell_packed(int(&coords[0])),
                    lepton_to_cell_packed(int(&coords[1])),
                );
                RallyEntry {
                    candidate: enemy
                        && (standing
                            || (house.difficulty() == HouseDifficulty::Hard && being_built)),
                    value: if enemy {
                        rally_value(category, rules.object(name), &rules, &house)
                    } else {
                        0
                    },
                    cell,
                    // Every fixture cell lookup misses: the dummy's level
                    // and slope are zero.
                    in_playfield: bounds.contains_height_aware_packed(
                        i32::from(cell.0),
                        i32::from(cell.1),
                        0,
                        0,
                    ),
                    // A building's cloak stage 15 is a module residual: VERA
                    // never writes it, so only the fixture can.
                    cloaked: int(&object["cloak"]) == 2
                        || (category == EntityCategory::Structure && int(&object["stage"]) == 15),
                }
            })
            .collect();
        let mut rng = SimRng::new(int(&row["seed"]) as u64);
        assert_eq!(
            pick_best_rally_target(&entries, &mut rng),
            cell(&row["cell"]),
            "{row}"
        );
        assert_eq!(
            rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "{row}"
        );
    }
}

/// AI_TryFireSW over each row's Supers: production `try_fire` on a house in
/// the row's state, its log in the oracle's terms. AI_FindBestRallyTarget
/// answers the row's cell (an enemy object stands there); the tail-call
/// pickers were stubs, so a Fire_SW they make is not compared here (their own
/// replays below compare it). The Supers are charged but not granted, so
/// ClickFire refuses each click and nothing launches.
#[test]
fn the_try_fire_arms_match_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "ai_try_fire");
    assert_eq!(rows.len(), 26);
    for row in rows {
        let kinds = row["kinds"].as_array().unwrap();
        let mut extra = String::from("[SuperWeaponTypes]\n");
        for index in 0..kinds.len() {
            extra += &format!("{index}=SW{index}\n");
        }
        for (index, kind) in kinds.iter().enumerate() {
            extra += &format!(
                "[SW{index}]\nType={}\nRechargeTime=1\n",
                TYPE_NAMES[int(&kind[0]) as usize]
            );
        }
        let mut rules = catalog_rules(&oracle, None, &extra);
        rules.general.ai_super_defense_frames = int(&row["defense_frames"]);
        let mut sim = Simulation::with_seed(5);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let computer = sim.interner.intern(COMPUTER);
        let enemy = sim.interner.intern(ENEMY);
        for (id, human) in [(computer, flag(&row["human"])), (enemy, true)] {
            sim.houses
                .register(HouseState::new(id, 0, None, human, 0, 10));
        }
        let house = sim.houses.get_mut(&computer).unwrap();
        house.set_player_control_for_test(flag(&row["control"]));
        house.set_enemy_house_for_test((int(&row["enemy_index"]) != -1).then_some(enemy));
        house.alert_super_weapon_defense(wide(cell(&row["defense"])), int(&row["defense_frame"]));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                int(&row["game_mode"]) != 0,
            ),
        );
        sim.session
            .set_binary_frame_for_test(int(&row["frame"]) as u32);
        sim.resolved_terrain = Some(test_grid(64, 64, test_terrain_cell));
        sim.set_playfield_bounds_for_test(Some(test_playfield_bounds()));
        crate::sim::arena_fixture::supply_native_map_keyed(&mut sim, "ai fire flat 64");
        let rally = cell(&row["rally"]);
        if rally != NO_CELL {
            sim.spawn_object_at_height(
                "PLAIN",
                ENEMY,
                rally.0 as u16,
                rally.1 as u16,
                0,
                0,
                &rules,
            )
            .expect("the rally object stands");
        }
        if flag(&row["storm"]) {
            sim.lightning_storm = LightningStorm::raging_for_test(enemy, (3, 3));
        }
        let mut ids = Vec::new();
        for (index, kind) in kinds.iter().enumerate() {
            let id = sim.interner.intern(&format!("SW{index}"));
            let mut instance = SuperWeaponInstance::new(id, computer, 0);
            instance.is_ready = flag(&kind[1]);
            sim.super_weapons
                .map_mut_for_test()
                .entry(computer)
                .or_default()
                .insert(id, instance);
            ids.push(id);
        }

        AI_FIRE_LOG.set(Some(Vec::new()));
        try_fire(
            &mut sim,
            &rules,
            computer,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let log = AI_FIRE_LOG.take().unwrap();

        let index = |id: InternedId| ids.iter().position(|&sw| sw == id).unwrap();
        let mut tail_calls = Vec::new();
        let mut seen = Vec::new();
        for event in log {
            match event {
                AiFireEvent::TryFire => {}
                AiFireEvent::BestRallyTarget => seen.push(json!(["best_rally_target"])),
                AiFireEvent::TailArm(arm, id) => {
                    let name = match arm {
                        AiFireArm::GroundRallyPoint => "ground",
                        AiFireArm::PsychicDominator => "psychic_dominator",
                        AiFireArm::GeneticMutator => "genetic_mutator",
                        other => panic!("{other:?}"),
                    };
                    seen.push(json!([name, index(id)]));
                    tail_calls.push(id);
                }
                AiFireEvent::Fire(id, _) if tail_calls.contains(&id) => {}
                AiFireEvent::Fire(id, (x, y)) => {
                    seen.push(json!(["fire", index(id), [x as i16, y as i16]]))
                }
            }
        }
        assert_eq!(&Value::from(seen), &row["events"], "{row}");
    }
}

/// AI_GroundRallyPoint: the seed the production houses give, the native
/// Find_Nearby_Passable_Cell arguments `ground_rally_point` passes, and the
/// fired cell from the row's found cell.
#[test]
fn the_ground_rally_point_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "ai_ground_rally_point");
    assert_eq!(rows.len(), 11);
    let mut interner = crate::sim::intern::StringInterner::default();
    let computer = interner.intern(COMPUTER);
    let enemy = interner.intern(ENEMY);
    for row in rows {
        let mut houses = crate::sim::houses::Houses::default();
        for (id, base, alternate) in [
            (computer, &row["own_base"], &row["own_alternate"]),
            (enemy, &row["enemy_base"], &row["enemy_alternate"]),
        ] {
            let mut house = HouseState::new(id, 0, None, false, 0, 10);
            let base = wide(cell(base));
            house.set_base_center_for_test((base != (0, 0)).then_some(base));
            house.set_alternate_base_center_for_test(wide(cell(alternate)));
            houses.register(house);
        }
        houses
            .get_mut(&computer)
            .unwrap()
            .set_enemy_house_for_test((int(&row["enemy_index"]) != -1).then_some(enemy));
        let events = row["events"].as_array().unwrap();
        let find = &events[0];
        assert_eq!(find[0], "find_nearby");
        assert_eq!(
            ground_rally_seed(&houses, computer),
            wide(cell(&find[1])),
            "{row}"
        );
        // Find_Nearby_Passable_Cell(seed, SpeedType 0 Foot, zone -1,
        // MovementZone 0 Normal, not bridge-aware, 5x5, no overlay, height or
        // occupancy test, bridges allowed, close to the empty cell, no
        // quadrant skip, not buildable): `NearbyArgs::plain`'s fixed
        // options and `ground_rally_point`'s arguments.
        assert_eq!(find[2], json!([0, -1, 0, 0, 5, 5, 0, 0, 0, 1]), "{row}");
        assert_eq!(find[3], json!([0, 0]), "{row}");
        assert_eq!(find[4], json!([0, 0]), "{row}");
        let fired = ground_rally_cell(wide(cell(&row["found"])));
        let expected = events.get(1).map(|fire| {
            assert_eq!(fire[0], "fire");
            assert_eq!(fire[1], row["fired"]);
            wide(cell(&fire[2]))
        });
        assert_eq!(fired, expected, "{row}");
    }
}

/// AI_Fire_GenMutator over each row's objects, spawned in order on a flat
/// map with the fixture's playfield: the counted cell, or no fire.
#[test]
fn the_genetic_mutator_target_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "ai_genetic_mutator");
    assert_eq!(rows.len(), 23);
    let rules = catalog_rules(&oracle, None, "");
    for row in rows {
        let mut sim = Simulation::with_seed(5);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let computer = sim.interner.intern(COMPUTER);
        for name in [COMPUTER, ENEMY, ALLY] {
            let id = sim.interner.intern(name);
            sim.houses
                .register(HouseState::new(id, 0, None, false, 0, 10));
        }
        // `+0x5788` bit 2: the computer counts the third house an ally.
        sim.house_alliances_mut_for_test()
            .entry(COMPUTER.to_ascii_uppercase())
            .or_default()
            .insert(ALLY.to_ascii_uppercase());
        sim.resolved_terrain = Some(test_grid(64, 64, test_terrain_cell));
        sim.set_playfield_bounds_for_test(Some(playfield(&oracle)));
        crate::sim::arena_fixture::supply_native_map_keyed(&mut sim, "ai fire flat 64");
        // Each listed object and its cell, in arrival order; the cells that
        // hold a Unit or Building.
        let mut listed = Vec::new();
        let mut mixed = Vec::new();
        for (index, object) in row["objects"].as_array().unwrap().iter().enumerate() {
            let owner = match object[1].as_str().unwrap() {
                "self" => COMPUTER,
                "enemy" => ENEMY,
                "ally" => ALLY,
                other => panic!("{other}"),
            };
            let (x, y) = wide(cell(&object[2]));
            let kind = match object[0].as_str().unwrap() {
                "infantry" => "GI",
                "unit" | "building" => {
                    // It cannot Unlimbo onto infantry: it stands apart, on
                    // the playfield, until its cell's list is rebuilt below.
                    let kind = if object[0] == "unit" { "TANK" } else { "PLAIN" };
                    let apart = (33 + index as u16, 33 - index as u16);
                    let id = sim
                        .spawn_object_at_height(kind, owner, apart.0, apart.1, 0, 0, &rules)
                        .unwrap_or_else(|| panic!("{object} stands"));
                    listed.push((id, (x, y)));
                    mixed.push((x, y));
                    continue;
                }
                other => panic!("{other}"),
            };
            let extra = &object[3];
            let high = extra["high"].as_bool().unwrap_or(false);
            let id = sim
                .spawn_object_at_height(kind, owner, x, y, 0, if high { 4 } else { 0 }, &rules)
                .unwrap_or_else(|| panic!("{object} stands"));
            let entity = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                air_movement_high(entity, &sim, &rules),
                high,
                "{object} high-flying"
            );
            if extra["bridge"].as_bool().unwrap_or(false) {
                assert!(sim.foot_mark_remove(
                    id,
                    Some(&rules),
                    None,
                    crate::sim::world::FrameEffects::default()
                ));
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .set_on_bridge(true);
                assert!(sim.foot_mark_put(
                    id,
                    Some(&rules),
                    None,
                    crate::sim::world::FrameEffects::default()
                ));
            }
            if extra["limbo"].as_bool().unwrap_or(false) {
                sim.techno_limbo(id);
            } else {
                listed.push((id, (x, y)));
            }
        }
        // A cell holding a Unit or Building gets its list rebuilt in arrival
        // order, each object linked as Unlimbo links it (`foot_place_down`:
        // at the head, a Building at the tail).
        for &at in &mixed {
            let members: Vec<u64> = listed
                .iter()
                .filter(|(_, cell)| *cell == at)
                .map(|&(id, _)| id)
                .collect();
            for &id in &members {
                sim.substrate.occupancy.remove(at.0, at.1, id);
            }
            for &id in &members {
                let entity = sim.substrate.entities.get(id).unwrap();
                let (sub_cell, insertion) = (
                    entity.sub_cell(),
                    crate::sim::occupancy::CellListInsertion::from_category(entity.category()),
                );
                sim.substrate.occupancy.add(
                    at.0,
                    at.1,
                    id,
                    MovementLayer::Ground,
                    sub_cell,
                    insertion,
                );
            }
        }
        let expected = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event[0] == "fire")
            .map(|fire| {
                assert_eq!(fire[1], 1);
                wide(cell(&fire[2]))
            });
        assert_eq!(
            genetic_mutator_target(&sim, &rules, computer),
            expected,
            "{row}"
        );
        // The new original control establishes the nonzero-cell early exit.
        // Exercise it on all existing positive/negative target fixtures, then
        // restore the original golden rather than duplicating its picker.
        sim.houses
            .get_mut(&computer)
            .unwrap()
            .set_preferred_super_weapon_target(scripted_preference(9));
        assert_eq!(genetic_mutator_target(&sim, &rules, computer), None);
        sim.houses
            .get_mut(&computer)
            .unwrap()
            .clear_preferred_super_weapon_target();
        assert_eq!(genetic_mutator_target(&sim, &rules, computer), expected);
    }
}

fn air_movement_high(
    entity: &crate::sim::game_entity::GameEntity,
    sim: &Simulation,
    rules: &RuleSet,
) -> bool {
    crate::sim::movement::air_movement::is_high_flying(
        entity,
        sim.resolved_terrain.as_ref(),
        Some((rules, &sim.interner)),
    )
}

/// The Force Shield's timing on its own, across the wrap of the alert's
/// frame plus `AISuperDefenseFrames=` (the `ai_try_fire` rows with a single
/// Force Shield).
#[test]
fn the_force_shield_cell_follows_the_alert() {
    let oracle = oracle();
    let mut rules = catalog_rules(&oracle, None, "");
    let mut compared = 0;
    for row in rows(&oracle, "ai_try_fire") {
        if row["kinds"] != json!([[10, true]]) {
            continue;
        }
        compared += 1;
        rules.general.ai_super_defense_frames = int(&row["defense_frames"]);
        let mut house = HouseState::new(InternedId::default(), 0, None, false, 0, 10);
        house.alert_super_weapon_defense(wide(cell(&row["defense"])), int(&row["defense_frame"]));
        let expected = row["events"]
            .as_array()
            .unwrap()
            .first()
            .map(|fire| wide(cell(&fire[2])));
        assert_eq!(
            force_shield_target(&house, &rules, int(&row["frame"])),
            expected,
            "{row}"
        );
    }
    assert_eq!(compared, 13);
}

/// The keys the chain reads, through the production reader on the retail
/// INIs: the AIIonCannon values (three of them commented out), the alert's
/// length and the IQ that fires; no retail building sets HoverPad=,
/// IsTemple= or IsPlug=, so those values never decide.
#[test]
fn retail_ai_fire_rules() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let values = &rules.general.ai_ion_cannon_values;
    let hundred = vec![100, 100, 100];
    let one = vec![1, 1, 1];
    assert_eq!(values.con_yard, hundred);
    assert_eq!(values.war_factory, hundred);
    assert_eq!(values.power, vec![60, 100, 100]);
    assert_eq!(values.tech_center, hundred);
    for list in [
        &values.engineer,
        &values.thief,
        &values.harvester,
        &values.mcv,
        &values.apc,
    ] {
        assert_eq!(list, &one);
    }
    assert_eq!(values.base_defense, vec![35, 35, 35]);
    assert!(values.plug.is_empty() && values.helipad.is_empty() && values.temple.is_empty());
    assert_eq!(rules.general.ai_super_defense_frames, 50);
    assert_eq!(rules.general.iq_super_weapons, 4);
    assert!(
        rules
            .all_objects()
            .all(|ty| !ty.hover_pad && !ty.is_temple && !ty.is_plug)
    );
}

/// Retail rules (the two anims' frame counts bound: the lib suite loads no
/// SHP) and a skirmish on a flat 64x64 map: Americans, a Hard computer house
/// whose enemy is Russians, a human house.
fn retail_skirmish() -> Option<(RuleSet, Simulation, InternedId, InternedId)> {
    let mut rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    for name in ["PSIWARN", "NUKETO"] {
        rules.bind_anim_frame_count_for_test(name, 20);
    }
    let mut sim = Simulation::with_seed(11);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let americans = sim.interner.intern("Americans");
    let russians = sim.interner.intern("Russians");
    for (id, side, human) in [(russians, 1, true), (americans, 0, false)] {
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    let house = sim.houses.get_mut(&americans).unwrap();
    house.set_difficulty_for_test(HouseDifficulty::Hard);
    house.set_enemy_house_for_test(Some(russians));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().super_weapons = true;
    sim.resolved_terrain = Some(test_grid(64, 64, test_terrain_cell));
    sim.set_playfield_bounds_for_test(Some(test_playfield_bounds()));
    crate::sim::arena_fixture::supply_native_map_keyed(&mut sim, "ai fire flat 64");
    Some((rules, sim, americans, russians))
}

/// What AI_FindBestRallyTarget reads of live objects (the replay above
/// supplies these facts from the rows): the enemy's standing objects are
/// candidates, its limbo, dead or airborne ones are not, other houses' are
/// worth nothing, any house's cloaked object draws, and a Hard house also
/// takes what a running enemy factory builds.
#[test]
fn retail_walk_reads_each_object() {
    let Some((rules, mut sim, americans, russians)) = retail_skirmish() else {
        return;
    };
    let spawn = |sim: &mut Simulation, kind, owner, at: (u16, u16), z| {
        sim.spawn_object_at_height(kind, owner, at.0, at.1, 0, z, &rules)
            .unwrap_or_else(|| panic!("{kind} stands"))
    };
    let yard = spawn(&mut sim, "NACNST", "Russians", (40, 40), 0);
    let tank = spawn(&mut sim, "HTNK", "Russians", (30, 50), 0);
    let parked = spawn(&mut sim, "HTNK", "Russians", (31, 50), 0);
    sim.techno_limbo(parked);
    let dead = spawn(&mut sim, "HTNK", "Russians", (32, 50), 0);
    sim.substrate
        .entities
        .get_mut(dead)
        .unwrap()
        .lifecycle
        .set_object_alive_for_test(false);
    let flying = spawn(&mut sim, "ORCA", "Russians", (20, 50), 0);
    sim.set_object_height(
        flying,
        600,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_ne!(
        sim.entity_display_layer(flying, Some(&rules)),
        Some(crate::sim::world::display_layers::DisplayLayer::GROUND)
    );
    let own = spawn(&mut sim, "NAPOWR", "Americans", (20, 10), 0);
    let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0);
    cloak.set_state_for_test(2);
    sim.substrate.entities.get_mut(own).unwrap().cloak = Some(cloak);
    let built = spawn(&mut sim, "HTNK", "Russians", (33, 50), 0);
    sim.techno_limbo(built);

    let entry = |sim: &Simulation, id| {
        rally_entry(
            sim,
            &rules,
            &sim.houses[&americans],
            russians,
            sim.substrate.entities.get(id).unwrap(),
        )
    };
    // The 4x4 yard's centre cell.
    assert_eq!(
        entry(&sim, yard),
        RallyEntry {
            candidate: true,
            value: 100,
            cell: (42, 42),
            in_playfield: true,
            cloaked: false,
        }
    );
    let fact = |sim: &Simulation, id| {
        let entry = entry(sim, id);
        (entry.candidate, entry.value, entry.cloaked)
    };
    assert_eq!(fact(&sim, tank), (true, 2, false));
    assert_eq!(fact(&sim, parked), (false, 2, false));
    assert_eq!(fact(&sim, dead), (false, 2, false));
    assert_eq!(fact(&sim, flying), (false, 1, false));
    assert_eq!(fact(&sim, own), (false, 0, true));

    // A factory builds the limbo tank: running, it is a Hard house's
    // candidate; idle, held or finished it is not, nor for a Normal house.
    let htnk = sim.interner.intern("HTNK");
    let factories = sim.production.factories_mut();
    assert!(factories.test_enqueue_kernel(russians, ProductionCategory::Vehicle, htnk, 1, 900));
    let factory = factories
        .test_factory_mut(russians, ProductionCategory::Vehicle)
        .unwrap();
    factory.object_mut_for_test().as_mut().unwrap().entity_id = Some(built);
    assert_eq!(fact(&sim, built), (false, 2, false), "no rate yet");
    let set = |sim: &mut Simulation, rate, suspended, progress| {
        let factory = sim
            .production
            .factories_mut()
            .test_factory_mut(russians, ProductionCategory::Vehicle)
            .unwrap();
        factory.set_step_rate_frames_for_test(rate);
        factory.set_suspended_for_test(suspended);
        factory.set_progress_for_test(progress);
    };
    set(&mut sim, 5, false, 0);
    assert_eq!(fact(&sim, built), (true, 2, false));
    sim.houses
        .get_mut(&americans)
        .unwrap()
        .set_difficulty_for_test(HouseDifficulty::Normal);
    assert_eq!(fact(&sim, built), (false, 2, false), "Normal");
    sim.houses
        .get_mut(&americans)
        .unwrap()
        .set_difficulty_for_test(HouseDifficulty::Hard);
    set(&mut sim, 5, true, crate::sim::production::PRODUCTION_STEPS);
    assert_eq!(fact(&sim, built), (false, 2, false), "completed");
    set(&mut sim, 5, true, 1);
    assert_eq!(fact(&sim, built), (false, 2, false), "held");
}

/// A Hard computer house in a skirmish fires its charged nuclear missile from
/// its first Strategy tick at the enemy's construction yard, the object it
/// values most (100 against a power plant's 60 and a tank's 2), through the
/// production frames.
#[test]
fn retail_computer_house_nukes_the_enemy_construction_yard() {
    let Some((rules, mut sim, americans, _)) = retail_skirmish() else {
        return;
    };
    // NukeCarrier's ordinary projectile path reads the native Size diamond.
    // A width0 fixture becomes empty when its map height is supplied; place
    // this launch scene inside a coherent Size/normalized LocalSize instead.
    sim.session.set_map_width_for_test(64);
    sim.session.set_map_height_for_test(64);
    sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
        theater: "TEMPERATE".into(),
        fill: "Clear".into(),
        level: 0,
        width: 32,
        height: 64,
        local_left: 2,
        local_top: 2,
        local_width: 28,
        local_height: 56,
    });
    assert!(sim.rebuild_dynamic_navigation(&rules));
    for cell in [
        (30, 30),
        (20, 20),
        (24, 20),
        (40, 40),
        (50, 40),
        (30, 50),
        (42, 42),
    ] {
        assert!(sim.map_cell_in_bounds(cell), "launch scene cell {cell:?}");
        assert!(crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(cell.0), i32::from(cell.1)),
            sim.playfield_bounds(),
            sim.resolved_terrain.as_ref(),
        ));
    }
    let silo = sim
        .spawn_object_at_height("NAMISL", "Americans", 30, 30, 0, 0, &rules)
        .expect("the silo stands");
    for at in [(20, 20), (24, 20)] {
        sim.spawn_object_at_height("NAPOWR", "Americans", at.0, at.1, 0, 0, &rules)
            .expect("the power plant stands");
    }
    for (kind, at) in [
        ("NACNST", (40, 40)),
        ("NAPOWR", (50, 40)),
        ("HTNK", (30, 50)),
    ] {
        sim.spawn_object_at_height(kind, "Russians", at.0, at.1, 0, 0, &rules)
            .unwrap_or_else(|| panic!("{kind} stands"));
    }
    let sw_type = sim.interner.intern("NukeSpecial");
    let mut instance = SuperWeaponInstance::new(sw_type, americans, 0);
    instance.is_active = true;
    instance.charge_start_tick = (sim.session.binary_frame()) as i32;
    instance.charge_duration = 9000;
    instance.is_ready = true;
    sim.super_weapons
        .map_mut_for_test()
        .entry(americans)
        .or_default()
        .insert(sw_type, instance);

    let mut carrier = false;
    for _ in 0..30 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), None, None, 33);
        if sim.projectiles.iter().any(|(_, bullet)| {
            bullet
                .payload()
                .installed_weapon()
                .is_some_and(|id| sim.interner.resolve(id) == "NukeCarrier")
        }) {
            carrier = true;
            break;
        }
    }
    assert!(
        carrier,
        "the missile flies: target={:?}, super={:?}, silo={:?}, power={:?}, house={:?}",
        sim.houses[&americans].nuke_target(),
        sim.super_weapons
            .get(&americans)
            .and_then(|supers| supers.get(&sw_type)),
        sim.substrate.entities.get(silo).map(|entity| (
            &entity.mission,
            entity.building_online(),
            entity.building_up(),
            entity.health.current()
        )),
        sim.power_states.get(&americans),
        (
            sim.houses[&americans].is_defeated(),
            sim.houses[&americans].enemy_house(),
            sim.houses[&americans].strategy_timer()
        )
    );
    // The yard's centre cell.
    assert_eq!(sim.houses[&americans].nuke_target(), (42, 42));
    let instance = &sim.super_weapons[&americans][&sw_type];
    assert!(!instance.is_ready);
    assert!(sim.sound_events.iter().any(|event| matches!(
        event,
        crate::sim::world::SimSoundEvent::SuperWeaponLaunched { owner, rx: 42, ry: 42, .. }
            if *owner == americans
    )));
}

/// AI_Fire_PsyDom against the native rows (`ai_psydom`): its gates, which
/// objects count (the house's own and its ally's never; CanBePermaMindControlled
/// and the air test), the 38 cells around each Foot, the ground lists' Foot
/// walk, the bridge list it never reads and the last-to-first tie. Native's
/// object of no house is here a fourth house's, which the computer is not
/// allied with. A second Unit in one cell spawns apart, as VERA cannot
/// Unlimbo it there, and is moved in with the cell's list rebuilt in arrival
/// order (each Unlimbo links a non-building at the head, a building at the
/// tail).
#[test]
fn the_psychic_dominator_target_matches_native() {
    use crate::sim::superweapon::invulnerability::{InvulnKind, apply_invulnerability};
    use crate::sim::superweapon::psychic_dominator::PsychicDominatorState;
    let oracle = oracle();
    let rows = rows(&oracle, "ai_psydom");
    assert_eq!(rows.len(), 26);
    let art = IniFile::from_str("[PLAIN]\nFoundation=1x1\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(
            "[InfantryTypes]\n0=GI\n1=PSIGI\n[VehicleTypes]\n0=TANK\n1=BALLOON\n\
             [AircraftTypes]\n0=JET\n[BuildingTypes]\n0=PLAIN\n\
             [GI]\nStrength=100\nSpeed=4\n\
             [PSIGI]\nStrength=100\nSpeed=4\nImmuneToPsionics=yes\n\
             [TANK]\nStrength=100\nSpeed=4\n\
             [BALLOON]\nStrength=100\nSpeed=4\nBalloonHover=yes\n\
             [JET]\nStrength=100\nSpeed=4\n[PLAIN]\nStrength=100\n",
        ),
        &art,
    )
    .unwrap();
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art));
    const NO_HOUSE: &str = "Nobody";
    for row in rows {
        let mut sim = Simulation::with_seed(5);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let computer = sim.interner.intern(COMPUTER);
        let enemy = sim.interner.intern(ENEMY);
        for name in [COMPUTER, ENEMY, ALLY, NO_HOUSE] {
            let id = sim.interner.intern(name);
            sim.houses
                .register(HouseState::new(id, 0, None, false, 0, 10));
        }
        sim.houses
            .get_mut(&computer)
            .unwrap()
            .set_enemy_house_for_test((int(&row["enemy_index"]) != -1).then_some(enemy));
        // `+0x5788` bit 2: the computer counts the third house an ally.
        sim.house_alliances_mut_for_test()
            .entry(COMPUTER.to_ascii_uppercase())
            .or_default()
            .insert(ALLY.to_ascii_uppercase());
        sim.resolved_terrain = Some(test_grid(64, 64, test_terrain_cell));
        sim.set_playfield_bounds_for_test(Some(playfield(&oracle)));
        crate::sim::arena_fixture::supply_native_map_keyed(&mut sim, "ai fire flat 64");
        let status = u8::try_from(int(&row["psydom"])).unwrap();
        sim.psychic_dominator = PsychicDominatorState::for_test(
            status,
            (0, 0),
            (status != 0).then_some(computer),
            None,
        );
        // Each listed object and its cell, in arrival order; the cells whose
        // list needs rebuilding.
        let mut listed = Vec::new();
        let mut rebuilt = Vec::new();
        for (index, object) in row["objects"].as_array().unwrap().iter().enumerate() {
            let owner = match object[1].as_str().unwrap() {
                "self" => COMPUTER,
                "enemy" => ENEMY,
                "ally" => ALLY,
                "none" => NO_HOUSE,
                other => panic!("{other}"),
            };
            let (x, y) = wide(cell(&object[2]));
            let extra = &object[3];
            let fact = |key: &str| extra[key].as_bool().unwrap_or(false);
            let kind = match (object[0].as_str().unwrap(), fact("immune"), fact("balloon")) {
                ("infantry", false, _) => "GI",
                ("infantry", true, _) => "PSIGI",
                ("unit", _, false) => "TANK",
                ("unit", _, true) => "BALLOON",
                ("aircraft", ..) => "JET",
                ("building", ..) => "PLAIN",
                (other, ..) => panic!("{other}"),
            };
            let occupied = listed.iter().any(|&(_, at)| at == (x, y));
            let apart = kind == "PLAIN" || (kind != "GI" && kind != "PSIGI" && occupied);
            let at = if apart {
                (33 + index as u16, 33 - index as u16)
            } else {
                (x, y)
            };
            let id = sim
                .spawn_object_at_height(
                    kind,
                    owner,
                    at.0,
                    at.1,
                    0,
                    if fact("high") { 4 } else { 0 },
                    &rules,
                )
                .unwrap_or_else(|| panic!("{object} stands"));
            if fact("high") && kind == "JET" {
                // Four levels up, as the spawn height lifts infantry.
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .position
                    .set_exact_z_leptons_for_test(Some(
                        4 * crate::util::lepton::LEPTONS_PER_LEVEL as i32,
                    ));
            }
            let entity = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                air_movement_high(entity, &sim, &rules),
                fact("high"),
                "{object} high-flying"
            );
            if apart {
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                entity.position.set_rx_for_test(x);
                entity.position.set_ry_for_test(y);
                sim.substrate.occupancy.remove(at.0, at.1, id);
                rebuilt.push((x, y));
            }
            if fact("curtain") {
                let frame = sim.session.binary_frame();
                apply_invulnerability(
                    sim.substrate.entities.get_mut(id).unwrap(),
                    frame,
                    10_000,
                    InvulnKind::IronCurtain,
                );
            }
            if fact("bridge") {
                assert!(sim.foot_mark_remove(
                    id,
                    Some(&rules),
                    None,
                    crate::sim::world::FrameEffects::default()
                ));
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .set_on_bridge(true);
                assert!(sim.foot_mark_put(
                    id,
                    Some(&rules),
                    None,
                    crate::sim::world::FrameEffects::default()
                ));
            }
            if fact("limbo") {
                sim.techno_limbo(id);
            } else {
                listed.push((id, (x, y)));
            }
        }
        for &at in &rebuilt {
            let members: Vec<u64> = listed
                .iter()
                .filter(|(_, cell)| *cell == at)
                .map(|&(id, _)| id)
                .collect();
            for &id in &members {
                sim.substrate.occupancy.remove(at.0, at.1, id);
            }
            for &id in &members {
                let entity = sim.substrate.entities.get(id).unwrap();
                let (sub_cell, insertion) = (
                    entity.sub_cell(),
                    crate::sim::occupancy::CellListInsertion::from_category(entity.category()),
                );
                sim.substrate.occupancy.add(
                    at.0,
                    at.1,
                    id,
                    MovementLayer::Ground,
                    sub_cell,
                    insertion,
                );
            }
        }
        let expected = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event[0] == "fire")
            .map(|fire| {
                assert_eq!(fire[1], 0);
                wide(cell(&fire[2]))
            });
        assert_eq!(
            psychic_dominator_target(&sim, &rules, computer),
            expected,
            "{row}"
        );
        sim.houses
            .get_mut(&computer)
            .unwrap()
            .set_preferred_super_weapon_target(scripted_preference(7));
        assert_eq!(psychic_dominator_target(&sim, &rules, computer), None);
        sim.houses
            .get_mut(&computer)
            .unwrap()
            .clear_preferred_super_weapon_target();
        assert_eq!(psychic_dominator_target(&sim, &rules, computer), expected);
    }
}
