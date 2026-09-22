use super::*;
use crate::map::playfield::PlayfieldBounds;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::PendingBuildingFire;
use crate::sim::snapshot::GameSnapshot;
use crate::util::fixed_math::SimFixed;

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\nCloakingStages=9\nCloakDelay=.02\n\
         [AudioVisual]\nCloakSound=NavalUnitEmerge\n\
         [VehicleTypes]\n0=SUB\n1=RANKED\n2=DEST\n\
         [BuildingTypes]\n0=NAYARD\n\
         [SUB]\nStrength=600\nSpeed=4\nCloakable=yes\nCloakingSpeed=1\nSensorsSight=7\n\
         [DEST]\nStrength=600\nSpeed=6\nSensors=yes\nSensorsSight=8\n\
         [RANKED]\nStrength=600\nSpeed=4\nVeteranAbilities=CLOAK\nEliteAbilities=CLOAK\n\
         [NAYARD]\nStrength=1000\nWeaponsFactory=yes\n",
    ))
    .expect("stock cloak rules")
}

fn spawned_sub() -> (Simulation, RuleSet, u64) {
    let rules = rules();
    let mut sim = Simulation::with_seed(0xC10A_C001);
    sim.fog.width = 64;
    sim.fog.height = 64;
    sim.playfield_bounds = Some(PlayfieldBounds::from_normalized_local_size(
        64, 2, 2, 56, 52,
    ));
    let bounds = sim.playfield_bounds.unwrap();
    let (rx, ry) = (8u16..56)
        .flat_map(|ry| (8u16..56).map(move |rx| (rx, ry)))
        .find(|&(rx, ry)| bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
        .expect("interior mode-one cell");
    let id = sim
        .spawn_object_at_height("SUB", "Soviet", rx, ry, 0, 0, &rules)
        .unwrap();
    (sim, rules, id)
}

#[test]
fn stock_cloak_producer_healthy_trace_uses_type_speed_and_no_rng() {
    let (mut sim, rules, id) = spawned_sub();
    let before = sim.scenario_rng.logical_state();
    tick_stock_cloak_producer(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        1
    );
    assert_eq!(sim.scenario_rng.logical_state(), before);
    let entity = sim.substrate.entities.get(id).unwrap();
    assert!(matches!(
        sim.sound_events.as_slice(),
        [crate::sim::world::SimSoundEvent::CloakSound {
            sound_id,
            rx,
            ry,
            sub_x,
            sub_y,
            world_z_leptons: 0,
        }] if sound_id == "NavalUnitEmerge"
            && *rx == entity.position.rx
            && *ry == entity.position.ry
            && *sub_x == entity.position.sub_x
            && *sub_y == entity.position.sub_y
    ));
    assert_eq!(
        sim.interner.get("NavalUnitEmerge"),
        None,
        "the transient entering-cloak cue must not mutate serialized interner state"
    );
    for frame in 1..=5 {
        sim.session.binary_frame = frame;
        tick_stock_cloak_producer(&mut sim, id, &rules);
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        2
    );
    assert_eq!(
        sim.sound_events.len(),
        1,
        "states one/two do not replay CloakSound"
    );
}

#[test]
fn stock_cloak_producer_current_fire_and_weapons_factory_contact_block_entry() {
    let (mut sim, rules, id) = spawned_sub();
    sim.substrate.entities.get_mut(id).unwrap().attack_target = Some(AttackTarget::new(999));
    tick_stock_cloak_producer(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        0
    );

    sim.substrate.entities.get_mut(id).unwrap().attack_target = None;
    let yard = sim
        .spawn_object_at_height("NAYARD", "Soviet", 30, 40, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .radio_contacts
        .insert(yard);
    tick_stock_cloak_producer(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        0
    );

    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .radio_contacts
        .remove(yard);
    tick_stock_cloak_producer(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        1
    );
}

#[test]
fn fully_cloaked_sub_holding_a_target_does_not_surface() {
    // CORRECTION. This test used to assert the opposite, encoding two VERA
    // misreadings at once: that `ShouldUncloak @ 0x006FBC90` treats "holds an
    // attack target" as activity, and that its tail
    // `CellClass::IsVisibleToHouse @ 0x004870B0` means cell visibility.
    //
    // Native: the head is `IsCloakable() && !IsUnderEMP && !vt+0x380 &&
    // !WarpIn && !WarpOut`, with no target term anywhere, and the tail reads
    // the `CloakedByHouses` bit — never set in stock YR. So a submerged
    // Typhoon that acquires a destroyer stays submerged, and only losing the
    // ability to sustain the cloak surfaces it.
    let (mut sim, rules, id) = spawned_sub();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .establish_unlimbo_fully_cloaked();
    sim.substrate.entities.get_mut(id).unwrap().attack_target = Some(AttackTarget::new(999));
    tick_stock_cloak_producer(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        2,
        "holding a target is CanAutoCloak step 4, not a ShouldUncloak term"
    );
    assert!(
        sim.sound_events.is_empty(),
        "no transition means no positional cue"
    );

    // Marking the owner's own cell visible must not change the answer either —
    // that was the substituted predicate.
    let owner = sim.substrate.entities.get(id).unwrap().owner;
    let (rx, ry) = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| (entity.position.rx, entity.position.ry))
        .unwrap();
    sim.fog.mark_visible_for_owner(owner, rx, ry);
    tick_stock_cloak_producer(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        2
    );
}

#[test]
fn chrono_warp_surfaces_a_fully_cloaked_sub() {
    // The surviving live `ShouldUncloak` arm in stock data once EMP is excluded:
    // `vt+0x1D4`/`vt+0x1D8` WarpIn/WarpOut. `StartUncloaking(0)` owns the cue.
    let (mut sim, rules, id) = spawned_sub();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .establish_unlimbo_fully_cloaked();
    sim.substrate.entities.get_mut(id).unwrap().teleport_state =
        Some(crate::sim::movement::teleport_movement::TeleportState {
            phase: crate::sim::movement::teleport_movement::TeleportPhase::Relocate,
            target_rx: 20,
            target_ry: 20,
            being_warped_ticks: 0,
        });
    tick_stock_cloak_producer(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        3
    );
    assert!(matches!(
        sim.sound_events.as_slice(),
        [crate::sim::world::SimSoundEvent::CloakSound { sound_id, .. }]
            if sound_id == "NavalUnitEmerge"
    ));
}

#[test]
fn stock_cloak_producer_honors_rank_selected_cloak_ability() {
    let (mut sim, rules, sub) = spawned_sub();
    let (rx, ry) = sim
        .substrate
        .entities
        .get(sub)
        .map(|entity| (entity.position.rx, entity.position.ry))
        .unwrap();
    let ranked = sim
        .spawn_object_at_height("RANKED", "Soviet", rx, ry, 0, 0, &rules)
        .unwrap();
    assert!(sim.substrate.entities.get(ranked).unwrap().cloak.is_none());
    sim.substrate.entities.get_mut(ranked).unwrap().veterancy = 100;
    tick_stock_cloak_producer(&mut sim, ranked, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(ranked)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        1
    );
}

/// Set the `CloakedByHouses` bit `CellClass::IsVisibleToHouse @ 0x004870B0`
/// reads — the outer gate of `TechnoClass+0x420 @ 0x006F4F3A`. Only a
/// `CloakGenerator=yes` building writes it in gamemd and stock YR ships none,
/// so this arm is dormant in an ordinary skirmish; the tests below still pin
/// its collect/dispatch shape for a mod that does ship one.
fn arm_cloak_field_for(sim: &mut Simulation, owner: InternedId, cell: (u16, u16)) {
    if !sim.session.house_order.contains(&owner) {
        sim.session.house_order.push(owner);
    }
    sim.fog.reset_cloaked_by_houses();
    let index = sim
        .base_reservation_house_index(owner)
        .expect("registered house index") as u8;
    assert!(sim.fog.set_cloaked_by_house(index, cell.0, cell.1));
}

#[test]
fn sensor_deposit_alone_never_force_cloaks_an_eligible_unit() {
    // The DRIFT this slice corrects. `TechnoClass+0x420 @ 0x006F4EB0` gates its
    // cloak arm on `CellClass::IsVisibleToHouse` — the `CloakedByHouses` bit
    // whose only writers are `BuildingClass::UpdateGapGenerator_Tick @
    // 0x004551B9 / 0x004553B3`, reached only for a `CloakGenerator=yes`
    // building. VERA substituted `fog.is_cell_visible(owner, ...)`, which is
    // true for a unit standing on its own revealed cell, so every sensor
    // deposit that touched the cell force-cloaked it and played CloakSound.
    let (mut sim, rules, cloaker) = spawned_sub();
    let (cell, owner) = sim
        .substrate
        .entities
        .get(cloaker)
        .map(|entity| ((entity.position.rx, entity.position.ry), entity.owner))
        .unwrap();
    sim.fog.mark_visible_for_owner(owner, cell.0, cell.1);

    let outcome = sensor_reevaluate_stock_cloak(&mut sim, cloaker, &rules);

    assert_eq!(outcome, SensorCloakReevaluation::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(cloaker)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        0,
        "no cloak generator field means the vt+0x420 cloak arm never runs"
    );
    assert!(sim.sound_events.is_empty());
}

#[test]
fn sensor_callback_reassigns_admitted_targeters_in_forward_techno_registration_order() {
    let (mut sim, rules, cloaker) = spawned_sub();
    let (cell, cloaker_owner) = sim
        .substrate
        .entities
        .get(cloaker)
        .map(|entity| ((entity.position.rx, entity.position.ry), entity.owner))
        .unwrap();
    arm_cloak_field_for(&mut sim, cloaker_owner, cell);

    let sensor_admitted = sim
        .spawn_object_at_height("RANKED", "Americans", cell.0 + 1, cell.1, 0, 0, &rules)
        .unwrap();
    let excluded = sim
        .spawn_object_at_height("RANKED", "Neutral", cell.0 + 2, cell.1, 0, 0, &rules)
        .unwrap();
    let same_owner = sim
        .spawn_object_at_height("RANKED", "Soviet", cell.0 + 3, cell.1, 0, 0, &rules)
        .unwrap();
    let later_sensor_admitted = sim
        .spawn_object_at_height("RANKED", "Americans", cell.0 + 4, cell.1, 0, 0, &rules)
        .unwrap();
    for id in [sensor_admitted, excluded, same_owner, later_sensor_admitted] {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.attack_target = Some(AttackTarget::new(cloaker));
        entity.passively_acquired_target = true;
    }
    let american_owner = sim.substrate.entities.get(sensor_admitted).unwrap().owner;
    sim.fog.increment_sensor_at(american_owner, cell.0, cell.1);

    // Logic registration is deliberately opposite to prove the callback uses
    // Techno class-array construction order, not the active-object vector.
    sim.substrate.logic.set_order_for_test(vec![
        later_sensor_admitted,
        same_owner,
        excluded,
        sensor_admitted,
        cloaker,
    ]);
    let outcome = sensor_reevaluate_stock_cloak(&mut sim, cloaker, &rules);

    assert!(outcome.cloak_transitioned);
    assert_eq!(
        outcome.reassigned_targeters,
        vec![sensor_admitted, same_owner, later_sensor_admitted],
        "native reverse collect plus reverse dispatch is forward Techno registration order"
    );
    assert!(
        !sim.substrate
            .entities
            .get(sensor_admitted)
            .unwrap()
            .passively_acquired_target
    );
    assert!(
        !sim.substrate
            .entities
            .get(same_owner)
            .unwrap()
            .passively_acquired_target,
        "same owner admits without targeter sensor coverage"
    );
    assert!(
        !sim.substrate
            .entities
            .get(later_sensor_admitted)
            .unwrap()
            .passively_acquired_target
    );
    assert!(
        sim.substrate
            .entities
            .get(excluded)
            .unwrap()
            .passively_acquired_target,
        "an unrelated owner without sensor coverage is not collected"
    );
    assert!(matches!(
        sim.sound_events.as_slice(),
        [crate::sim::world::SimSoundEvent::CloakSound { sound_id, .. }]
            if sound_id == "NavalUnitEmerge"
    ));
}

#[test]
fn already_cloaked_object_is_refused_by_can_auto_cloak_step_two() {
    // CORRECTION. This test used to assert that a state-two object still ran
    // the vt+0x420 collect/reassign transaction and cleared its targeter's
    // passive provenance. It cannot: `0x006F4F57` calls `CanAutoCloak`
    // (vt+0x2A0) and jumps past the whole arm on false, and `CanAutoCloak @
    // 0x006FBDC0` returns false at its second step — `if (param_1[0x88] == 2)
    // return false;` — for exactly this object. VERA's `can_auto_cloak` was
    // missing that step, which is what let the collection run.
    let (mut sim, rules, cloaker) = spawned_sub();
    let (cell, cloaker_owner) = sim
        .substrate
        .entities
        .get(cloaker)
        .map(|entity| ((entity.position.rx, entity.position.ry), entity.owner))
        .unwrap();
    sim.substrate
        .entities
        .get_mut(cloaker)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .establish_unlimbo_fully_cloaked();

    let targeter = sim
        .spawn_object_at_height("NAYARD", "Soviet", cell.0 + 1, cell.1, 0, 0, &rules)
        .unwrap();
    {
        let entity = sim.substrate.entities.get_mut(targeter).unwrap();
        let mut attack = AttackTarget::new(cloaker);
        attack.cooldown_ticks = 17;
        entity.weapon_burst.complete_shot(4);
        attack.burst_delay_ticks = 4;
        entity.pending_building_fire = Some(PendingBuildingFire {
            remaining_ticks: 7,
            weapon_slot: WeaponSlot::Secondary,
        });
        entity.attack_target = Some(attack);
        entity.passively_acquired_target = true;
    }
    // Arm even the dormant cloak-field bit, to prove the refusal comes from
    // CanAutoCloak and not from the outer CloakedByHouses gate.
    arm_cloak_field_for(&mut sim, cloaker_owner, cell);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let mission_before = sim.substrate.entities.get(targeter).unwrap().mission;
    let hash_before = sim.state_hash();

    let outcome = sensor_reevaluate_stock_cloak(&mut sim, cloaker, &rules);

    assert_eq!(outcome, SensorCloakReevaluation::default());
    assert!(sim.sound_events.is_empty());
    assert_eq!(
        sim.substrate
            .entities
            .get(cloaker)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        2
    );
    let entity = sim.substrate.entities.get(targeter).unwrap();
    assert!(
        entity.passively_acquired_target,
        "the transaction never runs, so provenance is untouched"
    );
    let attack = entity.attack_target.as_ref().unwrap();
    assert_eq!(attack.target, TargetKind::Entity(cloaker));
    assert_eq!(attack.cooldown_ticks, 17);
    assert_eq!(entity.weapon_burst.index(), 1);
    assert_eq!(attack.burst_delay_ticks, 4);
    assert_eq!(
        entity.pending_building_fire,
        Some(PendingBuildingFire {
            remaining_ticks: 7,
            weapon_slot: WeaponSlot::Secondary,
        })
    );
    assert_eq!(entity.mission, mission_before);
    assert_eq!(
        sim.state_hash(),
        hash_before,
        "a refused callback writes no authoritative state"
    );

    let bytes = GameSnapshot::save(&sim, 0, 0, "sensor-targeter", 0);
    let restored = GameSnapshot::load(&bytes)
        .expect("v88 sensor targeter snapshot")
        .sim;
    assert_eq!(restored.state_hash(), hash_before);
    assert!(
        restored
            .substrate
            .entities
            .get(targeter)
            .unwrap()
            .passively_acquired_target
    );
}

#[test]
fn the_weapon_rearm_countdown_blocks_the_next_auto_cloak() {
    // `CanAutoCloak @ 0x006FBDC0` step 3 reads `TechnoClass+0x2EC/+0x2F4`,
    // written by `TechnoClass::Fire_At @ 0x006FDD50`. VERA documented this pair
    // as the ReCloak delay and left it unarmed, so a sub that surfaced to fire
    // could dive again on the very next tick.
    let (mut sim, rules, id) = spawned_sub();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .arm_rearm_gate(0, 20);

    for frame in 0..20 {
        sim.session.binary_frame = frame;
        tick_stock_cloak_producer(&mut sim, id, &rules);
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .cloak
                .as_ref()
                .unwrap()
                .state,
            0,
            "frame {frame} is still inside the ROF window"
        );
    }
    sim.session.binary_frame = 20;
    tick_stock_cloak_producer(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        1,
        "the rearm countdown has expired, so the auto-cloak may begin"
    );
}

#[test]
fn a_non_allied_sensors_neighbour_surfaces_a_cloaked_mover_on_cell_entry() {
    // `FootClass::PerCellProcess @ 0x004D8802..0x004D8829`, the only live
    // `Sensors=` consumer.
    let (mut sim, rules, id) = spawned_sub();
    let cell = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| (entity.position.rx, entity.position.ry))
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .establish_unlimbo_fully_cloaked();

    // An own-house neighbour is skipped by the `Is_Ally_ByObject` clause.
    let friendly = sim
        .spawn_object_at_height("DEST", "Soviet", cell.0 + 1, cell.1, 0, 0, &rules)
        .unwrap();
    assert!(!uncloak_on_sensor_neighbour_after_cell_entry(
        &mut sim, id, &rules
    ));
    assert!(sim.sound_events.is_empty());
    // Leave cell, Logic and display membership through their common owner
    // before removing storage; raw erase left a dangling display identity.
    sim.object_conceal(friendly);
    sim.substrate.entities.remove(friendly);

    // A hostile one with `Sensors=yes` forces the surface, with the cue.
    sim.spawn_object_at_height("DEST", "Americans", cell.0 + 1, cell.1, 0, 0, &rules)
        .unwrap();
    assert!(uncloak_on_sensor_neighbour_after_cell_entry(
        &mut sim, id, &rules
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        3
    );
    assert!(matches!(
        sim.sound_events.as_slice(),
        [crate::sim::world::SimSoundEvent::CloakSound { sound_id, .. }]
            if sound_id == "NavalUnitEmerge"
    ));
}

#[test]
fn start_cloaking_drops_every_targeter_whose_house_cannot_sense_the_cell() {
    // `StartCloaking @ 0x00703770` opens with `Detach_All(false)`; each
    // receiver's `TechnoClass::PointerExpired @ 0x007077C0` keeps the pointer
    // only when its OWN house senses the expiring object's cell, or when it
    // shares the owner.
    let (mut sim, rules, cloaker) = spawned_sub();
    let cell = sim
        .substrate
        .entities
        .get(cloaker)
        .map(|entity| (entity.position.rx, entity.position.ry))
        .unwrap();
    let sensing = sim
        .spawn_object_at_height("DEST", "Americans", cell.0 + 1, cell.1, 0, 0, &rules)
        .unwrap();
    // RANKED carries no `SensorsSight=`, so these two deposit nothing of their
    // own and stay genuinely blind to the cell.
    let blind = sim
        .spawn_object_at_height("RANKED", "Neutral", cell.0 + 2, cell.1, 0, 0, &rules)
        .unwrap();
    let same_owner = sim
        .spawn_object_at_height("RANKED", "Soviet", cell.0 + 3, cell.1, 0, 0, &rules)
        .unwrap();
    for targeter in [sensing, blind, same_owner] {
        sim.substrate
            .entities
            .get_mut(targeter)
            .unwrap()
            .attack_target = Some(AttackTarget::new(cloaker));
    }
    let american = sim.substrate.entities.get(sensing).unwrap().owner;
    sim.fog.increment_sensor_at(american, cell.0, cell.1);

    let timers_before: Vec<_> = [sensing, blind, same_owner]
        .map(|id| sim.substrate.entities.get(id).unwrap().passive_scan_timer)
        .to_vec();
    let mut expected_rng = sim.scenario_rng.clone();
    let rng_before = sim.scenario_rng.logical_state();
    tick_stock_cloak_producer(&mut sim, cloaker, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(cloaker)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        1
    );
    assert!(
        sim.substrate
            .entities
            .get(sensing)
            .unwrap()
            .attack_target
            .is_some(),
        "a house that senses the cell keeps firing through the dive"
    );
    assert!(
        sim.substrate
            .entities
            .get(same_owner)
            .unwrap()
            .attack_target
            .is_some(),
        "same owner is exempt"
    );
    assert!(
        sim.substrate
            .entities
            .get(blind)
            .unwrap()
            .attack_target
            .is_none(),
        "everyone else loses the pointer at the instant of StartCloaking"
    );

    // `TechnoClass::PointerExpired @ 0x007079D1..0x00707A0D` re-arms the
    // `+0x180/+0x188` targeting timer with `RandomRanged(4, 8)` — one Scenario
    // draw — before `Assign_Target(NULL)`, and only on the arm that actually
    // clears. So exactly one receiver here spends a draw. The healthy cloak
    // producer itself consumes none (see
    // `stock_cloak_producer_healthy_trace_uses_type_speed_and_no_rng`), so the
    // whole delta below belongs to the detach.
    assert_ne!(
        sim.scenario_rng.logical_state(),
        rng_before,
        "the dive must spend the targeting-delay draw"
    );
    let drawn = expected_rng.next_range_u32_inclusive(4, 8);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state(),
        "only the receiver that loses its target spends the RandomRanged(4, 8) draw"
    );
    let timers_after: Vec<_> = [sensing, blind, same_owner]
        .map(|id| sim.substrate.entities.get(id).unwrap().passive_scan_timer)
        .to_vec();
    assert_eq!(
        timers_after[0], timers_before[0],
        "a retained target leaves the sensing receiver's timer untouched"
    );
    assert_eq!(
        timers_after[2], timers_before[2],
        "the same-owner exemption also skips the re-arm"
    );
    assert_ne!(timers_after[1], timers_before[1]);
    assert_eq!(timers_after[1].start_frame, sim.session.binary_frame);
    assert_eq!(
        timers_after[1].duration, drawn,
        "the re-armed delay is the RandomRanged(4, 8) result"
    );
}

#[test]
fn a_dive_reaches_every_registered_object_but_leaves_radio_contacts_intact() {
    // `Detach_All(false)` runs the SAME `TechnoClass -> RadioClass ->
    // ObjectClass` body as UnInit over EVERY registered object, so a receiver
    // that never targeted the diving object is still visited. But
    // `RadioClass::PointerExpired @ 0x0065AAC0` nulls a matching sparse slot
    // only on a NONZERO control — `0065aaf0 TEST BL,BL / 0065aaf2 JZ` skips the
    // `0065aaf4 MOV dword ptr [EAX],0x0` on control 0. So a dive leaves the
    // contact standing and only the UnInit at the end breaks it.
    let (mut sim, rules, cloaker) = spawned_sub();
    let cell = sim
        .substrate
        .entities
        .get(cloaker)
        .map(|entity| (entity.position.rx, entity.position.ry))
        .unwrap();
    let bystander = sim
        .spawn_object_at_height("RANKED", "Neutral", cell.0 + 2, cell.1, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(bystander)
        .unwrap()
        .mark_live_contact_with(cloaker);
    // A non-targeter spends no draw: the re-arm is inside the Target arm.
    let rng_before = sim.scenario_rng.logical_state();

    tick_stock_cloak_producer(&mut sim, cloaker, &rules);

    assert!(
        sim.substrate
            .entities
            .get(bystander)
            .unwrap()
            .has_live_contact_with(cloaker),
        "a dive is control 0, so the radio contact survives the broadcast"
    );
    assert_eq!(sim.scenario_rng.logical_state(), rng_before);

    // Control 1 — `ObjectClass::UnInit` dispatches the same roster walk with a
    // nonzero control at `0x005F6616`, and the slot clear then runs.
    sim.techno_limbo_with_rules(cloaker, &rules);
    assert!(
        !sim.substrate
            .entities
            .get(bystander)
            .unwrap()
            .has_live_contact_with(cloaker),
        "UnInit is control 1, so the same slot clear does fire"
    );
}

#[test]
fn a_sensing_house_keeps_both_target_and_destination_across_a_dive() {
    // `FootClass::PointerExpired @ 0x004D9960` computes its own `allowClear`
    // from a SECOND `SensorCountForHouse` call — `0x004D9A57 CALL 0x004870D0`,
    // at the diving object's `GetCoords` cell for the RECEIVER's house — and
    // that Boolean gates the `+0x5A0`/`+0x5A4` NavCom pair clear exactly as the
    // Techno body's copy gates the `+0x2B4`/`+0x2B8` Target pair. So a
    // Destroyer whose house senses the sub's cell keeps CHASING it, not just
    // shooting at it. `+0x5A8` (SuspendedNavCom) is cleared above that guard
    // and goes on both controls.
    let (mut sim, rules, cloaker) = spawned_sub();
    let cell = sim
        .substrate
        .entities
        .get(cloaker)
        .map(|entity| (entity.position.rx, entity.position.ry))
        .unwrap();
    let sensing = sim
        .spawn_object_at_height("DEST", "Americans", cell.0 + 1, cell.1, 0, 0, &rules)
        .unwrap();
    // RANKED carries no `SensorsSight=`, so it deposits nothing and stays blind.
    let blind = sim
        .spawn_object_at_height("RANKED", "Neutral", cell.0 + 2, cell.1, 0, 0, &rules)
        .unwrap();
    for chaser in [sensing, blind] {
        let entity = sim.substrate.entities.get_mut(chaser).unwrap();
        entity.attack_target = Some(AttackTarget::new(cloaker));
        entity.navigation.nav_com = Some(NavTargetRef::object(cloaker));
        entity.navigation.nav_com_aux = Some(NavTargetRef::object(cloaker));
        entity.navigation.suspended_nav_com = Some(NavTargetRef::object(cloaker));
    }
    let american = sim.substrate.entities.get(sensing).unwrap().owner;
    sim.fog.increment_sensor_at(american, cell.0, cell.1);

    tick_stock_cloak_producer(&mut sim, cloaker, &rules);

    let sensing_after = sim.substrate.entities.get(sensing).unwrap();
    assert!(
        sensing_after.attack_target.is_some(),
        "the sensing house keeps its Target (+0x2B4)"
    );
    assert_eq!(
        sensing_after.navigation.nav_com,
        Some(NavTargetRef::object(cloaker)),
        "and keeps its destination (+0x5A4) — it must not stop closing"
    );
    assert_eq!(
        sensing_after.navigation.nav_com_aux,
        Some(NavTargetRef::object(cloaker)),
        "the +0x5A0 half of the pair is behind the same allowClear"
    );
    assert_eq!(
        sensing_after.navigation.suspended_nav_com, None,
        "+0x5A8 is cleared above the guard, so it goes even for a sensing house"
    );

    let blind_after = sim.substrate.entities.get(blind).unwrap();
    assert!(
        blind_after.attack_target.is_none(),
        "a blind house loses the Target"
    );
    assert_eq!(
        blind_after.navigation.nav_com, None,
        "and loses the destination with it"
    );
    assert_eq!(blind_after.navigation.nav_com_aux, None);
    assert_eq!(blind_after.navigation.suspended_nav_com, None);
}

#[test]
fn a_limboed_entity_on_the_cell_no_longer_shadows_the_sensors_neighbour() {
    // `CellClass::Find_Nearest_Object @ 0x0047C3D0` walks the cell's own object
    // chain (`+0xE4`). `techno_limbo` unmarks the entity from the occupancy
    // grid — VERA's model of that chain — but never clears `entity.position`,
    // so a raw position scan kept finding the stale object and, taking the
    // first match, hid the Destroyer behind it.
    let (mut sim, rules, id) = spawned_sub();
    let cell = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| (entity.position.rx, entity.position.ry))
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .establish_unlimbo_fully_cloaked();

    // Lower stable id than the Destroyer, and it carries no `Sensors=`.
    let stale = sim
        .spawn_object_at_height("RANKED", "Neutral", cell.0 + 1, cell.1, 0, 0, &rules)
        .unwrap();
    let dest = sim
        .spawn_object_at_height("DEST", "Americans", cell.0 + 1, cell.1, 0, 0, &rules)
        .unwrap();
    assert!(stale < dest);
    sim.techno_limbo_with_rules(stale, &rules);
    assert_eq!(
        sim.substrate.entities.get(stale).unwrap().position.rx,
        cell.0 + 1,
        "limbo leaves the stale map position in place, which is why the raw scan was wrong"
    );

    assert!(
        uncloak_on_sensor_neighbour_after_cell_entry(&mut sim, id, &rules),
        "an off-chain limboed object must not shadow the Destroyer"
    );
}

#[test]
fn the_sensors_neighbour_pick_is_nearest_to_the_cell_origin_not_lowest_id() {
    // With the `{0, 0}` offset the callsite passes, `Find_Nearest_Object`
    // ranks by `ftol(Sqrt_Approx((X & 0xFF)^2 + (Y & 0xFF)^2))` — the object's
    // own sub-cell lepton offset from the cell's NW corner — and keeps the
    // first of equal distances.
    for dest_wins in [false, true] {
        let (mut sim, rules, id) = spawned_sub();
        let cell = sim
            .substrate
            .entities
            .get(id)
            .map(|entity| (entity.position.rx, entity.position.ry))
            .unwrap();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .cloak
            .as_mut()
            .unwrap()
            .establish_unlimbo_fully_cloaked();

        let blocker = sim
            .spawn_object_at_height("RANKED", "Neutral", cell.0 + 1, cell.1, 0, 0, &rules)
            .unwrap();
        let dest = sim
            .spawn_object_at_height("DEST", "Americans", cell.0 + 1, cell.1, 0, 0, &rules)
            .unwrap();
        let (near, far) = if dest_wins {
            (dest, blocker)
        } else {
            (blocker, dest)
        };
        for (entity_id, offset) in [(near, 10i32), (far, 200i32)] {
            let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
            entity.position.sub_x = SimFixed::from_num(offset);
            entity.position.sub_y = SimFixed::from_num(offset);
        }

        assert_eq!(
            uncloak_on_sensor_neighbour_after_cell_entry(&mut sim, id, &rules),
            dest_wins,
            "the single inspected object is the one nearest the cell origin"
        );
    }
}
