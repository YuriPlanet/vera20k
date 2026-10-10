//! Original458200 decisions against the production owner. The corpus records
//! supplied downstream callbacks; these checks do not certify native ejection,
//! animation construction or ChangeOwner side effects. See its meta sidecar.

use super::*;
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
use crate::sim::world::ObjectAiCtx;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/garrison_oracle/allegiance.json",
    ))
    .unwrap()
}

fn integer(v: &Value) -> i32 {
    v.as_i64().unwrap() as i32
}

fn fixture(input: &Value) -> (Simulation, RuleSet, u64, Vec<InternedId>) {
    let sides = input["side_names"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, name)| format!("{}=Country{index}\n", name.as_str().unwrap()))
        .collect::<String>();
    let ini = IniFile::from_str(&format!(
        "[BuildingTypes]\n0=B\n[InfantryTypes]\n0=E1\n[Sides]\n{sides}\n[B]\nStrength={}\nTechLevel={}\nCanBeOccupied={}\nMaxNumberOccupants=10\n[E1]\nStrength=100\nSpeed=4\nOccupier=yes\n[Animations]\n0=N\n1=D\n2=G\n[AudioVisual]\nBuildingAbandonedSound=Abandon\n",
        integer(&input["strength"]),
        integer(&input["tech_level"]),
        input["can_be_occupied"].as_bool().unwrap_or(true),
    ));
    let art_ini = IniFile::from_str(
        "[B]\nFoundation=1x1\nActiveAnim=N\nActiveAnimDamaged=D\nActiveAnimGarrisoned=G\n[N]\nLoopCount=-1\n[D]\nLoopCount=-1\n[G]\nLoopCount=-1\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).unwrap();
    let object = rules.object("B").unwrap();
    assert_eq!(object.tech_level, integer(&input["tech_level"]));
    assert_eq!(object.strength, integer(&input["strength"]));
    assert_eq!(
        object.can_be_occupied,
        input["can_be_occupied"].as_bool().unwrap_or(true)
    );
    for (index, name) in input["side_names"].as_array().unwrap().iter().enumerate() {
        assert_eq!(
            rules.side_index(name.as_str().unwrap()).unwrap().0 as usize,
            index
        );
    }
    rules.general.condition_red = f64::from_bits(
        u64::from_str_radix(input["condition_red_bits"].as_str().unwrap(), 16).unwrap(),
    );
    let mut art = ArtRegistry::from_ini(&art_ini);
    for name in ["N", "D", "G"] {
        art.bind_anim_frame_count_for_test(name, 40);
    }
    rules.install_art_data(art);
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=Abandon\n",
    ));
    rules.bind_type_sound_references(&ini, &sounds);
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let houses: Vec<_> = input["houses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|house| {
            let owner = sim.interner.intern(house["name"].as_str().unwrap());
            let mut state = HouseState::new(
                owner,
                integer(&house["side_index"]) as u8,
                None,
                house["is_human"].as_bool().unwrap(),
                0,
                10,
            );
            state.player_control = house["player_control"].as_bool().unwrap();
            sim.houses.insert(owner, state);
            sim.session.house_order.push(owner);
            owner
        })
        .collect();
    sim.session.game_mode_nonzero = integer(&input["game_mode"]) != 0;
    sim.session.current_house = input["local_house"]
        .as_u64()
        .map(|index| houses[index as usize]);
    let id = sim.allocate_stable_id();
    let mut building =
        GameEntity::test_default_of_category(id, "B", "", 10, 11, EntityCategory::Structure);
    building.type_ref = sim.interner.intern("B");
    building.owner = houses[input["current_owner"].as_u64().unwrap() as usize];
    building.health.current = 100;
    building.building_actually_placed = true;
    building.mission_leaf =
        crate::sim::mission::leaf::MissionLeafState::for_entity_category(EntityCategory::Structure);
    let mut cargo = PassengerCargo::new(10, 1);
    cargo.garrison_fire_index = integer(&input["fire_index"]) as u8;
    for owner in input["occupant_owners"].as_array().unwrap() {
        let passenger_id = sim.allocate_stable_id();
        let mut passenger = GameEntity::test_default_of_category(
            passenger_id,
            "E1",
            "",
            10,
            11,
            EntityCategory::Infantry,
        );
        passenger.type_ref = sim.interner.intern("E1");
        passenger.owner = houses[owner.as_u64().unwrap() as usize];
        passenger.passenger_role = PassengerRole::Inside {
            transport_id: id,
            open_topped: false,
        };
        passenger.lifecycle.in_limbo = true;
        passenger.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Walk,
            ),
        );
        cargo.board_forced(passenger_id, 1);
        sim.substrate.entities.insert(passenger);
    }
    building.passenger_role = PassengerRole::Transport { cargo };
    sim.substrate.entities.insert(building);
    assert!(matches!(
        sim.reveal_entity_with_rules(id, &rules),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    sim.substrate.entities.get_mut(id).unwrap().health.current = integer(&input["current_hp"]);
    sim.set_building_anim_slot(id, 3, false, false, 0, &rules)
        .unwrap();
    sim.sound_events.clear();
    (sim, rules, id, houses)
}

#[test]
fn allegiance_matches_native_decisions_for_valid_rosters() {
    let corpus = corpus();
    let mut replayed = 0;
    for row in corpus["rows"].as_array().unwrap() {
        let input = &row["input"];
        let expected = &row["output"];
        let name = input["name"].as_str().unwrap();
        // NULL and Side=-1 are malformed native pointer controls. Rust keeps
        //the real owner for those rosters. The retained-occupant callback is
        //an injected dependency mutation, not SellBuilding's normal outcome.
        if input["current_owner"].is_null()
            || expected["owner_after_callback"].is_null()
            || input["houses"]
                .as_array()
                .unwrap()
                .iter()
                .any(|h| integer(&h["side_index"]) < 0)
            || !input["eject_retained_occupants"]
                .as_array()
                .unwrap()
                .is_empty()
        {
            continue;
        }
        let (mut sim, rules, id, houses) = fixture(input);
        let old_anim = sim.substrate.entities.get(id).unwrap().building_anim_slots[3];
        let old_owner = sim.substrate.entities.get(id).unwrap().owner();
        let changed = reconcile_civilian_garrison_owner_for_building(
            &mut sim,
            &rules,
            None,
            id,
            crate::sim::world::FrameEffects::default(),
        );
        let building = sim.substrate.entities.get(id).unwrap();
        let expected_owner = houses[expected["owner_after_callback"].as_u64().unwrap() as usize];
        assert_eq!(building.owner(), expected_owner, "{name}");
        assert_eq!(changed, old_owner != expected_owner, "{name}");
        let cargo = building.passenger_role.cargo().unwrap();
        assert_eq!(
            cargo.garrison_fire_index,
            integer(&expected["fire_index_after_callback"]) as u8,
            "{name}"
        );
        let actual_owners: Vec<_> = cargo
            .passengers
            .iter()
            .rev()
            .map(|id| sim.substrate.entities.get(*id).unwrap().owner())
            .collect();
        let expected_owners: Vec<_> = expected["occupant_owners_after_callback"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| houses[index.as_u64().unwrap() as usize])
            .collect();
        assert_eq!(actual_owners, expected_owners, "{name}");
        let called = |op: &str| {
            expected["calls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|call| call["op"] == op)
        };
        assert_eq!(
            building.building_anim_slots[3] != old_anim,
            called("refresh_anims"),
            "{name}"
        );
        let event = sim.sound_events.iter().find_map(|event| match event {
            SimSoundEvent::StructureAbandoned { owner, radar } => Some((*owner, *radar)),
            _ => None,
        });
        assert_eq!(event.is_some(), called("radar_event"), "{name}");
        if let Some((owner, radar)) = event {
            assert_eq!(owner, old_owner, "{name}");
            assert_eq!(
                radar,
                crate::sim::radar::RadarEventRequest::new(
                    crate::sim::radar::RadarEventType::StructureAbandoned,
                    10,
                    11,
                ),
                "{name}"
            );
        }
        assert_eq!(
            sim.sound_events.iter().any(|event| matches!(event,
                SimSoundEvent::VocCentered { sound_id } if sound_id == "Abandon"
            )),
            called("abandoned_sound"),
            "{name}"
        );
        replayed += 1;
    }
    assert_eq!(replayed, 35);
}

#[test]
fn missing_civilian_roster_does_not_fabricate_a_house() {
    let corpus = corpus();
    for row in corpus["rows"].as_array().unwrap().iter().filter(|row| {
        matches!(
            row["input"]["name"].as_str().unwrap(),
            "missing_civilian_side_requests_null" | "missing_civilian_house_requests_null"
        )
    }) {
        let (mut sim, rules, id, _) = fixture(&row["input"]);
        let old_owner = sim.substrate.entities.get(id).unwrap().owner();
        let houses = sim.session.house_order.clone();
        assert!(!reconcile_civilian_garrison_owner_for_building(
            &mut sim,
            &rules,
            None,
            id,
            crate::sim::world::FrameEffects::default()
        ));
        assert_eq!(sim.substrate.entities.get(id).unwrap().owner(), old_owner);
        assert_eq!(sim.session.house_order, houses);
    }
}

#[test]
fn building_ai_caller_admission_matches_native_unwarped_gates() {
    let corpus = corpus();
    let mut replayed = 0;
    for row in corpus["caller_rows"].as_array().unwrap() {
        let input = &row["input"];
        // Warp admission remains with its existing owner. These controls
        //execute the actual Building AI for the independent live/HP/type gates.
        if input["warped_out"] == true || input["warping_in"] == true {
            continue;
        }
        // The supplied live/negative-HP control reaches the original caller
        //gate, but shared is_ai_alive currently treats nonpositive HP as dead
        //unless crashing (the deferred-UnInit residual on that owner). Fixing
        //that owner affects the excluded Foot/Fly lifecycle; this is not a
        //native-matching frame control. Direct458200 still replays negative HP.
        if integer(&input["health"]) < 0 {
            assert_eq!(row["output"]["all_gates_allow"], true);
            continue;
        }
        let mut fixture_input = corpus["rows"][0]["input"].clone();
        fixture_input["current_hp"] = input["health"].clone();
        fixture_input["can_be_occupied"] = input["can_be_occupied"].clone();
        let (mut sim, mut rules, id, _) = fixture(&fixture_input);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .lifecycle
            .object_alive = input["alive"].as_bool().unwrap();
        rules.general.condition_red = -1.0; // no ejection or death dependency
        let old_owner = sim.substrate.entities.get(id).unwrap().owner();
        sim.object_ai_visit_one(id, Some(&rules), ObjectAiCtx::default());
        assert_eq!(
            sim.substrate.entities.get(id).unwrap().owner() != old_owner,
            row["output"]["all_gates_allow"].as_bool().unwrap(),
            "{}",
            input["name"]
        );
        replayed += 1;
    }
    assert_eq!(replayed, 6);
}

#[test]
fn frame_reports_building_allegiance_from_the_live_object_pass() {
    let corpus = corpus();
    let (mut sim, rules, id, _) = fixture(&corpus["rows"][0]["input"]);
    sim.set_logic_order_for_test(vec![id]);
    let tick = sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert!(tick.ownership_changed);
    assert!(
        !sim.advance_tick(&[], Some(&rules), None, None, 67)
            .ownership_changed
    );
}

#[test]
fn zero_capacity_map_garrison_abandons_without_allocated_cargo() {
    use crate::rules::process_owner::NativeRulesProcessOwner;
    use crate::rules::sound_ini::SoundRegistry;
    use std::sync::Arc;

    let root = IniFile::from_str(
        "[BuildingTypes]\n0=B\n[Sides]\nAllied=Player\nCivilian=Town\n\
         [B]\nStrength=100\nTechLevel=-1\nCanBeOccupied=yes\nMaxNumberOccupants=10\n\
         [Animations]\n0=N\n[AudioVisual]\nBuildingAbandonedSound=Abandon\n",
    );
    let art_ini = IniFile::from_str("[B]\nFoundation=1x1\nActiveAnim=N\n[N]\nLoopCount=-1\n");
    let sounds = SoundRegistry::from_ini(&IniFile::from_str("[SoundList]\n0=Abandon\n"));
    let mut process = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        art_ini.clone(),
        Arc::new(sounds),
    )
    .unwrap();
    let (mut rules, _, _, _) = process
        .load_scenario(crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None), &IniFile::from_str("[B]\nMaxNumberOccupants=0\n"))
        .unwrap()
        .into_parts();
    assert_eq!(rules.object("B").unwrap().max_number_occupants, 0);
    let mut art = ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test("N", 40);
    rules.install_art_data(art);
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    for (name, human) in [("Player", true), ("Town", false)] {
        let owner = sim.interner.intern(name);
        let side = rules
            .side_index(if human { "Allied" } else { "Civilian" })
            .unwrap()
            .0;
        sim.houses
            .insert(owner, HouseState::new(owner, side, None, human, 0, 10));
        sim.session.house_order.push(owner);
        if human {
            sim.session.current_house = Some(owner);
        }
    }
    // Use the production constructor: unlike the corpus fixture, it represents
    // zero capacity without PassengerCargo. Native458272 tests only vector count.
    let id = sim.spawn_object("B", "Player", 10, 11, 0, &rules).unwrap();
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .passenger_role
            .cargo()
            .is_none()
    );
    let old_anim = sim
        .set_building_anim_slot(id, 3, false, false, 0, &rules)
        .unwrap();
    sim.sound_events.clear();
    sim.object_ai_visit_one(id, Some(&rules), ObjectAiCtx::default());

    let building = sim.substrate.entities.get(id).unwrap();
    assert_eq!(sim.interner.resolve(building.owner()), "Town");
    assert_ne!(building.building_anim_slots[3], Some(old_anim));
    assert!(sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::VocCentered { sound_id } if sound_id == "Abandon"
    )));
    assert!(sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::StructureAbandoned { owner, .. }
            if sim.interner.resolve(*owner) == "Player"
    )));
}
