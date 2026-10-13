//! Tests for the house defeat. `native_defeat_gate_corpus` and
//! `native_blowup_all_corpus` compare against
//! `tools/spatial_oracle/house_defeat_gate.json` and `house_blowup_all.json`,
//! produced by running the original code under Unicorn; the rest are Rust
//! regression tests of the production pass.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::capture_manager::{CaptureManagerState, MindControlLink};
use crate::sim::combat::{RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// `[Sides]` listing country `Country{n}` as side `n`, the one at
/// `civilian_side` named `Civilian` (absent past the list, as when no house
/// has that side).
fn rules(civilian_side: usize) -> RuleSet {
    let mut sides = String::from("[Countries]\n0=Country0\n1=Country1\n2=Country2\n3=Country3\n");
    sides.push_str("[Country0]\n[Country1]\n[Country2]\n[Country3]\n[Sides]\n");
    for index in 0..4 {
        if index == civilian_side {
            sides.push_str(&format!("Civilian=Country{index}\n"));
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

/// Houses `house0..` of type `Country{side}` (side `side` in [`rules`]) for
/// the given side indices, in that array order, in a non-campaign game past
/// frame zero.
fn sim_with_houses(sides: &[u8]) -> (Simulation, Vec<InternedId>) {
    let mut sim = Simulation::with_seed(5);
    sim.session.game_mode_nonzero = true;
    sim.session.binary_frame = 100;
    let houses = sides
        .iter()
        .enumerate()
        .map(|(n, &side)| {
            let id = sim.interner.intern(&format!("house{n}"));
            let country = sim.interner.intern(&format!("Country{side}"));
            sim.houses
                .insert(id, HouseState::new(id, side, Some(country), false, 0, 10));
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
    let cases: Vec<NativeGateCase> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_defeat_gate.json",
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
/// SetOriginalOwnerToCivilian executed; the Civilian side lookup supplied):
/// VERA's own receiver calls, in order, against the native ones (target,
/// damage = its Health, distance, C4Warhead, no attacker, ignoreDefenses,
/// no escape, no source house); a captive of another house handed to the
/// Civilian-side house, or killed when none exists; our own captive spared;
/// a warped object released first. The trigger-held original owner
/// (`trigger_owned_elsewhere_dies`) has no VERA state and is skipped. VERA's
/// deferred deletion never shifts the array; the removal case shows native
/// re-reading the slot, so the call order is the same.
#[test]
fn native_blowup_all_corpus() {
    let cases: Vec<NativeBlowupCase> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_blowup_all.json",
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
                sim.temporal_initiate_warp(
                    cleg,
                    Some(ids[n]),
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                );
                assert!(sim.substrate.entities.get(ids[n]).unwrap().is_warped_out());
                warpers.push((ids[n], cleg));
            }
        }
        let health_before: Vec<i32> = ids
            .iter()
            .map(|&id| sim.substrate.entities.get(id).unwrap().health.current)
            .collect();
        let blown = houses[input["house"].as_u64().unwrap() as usize];
        let c4 = sim.interner.intern("Super");
        let _ = super::take_blowup_trace();

        sim.house_blowup_all(
            blown,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );

        // Native: ReceiveDamage(&damage, distance, warhead, attacker,
        // ignoreDefenses, arg6, sourceHouse), in array order.
        let calls = super::take_blowup_trace();
        let native_calls: Vec<&serde_json::Value> = case
            .events
            .iter()
            .filter(|event| event[0] == "receive_damage")
            .collect();
        assert_eq!(calls.len(), native_calls.len(), "{name}: receiver calls");
        let mut native_victims = Vec::new();
        for (call, event) in calls.iter().zip(&native_calls) {
            let object = event[1].as_str().unwrap();
            let n = object["obj".len()..].parse::<usize>().unwrap();
            native_victims.push(n);
            assert_eq!(event[4], "c4", "{name}: the native warhead");
            assert_eq!(call.target_id, ids[n], "{name}: who, in order");
            assert_eq!(
                Some(i64::from(call.damage)),
                event[2].as_i64(),
                "{name}: obj{n}'s damage"
            );
            assert_eq!(
                Some(i64::from(call.distance_leptons)),
                event[3].as_i64(),
                "{name}: distance"
            );
            assert_eq!(call.warhead_ref, c4, "{name}: C4Warhead");
            assert_eq!(event[5].as_u64(), Some(0), "{name}: no native attacker");
            assert_eq!(call.attacker_id, RAD_NO_ATTACKER, "{name}: no attacker");
            assert_eq!(
                call.receiver_flags,
                ReceiverCallFlags {
                    ignore_defenses: event[6].as_u64() == Some(1),
                    arg6: event[7].as_u64() == Some(1),
                },
                "{name}: ignoreDefenses, no escape"
            );
            assert_eq!(event[8].as_u64(), Some(0), "{name}: no native source house");
            assert_eq!(call.source_house, None, "{name}: no source house");
        }
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
    assert_eq!(sim.houses[&houses[0]].tracking.buildings(), 1);
    let step = |sim: &mut Simulation| {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    };
    step(&mut sim);
    assert!(!sim.houses[&houses[0]].is_defeated);

    // The refinery dies; its tracking leaves with the frame-end drain.
    sim.uninit_with_rules(refinery, &rules);
    assert_eq!(sim.houses[&houses[0]].tracking.buildings(), 1);
    step(&mut sim);
    assert_eq!(sim.houses[&houses[0]].tracking.buildings(), 0);
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
        sim.houses[&houses[1]].stats.units_killed(),
        0,
        "no house is credited"
    );
    assert!(sim.substrate.entities.get(enemy_base).is_some());
}

/// MPlayer_Defeated (`0x004FC0BC..0x004FC10D`): a computer house at
/// `MaxIQLevels`, defeated under `[AI] Paranoid`, turns the computers left
/// paranoid. Computer_Paranoid skips the defeated house.
#[test]
fn a_computer_defeat_turns_the_computers_left_paranoid() {
    let mut rules = rules(3);
    rules.general.allies_allowed = true;
    let (mut sim, houses) = sim_with_houses(&[0, 1, 1, 1]);
    sim.session.game_options.short_game = true;
    sim.houses.get_mut(&houses[0]).unwrap().is_human = true;
    for computer in &houses[1..] {
        sim.houses.get_mut(computer).unwrap().current_iq = rules.general.max_iq_levels;
    }
    // Every house but house3 holds a counted building.
    for (n, refinery) in ["GAREFN", "NAREFN", "NAREFN"].into_iter().enumerate() {
        sim.spawn_object_at_height(
            refinery,
            &format!("house{n}"),
            10 + 10 * n as u16,
            10,
            0,
            0,
            &rules,
        )
        .unwrap();
    }
    for _ in 0..3 {
        if sim.houses[&houses[3]].is_defeated {
            break;
        }
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert!(sim.houses[&houses[3]].is_defeated);
    let allies = |n: usize| {
        sim.house_alliances
            .get(&format!("HOUSE{n}"))
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(
        allies(1),
        std::collections::BTreeSet::from(["HOUSE2".to_string()])
    );
    assert_eq!(
        allies(2),
        std::collections::BTreeSet::from(["HOUSE1".to_string()])
    );
    assert!(allies(0).is_empty() && allies(3).is_empty());
    assert_eq!(sim.houses[&houses[1]].enemy_house, Some(houses[0]));
}

/// Two houses on a bare 40x40 plane in a short game, `house0` the current
/// house; each owns one counted building.
fn two_house_plane(rules: &RuleSet) -> (Simulation, Vec<InternedId>, [u64; 2]) {
    let (mut sim, houses) = sim_with_houses(&[0, 1]);
    sim.fog.width = 40;
    sim.fog.height = 40;
    sim.session.game_options.short_game = true;
    sim.session.current_house = Some(houses[0]);
    let own = sim
        .spawn_object_at_height("GAREFN", "house0", 10, 10, 0, 0, rules)
        .unwrap();
    let enemy = sim
        .spawn_object_at_height("NAREFN", "house1", 30, 30, 0, 0, rules)
        .unwrap();
    (sim, houses, [own, enemy])
}

const CORNERS: [(u16, u16); 4] = [(0, 0), (39, 0), (0, 39), (39, 39)];

/// MPlayer_Defeated's local branch through the production frame: the current
/// house's defeat explores its whole plane (RevealEntireMap), writes its
/// map-clear byte and switches its radar on, which no later House update
/// rechecks (`0x004F84FB`).
#[test]
fn a_local_defeat_reveals_the_whole_map_and_keeps_the_radar_on() {
    let rules = rules(3);
    let (mut sim, houses, [own, _]) = two_house_plane(&rules);
    let step = |sim: &mut Simulation| {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    };
    step(&mut sim);
    for (x, y) in CORNERS {
        assert!(!sim.fog.is_cell_revealed(houses[0], x, y));
    }
    assert!(!crate::sim::radar::has_radar_for_owner(
        &sim, &rules, "house0"
    ));

    sim.uninit_with_rules(own, &rules);
    step(&mut sim);
    step(&mut sim);
    let local = &sim.houses[&houses[0]];
    assert!(local.is_defeated && local.map_is_clear);
    assert!(sim.local_player_defeated());
    assert!(sim.fog.whole_map_revealed_for_owner(houses[0]));
    for (x, y) in CORNERS {
        assert!(sim.fog.is_cell_revealed(houses[0], x, y), "{x},{y}");
        assert!(sim.fog.is_ground_unshrouded(houses[0], x, y), "{x},{y}");
        assert!(!sim.fog.is_cell_revealed(houses[1], x, y), "{x},{y}");
    }
    assert!(!sim.fog.whole_map_revealed_for_owner(houses[1]));
    assert!(crate::sim::radar::has_radar_for_owner(
        &sim, &rules, "house0"
    ));
    sim.power_states
        .get_mut(&houses[0])
        .unwrap()
        .recheck_radar();
    step(&mut sim);
    assert!(
        crate::sim::radar::has_radar_for_owner(&sim, &rules, "house0"),
        "no House rechecks radar after the local defeat"
    );
}

/// Another house's defeat writes its map-clear byte alone
/// (`0x004FC30C..0x004FC328`): no plane is explored and radar rechecks go on.
#[test]
fn another_house_defeat_writes_only_its_map_clear_byte() {
    let rules = rules(3);
    let (mut sim, houses, [_, enemy]) = two_house_plane(&rules);
    let step = |sim: &mut Simulation| {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    };
    step(&mut sim);
    sim.uninit_with_rules(enemy, &rules);
    step(&mut sim);
    step(&mut sim);
    let other = &sim.houses[&houses[1]];
    assert!(other.is_defeated && other.map_is_clear);
    assert!(!sim.local_player_defeated());
    assert!(!sim.houses[&houses[0]].map_is_clear);
    for house in &houses {
        assert!(!sim.fog.whole_map_revealed_for_owner(*house));
    }
    assert!(!sim.fog.is_cell_revealed(houses[0], 39, 39));
    assert!(!sim.fog.is_cell_revealed(houses[1], 0, 0));
}

/// RevealEntireMap's guards (`0x00577F30..0x00577FAD`): NULL reveals to the
/// current house without a map-clear byte; once House+240 is set a second
/// call only writes the named house's byte; another house gets its byte and
/// nothing else.
#[test]
fn reveal_entire_map_reveals_once_and_only_to_the_current_house() {
    let (mut sim, houses) = sim_with_houses(&[0, 1]);
    sim.fog.width = 8;
    sim.fog.height = 8;
    let context = crate::sim::world::UninitContext::default();
    sim.reveal_entire_map(None, context);
    assert!(
        !sim.fog.whole_map_revealed_for_owner(houses[0]),
        "no current house, no PlayerPtr"
    );

    sim.session.current_house = Some(houses[0]);
    sim.reveal_entire_map(None, context);
    assert!(!sim.houses[&houses[0]].map_is_clear);
    assert!(sim.fog.whole_map_revealed_for_owner(houses[0]));
    assert!(sim.fog.is_cell_revealed(houses[0], 7, 7));
    let plane = bincode::serialize(&sim.fog.by_owner[&houses[0]]).unwrap();

    sim.reveal_entire_map(Some(houses[0]), context);
    assert!(sim.houses[&houses[0]].map_is_clear);
    assert_eq!(
        bincode::serialize(&sim.fog.by_owner[&houses[0]]).unwrap(),
        plane
    );

    sim.reveal_entire_map(Some(houses[1]), context);
    assert!(sim.houses[&houses[1]].map_is_clear);
    assert!(!sim.fog.whole_map_revealed_for_owner(houses[1]));
    assert!(!sim.fog.is_cell_revealed(houses[1], 7, 7));
}

/// The Civilian-side house is asked of each house's HouseType side
/// (`[house+0x34]+0xBC` against `Find_Index("Civilian")`, every `0x006A46D0`
/// caller). A map country outside `[Sides]` keeps the constructor's -1 even
/// where the scenario's side resolver stores Civilian for its house through
/// the declared side name, so it is passed over for the Neutral house. With
/// no `Civilian` side, Find_Index's -1 matches that sideless type instead.
#[test]
fn the_civilian_house_is_asked_of_its_house_type() {
    for (civilian_side, expected) in [("Civilian", 1), ("Other", 0)] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=Americans\n1=MapCountry\n2=Neutral\n\
             [Americans]\n[MapCountry]\n[Neutral]\n\
             [Sides]\nAllied=Americans\n{civilian_side}=Neutral\n"
        )))
        .expect("civilian house rules");
        let mut sim = Simulation::with_seed(5);
        let houses: Vec<InternedId> = [("MapHouse", "MapCountry"), ("Neutral", "Neutral")]
            .into_iter()
            .map(|(name, country)| {
                let side = crate::sim::house_state::resolve_house_side_index(
                    &rules,
                    Some(country),
                    Some(civilian_side),
                    0,
                );
                let id = sim.interner.intern(name);
                let country = sim.interner.intern(country);
                sim.houses
                    .insert(id, HouseState::new(id, side, Some(country), false, 0, 10));
                sim.session.house_order.push(id);
                id
            })
            .collect();
        assert_eq!(
            sim.houses[&houses[0]].side_index, sim.houses[&houses[1]].side_index,
            "{civilian_side}: the resolver stores the declared side for the sideless country"
        );
        assert_eq!(
            sim.civilian_side_house(&rules),
            Some(houses[expected]),
            "{civilian_side}"
        );
    }
}
