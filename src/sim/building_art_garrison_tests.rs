//! Original458330 selection/order replay through the production animation owner.
//! The corpus records calls at451890, not its constructor/destructor effects;
//! these tests use real bound ART and the existing451890 implementation.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::art_data::{ArtRegistry, BuildingAnimVariantConfig};
use crate::rules::ini_parser::IniFile;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use serde_json::Value;
use std::collections::BTreeSet;

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap().try_into().unwrap()
}

fn fixture(input: &Value) -> (Simulation, RuleSet, u64) {
    let mut names = BTreeSet::from(["OLD"]);
    for variants in input["slot_names"].as_object().unwrap().values() {
        for name in variants.as_object().unwrap().values() {
            let name = name.as_str().unwrap();
            if !name.is_empty() {
                names.insert(name);
            }
        }
    }
    let mut rules_text = format!(
        "[BuildingTypes]\n0=B\n[B]\nStrength={}\nCanBeOccupied=yes\nTechLevel=-1\nUpgrades=3\n\
         [InfantryTypes]\n0=E1\n[E1]\nStrength=100\nOccupier=yes\n[Animations]\n",
        integer(&input["strength"]),
    );
    let mut art_text =
        String::from("[B]\nActiveAnim=OLD\nPowerUp1Anim=OLD\nPowerUp2Anim=OLD\nPowerUp3Anim=OLD\n");
    for (index, name) in names.iter().enumerate() {
        rules_text.push_str(&format!("{index}={name}\n"));
        art_text.push_str(&format!("[{name}]\nLoopCount=-1\nRate=150\n"));
    }
    let art_ini = IniFile::from_str(&art_text);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&rules_text), &art_ini)
            .unwrap();
    assert_eq!(
        rules.object("B").unwrap().strength,
        integer(&input["strength"])
    );
    // Supplied raw native double includes the unordered comparison control.
    rules.general.condition_yellow = f64::from_bits(
        u64::from_str_radix(input["condition_yellow_bits"].as_str().unwrap(), 16).unwrap(),
    );
    let mut art = ArtRegistry::from_ini(&art_ini);
    for name in names {
        art.bind_anim_frame_count_for_test(name, 40);
    }
    let entry = art.get_mut("B").unwrap();
    let template = entry
        .building_anims
        .iter()
        .find(|config| config.native_slot == 3)
        .unwrap()
        .clone();
    // Install the supplied native slot-presence controls with an identifiable
    // old animation. Requested names are installed after creation, so even an
    // empty normal name can test preservation of an already live slot.
    entry.building_anims = (0..21)
        .map(|slot| {
            let mut config = template.clone();
            config.native_slot = slot;
            config
        })
        .collect();
    rules.install_art_data(art);
    let (mut sim, _, id) = slot_test_fixture();
    for slot in input["live_slots"].as_array().unwrap() {
        let slot = integer(slot) as u8;
        let anim = sim
            .set_building_anim_slot(id, slot, false, false, 0, &rules)
            .unwrap();
        sim.substrate
            .anims
            .get_mut(anim)
            .unwrap()
            .runtime_mut_for_test()
            .current_frame = i32::from(slot) + 1;
    }
    let entry = rules.art_entry_mut_for_test("B").unwrap();
    for (slot, variants) in input["slot_names"].as_object().unwrap() {
        let slot: u8 = slot.parse().unwrap();
        let config = entry
            .building_anims
            .iter_mut()
            .find(|config| config.native_slot == slot)
            .unwrap();
        let variant = |key: &str| BuildingAnimVariantConfig {
            anim_type: variants[key].as_str().unwrap().to_owned(),
            loop_start: 0,
            loop_end: 0,
            loop_count: -1,
            rate: 1,
            start_frame: 0,
            ping_pong: false,
        };
        config.anim_type = variants["normal"].as_str().unwrap().to_owned();
        config.damaged_variant = Some(variant("damaged"));
        config.garrisoned_variant = Some(variant("garrisoned"));
    }
    let count = integer(&input["occupant_count"]);
    assert!((0..=1).contains(&count));
    let mut cargo = PassengerCargo::new(1, 0.0);
    for _ in 0..count {
        let occupant_id = sim.substrate.allocate_stable_id();
        let mut occupant = GameEntity::test_default_of_category(
            occupant_id,
            "E1",
            "A",
            2,
            2,
            EntityCategory::Infantry,
        );
        occupant.set_type_ref_for_test(sim.interner.intern("E1"));
        occupant.set_owner_for_test(sim.interner.intern("A"));
        occupant.passenger_role = PassengerRole::inside_for_test(id, false);
        sim.substrate.entities.insert(occupant);
        cargo.board_forced_chain(&[(occupant_id, 1.0)]);
    }
    let building = sim.substrate.entities.get_mut(id).unwrap();
    building.passenger_role = PassengerRole::transport(cargo);
    building.health.set_for_test(integer(&input["current_hp"]));
    building.building_art.damage_state_active = input["retained_damage_state"].as_bool().unwrap();
    (sim, rules, id)
}

#[test]
fn native_refresh_corpus_replays_through_live_animation_slots() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/garrison_oracle/allegiance.json",
    ))
    .unwrap();
    let mut replayed = 0;
    let mut excluded = BTreeSet::new();
    for row in corpus["refresh_rows"].as_array().unwrap() {
        let input = &row["input"];
        let output = &row["output"];
        let name = input["name"].as_str().unwrap();
        if !input["callback_mutations"].as_array().unwrap().is_empty()
            || !(0..=1).contains(&integer(&input["occupant_count"]))
        {
            excluded.insert(name);
            continue;
        }
        let (mut sim, rules, id) = fixture(input);
        let before = sim.entities().get(id).unwrap().building_art.anim_slots;
        sim.refresh_garrison_anims(id, &rules);
        let after = sim.entities().get(id).unwrap().building_art.anim_slots;
        let expected_calls = output["calls"].as_array().unwrap();
        let mut replaced = BTreeSet::new();
        let mut last_replacement = None;
        for call in expected_calls {
            let slot = integer(&call["slot"]) as usize;
            let old = before[slot].unwrap();
            let new = after[slot].unwrap();
            assert_ne!(new, old, "{name}: slot {slot} must be replaced");
            assert!(sim.anim(old).is_none(), "{name}: old slot {slot} survives");
            let anim = sim.anim(new).unwrap();
            assert_eq!(
                sim.interner.resolve(anim.type_id()),
                call["name"].as_str().unwrap(),
                "{name}: slot {slot} selection",
            );
            assert_eq!(
                i32::from(anim.runtime().delay_remaining),
                integer(&call["delay"]),
                "{name}: slot {slot} delay",
            );
            assert_eq!(anim.runtime().current_frame, slot as i32 + 1, "{name}");
            assert_eq!(anim.building_slot(), Some((id, slot as u8)), "{name}");
            // Shared451890 may first synchronize all live slots when retained
            // damage changes. Its extra allocations precede these final IDs;
            // their increasing order must still match native458330's calls.
            if let Some(previous) = last_replacement {
                assert!(new > previous, "{name}: native callback order");
            }
            last_replacement = Some(new);
            assert!(replaced.insert(slot));
        }
        for slot in 0..21 {
            if !replaced.contains(&slot) {
                assert_eq!(after[slot], before[slot], "{name}: untouched slot {slot}");
                if let Some(old) = before[slot] {
                    assert!(sim.anim(old).is_some(), "{name}: preserved slot {slot}");
                }
            }
        }
        let live_slots: Vec<_> = after
            .iter()
            .enumerate()
            .filter_map(|(slot, anim)| anim.map(|_| slot as i32))
            .collect();
        let expected_live: Vec<_> = output["live_slots_after_callback"]
            .as_array()
            .unwrap()
            .iter()
            .map(integer)
            .collect();
        assert_eq!(live_slots, expected_live, "{name}: slot presence");
        assert_eq!(sim.anims().count(), live_slots.len(), "{name}: orphan anim");
        replayed += 1;
    }
    assert_eq!(replayed, 20);
    // Native malformed-vector and enormous-count controls are not bounded
    // Rust cargo fixtures. The four callback controls deliberately substitute
    // mutations at451890; no native constructor evidence establishes them.
    assert_eq!(
        excluded,
        BTreeSet::from([
            "negative_count_is_empty",
            "max_positive_count_is_occupied",
            "callback_removes_later_slot",
            "callback_adds_later_slot",
            "callback_adds_earlier_slot_not_revisited",
            "callback_changes_later_live_queries",
        ]),
    );
}
