//! The existing total-only score model through native House save/load values.
//! Native field retention is compared; per-house kill-table aggregation and
//! the score model's harvested/kill split are not newly certified by this test.

use super::*;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::game_entity::GameEntity;
use crate::sim::rng::SimRng;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;
use serde_json::Value;

fn capture_accounting_corpus() -> Value {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_capture_accounting.json",
    ))
    .unwrap();
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(
        corpus["kind"],
        "bounded-original-engineer-capture-accounting"
    );
    assert_eq!(
        corpus["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    corpus
}

fn capture_count(value: &Value) -> u32 {
    u32::try_from(value.as_u64().unwrap()).unwrap()
}

fn capture_totals(fields: &Value) -> MatchStatistics {
    let building_kills = fields["building_kills"]
        .as_array()
        .unwrap()
        .iter()
        .fold(0_u32, |total, count| {
            total.wrapping_add(capture_count(count))
        });
    MatchStatistics::from_totals_for_test(
        0,
        building_kills,
        0,
        capture_count(&fields["building_losses"]),
        0,
        i32::try_from(fields["score"].as_i64().unwrap()).unwrap(),
    )
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
        house.stats = capture_totals(&row["before"]);
        sim.houses.insert(owner, house);
        let mut victim = GameEntity::test_default(1, "GAPOWR", "Americans", 10, 10);
        victim.owner = owner;
        victim.type_ref = sim.interner.intern("GAPOWR");
        victim.category = EntityCategory::Structure;
        victim.health.current = i32::try_from(row["before"]["health"].as_i64().unwrap()).unwrap();
        victim.dont_score = dont_score;
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
                None,
                None,
                crate::sim::combat::KillCallback::OwnerChange,
                &rules,
            );
            assert_eq!(sim.houses[&owner].stats, capture_totals(after), "{name}");
            let victim = sim.substrate.entities.get(1).unwrap();
            assert_eq!(victim.owner(), owner, "{name}: old owner still installed");
            assert_eq!(
                i64::from(victim.health.current),
                after["health"].as_i64().unwrap(),
                "{name}"
            );
            assert!(
                !victim.destruction_recorded,
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
    let mut stats = capture_totals(&row["before"]["new"]);
    stats.add_score(i32::try_from(native_price).unwrap());
    stats.record_kill(crate::map::entities::EntityCategory::Structure);
    assert_eq!(stats, capture_totals(&row["after"]["new"]));
}

#[test]
fn native_record_last_built_counter_wrap() {
    // Original whole4FB6B0 enters the existing-capacity49FA00 branch;
    // item49FA47 and total49FA50 are retained native outputs. Counter growth
    // and full PLACE are outside this shared total-only mutation comparison.
    let corpus = capture_accounting_corpus();
    let rows = corpus["built_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for (row, name) in rows.iter().zip(["built_zero", "built_wrap"]) {
        assert_eq!(row["name"], name);
        let mut stats = MatchStatistics::from_totals_for_test(
            0,
            0,
            0,
            0,
            capture_count(&row["before"]["total"]),
            0,
        );
        stats.record_built();
        assert_eq!(
            stats.built(),
            capture_count(&row["after"]["total"]),
            "{name}"
        );
        assert_eq!(row["after"]["item"], row["after"]["total"], "{name}");
        assert_eq!(
            row["observation"]["rng_before"], row["observation"]["rng_after"],
            "{name}: native counter control made no draw"
        );
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
        .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
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
    sim.houses.insert(owner, restored);
    assert_eq!(sim.state_hash(), prior);
}

fn native_totals(fields: &Value) -> MatchStatistics {
    let count = |field: &Value| u32::try_from(field.as_i64().unwrap()).unwrap();
    let table_total = |name: &str| fields[name].as_array().unwrap().iter().map(count).sum();
    MatchStatistics {
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
        // These fixtures have no harvested credits, so the existing split
        // represents the native combined score with this one signed value.
        score_points: i32::try_from(fields["score"].as_i64().unwrap()).unwrap(),
    }
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
        sim.session.map_name = "HOUSE-STATS.MAP".to_owned();
        // Whole Scenario load has its own seed-zero contract. The native
        // comparison here establishes the House fields across its Save/Load.
        sim.scenario_rng = SimRng::new(0);
        let owner = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.stats = native_totals(&row["before"]);
        sim.houses.insert(owner, house);
        sim.session.house_order.push(owner);
        let id = sim.allocate_stable_id();
        let mut hull = GameEntity::test_default(id, "AEGIS", "Americans", 113, 59);
        hull.type_ref = sim.interner.intern("AEGIS");
        hull.owner = owner;
        hull.category = crate::map::entities::EntityCategory::Unit;
        hull.health.current = 1;
        sim.substrate.entities.insert(hull);
        let bytes = GameSnapshot::save_validated(&sim, 1, 2, "native House state", 3);
        let mut restored = GameSnapshot::load_validated(&bytes, 1, 2, "HOUSE-STATS.MAP")
            .unwrap()
            .sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.houses[&owner].stats, native_totals(&row["after"]));
        assert_eq!(restored.state_hash(), sim.state_hash());
        assert_eq!(restored.rng_state(), sim.rng_state());
        if let Some(expected) = row["terminal_record_loss"].as_u64() {
            let rng = restored.rng_state();
            restored.record_the_kill(
                id,
                None,
                None,
                crate::sim::combat::KillCallback::Terminal,
                &rules,
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
    sim.session.map_name = "SCORE.MAP".to_owned();
    sim.scenario_rng = SimRng::new(0);
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, false, 0, 10);
    house.stats = MatchStatistics {
        units_killed: 2,
        buildings_killed: 1,
        units_lost: 1,
        buildings_lost: 3,
        built: 4,
        score_points: 700,
    };
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    let bytes = GameSnapshot::save_validated(&sim, 1, 2, "before score edge", 3);
    let mut restored = GameSnapshot::load_validated(&bytes, 1, 2, "SCORE.MAP")
        .unwrap()
        .sim;
    restored.retain_in_scenario_process_state_from(&sim);
    restored.restore_after_snapshot_load().unwrap();
    let before_rng = sim.rng_state();
    assert!(sim.finalize_terminal_score_snapshot());
    assert!(restored.finalize_terminal_score_snapshot());
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
