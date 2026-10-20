//! House statistics, produced items and score (`Economy::score`) through
//! native House save/load values. Native field retention is compared;
//! per-house kill-table aggregation is not newly certified by these tests.

use super::*;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::game_entity::GameEntity;
use crate::sim::rng::SimRng;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;
use serde_json::Value;

fn capture_accounting_corpus() -> &'static Value {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/engineer_capture_accounting.json");
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(
        corpus["kind"],
        "bounded-original-engineer-capture-accounting"
    );
    corpus
}

fn capture_count(value: &Value) -> u32 {
    u32::try_from(value.as_u64().unwrap()).unwrap()
}

/// The house's statistics and score as the corpus records them.
fn capture_totals(fields: &Value) -> (MatchStatistics, i32) {
    let building_kills = fields["building_kills"]
        .as_array()
        .unwrap()
        .iter()
        .fold(0_u32, |total, count| {
            total.wrapping_add(capture_count(count))
        });
    let stats = MatchStatistics::from_totals_for_test(
        0,
        building_kills,
        0,
        capture_count(&fields["building_losses"]),
        0,
    );
    (
        stats,
        i32::try_from(fields["score"].as_i64().unwrap()).unwrap(),
    )
}

fn house_totals(house: &HouseState) -> (MatchStatistics, i32) {
    (house.stats, house.economy.score())
}

/// Original702D40 controls execute through the reused EngineerJoinedFixture,
/// full retail GAPOWR readers and original CostOf. The checked repository
/// companion records the supplied flags/counters, raw stores and three RNG
/// states. This compares immediate NULL accounting, not full sale/death/Tags.
#[test]
fn native_null_record_kill_books_each_live_callback_immediately() {
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    let corpus = capture_accounting_corpus();
    let rows = corpus["record_kill_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    let mut compared = Vec::new();
    for row in rows {
        let name = row["name"].as_str().unwrap();
        if name == "null_sale_loss_suppression" {
            // Native44A1EF's retained Building+53C producer is not ported by
            // this capture chain. Do not replace it with a Selling predicate.
            continue;
        }
        let dont_score = row["input"]["dont_score"].as_bool().unwrap();
        let insignificant = row["input"]["insignificant"].as_bool().unwrap();
        let cost = corpus["native_building_entry_flags"]["cost"]
            .as_u64()
            .unwrap();
        let strength = corpus["native_type"]["strength"].as_u64().unwrap();
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[BuildingTypes]\n0=GAPOWR\n[GAPOWR]\nCost={cost}\nStrength={strength}\nDontScore={dont_score}\nInsignificant={insignificant}\n"
        )))
        .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, None, true, 0, 10);
        let (stats, score) = capture_totals(&row["before"]);
        house.stats = stats;
        house.economy.set_score_for_test(score);
        sim.houses.register(house);
        let mut victim = GameEntity::test_default(1, "GAPOWR", "Americans", 10, 10);
        victim.set_owner_for_test(owner);
        victim.set_type_ref_for_test(sim.interner.intern("GAPOWR"));
        victim.set_category_for_test(EntityCategory::Structure);
        victim
            .health
            .set_for_test(i32::try_from(row["before"]["health"].as_i64().unwrap()).unwrap());
        victim.tracking_facts.set_dont_score_for_test(dont_score);
        sim.substrate.entities.insert(victim);
        let before_rng = sim.rng_state();
        assert_eq!(
            row["observation"]["rng_before"], row["observation"]["rng_after"],
            "{name}: native NULL control made no draw"
        );
        let expected = row["after"].as_array().unwrap();
        assert_eq!(
            expected.len() as u64,
            row["input"]["repetitions"].as_u64().unwrap(),
            "{name}"
        );
        for after in expected {
            sim.record_the_kill(
                1,
                crate::sim::combat::KillSource::Techno(None),
                crate::sim::combat::KillCallback::OwnerChange,
                crate::sim::world::UninitContext::with_rules(&rules),
            );
            assert_eq!(
                house_totals(&sim.houses[&owner]),
                capture_totals(after),
                "{name}"
            );
            let victim = sim.substrate.entities.get(1).unwrap();
            assert_eq!(victim.owner(), owner, "{name}: old owner still installed");
            assert_eq!(
                i64::from(victim.health.current()),
                after["health"].as_i64().unwrap(),
                "{name}"
            );
            assert!(
                !victim.kill_record.is_recorded(),
                "{name}: live capture is not death"
            );
            assert_eq!(sim.rng_state(), before_rng, "{name}");
        }
        compared.push(name);
    }
    assert_eq!(
        compared,
        [
            "live_null",
            "repeated_live_null",
            "null_dont_score",
            "null_insignificant",
            "null_loss_wrap"
        ]
    );
}

#[test]
fn native_change_owner_score_and_kill_counter_wrap() {
    // Checked capture_wrap:7015D0 ADD and70164D INC. Full original
    // Walk/PerCell/ChangeOwner produced both the price and retained outputs;
    // this checks the shared mutation owner, not a second capture algorithm.
    let corpus = capture_accounting_corpus();
    let rows = corpus["capture_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    let row = rows
        .iter()
        .find(|row| row["name"] == "capture_wrap")
        .unwrap();
    let native_price = row["observation"]["trace"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "capture_score_store")
        .unwrap()["price"]
        .as_i64()
        .unwrap();
    let (mut stats, score) = capture_totals(&row["before"]["new"]);
    let mut economy = crate::sim::economy::Economy::new(0);
    economy.set_score_for_test(score);
    economy.add_score(i32::try_from(native_price).unwrap());
    stats.record_kill(crate::map::entities::EntityCategory::Structure);
    assert_eq!(
        (stats, economy.score()),
        capture_totals(&row["after"]["new"])
    );
}

#[test]
fn native_record_last_built_counter_wrap() {
    // Original whole4FB6B0 enters the existing-capacity49FA00 branch;
    // item49FA47 and total49FA50 are retained native outputs. Counter growth
    // and full PLACE are outside this shared owner mutation comparison.
    let corpus = capture_accounting_corpus();
    let rows = corpus["built_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for (row, name) in rows.iter().zip(["built_zero", "built_wrap"]) {
        assert_eq!(row["name"], name);
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, None, true, 0, 10);
        house.set_produced_count_for_test(
            ObjectCategory::Building,
            0,
            capture_count(&row["before"]["item"]) as i32,
        );
        assert_eq!(house.stats.built(), capture_count(&row["before"]["total"]));
        house.record_scored_production(ObjectCategory::Building, 0);
        assert_eq!(
            house.stats.built(),
            capture_count(&row["after"]["total"]),
            "{name}"
        );
        assert_eq!(
            house.produced_count(ObjectCategory::Building, 0) as u32,
            capture_count(&row["after"]["item"]),
            "{name}"
        );
        house.validate_production_history().unwrap();
        assert_eq!(
            row["observation"]["rng_before"], row["observation"]["rng_after"],
            "{name}: native counter control made no draw"
        );
    }
}

#[test]
fn produced_history_distinguishes_equal_built_totals_and_last_types() {
    let fixture = || {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        (sim, owner)
    };
    let (mut first, owner) = fixture();
    let (mut second, other_owner) = fixture();
    assert_eq!(owner, other_owner);
    for (sim, prior_index) in [(&mut first, 0), (&mut second, 1)] {
        let house = sim.houses.get_mut(&owner).unwrap();
        house.record_scored_production(ObjectCategory::Infantry, prior_index);
        house.record_scored_production(ObjectCategory::Infantry, 1);
        house.record_built_type(ObjectCategory::Infantry, 1);
        house.validate_production_history().unwrap();
    }
    assert_eq!(first.houses[&owner].stats, second.houses[&owner].stats);
    assert_eq!(
        first.houses[&owner].last_built_types,
        second.houses[&owner].last_built_types
    );
    assert_ne!(first.state_hash(), second.state_hash());
    let encoded = serde_json::to_vec(&first.houses[&owner]).unwrap();
    let restored: HouseState = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(restored.produced_count(ObjectCategory::Infantry, 0), 1);
    assert_eq!(restored.produced_count(ObjectCategory::Infantry, 1), 1);
    assert_eq!(restored.produced_count(ObjectCategory::Vehicle, 0), 0);
    assert_eq!(restored.produced_count(ObjectCategory::Infantry, 65), 0);
    restored.validate_production_history().unwrap();
}

#[test]
fn saved_produced_history_rejects_inconsistent_cache_or_sparse_items() {
    for corruption in 0..3 {
        let mut sim = Simulation::new();
        sim.session
            .set_map_name_for_test("PRODUCED-HISTORY.MAP".into());
        let owner = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, None, true, 0, 10);
        house.record_scored_production(ObjectCategory::Infantry, 0);
        match corruption {
            0 => house.stats.built += 1,
            1 => {
                house.produced_type_counts[ObjectCategory::Infantry as usize].insert(-1, 1);
            }
            _ => {
                house.produced_type_counts[ObjectCategory::Infantry as usize].insert(2, 0);
            }
        }
        sim.houses.register(house);
        let bytes = GameSnapshot::save_validated(&sim, 1, 2, "invalid produced history", 0);
        let mut saved = GameSnapshot::load_validated(&bytes, 1, 2, "PRODUCED-HISTORY.MAP")
            .unwrap()
            .sim;
        assert!(matches!(
            saved.restore_after_snapshot_load(),
            Err(crate::sim::snapshot::SnapshotRestoreError::InvalidHouseProductionHistory {
                owner: actual,
                ..
            }) if actual == owner
        ));
    }
}

#[test]
fn house_capture_notification_is_retained_and_hashed() {
    use std::hash::Hasher;

    let corpus = capture_accounting_corpus();
    let native = corpus["capture_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "capture_stock")
        .unwrap();
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, true, 0, 10);
    let hash = |house: &HouseState| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        house.hash_event_notifications(&mut hasher);
        hasher.finish()
    };
    assert_eq!(
        house.building_capture_notified(),
        native["before"]["old"]["notification"].as_u64().unwrap() != 0
    );
    let before = hash(&house);
    house.notify_building_capture();
    assert_eq!(
        house.building_capture_notified(),
        native["after"]["old"]["notification"].as_u64().unwrap() != 0
    );
    assert_ne!(hash(&house), before);
    let encoded = serde_json::to_vec(&house).unwrap();
    let restored: HouseState = serde_json::from_slice(&encoded).unwrap();
    assert!(restored.building_capture_notified());
    assert_eq!(hash(&restored), hash(&house));
    house.notify_building_capture();
    assert_eq!(hash(&house), hash(&restored));
}

#[test]
fn house_current_viewer_discovery_is_retained_without_changing_peer_hash() {
    // Original House ComputeCRC502D60 omits +1F4 just as64DAB0 omits
    // Techno41A/B/C. Distinct client viewers cannot diverge the peer hash
    // merely by retaining their own observation of this House.
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    assert!(!sim.houses[&owner].discovered_by_current_house());
    let prior = sim.state_hash();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .notify_discovered_by_current_house();
    assert_eq!(sim.state_hash(), prior);
    let encoded = serde_json::to_vec(&sim.houses[&owner]).unwrap();
    let restored: HouseState = serde_json::from_slice(&encoded).unwrap();
    assert!(restored.discovered_by_current_house());
    sim.houses.replace_for_test(restored);
    assert_eq!(sim.state_hash(), prior);
}

/// The house's statistics and score as the corpus records them.
fn native_totals(fields: &Value) -> (MatchStatistics, i32) {
    let count = |field: &Value| u32::try_from(field.as_i64().unwrap()).unwrap();
    let table_total = |name: &str| fields[name].as_array().unwrap().iter().map(count).sum();
    let stats = MatchStatistics {
        units_killed: table_total("units_killed"),
        buildings_killed: table_total("buildings_killed"),
        units_lost: count(&fields["units_lost"]),
        buildings_lost: count(&fields["buildings_lost"]),
        built: fields["built"]
            .as_array()
            .unwrap()
            .iter()
            .map(|counter| count(&counter["total"]))
            .sum(),
    };
    (
        stats,
        i32::try_from(fields["score"].as_i64().unwrap()).unwrap(),
    )
}

#[test]
fn native_house_statistics_survive_snapshot_and_retained_ship_terminal_record() {
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_house_stats.json",
    ))
    .unwrap();
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let mut sim = Simulation::new();
        sim.session
            .set_map_name_for_test("HOUSE-STATS.MAP".to_owned());
        // Whole Scenario load has its own seed-zero contract. The native
        // comparison here establishes the House fields across its Save/Load.
        sim.scenario_rng = SimRng::new(0);
        let owner = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        let (stats, score) = native_totals(&row["before"]);
        house.stats = stats;
        for (counter, category) in row["before"]["built"].as_array().unwrap().iter().zip([
            ObjectCategory::Building,
            ObjectCategory::Vehicle,
            ObjectCategory::Infantry,
            ObjectCategory::Aircraft,
        ]) {
            for (index, count) in counter["items"].as_array().unwrap().iter().enumerate() {
                house.set_produced_count_for_test(
                    category,
                    i32::try_from(index).unwrap(),
                    i32::try_from(count.as_i64().unwrap()).unwrap(),
                );
            }
        }
        assert_eq!(house.stats, stats, "native Built total equals its item sum");
        house.economy.set_score_for_test(score);
        sim.houses.register(house);
        let id = sim.substrate.allocate_stable_id();
        let mut hull = GameEntity::test_default(id, "AEGIS", "Americans", 113, 59);
        hull.set_type_ref_for_test(sim.interner.intern("AEGIS"));
        hull.set_owner_for_test(owner);
        hull.set_category_for_test(crate::map::entities::EntityCategory::Unit);
        hull.health.set_for_test(1);
        sim.substrate.entities.insert(hull);
        let bytes = GameSnapshot::save_validated(&sim, 1, 2, "native House state", 3);
        let mut restored = GameSnapshot::load_validated(&bytes, 1, 2, "HOUSE-STATS.MAP")
            .unwrap()
            .sim;
        restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(
            house_totals(&restored.houses[&owner]),
            native_totals(&row["after"])
        );
        for (counter, category) in row["after"]["built"].as_array().unwrap().iter().zip([
            ObjectCategory::Building,
            ObjectCategory::Vehicle,
            ObjectCategory::Infantry,
            ObjectCategory::Aircraft,
        ]) {
            for (index, count) in counter["items"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    i64::from(restored.houses[&owner].produced_count(category, index as i32)),
                    count.as_i64().unwrap(),
                    "{category:?} produced item{index} survives the saved world"
                );
            }
        }
        assert_eq!(restored.state_hash(), sim.state_hash());
        assert_eq!(restored.rng_state(), sim.rng_state());
        if let Some(expected) = row["terminal_record_loss"].as_u64() {
            let rng = restored.rng_state();
            restored.record_the_kill(
                id,
                crate::sim::combat::KillSource::Techno(None),
                crate::sim::combat::KillCallback::Terminal,
                crate::sim::world::UninitContext::with_rules(&rules),
            );
            assert_eq!(
                u64::from(restored.houses[&owner].stats.units_lost()),
                expected
            );
            assert_eq!(restored.rng_state(), rng);
        }
    }
}

#[test]
fn saved_live_statistics_preserve_terminal_score_and_rng_continuation() {
    let mut sim = Simulation::new();
    sim.session.set_map_name_for_test("SCORE.MAP".to_owned());
    sim.scenario_rng = SimRng::new(0);
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, false, 0, 10);
    house.stats = MatchStatistics {
        units_killed: 2,
        buildings_killed: 1,
        units_lost: 1,
        buildings_lost: 3,
        built: 0,
    };
    // Explicit four-produced-item prior for this synthetic score scene.
    house.set_produced_count_for_test(ObjectCategory::Infantry, 0, 4);
    house.economy.set_score_for_test(700);
    sim.houses.register(house);
    let bytes = GameSnapshot::save_validated(&sim, 1, 2, "before score edge", 3);
    let mut restored = GameSnapshot::load_validated(&bytes, 1, 2, "SCORE.MAP")
        .unwrap()
        .sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
    restored.restore_after_snapshot_load().unwrap();
    let before_rng = sim.rng_state();
    assert!(Simulation::finalize_terminal_score_snapshot(
        &sim.session,
        &mut sim.scenario_rng,
        &mut sim.terminal_score_snapshot,
        &sim.houses
    ));
    assert!(Simulation::finalize_terminal_score_snapshot(
        &restored.session,
        &mut restored.scenario_rng,
        &mut restored.terminal_score_snapshot,
        &restored.houses
    ));
    assert_ne!(
        sim.rng_state(),
        before_rng,
        "positive score consumes its existing bonus draw"
    );
    assert_eq!(
        restored.terminal_score_snapshot(),
        sim.terminal_score_snapshot()
    );
    assert_eq!(restored.rng_state(), sim.rng_state());
    assert_eq!(restored.state_hash(), sim.state_hash());
}
