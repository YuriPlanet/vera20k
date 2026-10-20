//! The object turn's `Per_Cell_Process(2)` callers run the whole per-cell
//! owner (`movement/per_cell.rs`), not a part of it.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::LocomotorState;

fn output_arrival_fixture() -> (Simulation, RuleSet) {
    use crate::map::entities::EntityCategory;
    use crate::sim::components::NavTargetRef;
    use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=125\nSpeed=4\n\
         [BuildingTypes]\n0=GAPILE\n[GAPILE]\nStrength=500\nHospital=no\nWeaponsFactory=no\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 0,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    for (id, name, category, cell) in [
        (1, "E1", EntityCategory::Infantry, (15, 16)),
        (2, "GAPILE", EntityCategory::Structure, (15, 14)),
    ] {
        let mut entity =
            GameEntity::test_default_of_category(id, name, "Americans", cell.0, cell.1, category);
        entity.set_owner_for_test(sim.intern("Americans"));
        entity.set_type_ref_for_test(sim.intern(name));
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.lifecycle.set_cell_marked(true);
        if category == EntityCategory::Infantry {
            entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
            entity
                .navigation
                .set_nav_com_for_test(Some(NavTargetRef::cell(15, 16)));
            entity.set_in_playfield(false);
        }
        sim.substrate.entities.insert(entity);
    }
    for message in [RadioMessage::Hello, RadioMessage::Tether] {
        assert_eq!(
            radio::transmit(
                &mut sim,
                1,
                2,
                message,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default()
            ),
            RadioResponse::Roger
        );
    }
    radio::take_transmit_log();
    (sim, rules)
}

/// Whole original Infantry arrival519630 calls51A80C before the Foot tail;
/// P2's two-GI output observes8→25→25→25→3 and untouched three RNG streams.
/// This focused supplied-arrival control covers that canonical producer and
/// receiver composition, not Factory cadence or the full paid Walk path.
#[test]
fn infantry_arrival_runs_reciprocal_clearance_then_the_existing_foot_tail() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::movement::PerCellReason;
    use crate::sim::radio;

    let (mut sim, rules) = output_arrival_fixture();
    let rng_before = sim.rng_state();
    sim.per_cell_process(
        1,
        PerCellReason::TurnComplete,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert!(radio::take_transmit_log().is_empty());
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().tether.partner(),
        Some(2)
    );
    assert!(!sim.substrate.entities.get(1).unwrap().in_playfield());

    sim.per_cell_process(
        1,
        PerCellReason::Arrival,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

    let actual = radio::take_transmit_log()
        .into_iter()
        .map(|event| (event.sender_sid, event.msg, event.target_sid, event.reply))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            (1, 8, 2, Some(1)),
            (2, 25, 1, Some(1)),
            (1, 25, 2, Some(1)),
            (2, 25, 1, Some(0)),
            (2, 3, 1, Some(1)),
        ]
    );
    for id in [1, 2] {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(entity.tether.partner(), None);
        assert!(entity.radio_contacts.is_empty());
    }
    let infantry = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        infantry.navigation.nav_com(),
        Some(NavTargetRef::cell(15, 16))
    );
    assert!(infantry.in_playfield(), "the existing Foot tail still runs");
    assert_eq!(sim.rng_state(), rng_before);

    sim.per_cell_process(
        1,
        PerCellReason::Arrival,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert!(
        radio::take_transmit_log().is_empty(),
        "live tether is re-read"
    );
}

///51A7F8 gates on Techno+418, not the presence of a contact. A contact-only
/// arrival is not a factory-specific forced teardown.
#[test]
fn an_untethered_infantry_arrival_preserves_its_contact_and_runs_the_foot_tail() {
    use crate::sim::movement::PerCellReason;
    use crate::sim::radio::{self, RadioMessage, RadioPayload};

    let (mut sim, rules) = output_arrival_fixture();
    radio::transmit(
        &mut sim,
        1,
        2,
        RadioMessage::Untether,
        RadioPayload::default(),
        Some(&rules),
        crate::sim::world::FrameEffects::default(),
    );
    radio::take_transmit_log();
    sim.per_cell_process(
        1,
        PerCellReason::Arrival,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

    assert!(radio::take_transmit_log().is_empty());
    for (id, partner) in [(1, 2), (2, 1)] {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(entity.tether.partner(), None);
        assert_eq!(entity.radio_contacts.slot(0), Some(partner));
    }
    assert!(sim.substrate.entities.get(1).unwrap().in_playfield());
}

/// Object AI `0x005F3F8D` calls `Per_Cell_Process(2)` when a fall grounds,
/// in the cell it fell through too; the Foot body's Techno tail then
/// promotes the object into the playfield (`0x006F511A`). The copy this
/// replaced ran only the neighbour update, and a Walk owner never reaches
/// the cell-change stand-in.
#[test]
fn a_parachute_landing_in_its_own_cell_runs_the_foot_body() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n[E1]\nStrength=125\nSpeed=4\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(0);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 0,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    let mut entity = GameEntity::test_default(1, "E1", "Americans", 5, 5);
    entity.set_owner_for_test(sim.intern("Americans"));
    entity.set_type_ref_for_test(sim.intern("E1"));
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    entity.position.set_exact_z_leptons_for_test(Some(2));
    entity.set_falling_down_for_test(true);
    entity.set_fall_rate_for_test(-3);
    sim.substrate.entities.insert(entity);
    sim.substrate.next_stable_object_id = 2;
    assert!(matches!(
        sim.reveal(1),
        super::super::RevealOutcome::Revealed { .. }
    ));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_in_playfield(false);

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.is_falling_down(), "the fall grounded this turn");
    assert_eq!((entity.position.rx(), entity.position.ry()), (5, 5));
    assert!(entity.in_playfield(), "the landing's Techno tail promotes");
}

/// Jumpjet54C8CB clears its destination and moving byte before PerCell(2),
/// then54C8FF always calls the class NULL setter. No in-range target is
/// needed for Unit741970 -> Foot4D94B0 to retire the order and reset timers.
#[test]
fn a_jumpjet_touchdown_without_a_target_runs_the_class_null_setter() {
    use crate::sim::components::{DriveCoord, NavTargetRef};
    use crate::sim::movement::jumpjet_movement::jumpjet_flight::{STATE_DESCEND, STATE_GROUND};
    use crate::sim::timer::CdTimer;
    use crate::util::fixed_math::SimFixed;

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=NIGHTHAWK\n\
         [NIGHTHAWK]\nStrength=200\nSpeed=14\nSpeedType=Hover\nMovementZone=Fly\n\
         Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n\
         JumpjetHeight=500\nJumpjetNoWobbles=yes\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(0);
    super::super::lifecycle_tests::install_common_raw_terrain(&mut sim, 32, 32, 0, None);
    sim.session.set_binary_frame_for_test(1000);
    let id = sim
        .construct_object_limbo_at_height(
            "NIGHTHAWK",
            "Americans",
            10,
            10,
            0,
            0,
            &rules,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert!(matches!(
        sim.reveal(id),
        super::super::RevealOutcome::Revealed { .. }
    ));
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.position.set_sub_x_for_test(SimFixed::from_num(128));
    entity.position.set_sub_y_for_test(SimFixed::from_num(128));
    entity.position.set_exact_z_leptons_for_test(Some(0));
    entity
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(10, 10)));
    entity
        .navigation
        .set_nav_com_aux_for_test(Some(NavTargetRef::cell(11, 10)));
    entity
        .navigation
        .set_nav_queue_for_test(vec![NavTargetRef::cell(12, 10)]);
    entity
        .navigation
        .path_replay
        .set_directions_for_test(vec![2, 2]);
    entity.navigation.path_runtime.set_path_blocked(true);
    entity
        .navigation
        .path_runtime
        .set_movement_timer_for_test(CdTimer::started(50, 5));
    entity
        .navigation
        .path_runtime
        .set_blocked_timer_for_test(CdTimer::started(40, 6));
    let runtime = entity
        .locomotor
        .as_mut()
        .unwrap()
        .jumpjet_runtime_mut()
        .unwrap();
    *runtime = runtime
        .clone()
        .with_phase_for_test(STATE_DESCEND)
        .with_moving_for_test(true)
        .with_landing_latched_for_test(true)
        .with_destination_for_test(DriveCoord::cell(10, 10, 0));
    let scenario_before = sim.scenario_rng.native_state_hex();

    let process = sim
        .process_air_locomotor(
            id,
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();

    assert!(process.per_cell_ran, "accepted touchdown ran PerCell");
    let entity = sim.substrate.entities.get(id).unwrap();
    let runtime = entity
        .locomotor
        .as_ref()
        .unwrap()
        .jumpjet_runtime()
        .unwrap();
    assert_eq!(runtime.phase(), STATE_GROUND);
    assert!(!runtime.moving());
    assert!(
        entity.attack_target.target().is_none(),
        "no conditional range stop"
    );
    assert_eq!(entity.navigation.nav_com(), None);
    assert_eq!(entity.navigation.nav_com_aux(), None);
    assert!(entity.navigation.nav_queue().is_empty());
    assert!(
        entity
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    let timing = &entity.navigation.path_runtime;
    assert!(!timing.path_blocked());
    assert_eq!(timing.movement_timer(), CdTimer::started(1000, 0));
    assert_eq!(
        timing.blocked_timer(),
        CdTimer::started(1000, rules.general.blockage_path_delay_ticks)
    );
    assert_eq!(sim.scenario_rng.native_state_hex(), scenario_before);
}
