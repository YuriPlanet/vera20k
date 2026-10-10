//! Native comparisons for a computer house's superweapon use
//! (`tools/superweapon_oracle.py` sections `ai_best_rally_target`,
//! `ai_try_fire`, `ai_ground_rally_point` and `ai_genetic_mutator`; `--check`
//! regenerates them), then the chain on retail rules.

use super::*;
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::test_grid;
use crate::rules::art_data::ArtRegistry;
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

fn oracle() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap()
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
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&text), &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
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
        house.difficulty = HouseDifficulty::from_native(int(&row["difficulty"])).unwrap();
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
                        && (standing || (house.difficulty == HouseDifficulty::Hard && being_built)),
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
                .insert(id, HouseState::new(id, 0, None, human, 0, 10));
            sim.session.house_order.push(id);
        }
        let house = sim.houses.get_mut(&computer).unwrap();
        house.player_control = flag(&row["control"]);
        house.enemy_house = (int(&row["enemy_index"]) != -1).then_some(enemy);
        house.alert_super_weapon_defense(wide(cell(&row["defense"])), int(&row["defense_frame"]));
        sim.session.game_mode_nonzero = int(&row["game_mode"]) != 0;
        sim.session.binary_frame = int(&row["frame"]) as u32;
        sim.resolved_terrain = Some(test_grid(64, 64, test_terrain_cell));
        sim.playfield_bounds = Some(test_playfield_bounds());
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
        let mut houses = std::collections::BTreeMap::new();
        for (id, base, alternate) in [
            (computer, &row["own_base"], &row["own_alternate"]),
            (enemy, &row["enemy_base"], &row["enemy_alternate"]),
        ] {
            let mut house = HouseState::new(id, 0, None, false, 0, 10);
            let base = wide(cell(base));
            house.base_center = (base != (0, 0)).then_some(base);
            house.alternate_base_center = wide(cell(alternate));
            houses.insert(id, house);
        }
        houses.get_mut(&computer).unwrap().enemy_house =
            (int(&row["enemy_index"]) != -1).then_some(enemy);
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
        // quadrant skip, not buildable): `find_plain_passable_cell`'s fixed
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
                .insert(id, HouseState::new(id, 0, None, false, 0, 10));
            sim.session.house_order.push(id);
        }
        // `+0x5788` bit 2: the computer counts the third house an ally.
        sim.house_alliances
            .entry(COMPUTER.to_ascii_uppercase())
            .or_default()
            .insert(ALLY.to_ascii_uppercase());
        sim.resolved_terrain = Some(test_grid(64, 64, test_terrain_cell));
        sim.playfield_bounds = Some(playfield(&oracle));
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
                sim.substrate.entities.get_mut(id).unwrap().on_bridge = true;
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
                    entity.sub_cell,
                    crate::sim::occupancy::CellListInsertion::from_category(entity.category),
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
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
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
    let (rules_ini, art_ini) = crate::rules::retail_ini_fixture::retail_rules_and_art()?;
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
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
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
    }
    let house = sim.houses.get_mut(&americans).unwrap();
    house.difficulty = HouseDifficulty::Hard;
    house.enemy_house = Some(russians);
    sim.session.game_mode_nonzero = true;
    sim.session.game_options.super_weapons = true;
    sim.resolved_terrain = Some(test_grid(64, 64, test_terrain_cell));
    sim.playfield_bounds = Some(test_playfield_bounds());
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
        .object_alive = false;
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
    cloak.state = 2;
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
    let factories = &mut sim.production.factories;
    assert!(factories.test_enqueue_kernel(russians, ProductionCategory::Vehicle, htnk, 1, 900));
    let factory = factories
        .test_factory_mut(russians, ProductionCategory::Vehicle)
        .unwrap();
    factory.object.as_mut().unwrap().entity_id = Some(built);
    assert_eq!(fact(&sim, built), (false, 2, false), "no rate yet");
    let set = |sim: &mut Simulation, rate, suspended, manual| {
        let factory = sim
            .production
            .factories
            .test_factory_mut(russians, ProductionCategory::Vehicle)
            .unwrap();
        (factory.step_rate_frames, factory.suspended, factory.manual) = (rate, suspended, manual);
    };
    set(&mut sim, 5, false, false);
    assert_eq!(fact(&sim, built), (true, 2, false));
    sim.houses.get_mut(&americans).unwrap().difficulty = HouseDifficulty::Normal;
    assert_eq!(fact(&sim, built), (false, 2, false), "Normal");
    sim.houses.get_mut(&americans).unwrap().difficulty = HouseDifficulty::Hard;
    set(&mut sim, 5, true, false);
    assert_eq!(fact(&sim, built), (false, 2, false), "completed");
    set(&mut sim, 5, false, true);
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
    sim.spawn_object_at_height("NAMISL", "Americans", 10, 10, 0, 0, &rules)
        .expect("the silo stands");
    for at in [(20, 10), (24, 10)] {
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
    instance.activate(9000, sim.session.binary_frame);
    instance.is_ready = true;
    sim.super_weapons
        .entry(americans)
        .or_default()
        .insert(sw_type, instance);

    let mut carrier = false;
    for _ in 0..30 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), None, None, 33);
        if sim
            .projectiles
            .iter()
            .any(|(_, bullet)| sim.interner.resolve(bullet.payload.weapon) == "NukeCarrier")
        {
            carrier = true;
            break;
        }
    }
    assert!(carrier, "the missile flies");
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
        &IniFile::from_str(
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
    rules.install_art_data(ArtRegistry::from_ini(&art));
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
                .insert(id, HouseState::new(id, 0, None, false, 0, 10));
            sim.session.house_order.push(id);
        }
        sim.houses.get_mut(&computer).unwrap().enemy_house =
            (int(&row["enemy_index"]) != -1).then_some(enemy);
        // `+0x5788` bit 2: the computer counts the third house an ally.
        sim.house_alliances
            .entry(COMPUTER.to_ascii_uppercase())
            .or_default()
            .insert(ALLY.to_ascii_uppercase());
        sim.resolved_terrain = Some(test_grid(64, 64, test_terrain_cell));
        sim.playfield_bounds = Some(playfield(&oracle));
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
                    .exact_z_leptons = Some(4 * crate::util::lepton::LEPTONS_PER_LEVEL as i32);
            }
            let entity = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                air_movement_high(entity, &sim, &rules),
                fact("high"),
                "{object} high-flying"
            );
            if apart {
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                entity.position.rx = x;
                entity.position.ry = y;
                sim.substrate.occupancy.remove(at.0, at.1, id);
                rebuilt.push((x, y));
            }
            if fact("curtain") {
                let frame = sim.session.binary_frame;
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
                sim.substrate.entities.get_mut(id).unwrap().on_bridge = true;
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
                    entity.sub_cell,
                    crate::sim::occupancy::CellListInsertion::from_category(entity.category),
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
    }
}
