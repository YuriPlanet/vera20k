//! Selection4AE750's ordered object loop4AE829..4AE85A, independent of capability grouping.
use super::*;

#[test]
fn mixed_engineer_c4_batches_restore_original_selection_order() {
    let owner = InternedId::default();
    let wrap = |payload| CommandEnvelope::new(owner, 42, payload);
    let capture = Command::CaptureBuilding {
        engineer_id: 20,
        target_building_id: 100,
    };
    let c4 = Command::PlantC4 {
        attacker_id: 30,
        target_building_id: 100,
    };
    let attack = Command::Attack {
        attacker_id: 10,
        target_id: 100,
    };
    let mut queued = vec![
        wrap(capture.clone()),
        wrap(c4.clone()),
        wrap(attack.clone()),
    ];
    restore_selection_dispatch_order(&mut queued, &[30, 10, 20]);
    assert_eq!(
        queued.iter().map(|e| &e.payload).collect::<Vec<_>>(),
        vec![&c4, &attack, &capture]
    );
    assert!(queued.iter().all(|e| e.execute_tick == 42));
}

#[test]
fn same_actor_orders_keep_their_original_sequence() {
    let owner = InternedId::default();
    let wrap = |payload| CommandEnvelope::new(owner, 42, payload);
    let mut queued = vec![
        wrap(Command::Stop { entity_id: 20 }),
        wrap(Command::Stop { entity_id: 10 }),
        wrap(Command::CaptureBuilding {
            engineer_id: 20,
            target_building_id: 100,
        }),
    ];
    restore_selection_dispatch_order(&mut queued, &[10, 20]);
    assert_eq!(command_actor_id(&queued[0].payload), Some(10));
    assert!(matches!(queued[1].payload, Command::Stop { entity_id: 20 }));
    assert!(matches!(
        queued[2].payload,
        Command::CaptureBuilding {
            engineer_id: 20,
            ..
        }
    ));
}

#[test]
fn prepared_factory_rallies_and_mobile_orders_keep_native_selection_order() {
    let owner = InternedId::default();
    let wrap = |payload| CommandEnvelope::new(owner, 42, payload);
    let mut queued = vec![
        wrap(Command::SetRally {
            rx: 7,
            ry: 8,
            producer_ids: vec![30],
        }),
        wrap(Command::SetRally {
            rx: 9,
            ry: 10,
            producer_ids: vec![20],
        }),
        wrap(Command::SellBuilding { entity_id: 20 }),
        wrap(Command::Move {
            entity_id: 10,
            target_rx: 7,
            target_ry: 8,
        }),
    ];
    restore_selection_dispatch_order(&mut queued, &[30, 10, 20]);
    assert_eq!(
        queued
            .iter()
            .map(|envelope| command_actor_id(&envelope.payload))
            .collect::<Vec<_>>(),
        vec![Some(30), Some(10), Some(20), Some(20)]
    );
    assert!(matches!(
        &queued[2].payload,
        Command::SetRally { rx: 9, ry: 10, .. }
    ));
    assert!(matches!(
        &queued[3].payload,
        Command::SellBuilding { entity_id: 20 }
    ));
}

#[test]
fn aggregated_rally_batch_keeps_existing_dispatch_order() {
    let owner = InternedId::default();
    let mut queued = vec![
        CommandEnvelope::new(
            owner,
            42,
            Command::SetRally {
                rx: 7,
                ry: 8,
                producer_ids: vec![30, 20],
            },
        ),
        CommandEnvelope::new(
            owner,
            42,
            Command::Move {
                entity_id: 10,
                target_rx: 7,
                target_ry: 8,
            },
        ),
    ];
    let original = queued.clone();
    restore_selection_dispatch_order(&mut queued, &[30, 10, 20]);
    assert_eq!(queued, original);
}
