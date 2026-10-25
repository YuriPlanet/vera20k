use super::*;
use serde_json::{Value, json};
use std::cell::Cell;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/object_health.json")
}

/// The scalar and callback-mutation comparisons call the same Object body as
/// production. This adapter supplies only storage and explicitly observed sinks.
struct LocalHealthHost<'a, F> {
    entity: &'a mut GameEntity,
    strength: &'a Cell<i32>,
    callback: F,
}

impl<F: FnMut(&mut GameEntity, HealthCallback)> ObjectHealthHost for LocalHealthHost<'_, F> {
    fn object(&self) -> Option<&GameEntity> {
        Some(self.entity)
    }

    fn object_mut(&mut self) -> Option<&mut GameEntity> {
        Some(self.entity)
    }

    fn current_strength(&self) -> Option<i32> {
        Some(self.strength.get())
    }

    fn invoke(&mut self, callback: HealthCallback) {
        (self.callback)(self.entity, callback);
    }
}

fn observe_virtual(
    entity: &mut GameEntity,
    callback: HealthCallback,
    mode: &str,
    kill_argument: i64,
    callbacks: &mut Vec<Value>,
) {
    // Tag visits have a separate observer. The original718 corpus
    // supplies an empty Tag attachment and records only these virtuals.
    if matches!(callback, HealthCallback::Tag { .. }) {
        return;
    }
    let slot = match callback {
        HealthCallback::Changed => "148",
        HealthCallback::Kill => "0E0",
        HealthCallback::Destroy => "0DC",
        HealthCallback::Tag { .. } => unreachable!(),
    };
    let argument = match callback {
        HealthCallback::Changed => 7,
        HealthCallback::Kill => kill_argument,
        HealthCallback::Destroy => 1,
        HealthCallback::Tag { .. } => unreachable!(),
    };
    let before = entity.health.current();
    let alive_before = u8::from(entity.lifecycle.object_alive());
    let mutation = match (callback, mode) {
        (HealthCallback::Changed, "heal_zero") => Some(0),
        (HealthCallback::Changed, "heal_negative") => Some(-17),
        (HealthCallback::Changed, "heal_above") | (HealthCallback::Destroy, "destroy_revive") => {
            Some(70001)
        }
        _ => None,
    };
    let mut writes = Vec::new();
    if let Some(health) = mutation {
        entity.health.set_for_test(health);
        writes.push(json!({"field":"actual", "value":health}));
    }
    if matches!(
        (callback, mode),
        (HealthCallback::Changed, "heal_clear_alive")
            | (HealthCallback::Destroy, "destroy_clear_alive")
            | (HealthCallback::Kill, "kill_clear_alive")
    ) {
        entity.lifecycle.set_object_alive_for_test(false);
        writes.push(json!({"field":"alive", "value":0}));
    }
    callbacks.push(json!({"slot":slot,"argument":argument,"health_before":before,
                    "alive_before":alive_before,"supplied_writes":writes,"health_after":entity.health.current(),
                    "alive_after":u8::from(entity.lifecycle.object_alive())}));
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
        entity
            .health
            .set_for_test(input["current"].as_i64().unwrap() as i32);
        entity
            .lifecycle
            .set_object_alive_for_test(input["alive"] == 1);
        entity.estimated_health =
            crate::sim::estimated_health::EstimatedHealth::from_raw(-987654321);
        let mut packet = input["damage"].as_i64().unwrap() as i32;
        let admitted = packet != 0;
        let mut callbacks = Vec::new();
        let strength = Cell::new(input["strength"].as_i64().unwrap() as i32);
        let callback = |entity: &mut GameEntity, callback: HealthCallback| {
            observe_virtual(
                entity,
                callback,
                input["callback"].as_str().unwrap(),
                0,
                &mut callbacks,
            );
        };
        let mut host = LocalHealthHost {
            entity: &mut entity,
            strength: &strength,
            callback,
        };
        let result = commit(
            &mut host,
            &mut packet,
            strength.get(),
            admitted,
            input["kind"] == "building" && input["can_c4"] == false,
            0.25,
            false,
        );
        drop(host);
        assert_eq!(
            entity.health.current(),
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
            u8::from(entity.lifecycle.object_alive()),
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
        (100, 100, false, 0, DamageState::AlreadyDead, false, false),
        (-17, 1, true, -17, DamageState::Unaffected, false, false),
    ] {
        let mut world = crate::sim::world::Simulation::new();
        let mut entity = GameEntity::test_default(1, "X", "Americans", 2, 2);
        entity.set_type_ref_for_test(interner.intern("X"));
        entity.set_owner_for_test(interner.intern("Americans"));
        entity.health.set_for_test(hp);
        entity.lifecycle.set_object_alive_for_test(alive);
        entity.estimated_health = crate::sim::estimated_health::EstimatedHealth::from_raw(-123);
        world.interner = interner.clone();
        world.substrate.entities.insert(entity);
        let event =
            EntityDamageEvent::area(1, damage, 0, super::super::RAD_NO_ATTACKER, None, warhead);
        let outcome = DamageOutcome {
            readiness_damage: None,
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
            &mut world,
            &rules,
            None,
            Some(resolved),
            crate::sim::world::FrameEffects::default(),
            false,
        )
        .unwrap();
        assert_eq!(
            world.substrate.entities.get(1).unwrap().health.current(),
            expected
        );
        assert_eq!(
            world
                .substrate
                .entities
                .get(1)
                .unwrap()
                .estimated_health
                .get(),
            -123
        );
        assert_eq!(receipt.state, expected_state);
        assert_eq!(receipt.reached_exact_zero, object_kill);
        assert_eq!(receipt.entered_techno_death, techno_death);
        // Result 5 returns before the survivor tail (`0x00701F80`).
        assert_eq!(receipt.uncloak_after_damage, alive && !techno_death);
        assert_eq!(
            receipt.synchronous_retaliation,
            alive && !techno_death && damage >= 0
        );
        if damage == 65536 {
            assert_eq!(receipt.positive_postlude.unwrap().0, 65536);
        }
    }
}

/// The integer `is_full` answers what the masked chop53 division and
/// FCOMP against 1.0 answer, including x/0, 0/0 and the i32 extremes.
#[test]
fn is_full_matches_the_masked_ratio_compare() {
    use crate::util::native_x87::MaskedX87Ordering::{Equal, Greater};
    let mut values = vec![i32::MIN, i32::MIN + 1, i32::MAX - 1, i32::MAX];
    for base in [0i32, 1, 2, 3, 100, 600, 1 << 24, 1 << 30] {
        for delta in [-1i32, 0, 1] {
            let v = base + delta;
            values.extend([v, -v]);
        }
    }
    for &strength in &values {
        for &health in &values {
            let health = Health::new(health);
            let native = matches!(health.compare_ratio(strength, 1.0), Equal | Greater);
            assert_eq!(health.is_full(strength), native, "{health:?} / {strength}");
        }
    }
}

#[path = "object_health_trigger_tests.rs"]
mod trigger_tests;
