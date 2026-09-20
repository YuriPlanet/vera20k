use super::*;
use serde_json::{Value, json};

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/object_health.json"
    ))
    .unwrap()
}

#[test]
fn actual_signed_object_commit_matches_original_packet_results_and_callbacks() {
    let corpus = corpus();
    let rows = corpus["receiver"].as_array().unwrap();
    assert_eq!(rows.len(), 718);
    for row in rows {
        let input = &row["input"];
        let output = &row["output"];
        let mut entity = GameEntity::test_default(1, "X", "Americans", 2, 2);
        entity.health.current = input["current"].as_i64().unwrap() as i32;
        entity.lifecycle.object_alive = input["alive"] == 1;
        entity.estimated_health =
            crate::sim::estimated_health::EstimatedHealth::from_raw(-987654321);
        let mut packet = input["damage"].as_i64().unwrap() as i32;
        let admitted = packet != 0;
        let mut callbacks = Vec::new();
        let result = commit(
            &mut entity,
            &mut packet,
            input["strength"].as_i64().unwrap() as i32,
            admitted,
            input["kind"] == "building" && input["can_c4"] == false,
            0.25,
            |entity, callback| {
                let slot = match callback {
                    HealthCallback::Changed => "148",
                    HealthCallback::Kill => "0E0",
                    HealthCallback::Destroy => "0DC",
                };
                let argument = match callback {
                    HealthCallback::Changed => 7,
                    HealthCallback::Kill => 0,
                    HealthCallback::Destroy => 1,
                };
                let before = entity.health.current;
                let alive_before = u8::from(entity.lifecycle.object_alive);
                let mode = input["callback"].as_str().unwrap();
                let mutation = match (callback, mode) {
                    (HealthCallback::Changed, "heal_zero") => Some(0),
                    (HealthCallback::Changed, "heal_negative") => Some(-17),
                    (HealthCallback::Changed, "heal_above")
                    | (HealthCallback::Destroy, "destroy_revive") => Some(70001),
                    _ => None,
                };
                let mut writes = Vec::new();
                if let Some(health) = mutation {
                    entity.health.current = health;
                    writes.push(json!({"field":"actual", "value":health}));
                }
                if matches!(
                    (callback, mode),
                    (HealthCallback::Changed, "heal_clear_alive")
                        | (HealthCallback::Destroy, "destroy_clear_alive")
                        | (HealthCallback::Kill, "kill_clear_alive")
                ) {
                    entity.lifecycle.object_alive = false;
                    writes.push(json!({"field":"alive", "value":0}));
                }
                callbacks.push(json!({"slot":slot,"argument":argument,"health_before":before,
                    "alive_before":alive_before,"supplied_writes":writes,"health_after":entity.health.current,
                    "alive_after":u8::from(entity.lifecycle.object_alive)}));
            },
        );
        assert_eq!(
            entity.health.current,
            output["actual"].as_i64().unwrap() as i32,
            "{input}"
        );
        assert_eq!(entity.estimated_health.get(), -987654321, "{input}");
        assert_eq!(
            packet,
            output["damage_packet"].as_i64().unwrap() as i32,
            "{input}"
        );
        assert_eq!(
            result as i32,
            output["result"].as_i64().unwrap() as i32,
            "{input}"
        );
        assert_eq!(
            u8::from(entity.lifecycle.object_alive),
            output["alive"].as_u64().unwrap() as u8,
            "{input}"
        );
        assert_eq!(json!(callbacks), output["callbacks"], "{input}");
    }
}

#[test]
fn receiver_commit_keeps_negative_heal_on_exact_zero_techno_tail_and_wide_hits() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::combat::{EntityDamageEvent, ResolvedReceiveDamage, damage::DamageOutcome};
    use crate::sim::entity_store::EntityStore;
    use crate::sim::intern::test_interner;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=X\n[X]\nStrength=70000\nArmor=none\n",
    ))
    .unwrap();
    let mut interner = test_interner();
    let warhead = interner.intern("WH");
    for (hp, damage, alive, expected, expected_state, object_kill, techno_death) in [
        (
            1,
            i32::MIN,
            true,
            -2147483647,
            DamageState::Unaffected,
            false,
            false,
        ),
        (65536, 65536, true, 0, DamageState::Dead, true, true),
        (100, 1, false, 99, DamageState::AlreadyDead, false, false),
        (100, 100, false, 0, DamageState::AlreadyDead, false, true),
        (-17, 1, true, -17, DamageState::Unaffected, false, false),
    ] {
        let mut entities = EntityStore::new();
        let mut entity = GameEntity::test_default(1, "X", "Americans", 2, 2);
        entity.type_ref = interner.intern("X");
        entity.owner = interner.intern("Americans");
        entity.health.current = hp;
        entity.lifecycle.object_alive = alive;
        entity.estimated_health = crate::sim::estimated_health::EstimatedHealth::from_raw(-123);
        entities.insert(entity);
        let event =
            EntityDamageEvent::area(1, damage, 0, super::super::RAD_NO_ATTACKER, None, warhead);
        let outcome = DamageOutcome {
            apply_object_damage: hp > 0,
            hp_delta: if hp > 0 { damage } else { 0 },
            post_object_damage: Some(damage),
            psychedelic_value: None,
            invulnerability_impact_damage: None,
            reached_survivor_postlude: true,
        };
        let resolved = ResolvedReceiveDamage {
            outcome,
            invulnerability_impact: None,
        };
        let receipt = super::super::receiver_health::commit_receiver_health(
            &event,
            &mut entities,
            &rules,
            &interner,
            &Default::default(),
            None,
            None,
            Some(Some(resolved)),
            0,
        )
        .unwrap();
        assert_eq!(entities.get(1).unwrap().health.current, expected);
        assert_eq!(entities.get(1).unwrap().estimated_health.get(), -123);
        assert_eq!(receipt.state, expected_state);
        assert_eq!(receipt.reached_exact_zero, object_kill);
        assert_eq!(receipt.entered_techno_death, techno_death);
        assert_eq!(receipt.uncloak_after_damage, !techno_death);
        if damage == 65536 {
            assert_eq!(receipt.positive_postlude.unwrap().0, 65536);
        }
    }
}
