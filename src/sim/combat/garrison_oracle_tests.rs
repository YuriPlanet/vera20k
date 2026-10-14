//! Replays of original occupied-Building queries. The corpus and its sidecar
//! bound these claims to GetWeapon, GetCurrentWeapon, GetWeaponRange,
//! IsOccupied, HalfFoundation and the occupied Greatest_Threat bound slice.

use super::combat_weapon::{
    current_weapon, get_weapon, half_foundation, is_occupied, weapon_range,
};
use crate::map::entities::EntityCategory;
use crate::rules::foundation::FOUNDATION_TABLE;
use crate::rules::ini_parser::IniFile;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::{StringInterner, test_interner};
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use serde_json::Value;
use std::fmt::Write;

/// FireAt advances the occupant at 6FF031..6FF085 before GetROF queries
/// GetWeapon. The null-weapon arm at 6FCFD2..6FCFDD returns one without a
/// draw; `techno_rearm.json` executes that arm and records the complete RNG.
/// The query corpus separately covers normal/elite missing occupant weapons.
/// This is a production-emission regression with supplied cargo/shot state,
/// not a native comparison of boarding or the complete FireAt lifecycle.
#[test]
fn a_shot_before_an_unarmed_occupant_rearms_without_an_rng_draw() {
    let rows: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/techno_rearm.json",
    ))
    .unwrap();
    let golden = rows
        .iter()
        .find(|row| row["input"]["no_weapon"] == true)
        .unwrap();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=HOUSE\n[InfantryTypes]\n0=ARMED\n1=UNARMED\n\
         [HOUSE]\nStrength=800\nCanBeOccupied=yes\nCanOccupyFire=yes\n\
         [ARMED]\nStrength=125\nPrimary=Gun\nOccupyWeapon=Gun\n\
         [UNARMED]\nStrength=125\n\
         [Gun]\nDamage=5\nROF=120\nRange=6\nProjectile=InvisibleHigh\nWarhead=SA\n\
         [InvisibleHigh]\nInviso=yes\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();

    for rank in [0, 200] {
        let mut world = crate::sim::world::Simulation::new();
        let mut building = GameEntity::test_default_of_category(
            1,
            "HOUSE",
            "Test",
            10,
            10,
            EntityCategory::Structure,
        );
        let mut cargo = PassengerCargo::new(5, 0);
        // Occupants in admission order: armed, then unarmed.
        for (id, kind) in [(2, "ARMED"), (3, "UNARMED")] {
            let mut occupant = GameEntity::test_default_of_category(
                id,
                kind,
                "Test",
                10,
                10,
                EntityCategory::Infantry,
            );
            occupant.veterancy.set_rank_for_test(rank);
            occupant.passenger_role = PassengerRole::inside_for_test(1, false);
            cargo.add_occupant(id, 1);
            world.substrate.entities.insert(occupant);
        }
        assert_eq!(cargo.passengers(), [2, 3]);
        building.passenger_role = PassengerRole::transport(cargo);
        world.substrate.entities.insert(building);
        world.interner = test_interner();
        crate::sim::arena_fixture::flat_arena(&mut world, &rules);
        world.scenario_rng = crate::sim::rng::SimRng::new(31);
        assert_eq!(world.scenario_rng.native_state_hex(), golden["rng_before"]);

        let building = world.substrate.entities.get(1).unwrap();
        let obj = rules.object("HOUSE").unwrap();
        let selected = super::combat_weapon::resolve_weapon_index(
            &rules,
            building,
            obj,
            0,
            &world.substrate.entities,
            &world.interner,
        )
        .expect("the armed occupant's gun");
        let shot = super::world_fire::AdmittedFire {
            snap: super::build_attacker_snapshot(
                building,
                super::TargetKind::Cell(14, 10),
                Some(super::GarrisonSnapshot {
                    fire_index: 0,
                    occupant_count: 2,
                }),
            ),
            obj,
            selected,
            target_coords: (
                14,
                10,
                crate::util::fixed_math::SimFixed::from_num(128),
                crate::util::fixed_math::SimFixed::from_num(128),
            ),
            target_type_ref: building.type_ref(),
            is_garrison: true,
        };
        let mut emitted = super::CombatEmit::default();
        assert!(
            super::world_fire::emit_admitted_fire(
                &mut world,
                &rules,
                shot,
                17,
                &mut emitted,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .is_some()
        );
        assert_eq!(emitted.fire_events.len(), 1);
        let building = world.substrate.entities.get(1).unwrap();
        assert_eq!(
            building
                .passenger_role
                .cargo()
                .unwrap()
                .garrison_fire_index(),
            1
        );
        assert!(
            get_weapon(
                building,
                obj,
                0,
                &world.substrate.entities,
                &rules,
                &world.interner
            )
            .is_none()
        );
        assert_eq!(building.rearm_timer.start_frame(), 17);
        assert_eq!(
            i64::from(building.rearm_timer.duration()),
            golden["output"].as_i64().unwrap(),
            "missing weapon at occupant rank {rank}"
        );
        assert_eq!(world.scenario_rng.native_state_hex(), golden["rng_after"]);
    }
}

/// Supply the native corpus's type links and live state through existing
/// fixture owners. Expected outputs remain in the executed native corpus.
pub(super) fn for_each_native_case(
    mut check: impl FnMut(&Value, &GameEntity, &ObjectType, &EntityStore, &RuleSet, &StringInterner),
) {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/garrison_oracle/weapon_range.json",
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let input = &case["input"];
        let occupants = input["occupants"].as_array().unwrap();
        let turreted = input["building_turret_count"].as_i64().unwrap() > 0;
        let mut ini = format!(
            "[CombatDamage]\nOccupyWeaponRange={}\n[BuildingTypes]\n0=HOUSE\n[InfantryTypes]\n",
            input["occupy_range"]
        );
        for index in 0..occupants.len() {
            writeln!(ini, "{index}=OCC{index}").unwrap();
        }
        ini.push_str("[WeaponTypes]\n");
        let ranges = corpus["weapon_range_leptons"].as_object().unwrap();
        for (index, name) in ranges.keys().enumerate() {
            writeln!(ini, "{index}={name}").unwrap();
        }
        writeln!(
            ini,
            "[HOUSE]\nCanBeOccupied={}\nCanOccupyFire={}\nTurretCount={}\nWeaponCount=2",
            input["can_be_occupied"], input["can_occupy_fire"], input["building_turret_count"]
        )
        .unwrap();
        let slots = if turreted {
            ["Weapon1", "Weapon2", "EliteWeapon1", "EliteWeapon2"]
        } else {
            ["Primary", "Secondary", "ElitePrimary", "EliteSecondary"]
        };
        let keys = ["primary", "secondary", "elite_primary", "elite_secondary"];
        for (key, slot) in keys.iter().zip(slots) {
            if let Some(name) = input["building_weapons"][key].as_str() {
                writeln!(ini, "{slot}={name}").unwrap();
            }
        }
        for (index, occupant) in occupants.iter().enumerate() {
            writeln!(ini, "[OCC{index}]").unwrap();
            for (key, slot) in [
                ("primary", "Primary"),
                ("secondary", "Secondary"),
                ("elite_primary", "ElitePrimary"),
                ("elite_secondary", "EliteSecondary"),
                ("occupy_weapon", "OccupyWeapon"),
                ("elite_occupy_weapon", "EliteOccupyWeapon"),
            ] {
                if let Some(name) = occupant[key].as_str() {
                    writeln!(ini, "{slot}={name}").unwrap();
                }
            }
        }
        for (name, range) in ranges {
            writeln!(ini, "[{name}]\nRange={}", range.as_i64().unwrap() / 256).unwrap();
        }
        let foundation = FOUNDATION_TABLE[input["foundation_id"].as_u64().unwrap() as usize];
        let art = IniFile::from_str(&format!("[HOUSE]\nFoundation={}\n", foundation.name));
        let rules =
            RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&ini), &art).unwrap();
        let object = rules.object("HOUSE").unwrap();
        let mut building = GameEntity::test_default_of_category(
            1,
            "HOUSE",
            "Test",
            10,
            10,
            EntityCategory::Structure,
        );
        building
            .type_facts
            .set_foundation_for_test(object.foundation.clone());
        building
            .veterancy
            .set_rank_for_test((input["building_veterancy"].as_f64().unwrap() * 100.0) as u16);
        building.set_gunner_selection_for_test(
            input["current_weapon_number"].as_i64().unwrap() as i32,
            0,
        );
        let mut cargo = PassengerCargo::new(5, 0);
        let mut entities = EntityStore::new();
        for (index, occupant) in occupants.iter().enumerate() {
            let id = index as u64 + 2;
            let mut entity = GameEntity::test_default_of_category(
                id,
                &format!("OCC{index}"),
                "Test",
                10,
                10,
                EntityCategory::Infantry,
            );
            entity
                .veterancy
                .set_rank_for_test((occupant["veterancy"].as_f64().unwrap() * 100.0) as u16);
            cargo.add_occupant(id, 1);
            entities.insert(entity);
        }
        cargo.set_garrison_fire_index_for_test(input["fire_index"].as_u64().unwrap() as u8);
        building.passenger_role = PassengerRole::transport(cargo);
        check(case, &building, object, &entities, &rules, &test_interner());
    }
}

#[test]
fn occupied_weapon_and_foundation_queries_match_original_instructions() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/garrison_oracle/weapon_range.json",
    ))
    .unwrap();
    for row in corpus["foundations"].as_array().unwrap() {
        let foundation = FOUNDATION_TABLE[row["foundation_id"].as_u64().unwrap() as usize];
        assert_eq!(u64::from(foundation.width), row["width"].as_u64().unwrap());
        assert_eq!(
            u64::from(foundation.height),
            row["height"].as_u64().unwrap()
        );
    }
    for_each_native_case(|case, building, object, entities, rules, interner| {
        let name = case["input"]["name"].as_str().unwrap();
        assert_eq!(
            is_occupied(building, object),
            case["occupied"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(
            i64::from(half_foundation(object)),
            case["half_foundation"].as_i64().unwrap(),
            "{name}"
        );
        for (index, expected) in corpus["weapon_indices"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["weapon_ids"].as_array().unwrap())
        {
            assert_eq!(
                get_weapon(
                    building,
                    object,
                    index.as_i64().unwrap() as i32,
                    entities,
                    rules,
                    interner
                )
                .map(|weapon| weapon.id.as_str()),
                expected.as_str(),
                "{name}: slot {index}"
            );
        }
        assert_eq!(
            current_weapon(building, object, entities, rules, interner)
                .map(|weapon| weapon.id.as_str()),
            case["current_weapon_id"].as_str(),
            "{name}: current"
        );
        for (index, expected) in case["weapon_ranges"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                i64::from(weapon_range(
                    building,
                    object,
                    index as i32,
                    entities,
                    rules,
                    interner
                )),
                expected.as_i64().unwrap(),
                "{name}: range {index}"
            );
        }
    });
}
