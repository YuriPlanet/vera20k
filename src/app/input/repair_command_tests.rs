//! Rust repair input admission and ordinary app scheduler controls.
//! The gate is shared with the cursor/click owner; window coordinates, native
//! mouse input and native OutList execution are outside these component tests.

use super::{own_repairable_building_id_for_owner, schedule_command_in_sim};
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::command_queue::OUT_LIST_CAPACITY;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::production;
use crate::sim::world::Simulation;

fn fixture() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=REPAIRABLE\n1=NOREPAIR\n2=NOCLICK\n\
         [VehicleTypes]\n0=UNIT\n\
         [REPAIRABLE]\nStrength=1000\nCost=2500\nRepairable=yes\nClickRepairable=yes\n\
         [NOREPAIR]\nStrength=1000\nRepairable=no\nClickRepairable=yes\n\
         [NOCLICK]\nStrength=1000\nRepairable=yes\nClickRepairable=no\n\
         [UNIT]\nStrength=1000\nRepairable=yes\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    // Registration order deliberately differs from the target/sender identity.
    let local = sim.intern("Local");
    let enemy = sim.intern("Enemy");
    for owner in [enemy, local] {
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, 0, None, true, 5000, 10));
    }
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(local));
    sim.session_mut_for_test().set_tick_for_test(123);
    sim.session_mut_for_test().set_binary_frame_for_test(77);

    for (id, owner, type_name, health, category) in [
        (42, local, "REPAIRABLE", 600, EntityCategory::Structure),
        (43, enemy, "REPAIRABLE", 600, EntityCategory::Structure),
        (44, local, "REPAIRABLE", 1000, EntityCategory::Structure),
        (45, local, "NOREPAIR", 600, EntityCategory::Structure),
        (46, local, "NOCLICK", 600, EntityCategory::Structure),
        (47, local, "UNIT", 600, EntityCategory::Unit),
        (48, local, "REPAIRABLE", 0, EntityCategory::Structure),
    ] {
        let type_ref = sim.intern(type_name);
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            12,
            12,
            0,
            0,
            owner,
            Health::new(health),
            type_ref,
            category,
            0,
            5,
            false,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        sim.entities_mut_for_test().insert(entity);
    }
    (sim, rules)
}

#[test]
fn repair_mode_admission_keeps_own_building_and_can_repair_gates() {
    let (sim, rules) = fixture();
    assert_eq!(
        own_repairable_building_id_for_owner(&sim, &rules, "Local", 42),
        Some(42)
    );
    for (id, reason) in [
        (43, "enemy"),
        (44, "full health"),
        (45, "Repairable=no"),
        (46, "ClickRepairable=no"),
        (47, "mobile"),
        (48, "dead"),
        (49, "missing"),
    ] {
        assert_eq!(
            own_repairable_building_id_for_owner(&sim, &rules, "Local", id),
            None,
            "repair input refuses {reason}"
        );
    }
    assert!(sim.pending_command_snapshot().is_empty());
}

#[test]
fn admitted_repair_uses_saved_native_sender_frame_and_actor_without_toggling() {
    let (mut sim, rules) = fixture();
    let entity_id = own_repairable_building_id_for_owner(&sim, &rules, "Local", 42).unwrap();
    let local = sim.interner().get("Local").unwrap();
    let rng_before = sim.rng_state();
    assert_eq!(
        schedule_command_in_sim(&mut sim, "Local", Command::ToggleRepair { entity_id }),
        Some(123)
    );
    let record = {
        let queues = sim.network_command_queues();
        assert_eq!(queues.local_len(), 1);
        queues.local_records().unwrap()[0].clone()
    };
    assert_eq!(record.opcode(), 0x15);
    assert_eq!(record.house_id(), 1);
    assert_eq!(record.frame_stamp(), 77);
    assert_eq!(&record.payload()[..5], &[42, 0, 0, 0, 0x34]);
    assert_eq!(record.flags(), 0);
    assert!(record.payload()[5..].iter().all(|&byte| byte == 0));

    // Moving the current app/session inputs cannot rewrite an accepted event.
    let enemy = sim.interner().get("Enemy").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(enemy));
    sim.session_mut_for_test().set_tick_for_test(124);
    sim.session_mut_for_test().set_binary_frame_for_test(78);
    assert_eq!(
        sim.network_command_queues().local_records().unwrap()[0],
        &record
    );
    assert_eq!(
        sim.pending_command_snapshot(),
        [CommandEnvelope::new(
            local,
            123,
            Command::ToggleRepair { entity_id }
        )]
    );
    let building = sim.entities().get(entity_id).unwrap();
    assert!(!building.repairing.is_on());
    assert_eq!(building.health.current(), 600);
    assert_eq!(production::credits_for_owner(&sim, "Local"), 5000);
    assert_eq!(production::credits_for_owner(&sim, "Enemy"), 5000);
    assert_eq!(sim.rng_state(), rng_before);
}

#[test]
fn repair_uses_the_shared_outlist_capacity_and_full_input_stays_handled() {
    let (mut sim, rules) = fixture();
    let entity_id = own_repairable_building_id_for_owner(&sim, &rules, "Local", 42).unwrap();
    for _ in 0..OUT_LIST_CAPACITY - 1 {
        assert_eq!(
            schedule_command_in_sim(&mut sim, "Local", Command::Stop { entity_id }),
            Some(123)
        );
    }
    assert_eq!(
        schedule_command_in_sim(&mut sim, "Local", Command::ToggleRepair { entity_id }),
        Some(123)
    );
    let records_before: Vec<_> = {
        let queues = sim.network_command_queues();
        assert_eq!(queues.local_len(), OUT_LIST_CAPACITY);
        queues
            .local_records()
            .unwrap()
            .into_iter()
            .cloned()
            .collect()
    };
    assert!(
        records_before[..OUT_LIST_CAPACITY - 1]
            .iter()
            .all(|record| record.opcode() == 0x06)
    );
    assert_eq!(records_before.last().unwrap().opcode(), 0x15);

    // The ordinary input owner reports its issue tick even when the shared
    // OutList declines the record. The earlier accepted records stay intact.
    assert_eq!(
        schedule_command_in_sim(&mut sim, "Local", Command::ToggleRepair { entity_id }),
        Some(123)
    );
    let queues = sim.network_command_queues();
    assert_eq!(queues.local_len(), OUT_LIST_CAPACITY);
    assert_eq!(
        queues.local_records().unwrap(),
        records_before.iter().collect::<Vec<_>>()
    );
    assert!(!sim.entities().get(entity_id).unwrap().repairing.is_on());
}

#[test]
fn ordinary_repair_never_falls_back_to_semantic_when_native_encoding_fails() {
    let (mut sim, _) = fixture();
    assert_eq!(
        schedule_command_in_sim(
            &mut sim,
            "Local",
            Command::ToggleRepair {
                entity_id: u64::MAX
            }
        ),
        None
    );
    assert_eq!(
        schedule_command_in_sim(
            &mut sim,
            "Unregistered",
            Command::ToggleRepair { entity_id: 42 }
        ),
        None
    );
    assert!(sim.pending_command_snapshot().is_empty());
}
