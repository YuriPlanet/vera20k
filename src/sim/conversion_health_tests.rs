use super::{ConversionKind, native_health};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::estimated_health::EstimatedHealth;
use crate::sim::world::Simulation;
use std::collections::BTreeMap;

#[test]
fn original_conversion_health_corpus() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/conversion_health.json"
    ))
    .unwrap();
    for row in corpus["rows"].as_array().unwrap() {
        let kind = match row["kind"].as_str().unwrap() {
            "unit" => ConversionKind::Unit,
            "building" => ConversionKind::Building,
            other => panic!("unknown kind {other}"),
        };
        let value = |key: &str| row[key].as_i64().unwrap() as i32;
        assert_eq!(
            native_health(
                value("current"),
                value("source_strength"),
                value("destination_strength"),
                kind
            ),
            value("actual"),
            "{row}"
        );
        assert_eq!(value("actual"), value("estimated"));
        // Exercise the same capture/application used by all four production
        // callers, with full signed actual storage, including the 26 rows that
        // the former u16 refusal could not represent.
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=SRC\n1=DST\n[SRC]\nStrength={}\n[DST]\nStrength={}\n",
            value("source_strength"),
            value("destination_strength")
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(12);
        let source = sim
            .construct_object_limbo_at_height("SRC", "Neutral", 10, 10, 0, 0, &rules)
            .unwrap();
        let destination = sim
            .construct_object_limbo_at_height("DST", "Neutral", 10, 10, 0, 0, &rules)
            .unwrap();
        damage(&mut sim, source, value("current"));
        let captured = super::ConversionHealth::capture(
            sim.substrate.entities.get(source).unwrap(),
            rules.object("SRC").unwrap(),
            rules.object("DST").unwrap(),
            kind,
        );
        captured.apply(sim.substrate.entities.get_mut(destination).unwrap());
        assert_health(&sim, destination, value("actual"));
    }
}

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MCV\n1=SMIN\n[BuildingTypes]\n0=YARD\n1=YAREFN\n[InfantryTypes]\n0=SLAV\n\
         [MCV]\nStrength=100\nSpeed=5\nDeploysInto=YARD\n\
         [YARD]\nStrength=1000\nFoundation=1x1\nDeployFacing=0\nUndeploysInto=MCV\n\
         [SMIN]\nStrength=100\nSpeed=3\nDeploysInto=YAREFN\nEnslaves=SLAV\nSlavesNumber=1\n\
         [YAREFN]\nStrength=1000\nFoundation=1x1\nDeployFacing=0\nUndeploysInto=SMIN\nEnslaves=SLAV\nSlavesNumber=1\n\
         [SLAV]\nStrength=125\nSpeed=3\nStorage=4\n",
    ))
    .unwrap()
}

fn damage(sim: &mut Simulation, id: u64, actual: i32) {
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.health.current = actual;
    entity.estimated_health = EstimatedHealth::from_raw(-73);
}

fn assert_health(sim: &Simulation, id: u64, actual: i32) {
    let entity = sim.substrate.entities.get(id).unwrap();
    assert_eq!(entity.health.current, actual);
    assert_eq!(entity.estimated_health.get(), actual);
}

#[test]
fn mcv_conversion_uses_actual_and_type_strength_then_resets_estimate() {
    let rules = rules();
    let mut sim = Simulation::with_seed(123);
    let source = sim
        .spawn_object_at_height("MCV", "Neutral", 10, 10, 0, 0, &rules)
        .unwrap();
    damage(&mut sim, source, 25);
    assert!(sim.deploy_mcv(source, &rules, &BTreeMap::new()));
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(source).is_none());
    let destination = sim
        .substrate
        .entities
        .values()
        .find(|e| sim.interner.resolve(e.type_ref()) == "YARD")
        .unwrap()
        .stable_id();
    assert_health(&sim, destination, 250);
}

#[test]
fn building_conversion_reads_health_when_animation_finishes() {
    let rules = rules();
    let mut sim = Simulation::with_seed(123);
    let source = sim
        .spawn_object_at_height("YARD", "Neutral", 10, 10, 0, 0, &rules)
        .unwrap();
    damage(&mut sim, source, 750);
    assert!(sim.undeploy_building(source, &rules, true));
    damage(&mut sim, source, 250);
    sim.substrate
        .entities
        .get_mut(source)
        .unwrap()
        .building_down
        .as_mut()
        .unwrap()
        .finish_for_test();
    sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 22);
    assert!(sim.substrate.entities.get(source).is_none());
    let destination = sim
        .substrate
        .entities
        .values()
        .find(|e| sim.interner.resolve(e.type_ref()) == "MCV")
        .unwrap()
        .stable_id();
    assert_health(&sim, destination, 25);
}

/// UnitClass::Deploy (`deploy_mcv`) of `source`, answering the building it
/// became.
fn deploy(sim: &mut Simulation, rules: &RuleSet, source: u64, into: &str) -> u64 {
    assert!(sim.deploy_mcv(source, rules, &BTreeMap::new()));
    sim.substrate
        .entities
        .values()
        .find(|entity| {
            entity.lifecycle.object_alive && sim.interner.resolve(entity.type_ref()) == into
        })
        .unwrap()
        .stable_id()
}

/// The building's undeploy (`undeploy_building`) run to its conversion
/// (`tick_building_down`), answering the unit it became. A just-deployed
/// building is taken as built up, as the undeploy requires.
fn undeploy(sim: &mut Simulation, rules: &RuleSet, building: u64, into: &str) -> u64 {
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .building_up = None;
    assert!(sim.undeploy_building(building, rules, true));
    let down = sim
        .substrate
        .entities
        .get_mut(building)
        .unwrap()
        .building_down
        .as_mut()
        .unwrap();
    down.finish_for_test();
    sim.advance_tick(&[], Some(rules), &BTreeMap::new(), None, None, 22);
    sim.substrate
        .entities
        .values()
        .find(|entity| {
            entity.lifecycle.object_alive && sim.interner.resolve(entity.type_ref()) == into
        })
        .unwrap()
        .stable_id()
}

#[test]
fn slave_conversions_reset_master_health_but_preserve_retained_slave_state() {
    let rules = rules();
    let mut sim = Simulation::with_seed(123);
    let source = sim
        .spawn_object_at_height("SMIN", "Neutral", 10, 10, 0, 0, &rules)
        .unwrap();
    let pool = |sim: &Simulation, master: u64| -> Vec<u64> {
        sim.substrate
            .entities
            .get(master)
            .and_then(|entity| entity.slave_manager.as_ref())
            .map(|manager| manager.slaves().collect())
            .unwrap_or_default()
    };
    let slaves = pool(&sim, source);
    for &slave in &slaves {
        damage(&mut sim, slave, 100);
    }
    damage(&mut sim, source, 25);
    let building = deploy(&mut sim, &rules, source, "YAREFN");
    assert_health(&sim, building, 250);
    assert_eq!(pool(&sim, building), slaves);
    damage(&mut sim, building, 500);
    let unit = undeploy(&mut sim, &rules, building, "SMIN");
    assert_health(&sim, unit, 50);
    assert_eq!(pool(&sim, unit), slaves);
    for slave in slaves {
        let child = sim.substrate.entities.get(slave).unwrap();
        assert_eq!(child.health.current, 100);
        assert_eq!(child.estimated_health.get(), -73);
    }
}

#[test]
fn all_four_conversion_callers_preserve_results_above_u16() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MCV\n1=SMIN\n[BuildingTypes]\n0=YARD\n1=YAREFN\n[InfantryTypes]\n0=SLAV\n\
         [MCV]\nStrength=100000\nSpeed=5\nDeploysInto=YARD\n\
         [YARD]\nStrength=1000000\nFoundation=1x1\nDeployFacing=0\nUndeploysInto=MCV\n\
         [SMIN]\nStrength=100000\nSpeed=3\nDeploysInto=YAREFN\nEnslaves=SLAV\nSlavesNumber=1\n\
         [YAREFN]\nStrength=1000000\nFoundation=1x1\nDeployFacing=0\nUndeploysInto=SMIN\nEnslaves=SLAV\nSlavesNumber=1\n\
         [SLAV]\nStrength=125\nSpeed=3\nStorage=4\n"
    )).unwrap();
    let mut sim = Simulation::with_seed(123);
    let source = sim
        .spawn_object_at_height("MCV", "Neutral", 10, 10, 0, 0, &rules)
        .unwrap();
    damage(&mut sim, source, 75_000);
    assert!(sim.deploy_mcv(source, &rules, &BTreeMap::new()));
    sim.flush_pending_delete();
    let yard = sim
        .substrate
        .entities
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "YARD")
        .unwrap()
        .stable_id();
    assert_health(&sim, yard, 750_000);
    for _ in 0..30 {
        sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 22);
    }
    assert!(sim.undeploy_building(yard, &rules, true));
    sim.substrate
        .entities
        .get_mut(yard)
        .unwrap()
        .building_down
        .as_mut()
        .unwrap()
        .finish_for_test();
    sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 22);
    let mcv = sim
        .substrate
        .entities
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "MCV")
        .unwrap()
        .stable_id();
    assert_health(&sim, mcv, 75_000);

    let mut sim = Simulation::with_seed(123);
    let source = sim
        .spawn_object_at_height("SMIN", "Neutral", 10, 10, 0, 0, &rules)
        .unwrap();
    damage(&mut sim, source, 75_000);
    let building = deploy(&mut sim, &rules, source, "YAREFN");
    assert_health(&sim, building, 750_000);
    let unit = undeploy(&mut sim, &rules, building, "SMIN");
    assert_health(&sim, unit, 75_000);
}

#[test]
fn mcv_building_slots_wait_for_completion_and_use_converted_health() {
    let rules_ini = IniFile::from_str(
        "[VehicleTypes]\n0=MCV\n[BuildingTypes]\n0=YARD\n[Animations]\n0=N\n1=D\n[MCV]\nStrength=100\nSpeed=5\nDeploysInto=YARD\n[YARD]\nStrength=1000\nFoundation=1x1\nDeployFacing=0\n",
    );
    let art_ini = IniFile::from_str(
        "[YARD]\nIdleAnim=N\nIdleAnimDamaged=D\n[N]\nLoopCount=-1\nRandomRate=900,180\n[D]\nLoopCount=-1\nRandomRate=900,180\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    for name in ["N", "D"] {
        art.bind_anim_frame_count_for_test(name, 100);
    }
    rules.merge_art_data(&art);
    // Native427D00 converts each endpoint through900/rate and clamps the
    // first to the second. Authored1,5 collapses to180,180 and draws nothing;
    // authored900,180 supplies the intended nondegenerate1..5 delay range.
    for name in ["N", "D"] {
        assert_eq!(
            rules
                .art_registry
                .anim_runtime_config(name)
                .unwrap()
                .random_rate_logic_frames,
            Some((1, 5))
        );
    }
    let mut sim = Simulation::with_seed(123);
    let source = sim
        .spawn_object_at_height("MCV", "Neutral", 10, 10, 0, 0, &rules)
        .unwrap();
    damage(&mut sim, source, 25);
    // Original Techno constructor6F3249..6F3259 consumes exactly one
    // Scenario65C780 draw and stores its low word at+3C8. No Anim draws yet.
    let mut expected_rng = sim.scenario_rng.clone();
    let constructor_word = expected_rng.next_u32() as u16;
    let before_rng = expected_rng.state();
    let destination_id = sim.substrate.next_stable_object_id;
    assert!(sim.deploy_mcv(source, &rules, &BTreeMap::new()));
    let destination = sim.entities().get(destination_id).unwrap();
    assert_eq!(destination.health.current, 250);
    assert_eq!(destination.techno_ctor_random_word, constructor_word);
    assert!(destination.building_up.is_some());
    assert!(!destination.building_actually_placed);
    assert!(destination.building_anim_slots.iter().all(Option::is_none));
    assert_eq!(sim.substrate.anims.len(), 0);
    assert_eq!(sim.substrate.next_stable_object_id, destination_id + 1);
    assert_eq!(sim.scenario_rng.state(), before_rng);
    // Drive the actual simulation completion producer.
    for _ in 0..31 {
        sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
    }
    let destination = sim.entities().get(destination_id).unwrap();
    assert!(destination.building_up.is_none());
    assert!(destination.building_actually_placed);
    let anim = sim
        .anim(destination.building_anim_slots[18].unwrap())
        .unwrap();
    assert_eq!(sim.interner.resolve(anim.type_id), "D");
    let expected_rate = expected_rng.next_range_u32_inclusive(1, 5) as u16;
    assert_eq!(anim.runtime.rate_reload, expected_rate);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    assert_ne!(sim.scenario_rng.state(), before_rng);
}
