//! Snapshot tests for object lifecycle and cell occupancy: raw occupation and
//! air slots, cell fog, the live object order, limbo and pending-delete
//! authority, and the logic scheduler.

use super::*;

#[test]
fn gsi_04_12_raw_occupation_snapshot_roundtrip_preserves_both_planes() {
    let mut sim = Simulation::new();
    let ground_owner = sim.interner.intern("Americans");
    let deck_owner = sim.interner.intern("Russians");
    sim.substrate.raw_cell_occupation.mark_ground(17, 23, 0x23);
    sim.substrate.raw_cell_occupation.mark_deck(17, 23, 0xC4);
    sim.substrate
        .raw_cell_occupation
        .mark_ground_infantry(17, 23, 0x04, ground_owner);
    sim.substrate
        .raw_cell_occupation
        .mark_deck_infantry(17, 23, 0x08, deck_owner);
    sim.substrate.raw_cell_occupation.mark_ground(2, 31, 0x02);
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // occupation-plane persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_12_raw_occupation", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes)
            .expect("current snapshot header")
            .layout,
        super::snapshot_layout()
    );

    let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("restore transient caches without replacing raw bytes");

    assert_eq!(
        restored.substrate.raw_cell_occupation.ground_bits(17, 23),
        0x27
    );
    assert_eq!(
        restored.substrate.raw_cell_occupation.deck_bits(17, 23),
        0xCC
    );
    assert_eq!(
        restored
            .substrate
            .raw_cell_occupation
            .ground_infantry_owner(17, 23),
        Some(ground_owner)
    );
    assert_eq!(
        restored
            .substrate
            .raw_cell_occupation
            .deck_infantry_owner(17, 23),
        Some(deck_owner)
    );
    assert_eq!(
        restored.substrate.raw_cell_occupation.ground_bits(2, 31),
        0x02
    );
    assert_eq!(restored.state_hash(), expected_hash);
}

/// The cell `AltObject` slots (`CellClass+0xE0`) are a serialized authority:
/// which owner holds a cell, and when it was released, cannot be rebuilt
/// from entity positions or locomotor phase, so they must survive a save
/// verbatim and reach the state hash.
#[test]
fn air_slot_snapshot_roundtrip_preserves_the_cell_holders() {
    let empty_hash = Simulation::new().state_hash();
    let mut sim = Simulation::new();
    assert!(sim.substrate.set_cell_air_slot((17, 23), Some(41)));
    assert!(sim.substrate.set_cell_air_slot((2, 31), Some(99)));
    // `0x00487D70` refuses a claim while another object holds the slot,
    // which is what keeps one hovering Jumpjet per cell.
    assert!(!sim.substrate.set_cell_air_slot((17, 23), Some(42)));
    assert_eq!(sim.substrate.air_slots.holder(17, 23), Some(41));
    let expected_hash = sim.state_hash();
    assert_ne!(
        expected_hash, empty_hash,
        "held air slots must reach the state hash"
    );

    let bytes = GameSnapshot::save(&sim, 0, 0, "air_slot_roundtrip", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes)
            .expect("current snapshot header")
            .layout,
        super::snapshot_layout()
    );

    let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("restore transient caches without replacing the slots");

    assert_eq!(restored.substrate.air_slots.holder(17, 23), Some(41));
    assert_eq!(restored.substrate.air_slots.holder(2, 31), Some(99));
    assert_eq!(restored.substrate.air_slots.holder(5, 5), None);

    // The post-restore hash check is differential on purpose: a bare
    // `Simulation::new()` does not round-trip its own state hash, which a
    // no-slot control sim shows just as well, so comparing `restored`
    // against `expected_hash` would be measuring that instead. What matters
    // here is that the restored slots are still hash-visible.
    let control = GameSnapshot::save(&Simulation::new(), 0, 0, "air_slot_control", 0);
    let mut control = GameSnapshot::load(&control).expect("control snapshot").sim;
    control
        .restore_after_snapshot_load()
        .expect("restore the control");
    assert_ne!(
        restored.state_hash(),
        control.state_hash(),
        "restored air slots must still reach the state hash"
    );
}

/// Native r8 slot_claim_replacement_release retains+560=(10,10) while
/// replacement_claim stores+564=(11,10). A save must retain both Cells and
/// continue the executed same-owner/empty-release behavior from that point.
#[test]
fn foot_air_distinct_native_caches_roundtrip_and_continue_notifications() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::with_seed(0);
    sim.session.set_map_width_for_test(40);
    sim.session.set_map_height_for_test(40);
    let id = sim.substrate.allocate_stable_id();
    let owner = sim.intern("Americans");
    let type_ref = sim.intern("SHAD");
    let entity = GameEntity::new_at_frame_zero_for_test(
        id,
        10,
        10,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        EntityCategory::Unit,
        0,
        5,
        true,
    );
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(id);
    Simulation::aircraft_tracker_add(&sim.session, &mut sim.substrate, id);
    assert!(sim.substrate.set_cell_air_slot((10, 10), Some(id)));
    assert!(sim.substrate.set_cell_air_slot((11, 10), Some(id)));
    let retained = sim.substrate.entities.get(id).unwrap().clone();
    let expected_hash = sim.state_hash();

    // Independent values affect lockstep even when the raw grid, vector
    // membership, position and the other cache are unchanged.
    *sim.substrate.entities.get_mut(id).unwrap() =
        retained.clone().with_air_tracker_cell_for_test((11, 10));
    assert_ne!(
        sim.state_hash(),
        expected_hash,
        "Foot+560 is independently hashed"
    );
    *sim.substrate.entities.get_mut(id).unwrap() =
        retained.clone().with_air_slot_cell_for_test((10, 10));
    assert_ne!(
        sim.state_hash(),
        expected_hash,
        "Foot+564 is independently hashed"
    );
    *sim.substrate.entities.get_mut(id).unwrap() = retained;

    let bytes = GameSnapshot::save(&sim, 0, 0, "foot_air_caches", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("v290 Foot air snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("restore Foot air continuation");
    let actor = restored.substrate.entities.get(id).unwrap();
    assert_eq!(actor.air_tracker_cell(), (10, 10));
    assert_eq!(actor.air_slot_cell(), (11, 10));
    assert_eq!(restored.substrate.air_slots.holder(10, 10), None);
    assert_eq!(restored.substrate.air_slots.holder(11, 10), Some(id));
    assert_eq!(restored.state_hash(), expected_hash);

    for owner in [Some(id), None] {
        assert!(sim.substrate.set_cell_air_slot((11, 10), owner));
        assert!(restored.substrate.set_cell_air_slot((11, 10), owner));
        assert_eq!(restored.substrate.air_slots.holder(11, 10), None);
        let actor = restored.substrate.entities.get(id).unwrap();
        assert_eq!(actor.air_tracker_cell(), (10, 10));
        assert_eq!(actor.air_slot_cell(), (11, 10));
        assert_eq!(restored.state_hash(), sim.state_hash());
    }

    // A non-null slot Cell can coexist with an empty raw grid. Save that
    // executed stale-cache state too; load must not infer+564 from Cell+E0.
    let stale = GameSnapshot::save(&restored, 0, 0, "foot_air_stale_slot", 0);
    let mut stale = GameSnapshot::load(&stale)
        .expect("stale Foot slot snapshot")
        .sim;
    stale
        .restore_after_snapshot_load()
        .expect("restore stale slot cache");
    assert_eq!(
        stale.substrate.entities.get(id).unwrap().air_slot_cell(),
        (11, 10)
    );
    assert_eq!(
        stale.substrate.entities.get(id).unwrap().air_tracker_cell(),
        (10, 10)
    );
    assert_eq!(stale.substrate.air_slots.holder(11, 10), None);
    assert_eq!(stale.state_hash(), restored.state_hash());
    assert!(stale.substrate.set_cell_air_slot((12, 10), Some(id)));
    assert!(stale.substrate.set_cell_air_slot((12, 10), None));
    assert_eq!(
        stale.substrate.entities.get(id).unwrap().air_slot_cell(),
        (0, 0)
    );
    assert_eq!(
        stale.substrate.entities.get(id).unwrap().air_tracker_cell(),
        (10, 10)
    );
}

/// The executed unit_uninit_live_slot control retires the live Foot while
/// keeping+564 and its occupied Cell+E0. Only the later selected destructor
/// block clears that matching slot; a save between these phases is valid.
#[test]
fn foot_air_retired_cached_slot_survives_restore_until_physical_destruction() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::with_seed(0);
    sim.session.set_map_width_for_test(40);
    sim.session.set_map_height_for_test(40);
    let owner = sim.intern("Americans");
    let type_ref = sim.intern("SHAD");
    for rx in [10, 12] {
        let id = sim.substrate.allocate_stable_id();
        let entity = GameEntity::new_at_frame_zero_for_test(
            id,
            rx,
            10,
            0,
            0,
            owner,
            Health::new(100),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(entity);
        sim.add_entity_occupancy(id);
    }
    Simulation::aircraft_tracker_add(&sim.session, &mut sim.substrate, 1);
    assert!(sim.substrate.set_cell_air_slot((10, 10), Some(1)));
    assert!(sim.substrate.set_cell_air_slot((12, 10), Some(2)));
    let foreign_cache = sim.substrate.entities.get(2).unwrap().air_slot_cell();
    sim.uninit(1);
    let retired_hash = sim.state_hash();
    assert_eq!(sim.substrate.air_slots.holder(10, 10), Some(1));

    let bytes = GameSnapshot::save(&sim, 0, 0, "retired_foot_air_slot", 0);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("retired Foot snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("restore retired Foot before drain");
    let actor = restored.substrate.entities.get(1).unwrap();
    assert!(!actor.lifecycle.object_alive());
    assert!(actor.lifecycle.in_limbo());
    assert_eq!(actor.air_tracker_cell(), (0, 0));
    assert_eq!(actor.air_slot_cell(), (10, 10));
    assert_eq!(restored.substrate.air_slots.holder(10, 10), Some(1));
    assert_eq!(restored.substrate.pending_delete.object_ids(), vec![1]);
    assert_eq!(restored.state_hash(), retired_hash);

    // This exercises the existing Rust drain and shared finalization
    // integration; native proof here is bounded to the slot-cache block.
    restored.process_pending_delete();
    sim.process_pending_delete();
    assert!(restored.substrate.entities.get(1).is_none());
    assert_eq!(restored.substrate.air_slots.holder(10, 10), None);
    assert_eq!(restored.substrate.air_slots.holder(12, 10), Some(2));
    assert_eq!(
        restored.substrate.entities.get(2).unwrap().air_slot_cell(),
        foreign_cache
    );
    assert!(restored.substrate.pending_delete.is_empty());
    assert_eq!(restored.state_hash(), sim.state_hash());
}

#[test]
fn cell_fog_sensor_and_cloak_state_roundtrips_with_hash() {
    use crate::map::entities::EntityCategory;
    use crate::sim::cloak_disguise::CloakRuntime;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::new();
    let viewer = sim.interner.intern("AMERICANS");
    let source_owner = sim.interner.intern("RUSSIANS");
    let type_ref = sim.interner.intern("SUB");
    sim.fog.set_width_for_test(8);
    sim.fog.set_height_for_test(8);
    sim.fog
        .insert_fogged_object_footprint(viewer, 91, vec![(3, 3), (4, 3)]);
    sim.fog.sensors_add_at(viewer, (3, 3), 2);
    assert!(sim.fog.set_cloaked_by_house(7, 3, 3));
    assert!(
        sim.fog
            .draw_objects_cloaked(Some(source_owner), source_owner, 7, 3, 3)
    );
    let entity_id = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        entity_id,
        3,
        3,
        0,
        0,
        source_owner,
        Health::new(600),
        type_ref,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    let mut cloak = CloakRuntime::new(0);
    cloak.establish_unlimbo_fully_cloaked();
    entity.cloak = Some(Box::new(cloak));
    sim.substrate.entities.insert(entity);
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // fog/sensor/cloak persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "cell_fog_sensor_cloak", 0);
    let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

    assert_eq!(restored.state_hash(), expected_hash);
    assert_eq!(restored.fog.fogged_objects().len(), 1);
    assert!(restored.fog.has_sensor_for_house(viewer, 3, 3));
    assert!(restored.fog.is_cloaked_by_house(7, 3, 3));
    let restored_entity = restored.substrate.entities.get(entity_id).unwrap();
    assert_eq!(
        restored_entity.cloak,
        sim.substrate.entities.get(entity_id).unwrap().cloak
    );
}

/// The active order is serialized directly and restored verbatim — not
/// re-derived, not sorted. (ledger 13)
#[test]
fn saveload_restores_live_object_order_verbatim() {
    use crate::sim::game_entity::GameEntity;
    let mut sim = Simulation::new();
    for id in [10u64, 20, 30] {
        sim.substrate
            .entities
            .insert(GameEntity::test_default(id, "MTNK", "Americans", 5, 5));
        sim.register_live_object(id);
    }
    // Force an order whose sequence differs from stable-id order.
    sim.set_logic_order_for_test(vec![20, 10, 30]);

    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
    assert_eq!(restored.live_object_order_snapshot(), vec![20, 10, 30]);
}

/// Rust snapshots preserve a dead-limbo object's independent state and the
/// ordered pending-delete boundary instead of reconstructing either from
/// global storage or LogicVector membership.
#[test]
fn lifecycle_authority_pending_boundary_roundtrips_queue_and_state() {
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(7, "MTNK", "Americans", 5, 5);
    entity.lifecycle.set_object_alive_for_test(false);
    entity.lifecycle.set_in_limbo_for_test(true);
    entity.lifecycle.set_cell_marked_for_test(false);
    entity.seldom.in_logic_vector.set_for_test(false);
    entity.death.set_dying_for_test(true);
    entity
        .seldom
        .removal_facts
        .set_dirty_rect_eligible_for_test(true);
    entity
        .seldom
        .kill_record
        .set_destruction_recorded_for_test(true);
    sim.substrate.entities.insert(entity);
    for id in [7, 3, 7] {
        sim.substrate.pending_delete.queue_object(id);
    }
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // lifecycle-boundary persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash_before = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
    let restored_entity = restored.substrate.entities.get(7).expect("entity restored");

    assert!(!restored_entity.lifecycle.object_alive());
    assert!(restored_entity.lifecycle.in_limbo());
    assert!(!restored_entity.lifecycle.cell_marked());
    assert!(!restored_entity.seldom.in_logic_vector.is_member());
    assert!(restored_entity.death.is_dying());
    assert!(restored_entity.seldom.removal_facts.dirty_rect_eligible());
    assert!(restored_entity.seldom.kill_record.is_recorded());
    assert_eq!(
        restored.substrate.pending_delete.object_ids(),
        vec![7, 3, 7]
    );
    assert_eq!(restored.state_hash(), hash_before);
}

#[test]
fn lifecycle_authority_logic_rebuild_does_not_rederive_limbo_or_mark() {
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::new();
    let mut off_cell_member = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    off_cell_member.lifecycle.set_in_limbo_for_test(false);
    off_cell_member.lifecycle.set_cell_marked_for_test(false);
    sim.substrate.entities.insert(off_cell_member);
    sim.substrate
        .logic
        .try_push(1, crate::sim::world::ObjectKind::Entity)
        .expect("logic fixture append");

    let mut marked_non_member = GameEntity::test_default(2, "MTNK", "Americans", 6, 6);
    marked_non_member.lifecycle.set_in_limbo_for_test(false);
    marked_non_member.lifecycle.set_cell_marked_for_test(true);
    sim.substrate.entities.insert(marked_non_member);

    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
    restored.rebuild_logic_membership();

    let member = restored.substrate.entities.get(1).expect("member restored");
    assert!(member.seldom.in_logic_vector.is_member());
    assert!(!member.lifecycle.in_limbo());
    assert!(!member.lifecycle.cell_marked());

    let non_member = restored
        .substrate
        .entities
        .get(2)
        .expect("non-member restored");
    assert!(!non_member.seldom.in_logic_vector.is_member());
    assert!(!non_member.lifecycle.in_limbo());
    assert!(non_member.lifecycle.cell_marked());
}

#[test]
fn lifecycle_authority_bookkeeping_facts_roundtrip_and_change_state_hash() {
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::new();
    sim.substrate
        .entities
        .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // lifecycle bookkeeping persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let default_hash = sim.state_hash();

    let entity = sim.substrate.entities.get_mut(1).expect("fixture entity");
    entity
        .seldom
        .removal_facts
        .set_dirty_rect_eligible_for_test(true);
    entity
        .seldom
        .kill_record
        .set_destruction_recorded_for_test(true);
    let changed_hash = sim.state_hash();
    assert_ne!(default_hash, changed_hash);

    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
    let restored_entity = restored.substrate.entities.get(1).expect("entity restored");
    assert!(restored_entity.seldom.removal_facts.dirty_rect_eligible());
    assert!(restored_entity.seldom.kill_record.is_recorded());
    assert_eq!(restored.state_hash(), changed_hash);
}

#[test]
fn techno_playfield_membership_roundtrips_and_changes_state_hash_v87() {
    use crate::sim::game_entity::GameEntity;

    let mut sim = Simulation::new();
    sim.substrate
        .entities
        .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let default_hash = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_in_playfield(true);
    let member_hash = sim.state_hash();
    assert_ne!(default_hash, member_hash);

    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let restored = GameSnapshot::load(&bytes)
        .expect("v87 membership loads")
        .sim;
    assert!(restored.substrate.entities.get(1).unwrap().in_playfield());
    assert_eq!(restored.state_hash(), member_hash);
}

// --- Slice 1: reveal/conceal/unlimbo/uninit lifecycle chokepoint ---

/// Slice 3: `unlimbo(ge)` places the entity into BOTH the active order and
/// occupancy in one atomic call — a caller can never observe it in `logic`
/// without occupancy, because the method returns only after both. The
/// house tracks it (Add_Tracking, Added_To_Game). (No-op collapse: same
/// end state as the old 4-step.)
#[test]
fn unlimbo_ge_places_into_logic_and_occupancy_atomically() {
    use crate::sim::game_entity::GameEntity;
    let mut sim = Simulation::new();
    let ge = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    // `place_spawned` resolves the owner and Unlimbo the type against
    // `sim.interner`; test_default interns both in the test interner.
    *sim.interner = crate::sim::intern::test_interner();
    let (id, outcome) = sim.unlimbo(ge);
    assert!(matches!(outcome, RevealOutcome::Revealed { .. }));

    let e = sim.substrate.entities.get(id).expect("entity in store");
    assert!(
        e.seldom.in_logic_vector.is_member(),
        "must be in the active order"
    );
    assert!(!e.lifecycle.in_limbo());
    assert!(e.lifecycle.cell_marked());
    assert_eq!(sim.live_object_order_snapshot(), vec![id]);
    assert!(
        sim.substrate.occupancy.contains_entity(5, 5, id),
        "must be registered in its foundation cell",
    );
    #[cfg(debug_assertions)]
    sim.debug_assert_lifecycle_consistent();
}

/// Slice 3: `create_limbo(ge)` stores the entity and its house tracks it
/// (Add_Tracking) but leaves it OUT of the active order and OUT of occupancy (born InLimbo).
#[test]
fn create_limbo_leaves_entity_out_of_logic_and_occupancy() {
    use crate::sim::game_entity::GameEntity;
    let mut sim = Simulation::new();
    let mut ge = GameEntity::test_default(2, "E1", "Americans", 6, 6);
    // `place_spawned` resolves the owner against `sim.interner`; re-intern so
    // the id is valid there (test_default uses the thread-local test interner).
    ge.set_owner_for_test(sim.interner.intern("Americans"));
    let id = Simulation::create_limbo(&mut sim.substrate, &mut sim.houses, ge);

    let e = sim.substrate.entities.get(id).expect("entity in store");
    assert!(
        !e.seldom.in_logic_vector.is_member(),
        "limbo object is not an active member"
    );
    assert!(e.lifecycle.in_limbo());
    assert!(!e.lifecycle.cell_marked());
    assert!(sim.live_object_order_snapshot().is_empty());
    assert!(
        !sim.substrate.occupancy.contains_entity(6, 6, id),
        "limbo object must not occupy a cell",
    );
}

// --- LogicClass live count-reload pass (scheduler contract) ---

/// Insert an entity into the store and append it to the active order.
fn spawn_and_register(sim: &mut Simulation, id: u64) {
    use crate::sim::game_entity::GameEntity;
    sim.substrate
        .entities
        .insert(GameEntity::test_default(id, "MTNK", "Americans", 5, 5));
    sim.register_live_object(id);
}

/// Registering the same object twice is a no-op: the order keeps one entry
/// and the body runs for it exactly once.
#[test]
fn logic_scheduler_duplicate_registration_is_idempotent() {
    let mut sim = Simulation::new();
    spawn_and_register(&mut sim, 1);
    sim.register_live_object(1); // duplicate
    assert_eq!(sim.live_object_order_snapshot(), vec![1]);

    let mut visits = 0;
    sim.for_each_live_object(|_, id| {
        if id == 1 {
            visits += 1;
        }
    });
    assert_eq!(visits, 1);
}

/// Literal saved Cell lists survive re-entry order and actual restore.
/// Restore hydrates metadata; it does not replay the historical enter stamps.
#[test]
fn saveload_occupancy_list_order_survives_reentry() {
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;
    let mut sim = Simulation::new();
    for _ in 0..3 {
        let id = sim.substrate.allocate_stable_id();
        let mut entity = GameEntity::test_default(id, "E1", "Americans", 5, 5);
        entity.set_category_for_test(EntityCategory::Infantry);
        entity.lifecycle.set_in_limbo_for_test(false);
        sim.substrate.entities.insert(entity);
        sim.add_entity_occupancy(id);
    }
    *sim.interner = crate::sim::intern::test_interner();
    sim.remove_entity_occupancy(1);
    sim.add_entity_occupancy(1);
    let list = |sim: &Simulation| -> Vec<u64> {
        sim.substrate
            .occupancy
            .get(5, 5)
            .unwrap()
            .iter_layer(MovementLayer::Ground)
            .map(|o| o.entity_id)
            .collect()
    };
    assert_eq!(list(&sim), vec![1, 3, 2]);
    // Snapshot deserialization intentionally executes native Scenario Seed0.
    // Compare this state-only roundtrip on that same admitted RNG cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash = sim.state_hash();
    let saved = GameSnapshot::save(&sim, 0, 0, "re-entry list", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(list(&restored), vec![1, 3, 2]);
    assert_eq!(restored.state_hash(), hash);
    restored.remove_entity_occupancy(3);
    assert_eq!(list(&restored), vec![1, 2]);
    restored.add_entity_occupancy(3);
    assert_eq!(list(&restored), vec![3, 1, 2]);
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(list(&restored), vec![3, 1, 2], "fixup is idempotent");
}
