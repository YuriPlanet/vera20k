//! Retained aircraft deployment history through production construction/reveal.
use super::{PlacementEvidence, Simulation};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::snapshot::GameSnapshot;

fn rules(selectable: bool, landable: bool, weapon: &str, elite: &str) -> RuleSet {
    let yes = |value| if value { "yes" } else { "no" };
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[AircraftTypes]\n0=PLANE\n[InfantryTypes]\n[VehicleTypes]\n[BuildingTypes]\n\
         [PLANE]\nStrength=100\nSpeed=10\nSelectable={}\nLandable={}\nPrimary={}\nElitePrimary={}\n\
         Locomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\n\
         [gun]\nDamage=1\n[camera]\nDamage=1\nCamera=yes\n",
        yes(selectable),
        yes(landable),
        if weapon == "none" { "" } else { weapon },
        if elite == "none" { "" } else { elite },
    )))
    .unwrap()
}

#[test]
fn mission_only_aircraft_reveal_matches_original_flag_histories() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_mission_only.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 96);
    for row in rows {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap();
        let rules = rules(
            flag("selectable"),
            flag("landable"),
            input["weapon"].as_str().unwrap(),
            input["elite_weapon"].as_str().unwrap(),
        );
        let mut sim = Simulation::new();
        let id = sim
            .construct_object_limbo_at_height("PLANE", "Americans", 10, 10, 0, 0, &rules)
            .unwrap();
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        assert!(
            !entity.is_mission_only(),
            "construction is not successful Unlimbo"
        );
        entity.veterancy = input["veterancy"].as_u64().unwrap() as u16 * 100;
        if flag("previous") {
            entity.mark_mission_only();
        }
        let placement = if flag("success") {
            PlacementEvidence::MarkSucceeded
        } else {
            PlacementEvidence::MarkFailed
        };
        let rng = sim.scenario_rng.logical_state();
        let result = sim.reveal_constructed_object_at_height(id, 10, 10, 0, 0, placement, &rules);
        assert_eq!(result.is_some(), flag("success"), "{row}");
        assert_eq!(
            sim.substrate.entities.get(id).unwrap().is_mission_only(),
            row["mission_only"].as_bool().unwrap(),
            "{row}"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            rng,
            "flag suffix draws no RNG"
        );
    }
}

#[test]
fn mission_only_survives_snapshot_limbo_and_ordinary_type_reveal() {
    let special = rules(true, true, "camera", "none");
    let normal = rules(true, true, "gun", "none");
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object_at_height("PLANE", "Americans", 10, 10, 0, 0, &special)
        .unwrap();
    assert!(sim.substrate.entities.get(id).unwrap().is_mission_only());
    sim.conceal(id);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let saved = GameSnapshot::save(&sim, 0, 0, "deployment", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.retain_in_scenario_process_state_from(&sim);
    for world in [&mut sim, &mut restored] {
        assert!(world.substrate.entities.get(id).unwrap().is_mission_only());
        assert!(
            world
                .reveal_constructed_object_at_height(
                    id,
                    11,
                    10,
                    0,
                    0,
                    PlacementEvidence::MarkSucceeded,
                    &normal
                )
                .is_some()
        );
        assert!(
            world.substrate.entities.get(id).unwrap().is_mission_only(),
            "ordinary type cannot clear deployment history"
        );
    }
    assert_eq!(sim.state_hash(), restored.state_hash());
}

#[test]
fn mission_only_has_its_own_hash_fold() {
    let rules = rules(true, true, "gun", "none");
    let mut sim = Simulation::new();
    let id = sim
        .construct_object_limbo_at_height("PLANE", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let before = sim.state_hash();
    let old = sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(189));
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mark_mission_only();
    assert_ne!(sim.state_hash(), before);
    assert_eq!(
        sim.state_hash_with_schema(super::hash_schema::HashSchema::Before(189)),
        old
    );
}
