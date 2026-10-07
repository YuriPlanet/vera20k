//! Replay of every row of `tools/ai_strategy_oracle.json` against the
//! Strategy owner, then unit tests of the anger nodes.
//!
//! The AI_TryFireSW call is compared (its body has its own replay,
//! `superweapon/ai_fire_tests.rs`). Not compared, as residuals (module doc):
//! the Check_Build_Need and Manage_Build_Queue calls, All_To_Hunt's team
//! removal and its garrison release. The money queries read the credits;
//! every cell the search looks up is compared as the house's base origin.

use super::*;
use std::collections::BTreeSet;

use crate::rules::ini_parser::IniFile;
use crate::sim::components::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::rng::SimRng;
use crate::sim::timer::CdTimer;
use serde_json::Value;

fn oracle() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/ai_strategy_oracle.json")).unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn flag(value: &Value) -> bool {
    value.as_bool().unwrap()
}

fn cell(value: &Value) -> (u16, u16) {
    (int(&value[0]) as u16, int(&value[1]) as u16)
}

fn events<'a>(row: &'a Value, kind: &str) -> Vec<&'a Value> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event[0] == kind)
        .collect()
}

/// A factory (`Factory=`), a plain building and one Foot type of each kind;
/// both buildings sell (a Buildup control).
fn rules() -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=FOOT\n[VehicleTypes]\n0=WHEELS\n[AircraftTypes]\n0=WINGS\n\
         [BuildingTypes]\n0=FACTORY\n1=PLAIN\n\
         [FACTORY]\nFactory=UnitType\n[PLAIN]\n[FOOT]\n[WHEELS]\n[WINGS]\n",
    ))
    .unwrap();
    for building in ["FACTORY", "PLAIN"] {
        rules.set_buildup_control_for_test(building, [0, 17, 3]);
    }
    rules
}

/// Computer houses `H0..` in HouseClass::Array order.
fn houses(game_mode_nonzero: bool, count: usize) -> (Simulation, Vec<InternedId>) {
    let mut sim = Simulation::new();
    sim.session.game_mode_nonzero = game_mode_nonzero;
    let names: Vec<InternedId> = (0..count)
        .map(|index| sim.interner.intern(&format!("H{index}")))
        .collect();
    for &name in &names {
        sim.houses
            .insert(name, HouseState::new(name, 0, None, false, 0, 10));
    }
    sim.session.house_order = names.clone();
    (sim, names)
}

/// An object on the map, out of limbo, after every earlier one.
fn spawn(
    sim: &mut Simulation,
    owner: InternedId,
    type_id: &str,
    category: EntityCategory,
    health: i32,
) -> u64 {
    let id = sim.substrate.next_stable_object_id;
    let type_ref = sim.interner.intern(type_id);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        10,
        10,
        0,
        0,
        owner,
        Health { current: health },
        type_ref,
        category,
        0,
        5,
        false,
    );
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    sim.substrate.entities.insert(entity);
    sim.substrate.next_stable_object_id = id + 1;
    id
}

/// A building in `owner`'s list (House+0x68).
fn spawn_building(
    sim: &mut Simulation,
    owner: InternedId,
    type_id: &str,
    health: i32,
    limbo: bool,
) -> u64 {
    let id = spawn(sim, owner, type_id, EntityCategory::Structure, health);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .lifecycle
        .in_limbo = limbo;
    sim.append_house_base_building_for_test(id);
    id
}

fn selling(sim: &Simulation, id: u64) -> bool {
    sim.substrate.entities.get(id).unwrap().building_down()
}

#[test]
fn the_strategy_timer_and_its_gates_match_native() {
    let rules = rules();
    let rows = oracle()["schedule"].as_array().unwrap().clone();
    for row in &rows {
        let (mut sim, names) = houses(int(&row["game_mode"]) != 0, 1);
        let owner = names[0];
        let house = sim.houses.get_mut(&owner).unwrap();
        house.is_human = flag(&row["human"]);
        house.player_control = flag(&row["control"]);
        house.multiplay_passive = flag(&row["passive"]);
        house.strategy_timer = CdTimer::from_raw(int(&row["start"]), int(&row["delay"]));
        // A live factory and cash keep Strategy to its draw.
        house.economy.credits = 1000;
        spawn_building(&mut sim, owner, "FACTORY", 1000, false);
        sim.session.binary_frame = int(&row["frame"]) as u32;
        sim.scenario_rng = SimRng::answering(1, 7, int(&row["answer"]) - 105);
        let mut expected = sim.scenario_rng.clone();
        if flag(&row["called"]) {
            expected.next_range_i32_inclusive(1, 7);
        }

        update_strategy(&mut sim, &rules, owner, None);

        let timer = sim.houses[&owner].strategy_timer;
        assert_eq!(
            (timer.start_frame(), timer.duration()),
            (int(&row["start_after"]), int(&row["delay_after"])),
            "{row}"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected.logical_state(),
            "{row}"
        );
    }
    assert_eq!(rows.len(), 22);
}

#[test]
fn the_strategy_matches_native() {
    let mut rules = rules();
    let rows = oracle()["strategy"].as_array().unwrap().clone();
    for row in &rows {
        let label = &row["label"];
        rules.general.iq_super_weapons = int(&row["iq_superweapons"]);
        let peers = row["peers"].as_array().unwrap();
        let (mut sim, names) = houses(int(&row["game_mode"]) != 0, peers.len() + 1);
        let owner = names[int(&row["self_index"]) as usize];
        let others: Vec<InternedId> = names
            .iter()
            .copied()
            .filter(|&name| name != owner)
            .collect();
        let mut allies = BTreeSet::new();
        for (&name, peer) in others.iter().zip(peers) {
            let house = sim.houses.get_mut(&name).unwrap();
            house.multiplay_passive = flag(&peer["passive"]);
            house.is_defeated = flag(&peer["defeated"]);
            if flag(&peer["ally"]) {
                allies.insert(sim.interner.resolve(name).to_string());
            }
            let anger = int(&peer["anger"]);
            if anger != 0 {
                let owner_house = sim.houses.get_mut(&owner).unwrap();
                owner_house.grudge_scores.insert(name, anger);
            }
        }
        let owner_name = sim.interner.resolve(owner).to_string();
        sim.house_alliances.insert(owner_name, allies);
        let house = sim.houses.get_mut(&owner).unwrap();
        house.multiplay_passive = flag(&row["self_passive"]);
        house.enemy_house = usize::try_from(int(&row["enemy"]))
            .ok()
            .map(|index| names[index]);
        house.base_center = Some(cell(&row["centre"]));
        house.alternate_base_center = cell(&row["alternate"]);
        house.strategy_emergency.mode = int(&row["mode"]);
        house.strategy_emergency.last_building_attack_frame = int(&row["attack_frame"]);
        house.economy.credits = int(&row["money"]);
        house.current_iq = int(&row["iq"]);
        // VERA's list holds no null item; a native null is skipped by both
        // walks.
        let mut sellable = Vec::new();
        let mut counted = 0;
        for building in row["buildings"].as_array().unwrap() {
            if building.is_null() {
                continue;
            }
            let (alive, limbo) = (flag(&building["alive"]), flag(&building["limbo"]));
            let type_id = if flag(&building["factory"]) {
                "FACTORY"
            } else {
                "PLAIN"
            };
            let id = spawn_building(
                &mut sim,
                owner,
                type_id,
                if alive { 1000 } else { 0 },
                limbo,
            );
            counted += 1;
            if alive && !limbo {
                sellable.push(id);
            }
        }
        let house = sim.houses.get_mut(&owner).unwrap();
        house.tracking.set_buildings_for_test(counted);
        sim.session.binary_frame = int(&row["frame"]) as u32;
        let draws = events(row, "draw");
        assert_eq!(draws.len(), 1, "{label}");
        assert_eq!(
            [int(&draws[0][1]), int(&draws[0][2]), int(&draws[0][3])],
            [0x218, 1, 7],
            "{label}"
        );
        sim.scenario_rng = SimRng::answering(1, 7, int(&draws[0][4]));
        let mut expected = sim.scenario_rng.clone();
        expected.next_range_i32_inclusive(1, 7);

        use crate::sim::superweapon::ai_fire::{AI_FIRE_LOG, AiFireEvent};
        AI_FIRE_LOG.set(Some(Vec::new()));
        let delay = building_strategy(&mut sim, &rules, owner, None);
        let tried = AI_FIRE_LOG
            .take()
            .unwrap()
            .into_iter()
            .filter(|event| *event == AiFireEvent::TryFire)
            .count();
        assert_eq!(
            tried,
            events(row, "try_fire_sw").len(),
            "{label}: AI_TryFireSW"
        );

        let house = &sim.houses[&owner];
        assert_eq!(delay, int(&row["delay"]), "{label}");
        assert_eq!(
            house.strategy_emergency.mode,
            int(&row["mode_after"]),
            "{label}"
        );
        assert_eq!(
            house.enemy_house,
            usize::try_from(int(&row["enemy_after"]))
                .ok()
                .map(|index| names[index]),
            "{label}"
        );
        let anger: Vec<i32> = others
            .iter()
            .map(|name| house.grudge_scores.get(name).copied().unwrap_or(0))
            .collect();
        let native_anger: Vec<i32> = row["anger_after"]
            .as_array()
            .unwrap()
            .iter()
            .map(int)
            .collect();
        assert_eq!(anger, native_anger, "{label}");
        for event in events(row, "cell") {
            assert_eq!(
                house.base_origin(),
                (int(&event[1]) as u16, int(&event[2]) as u16),
                "{label}"
            );
        }
        assert_eq!(
            house.strategy_emergency.all_to_hunt_bias,
            !events(row, "all_to_hunt").is_empty(),
            "{label}"
        );
        let sold = !events(row, "fire_sale").is_empty();
        for &id in &sellable {
            assert_eq!(selling(&sim, id), sold, "{label}");
        }
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected.logical_state(),
            "{label}"
        );
    }
    assert_eq!(rows.len(), 188);
}

#[test]
fn the_fire_sale_matches_native() {
    let rules = rules();
    for row in oracle()["fire_sale"].as_array().unwrap() {
        let (mut sim, names) = houses(true, 1);
        let owner = names[0];
        let slots: Vec<Option<u64>> = row["buildings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                (!entry.is_null()).then(|| {
                    spawn_building(&mut sim, owner, "PLAIN", int(&entry[1]), flag(&entry[0]))
                })
            })
            .collect();
        let house = sim.houses.get_mut(&owner).unwrap();
        house.tracking.set_buildings_for_test(int(&row["current"]));

        fire_sale(&mut sim, &rules, owner, None);

        let sold: Vec<i32> = (0..slots.len())
            .filter(|&slot| slots[slot].is_some_and(|id| selling(&sim, id)))
            .map(|slot| slot as i32)
            .collect();
        let native: Vec<i32> = events(row, "sell")
            .iter()
            .map(|event| {
                assert_eq!(int(&event[2]), 1, "the computer's Sell_Back");
                int(&event[1])
            })
            .collect();
        assert_eq!(sold, native, "{}", row["label"]);
    }
}

/// All_To_Hunt against the native rows: who queues Hunt, and which object
/// the Dominator holds takes its `Strength=` as `C4Warhead=` damage instead
/// (`ReceiveDamage(&Strength, 0, C4Warhead, NULL, 1, 1, NULL)`). Each slot's
/// type carries the row's `Strength=` and `Insignificant=`; the C4 warhead's
/// halved verses show that the damage ignores defences.
#[test]
fn all_to_hunt_matches_native() {
    for row in oracle()["all_to_hunt"].as_array().unwrap() {
        let label = &row["label"];
        let technos = row["technos"].as_array().unwrap();
        let mut ini = String::from(
            "[CombatDamage]\nC4Warhead=C4WH\n\
             [C4WH]\nVerses=50%,50%,50%,50%,50%,50%,50%,50%,50%,50%,50%\n",
        );
        let mut registries: [(&str, Vec<String>); 4] = [
            ("InfantryTypes", Vec::new()),
            ("VehicleTypes", Vec::new()),
            ("AircraftTypes", Vec::new()),
            ("BuildingTypes", Vec::new()),
        ];
        for (slot, techno) in technos.iter().enumerate() {
            let registry = if flag(&techno["foot"]) { slot % 3 } else { 3 };
            registries[registry].1.push(format!("T{slot}"));
            ini += &format!(
                "[T{slot}]\nStrength={}\nInsignificant={}\n",
                int(&techno["strength"]),
                if flag(&techno["insignificant"]) {
                    "yes"
                } else {
                    "no"
                }
            );
        }
        for (section, types) in &registries {
            ini += &format!("[{section}]\n");
            for (index, name) in types.iter().enumerate() {
                ini += &format!("{index}={name}\n");
            }
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let (mut sim, names) = houses(true, 2);
        sim.resolve_type_handles(&rules);
        let (owner, other) = (names[0], names[1]);
        sim.houses.get_mut(&owner).unwrap().is_human = flag(&row["human"]);
        let mut slots = Vec::new();
        for (slot, techno) in technos.iter().enumerate() {
            // A Techno neither Foot nor Building is beyond VERA's objects.
            let foot = flag(&techno["foot"]);
            if !foot && int(&techno["kind"]) != 6 {
                continue;
            }
            let techno_owner = if flag(&techno["owner"]) { owner } else { other };
            // Native tests the Foot flag, not the kind: each Foot kind takes
            // its turn.
            let category = match (foot, slot % 3) {
                (false, _) => EntityCategory::Structure,
                (true, 0) => EntityCategory::Infantry,
                (true, 1) => EntityCategory::Unit,
                (true, _) => EntityCategory::Aircraft,
            };
            // One point more than the damage, so a hit leaves it standing.
            let health = int(&techno["strength"]) + 1;
            let id = spawn(
                &mut sim,
                techno_owner,
                &format!("T{slot}"),
                category,
                health,
            );
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.lifecycle.cell_marked = flag(&techno["down"]);
            entity.lifecycle.in_limbo = flag(&techno["limbo"]);
            if flag(&techno["permanent"]) {
                entity.mind_control =
                    crate::sim::capture_manager::MindControlLink::permanent_for_test();
            }
            slots.push((slot as i32, id, health));
        }

        all_to_hunt(&mut sim, &rules, owner, None);

        let hunting: Vec<i32> = slots
            .iter()
            .filter(|&&(_, id, _)| {
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .mission
                    .queued()
                    .known()
                    == Some(MissionType::Hunt)
            })
            .map(|&(slot, _, _)| slot)
            .collect();
        let expressible: Vec<i32> = slots.iter().map(|&(slot, _, _)| slot).collect();
        let mut native: Vec<i32> = events(row, "mission")
            .iter()
            .map(|event| {
                assert_eq!([int(&event[2]), int(&event[3])], [15, 0], "{label}");
                int(&event[1])
            })
            .filter(|slot| expressible.contains(slot))
            .collect();
        native.sort_unstable();
        assert_eq!(hunting, native, "{label}");

        let damaged: Vec<(i32, i32)> = slots
            .iter()
            .filter_map(|&(slot, id, health)| {
                let now = sim.substrate.entities.get(id).unwrap().health.current;
                (now != health).then_some((slot, health - now))
            })
            .collect();
        let mut native: Vec<(i32, i32)> = events(row, "damage")
            .iter()
            .map(|event| {
                // Distance 0, no attacker, IgnoreDefenses, the passengers kept
                // in, no attacking house; the warhead is the Rules' C4Warhead
                // (`+0xFA8`).
                assert_eq!(
                    [5, 6, 7, 8].map(|index| int(&event[index])),
                    [0, 1, 1, 0],
                    "{label}"
                );
                assert_eq!(int(&event[3]), 0, "{label}");
                assert_eq!(int(&event[4]), int(&row["c4_warhead"]), "{label}");
                (int(&event[1]), int(&event[2]))
            })
            .collect();
        native.sort_unstable();
        assert_eq!(damaged, native, "{label}");
        assert_eq!(
            sim.houses[&owner].strategy_emergency.all_to_hunt_bias,
            flag(&row["latch"]),
            "{label}"
        );
    }
}

fn state(mode: i32, last_attack: i32) -> HouseStrategyEmergencyState {
    HouseStrategyEmergencyState {
        mode,
        all_to_hunt_bias: false,
        last_building_attack_frame: last_attack,
        last_attacker_house_index: -1,
    }
}

fn house(name: InternedId, side: u8) -> HouseState {
    HouseState::new(name, side, None, false, 0, 10)
}

#[test]
fn gsi_04_05_update_anger_nodes_wraps_and_selects_first_positive_eligible_peer() {
    let mut interner = StringInterner::new();
    let owner = interner.intern("OWNER");
    let allied = interner.intern("ALLIED");
    let defeated = interner.intern("DEFEATED");
    let missing = interner.intern("MISSING");
    let second = interner.intern("SECOND");
    let first = interner.intern("FIRST");
    let unregistered = interner.intern("UNREGISTERED");
    let house_order = [owner, allied, defeated, missing, first, second];
    assert!(
        second < first,
        "fixture intern order must oppose the equal-score House order"
    );
    let mut defeated_house = house(defeated, 2);
    defeated_house.is_defeated = true;
    let mut houses = BTreeMap::from([
        (owner, house(owner, 0)),
        (allied, house(allied, 1)),
        (defeated, defeated_house),
        (first, house(first, 3)),
        (second, house(second, 4)),
    ]);
    let mut alliances = HouseAllianceMap::new();
    alliances.insert("OWNER".to_string(), BTreeSet::from(["ALLIED".to_string()]));
    {
        let anger = &mut houses.get_mut(&owner).unwrap().grudge_scores;
        anger.insert(allied, 99);
        anger.insert(defeated, 98);
        anger.insert(missing, 97);
        anger.insert(first, 7);
        anger.insert(second, 7);
    }

    update_anger_nodes(
        &mut houses,
        &house_order,
        &alliances,
        &interner,
        owner,
        first,
        0,
    );
    assert_eq!(houses[&owner].enemy_house, Some(first));

    houses.get_mut(&owner).unwrap().enemy_house = None;
    let missing_score = houses[&owner].grudge_scores[&missing];
    update_anger_nodes(
        &mut houses,
        &house_order,
        &alliances,
        &interner,
        owner,
        missing,
        5,
    );
    assert_eq!(houses[&owner].grudge_scores[&missing], missing_score);
    assert_eq!(
        houses[&owner].enemy_house,
        Some(first),
        "a rejected in-order null peer still triggers the full ordered rescan"
    );

    houses
        .get_mut(&owner)
        .unwrap()
        .grudge_scores
        .insert(first, i32::MAX);
    houses
        .get_mut(&owner)
        .unwrap()
        .grudge_scores
        .insert(second, 1);
    update_anger_nodes(
        &mut houses,
        &house_order,
        &alliances,
        &interner,
        owner,
        first,
        1,
    );
    assert_eq!(houses[&owner].grudge_scores[&first], i32::MIN);
    assert_eq!(houses[&owner].enemy_house, Some(second));

    update_anger_nodes(
        &mut houses,
        &house_order,
        &alliances,
        &interner,
        owner,
        unregistered,
        5,
    );
    assert!(!houses[&owner].grudge_scores.contains_key(&unregistered));
    assert_eq!(houses[&owner].enemy_house, Some(second));
    houses
        .get_mut(&owner)
        .unwrap()
        .grudge_scores
        .insert(second, 0);
    update_anger_nodes(
        &mut houses,
        &house_order,
        &alliances,
        &interner,
        owner,
        owner,
        5,
    );
    assert!(!houses[&owner].grudge_scores.contains_key(&owner));
    assert_eq!(houses[&owner].enemy_house, None);
}

#[test]
fn gsi_04_05_sparse_zero_updates_preserve_representation_and_rescan() {
    let mut interner = StringInterner::new();
    let owner = interner.intern("OWNER");
    let first = interner.intern("FIRST");
    let second = interner.intern("SECOND");
    let house_order = [owner, first, second];
    let mut houses = BTreeMap::from([
        (owner, house(owner, 0)),
        (first, house(first, 1)),
        (second, house(second, 2)),
    ]);
    houses
        .get_mut(&owner)
        .unwrap()
        .grudge_scores
        .insert(second, 5);
    houses.get_mut(&owner).unwrap().enemy_house = Some(first);

    update_anger_nodes(
        &mut houses,
        &house_order,
        &HouseAllianceMap::new(),
        &interner,
        owner,
        first,
        0,
    );
    assert!(!houses[&owner].grudge_scores.contains_key(&first));
    assert_eq!(houses[&owner].enemy_house, Some(second));

    houses
        .get_mut(&owner)
        .unwrap()
        .grudge_scores
        .insert(first, 5);
    houses.get_mut(&owner).unwrap().enemy_house = None;
    update_anger_nodes(
        &mut houses,
        &house_order,
        &HouseAllianceMap::new(),
        &interner,
        owner,
        first,
        -5,
    );
    assert_eq!(houses[&owner].grudge_scores.get(&first), Some(&0));
    assert_eq!(houses[&owner].enemy_house, Some(second));
}

#[test]
fn gsi_04_05_anger_decay_uses_signed_frame_boundaries_and_strict_score_gate() {
    let mut interner = StringInterner::new();
    let owner = interner.intern("OWNER");
    let minimum = interner.intern("MINIMUM");
    let zero = interner.intern("ZERO");
    let one = interner.intern("ONE");
    let two = interner.intern("TWO");
    let house_order = [owner, minimum, zero, one, two];
    let mut base = house(owner, 0);
    base.grudge_scores.insert(minimum, i32::MIN);
    base.grudge_scores.insert(zero, 0);
    base.grudge_scores.insert(one, 1);
    base.grudge_scores.insert(two, 2);

    for frame in [99, 101] {
        let mut unchanged = base.clone();
        decay_anger_scores(&mut unchanged, &house_order, frame);
        assert_eq!(unchanged.grudge_scores, base.grudge_scores);
    }
    for frame in [100, -100, 0] {
        let mut decayed = base.clone();
        decay_anger_scores(&mut decayed, &house_order, frame);
        assert_eq!(decayed.grudge_scores[&minimum], i32::MIN);
        assert_eq!(decayed.grudge_scores[&zero], 0);
        assert_eq!(decayed.grudge_scores[&one], 1);
        assert_eq!(decayed.grudge_scores[&two], 1);
    }
}

#[test]
fn gsi_04_05_anger_decay_does_not_reselect_enemy() {
    let mut interner = StringInterner::new();
    let owner = interner.intern("OWNER");
    let selected = interner.intern("SELECTED");
    let stronger = interner.intern("STRONGER");
    let house_order = [owner, selected, stronger];
    let mut owner_house = house(owner, 0);
    owner_house.grudge_scores.insert(selected, 2);
    owner_house.grudge_scores.insert(stronger, 5);
    owner_house.enemy_house = Some(selected);

    decay_anger_scores(&mut owner_house, &house_order, 100);

    assert_eq!(owner_house.grudge_scores[&selected], 1);
    assert_eq!(owner_house.grudge_scores[&stronger], 4);
    assert_eq!(owner_house.enemy_house, Some(selected));
}

#[test]
fn all_to_hunt_override_is_persistent_and_follows_designated_enemy() {
    let owner = InternedId::from_index(1);
    let first_enemy = InternedId::from_index(2);
    let second_enemy = InternedId::from_index(3);
    let bystander = InternedId::from_index(4);
    let mut house = HouseState::new(owner, 0, None, false, 0, 10);

    house.enemy_house = Some(first_enemy);
    assert_eq!(all_to_hunt_score_override(&house, bystander), None);

    house.strategy_emergency.set_all_to_hunt_bias();
    assert_eq!(all_to_hunt_score_override(&house, first_enemy), None);
    assert_eq!(all_to_hunt_score_override(&house, bystander), Some(1));

    house.enemy_house = Some(second_enemy);
    assert_eq!(all_to_hunt_score_override(&house, first_enemy), Some(1));
    assert_eq!(all_to_hunt_score_override(&house, second_enemy), None);

    house.enemy_house = None;
    assert_eq!(all_to_hunt_score_override(&house, bystander), None);
    assert!(house.strategy_emergency.all_to_hunt_bias());
}

#[test]
fn native_entry_writers_change_only_their_owned_fields() {
    let mut emergency = state(-7, 123);
    emergency.set_state_four();
    assert_eq!(emergency.mode(), 4);
    assert_eq!(emergency.last_building_attack_frame(), 123);
    assert!(!emergency.all_to_hunt_bias());

    emergency.note_building_attack(-55);
    assert_eq!(emergency.last_building_attack_frame(), -55);
    assert_eq!(emergency.mode(), 4);
}

#[test]
fn non_bincode_missing_house_field_uses_native_constructor_defaults() {
    let owner = InternedId::from_index(1);
    let house = HouseState::new(owner, 0, None, false, 0, 10);
    let mut value = serde_json::to_value(house).expect("HouseState serializes to JSON");
    let object = value.as_object_mut().expect("HouseState JSON is an object");
    object.remove("strategy_emergency");
    object.remove("strategy_timer");

    let restored: HouseState =
        serde_json::from_value(value).expect("serde default fills the absent field");
    assert_eq!(
        restored.strategy_emergency,
        HouseStrategyEmergencyState::default()
    );
    assert_eq!(restored.strategy_emergency.mode(), 0);
    assert!(!restored.strategy_emergency.all_to_hunt_bias());
    assert_eq!(restored.strategy_emergency.last_building_attack_frame(), 0);
    assert_eq!(
        restored.strategy_timer,
        crate::sim::house_state::strategy_timer_at_construction()
    );
}
