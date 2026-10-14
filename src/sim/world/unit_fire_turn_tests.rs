//! Production host ordering regressions. Original UnitAI calls Foot73647B,
//! Fire7365E1, Facing7365E8, then its second Ready/Commence7366EF..736703;
//! Logic55B613 visits newly appended Bullets in the same live pass.
//! Infantry51BF59 likewise fires in its own slot, after its second Commence.
//! Native FV paid-movement comparison lives with fv_cell_attack. These
//! supplied actor/track inputs isolate cross-object scheduling, not Drive math.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, TrackProgress};
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::DriveLocomotionRuntime;
use crate::sim::projectile::ProjectileCoord;
use crate::util::fixed_math::SimFixed;

fn fixture() -> (Simulation, RuleSet, u64, u64) {
    fixture_with_first("TANK")
}

fn fixture_with_first(first_type: &str) -> (Simulation, RuleSet, u64, u64) {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[AudioVisual]\nGravity=6\n[VehicleTypes]\n0=TANK\n\
         [TANK]\nStrength=200\nPrimary=Gun\nTurret=yes\nTurretROT=5\n\
         Speed=10\nSpeedType=Track\nMovementZone=Normal\nPassive=yes\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [InfantryTypes]\n0=INF\n[INF]\nStrength=200\nPrimary=Gun\n\
         Speed=4\nSpeedType=Foot\nMovementZone=Normal\nPassive=yes\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [Gun]\nDamage=10\nROF=30\nRange=6\nSpeed=30\n\
         OmniFire=yes\nProjectile=Round\nWarhead=WH\n\
         [Round]\nInviso=no\nROT=0\n\
         [WH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[TANK]\nPrimaryFireFLH=0,0,128\n"),
    ));
    let mut sim = Simulation::with_seed(0x7365e1);
    for name in ["Americans", "Russians"] {
        let owner = sim.interner.intern(name);
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
        sim.session.house_order.push(owner);
    }
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let first = sim
        .spawn_object(first_type, "Americans", 16, 16, 0, &rules)
        .unwrap();
    let second = sim
        .spawn_object("TANK", "Russians", 20, 16, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    sim.session.binary_frame = 10;
    for id in [first, second] {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: MissionId::from_known(MissionType::Attack),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::from_raw(10, 1000),
            });
        entity.rearm_timer = crate::sim::timer::CdTimer::started(10, 0);
        entity.attack_target.set_for_test(Some(TargetKind::Cell(
            entity.position.rx,
            entity.position.ry - 3,
        )));
    }
    (sim, rules, first, second)
}

fn install_paid_track(sim: &mut Simulation, id: u64) {
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    let (x, y) = (entity.position.rx, entity.position.ry);
    entity.drive_accelerates = false;
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                DriveLocomotionRuntime::default()
                    .with_head_to_for_test(Some(DriveCoord::cell(x, y - 1, 0)))
                    .with_track_valid_for_test(true)
                    .with_target_speed_fraction_for_test(SimFixed::from_num(1))
                    .with_track_for_test(TrackProgress {
                        turn_index: 0,
                        cursor: 0,
                        reversed: false,
                        residual: 0,
                    })
            ))
    );
    // The accepted head (x, y-1) was the route's last cell, so Foot+5E0
    // holds no word beyond it: the committed head alone models the route.
}

fn position(sim: &Simulation, id: u64) -> ProjectileCoord {
    let entity = sim.substrate.entities.get(id).unwrap();
    ProjectileCoord::new(
        i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>(),
        i32::from(entity.position.ry) * 256 + entity.position.sub_y.to_num::<i32>(),
        entity.position.exact_z_leptons.unwrap_or(0),
    )
}

#[test]
fn paid_move_fires_before_later_unit_and_new_bullet_slots() {
    let (mut sim, rules, first, second) = fixture();
    install_paid_track(&mut sim, first);
    let before = position(&sim, first);
    let mut visits = Vec::new();
    let mut first_after_move = None;
    sim.try_for_each_live_object::<std::convert::Infallible>(|world, id| {
        world
            .advance_live_object_turn(id, Some(&rules), techno_ai::ObjectAiCtx::default())
            .unwrap();
        visits.push((id, world.fire_events.len()));
        if id == first {
            first_after_move = Some(position(world, first));
        }
        Ok(())
    })
    .unwrap();
    assert_ne!(first_after_move.unwrap(), before, "supplied paid track ran");
    assert_eq!(&visits[..2], &[(first, 1), (second, 2)]);
    assert_eq!(sim.fire_events.len(), 2);
    let fire = &sim.fire_events[0];
    let after = first_after_move.unwrap();
    assert_eq!(
        (
            i32::from(fire.origin_snapshot.rx) * 256 + fire.origin_snapshot.sub_x.to_num::<i32>(),
            i32::from(fire.origin_snapshot.ry) * 256 + fire.origin_snapshot.sub_y.to_num::<i32>(),
        ),
        (after.x, after.y),
        "FireAt reads its own completed movement"
    );
    let bullets: Vec<_> = sim.projectiles.iter().map(|(_, bullet)| bullet).collect();
    assert_eq!(bullets.len(), 2);
    for bullet in bullets {
        assert!(visits[2..].contains(&(bullet.id, 2)));
        assert!(
            bullet.in_logic_vector,
            "supplied flight remains inside native Size diamond"
        );
        assert_ne!(bullet.position, bullet.launch_origin, "same-pass BulletAI");
    }
}

#[test]
fn earlier_unit_fire_reads_later_targets_pose_before_its_movement() {
    let (mut sim, rules, first, second) = fixture();
    install_paid_track(&mut sim, second);
    sim.substrate
        .entities
        .get_mut(first)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(second)));
    sim.substrate
        .entities
        .get_mut(second)
        .unwrap()
        .attack_target
        .set_for_test(None);
    let before = position(&sim, second);
    sim.advance_live_object_pass(
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_ne!(position(&sim, second), before);
    let bullet = sim
        .projectiles
        .iter()
        .map(|(_, bullet)| bullet)
        .find(|bullet| bullet.source_id == first)
        .expect("first Unit fired before the later target moved");
    assert_eq!(bullet.launch_target, before);
}

#[test]
fn attack_move_acquires_before_its_foot_fire_slot() {
    for first_type in ["TANK", "INF"] {
        let (mut sim, rules, first, _) = fixture_with_first(first_type);
        let actor = sim.substrate.entities.get_mut(first).unwrap();
        actor.attack_target.set_for_test(None);
        // The event's state: Move committed with the order stored, and a
        // targeting timer that has run out. The targeting slot of
        // TechnoClass::AI (`0x006FA681..0x006FA695`) runs the attack-mover's
        // Arrival_Target_Handler, whose scan acquires and commits Attack
        // before Unit7365E1 or Infantry51BF59 fire.
        actor
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: MissionId::from_known(MissionType::Move),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::from_raw(10, 1000),
            });
        actor.passive_scan_timer.reset(10);
        actor.mega_mission = crate::sim::mission::mega_mission::MegaMission::attack_move(Some(
            crate::sim::combat::TargetKind::Cell(24, 16),
        ));
        sim.advance_live_object_turn(first, Some(&rules), techno_ai::ObjectAiCtx::default())
            .unwrap();
        assert_eq!(sim.fire_events.len(), 1);
        assert_eq!(sim.fire_events[0].attacker_id, first);
    }
}

#[test]
fn global_combat_tail_cannot_fire_a_ready_foot_a_second_time() {
    for first_type in ["TANK", "INF"] {
        let (mut sim, rules, first, second) = fixture_with_first(first_type);
        sim.substrate
            .entities
            .get_mut(second)
            .unwrap()
            .attack_target
            .set_for_test(None);
        sim.advance_live_object_turn(first, Some(&rules), techno_ai::ObjectAiCtx::default())
            .unwrap();
        assert_eq!(sim.fire_events.len(), 1);
        sim.substrate.entities.get_mut(first).unwrap().rearm_timer =
            crate::sim::timer::CdTimer::started(10, 0);
        let before = sim.scenario_rng.state();
        let result = sim.tick_combat_with_fatal_lifecycle(
            &rules,
            None,
            100,
            &[first, second],
            &BTreeSet::new(),
            &Default::default(),
            &[],
            crate::sim::world::FrameEffects::default(),
        );
        assert!(result.consequences.fire_events().is_empty());
        assert!(result.unit_facing.is_empty());
        assert_eq!(sim.scenario_rng.state(), before);
        assert_eq!(sim.fire_events.len(), 1);
    }
}
