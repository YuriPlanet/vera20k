//! Tests for the house defeat. `native_defeat_gate_corpus` and
//! `native_blowup_all_corpus` compare against
//! `tools/spatial_oracle/house_defeat_gate.json` and `house_blowup_all.json`,
//! produced by running the original code under Unicorn; the rest are Rust
//! regression tests of the production pass.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::capture_manager::{CaptureManagerState, MindControlLink};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// `[Sides]` with `Civilian` at `civilian_side` (absent past the list, as
/// when no house has that side).
fn rules(civilian_side: usize) -> RuleSet {
    let mut sides = String::from("[Sides]\n");
    for index in 0..4 {
        if index == civilian_side {
            sides.push_str("Civilian=Neutral\n");
        } else {
            sides.push_str(&format!("Side{index}=Country{index}\n"));
        }
    }
    let text = format!(
        "{sides}\
[General]
BaseUnit=AMCV,SMCV,PCV
[AI]
BuildRefinery=NAREFN,GAREFN,YAREFN
[CombatDamage]
C4Warhead=Super
[InfantryTypes]
0=CLEG
[VehicleTypes]
0=TANK
1=AMCV
2=SMCV
3=PCV
[AircraftTypes]
[BuildingTypes]
0=NAREFN
1=GAREFN
2=YAREFN
[CLEG]
Strength=125
Primary=NeutronRifle
[TANK]
Strength=1000
Armor=heavy
[AMCV]
Strength=1000
[SMCV]
Strength=1000
[PCV]
Strength=1000
[NAREFN]
Strength=1000
[GAREFN]
Strength=1000
[YAREFN]
Strength=1000
[NeutronRifle]
Damage=8
Projectile=Invisible
Warhead=ChronoBeam
[Invisible]
Inviso=yes
[Warheads]
0=Super
1=ChronoBeam
[Super]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[ChronoBeam]
Temporal=yes
"
    );
    RuleSet::from_ini(&IniFile::from_str(&text)).expect("house defeat rules")
}

/// Houses `house0..` with the given side indices, in that array order, in a
/// non-campaign game past frame zero.
fn sim_with_houses(sides: &[u8]) -> (Simulation, Vec<InternedId>) {
    let mut sim = Simulation::with_seed(5);
    sim.session.game_mode_nonzero = true;
    sim.session.binary_frame = 100;
    let houses = sides
        .iter()
        .enumerate()
        .map(|(n, &side)| {
            let id = sim.interner.intern(&format!("house{n}"));
            sim.houses
                .insert(id, HouseState::new(id, side, None, false, 0, 10));
            sim.session.house_order.push(id);
            id
        })
        .collect();
    (sim, houses)
}

#[derive(serde::Deserialize)]
struct NativeGateCase {
    input: serde_json::Value,
    events: Vec<String>,
}

/// `HouseClass::Update`'s gate (`0x004F8E86..0x004F8F82`) against the
/// original, case by case: the campaign, Defeated, frame and
/// MultiplayPassive refusals, the short game's `+0x2F0 > 0` and BaseUnit
/// entries 1, 2 and 0 (a negative building count defeats, cancelling MCV
/// counts defeat, on-map units do not count), and the normal game's zero sum
/// (a cancelling sum defeats; BuildRefinery's third type counts only when
/// set).
#[test]
fn native_defeat_gate_corpus() {
    let cases: Vec<NativeGateCase> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/house_defeat_gate.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 21);
    for case in cases {
        let input = &case.input;
        let name = input["name"].as_str().unwrap();
        let int = |key: &str| input[key].as_i64().unwrap_or(0) as i32;
        let mut rules = rules(3);
        if input["build_refinery_2"].as_bool() == Some(false) {
            rules.build_refinery_types.truncate(2);
        }
        let (mut sim, houses) = sim_with_houses(&[0]);
        let house = houses[0];
        sim.session.game_mode_nonzero = input["game_mode"].as_i64().unwrap_or(1) != 0;
        sim.session.binary_frame = input["frame"].as_i64().unwrap_or(100) as u32;
        sim.session.game_options.short_game = input["short_game"].as_i64().unwrap_or(1) != 0;
        let types = ["AMCV", "SMCV", "PCV"].map(|name| sim.interner.intern(name));
        let yarefn = sim.interner.intern("YAREFN");
        let base_units: Vec<(InternedId, i32)> = input["base_units"]
            .as_object()
            .map(|slots| {
                slots
                    .iter()
                    .map(|(slot, n)| {
                        (
                            types[slot.parse::<usize>().unwrap()],
                            n.as_i64().unwrap() as i32,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        {
            let state = sim.houses.get_mut(&house).unwrap();
            state.is_defeated = int("defeated") != 0;
            state.multiplay_passive = int("passive") != 0;
            state.tracking.set_for_test(
                int("buildings"),
                &base_units,
                (
                    int("active_units"),
                    int("active_infantry"),
                    int("active_aircraft"),
                ),
                &[(yarefn, int("active_yarefn"))],
            );
        }
        let was_defeated = sim.houses[&house].is_defeated;

        sim.check_defeat(Some(&rules), None);

        let native_defeats = case.events == ["blowup_all", "mplayer_defeated"];
        assert!(
            native_defeats || case.events.is_empty(),
            "{name}: {:?}",
            case.events
        );
        assert_eq!(
            !was_defeated && sim.houses[&house].is_defeated,
            native_defeats,
            "{name}"
        );
    }
}

#[derive(serde::Deserialize)]
struct NativeBlowupCase {
    input: serde_json::Value,
    events: Vec<serde_json::Value>,
    original_owners: std::collections::BTreeMap<String, Vec<String>>,
}

/// `HouseClass::Blowup_All @ 0x004FC6D0` against the original, case by case
/// (with GetOriginalOwner, the CaptureManager node scan and
/// SetOriginalOwnerToCivilian executed): who dies, in array order, each to
/// its own Health as C4 damage; a captive of another house handed to the
/// Civilian-side house, or killed when none exists; our own captive spared;
/// a warped object released first. The trigger-held original owner
/// (`trigger_owned_elsewhere_dies`) has no VERA state and is skipped; VERA's
/// deferred deletion never shifts the array, so the removal case compares
/// its victims only.
#[test]
fn native_blowup_all_corpus() {
    let cases: Vec<NativeBlowupCase> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/house_blowup_all.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 10);
    for case in cases {
        let input = &case.input;
        let name = input["name"].as_str().unwrap();
        if name == "trigger_owned_elsewhere_dies" {
            continue;
        }
        let sides: Vec<u8> = input["houses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|side| side.as_u64().unwrap() as u8)
            .collect();
        let civilian_side = input["civilian_side"].as_u64().unwrap_or(99) as usize;
        let rules = rules(civilian_side);
        let (mut sim, houses) = sim_with_houses(&sides);
        let objects = input["objects"].as_array().unwrap();
        let ids: Vec<u64> = objects
            .iter()
            .enumerate()
            .map(|(n, object)| {
                let owner = houses[object["owner"].as_u64().unwrap() as usize];
                let owner_name = sim.interner.resolve(owner).to_string();
                let id = 100 + n as u64;
                let mut entity =
                    GameEntity::test_default(id, "TANK", &owner_name, 10 + n as u16, 10);
                entity.owner = owner;
                entity.type_ref = sim.interner.intern("TANK");
                entity.category = EntityCategory::Unit;
                entity.health.current = object["health"].as_i64().unwrap_or(100) as i32;
                sim.substrate.entities.insert(entity);
                id
            })
            .collect();
        if let Some(controllers) = input["controllers"].as_object() {
            for (controller, nodes) in controllers {
                let controller = ids[controller.parse::<usize>().unwrap()];
                let nodes: Vec<(u64, InternedId)> = nodes
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|node| {
                        (
                            ids[node[0].as_u64().unwrap() as usize],
                            houses[node[1].as_u64().unwrap() as usize],
                        )
                    })
                    .collect();
                for &(victim, _) in &nodes {
                    sim.substrate.entities.get_mut(victim).unwrap().mind_control =
                        MindControlLink::controlled_by_for_test(controller);
                }
                sim.substrate
                    .entities
                    .get_mut(controller)
                    .unwrap()
                    .capture_manager = Some(CaptureManagerState::with_nodes_for_test(&nodes));
            }
        }
        let mut warpers = Vec::new();
        for (n, object) in objects.iter().enumerate() {
            if object["warped"].as_bool() == Some(true) {
                let cleg = sim
                    .spawn_object_at_height("CLEG", "house1", 2, 2 + n as u16, 0, 0, &rules)
                    .unwrap();
                sim.temporal_initiate_warp(cleg, Some(ids[n]), &rules);
                assert!(sim.substrate.entities.get(ids[n]).unwrap().is_warped_out());
                warpers.push((ids[n], cleg));
            }
        }
        let health_before: Vec<i32> = ids
            .iter()
            .map(|&id| sim.substrate.entities.get(id).unwrap().health.current)
            .collect();
        let blown = houses[input["house"].as_u64().unwrap() as usize];

        sim.house_blowup_all(blown, &rules, None);

        let native_victims: Vec<usize> = case
            .events
            .iter()
            .filter(|event| event[0] == "receive_damage")
            .map(|event| {
                // ReceiveDamage(&Health, 0, C4Warhead, null, 1, 1, null).
                assert_eq!(event[4], "c4", "{name}: the warhead");
                assert_eq!(
                    (event[5].as_u64(), event[6].as_u64(), event[7].as_u64()),
                    (Some(0), Some(1), Some(1)),
                    "{name}: no attacker, ignoreDefenses, no escape"
                );
                let object = event[1].as_str().unwrap();
                let n = object["obj".len()..].parse::<usize>().unwrap();
                assert_eq!(
                    event[2].as_i64(),
                    Some(i64::from(health_before[n])),
                    "{name}: its own Health"
                );
                n
            })
            .collect();
        for (n, &id) in ids.iter().enumerate() {
            let entity = sim.substrate.entities.get(id).unwrap();
            let hit = native_victims.contains(&n);
            assert_eq!(
                entity.health.current == 0,
                hit || health_before[n] == 0,
                "{name}: obj{n} dies"
            );
            if hit {
                assert!(entity.killed_by.is_none(), "{name}: no kill credit");
            }
        }
        for (target, _) in &warpers {
            assert!(
                !sim.substrate.entities.get(*target).unwrap().is_warped_out(),
                "{name}: the chain let go"
            );
        }
        // The oracle stubs ReceiveDamage, so its nodes outlive the deaths; in
        // VERA a dead controller frees its captives and a dead captive's node
        // expires. Compare the nodes of the survivors.
        for (controller, native_owners) in &case.original_owners {
            let n = controller.parse::<usize>().unwrap();
            if native_victims.contains(&n) {
                continue;
            }
            let nodes = input["controllers"][controller].as_array().unwrap();
            let manager = sim
                .substrate
                .entities
                .get(ids[n])
                .and_then(|entity| entity.capture_manager.as_ref())
                .expect("a living controller keeps its manager");
            for (node, native_owner) in nodes.iter().zip(native_owners) {
                let victim = node[0].as_u64().unwrap() as usize;
                if native_victims.contains(&victim) {
                    continue;
                }
                let owner = manager.original_owner(ids[victim]).unwrap();
                assert_eq!(
                    sim.interner.resolve(owner),
                    native_owner,
                    "{name}: obj{victim}'s original owner"
                );
            }
        }
    }
}

/// In production, a short game's defeat follows the last counted building's
/// deletion: the next frame's house pass blows up the rest of the army, with
/// no kill credit, and then defeats the house.
#[test]
fn losing_the_last_building_blows_up_the_army_next_frame() {
    let rules = rules(3);
    let (mut sim, houses) = sim_with_houses(&[0, 1]);
    sim.session.game_options.short_game = true;
    let refinery = sim
        .spawn_object_at_height("GAREFN", "house0", 10, 10, 0, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object_at_height("TANK", "house0", 20, 20, 0, 0, &rules)
        .unwrap();
    let enemy_base = sim
        .spawn_object_at_height("NAREFN", "house1", 30, 30, 0, 0, &rules)
        .unwrap();
    assert_eq!(sim.houses[&houses[0]].tracking.buildings_for_test(), 1);
    let step = |sim: &mut Simulation| {
        sim.advance_tick(
            &[],
            Some(&rules),
            &std::collections::BTreeMap::new(),
            None,
            None,
            67,
        );
    };
    step(&mut sim);
    assert!(!sim.houses[&houses[0]].is_defeated);

    // The refinery dies; its tracking leaves with the frame-end drain.
    sim.uninit_with_rules(refinery, &rules);
    assert_eq!(sim.houses[&houses[0]].tracking.buildings_for_test(), 1);
    step(&mut sim);
    assert_eq!(sim.houses[&houses[0]].tracking.buildings_for_test(), 0);
    assert!(
        !sim.houses[&houses[0]].is_defeated,
        "counted until its deletion"
    );
    step(&mut sim);
    assert!(sim.houses[&houses[0]].is_defeated);
    assert!(sim.houses[&houses[1]].has_won);
    assert!(
        sim.substrate
            .entities
            .get(tank)
            .is_none_or(|entity| entity.health.current == 0),
        "the army is blown up"
    );
    assert_eq!(
        sim.houses[&houses[1]].stats.units_killed, 0,
        "no house is credited"
    );
    assert!(sim.substrate.entities.get(enemy_base).is_some());
}
