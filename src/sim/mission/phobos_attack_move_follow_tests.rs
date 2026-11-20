//! Phobos `AttackMove.Follow.*` through the production arrival handler.
//!
//! `FootClass_UpdateAttackMove_Follow @ 0x4DF3A6`
//! (`Ext/Techno/Hooks.cpp:1266-1358`): `AttackMove.Follow=`,
//! `.Follow.IncludeAir=` and `.Follow.IfMindControlIsFull=`. No retail INI or
//! ART is involved.
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{ObjectAiCtx, Simulation};

use super::MegaMission;

/// Two American vehicles: `FOLLOW` carries the keys under test, `LEAD` is an
/// ordinary attack-mover. Both are armed so `Can_Attack_Move` admits them.
fn rules_with(follow_body: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        // The hand-off's `QueueMission(Area_Guard, 1)` keeps its queued order
        // only while readiness still consults the locomotor; these fixtures
        // are human-owned, and Phobos' default skips that query.
        "[General]\nReadyToNextMission.MovingCheck=yes\n\
         [VehicleTypes]\n0=FOLLOW\n1=LEAD\n\
         [FOLLOW]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\n\
         CanPassiveAquire=yes\n{follow_body}\
         [LEAD]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\nCanPassiveAquire=yes\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [Warheads]\n0=HIT\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .expect("attack-move follow rules read")
}

/// The arena with `FOLLOW` at (10,10) and `LEAD` at (14,10), both on an attack
/// move to (20,10) and both currently on Mission::Move, the state the hook
/// expects (`GetCurrentMission` == Move/Attack keeps the mega mission).
fn pair(follow_body: &str) -> (Simulation, RuleSet, u64, u64) {
    let rules = rules_with(follow_body);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 11);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    let follower = sim
        .spawn_object("FOLLOW", "Americans", 10, 10, 0, &rules)
        .expect("follower");
    let leader = sim
        .spawn_object("LEAD", "Americans", 14, 10, 0, &rules)
        .expect("leader");
    for id in [follower, leader] {
        let entity = sim.entities_mut_for_test().get_mut(id).expect("placed");
        entity.lifecycle.set_in_limbo_for_test(false);
        entity
            .mission
            .set_current_for_test(MissionId::from_known(MissionType::Move), 0);
        entity.mega_mission =
            MegaMission::for_test(true, Some(TargetKind::Cell(20, 10)), None, false);
    }
    (sim, rules, follower, leader)
}

/// `SetDestination` runs the real path planner, so the arena must know the
/// cell map. It does; a failure here would only be a missing cell, which the
/// test does not depend on — the archive target and the mission are the
/// observable hand-off.
#[test]
fn follow_hands_the_object_to_the_nearest_friendly_attack_mover() {
    let (mut sim, rules, follower, leader) = pair("AttackMove.Follow=yes\n");
    sim.foot_arrival_target_handler(follower, &rules, ObjectAiCtx::default());

    let entity = sim.entities().get(follower).expect("follower");
    // `Queue_Mission(Area_Guard, 1)` writes the queue first and commences only
    // if `Ready_To_Commence` agrees; the fixture's Move handler is still
    // mid-body, so the queued order is the observable guarantee.
    assert_eq!(
        entity.mission.queued().known(),
        Some(MissionType::AreaGuard),
        "QueueMission(Area_Guard, 1) replaces the attack move"
    );
    assert_eq!(
        entity.archive_target(),
        Some(TargetKind::Entity(leader)),
        "SetArchiveTarget names the followed object"
    );
    assert_eq!(
        sim.mega_mission(follower),
        MegaMission::default(),
        "ClearMegaMissionData empties the order in the follow arm"
    );
    // `+= WhatAmI() == Infantry ? 1 : 3` (`:1338`): a Unit costs three slots.
    assert_eq!(
        sim.entities()
            .get(leader)
            .expect("leader")
            .phobos_attack_move_follower_temp_count(),
        3
    );
}

/// The same scene without the key: the hook returns 0, so the native arrival
/// body runs and the mega mission survives untouched.
#[test]
fn without_the_key_the_native_arrival_body_keeps_the_attack_move() {
    let (mut sim, rules, follower, leader) = pair("");
    sim.foot_arrival_target_handler(follower, &rules, ObjectAiCtx::default());

    let entity = sim.entities().get(follower).expect("follower");
    assert_eq!(
        entity.mission.current().known(),
        Some(MissionType::Move),
        "the native body never queues Area Guard"
    );
    assert_eq!(entity.archive_target(), None);
    assert_eq!(
        sim.mega_mission(follower).destination(),
        Some(TargetKind::Cell(20, 10)),
        "the attack move is still in force"
    );
    assert_eq!(
        sim.entities()
            .get(leader)
            .expect("leader")
            .phobos_attack_move_follower_temp_count(),
        0
    );
}

/// A leader already at six followers is surrounded: `:1317` skips it, and the
/// no-candidate arm re-issues the follower's own MegaDestination but still
/// clears the mega mission.
#[test]
fn a_leader_at_six_followers_is_skipped_but_the_order_still_ends() {
    let (mut sim, rules, follower, leader) = pair("AttackMove.Follow=yes\n");
    sim.entities_mut_for_test()
        .get_mut(leader)
        .expect("leader")
        .set_phobos_attack_move_follower_temp_count(6);
    sim.foot_arrival_target_handler(follower, &rules, ObjectAiCtx::default());

    let entity = sim.entities().get(follower).expect("follower");
    assert_eq!(entity.archive_target(), None, "no leader was taken");
    assert_ne!(
        entity.mission.queued().known(),
        Some(MissionType::AreaGuard),
        "the no-candidate arm queues no new mission"
    );
    assert_eq!(
        sim.mega_mission(follower),
        MegaMission::default(),
        "`ClearMegaMissionData()` runs in the no-candidate arm too"
    );
    assert_eq!(
        sim.entities()
            .get(leader)
            .expect("leader")
            .phobos_attack_move_follower_temp_count(),
        6,
        "a refused leader is not charged"
    );
}

/// `Follow.IfMindControlIsFull=` (`:1297`): inert without a capture manager,
/// active with a finite one that has no free slot. `IsFull @ 0x004722A0`
/// answers a zero-capacity manager as full.
#[test]
fn follow_if_mind_control_is_full_needs_a_full_capture_manager() {
    // 1. No manager: the key changes nothing, the native body keeps the order.
    let (mut sim, rules, follower, _) = pair("AttackMove.Follow.IfMindControlIsFull=yes\n");
    sim.foot_arrival_target_handler(follower, &rules, ObjectAiCtx::default());
    let entity = sim.entities().get(follower).expect("follower");
    assert_eq!(
        entity.mission.current().known(),
        Some(MissionType::Move),
        "without a manager the hook returns 0"
    );
    assert_eq!(entity.archive_target(), None);

    // 2. A full manager: the follow arm takes over.
    let (mut sim, rules, follower, leader) = pair("AttackMove.Follow.IfMindControlIsFull=yes\n");
    sim.entities_mut_for_test()
        .get_mut(follower)
        .expect("follower")
        .capture_manager = Some(Box::new(
        crate::sim::capture_manager::CaptureManagerState::with_victims_for_test(0, false, &[]),
    ));
    sim.foot_arrival_target_handler(follower, &rules, ObjectAiCtx::default());
    let entity = sim.entities().get(follower).expect("follower");
    assert_eq!(
        entity.archive_target(),
        Some(TargetKind::Entity(leader)),
        "a full capture manager enables the follow search"
    );
    assert_eq!(
        entity.mission.queued().known(),
        Some(MissionType::AreaGuard)
    );
}

/// `Follow.IncludeAir=` (`:1299-1300`): a leader taken off the cell list and
/// registered with the AircraftTracker is invisible to the ground arm. Only
/// the key lets the airborne arm find it.
#[test]
fn follow_include_air_decides_whether_an_airborne_leader_is_seen() {
    for include_air in [false, true] {
        let body = if include_air {
            "AttackMove.Follow=yes\nAttackMove.Follow.IncludeAir=yes\n"
        } else {
            "AttackMove.Follow=yes\n"
        };
        let (mut sim, rules, follower, leader) = pair(body);
        let (rx, ry) = {
            let entity = sim.entities().get(leader).expect("leader");
            (entity.position.rx(), entity.position.ry())
        };
        // The tracker arm only sees objects off the cell list.
        sim.substrate.occupancy.remove(rx, ry, leader);
        let bucket = crate::sim::occupancy::air_spatial_bucket_index(
            rx,
            ry,
            sim.session.map_width(),
            sim.session.map_height(),
        );
        sim.substrate
            .entities
            .set_air_membership(leader, Some(bucket), 1);
        sim.entities_mut_for_test()
            .get_mut(leader)
            .expect("leader")
            .position
            .set_exact_z_leptons_for_test(Some(1000));

        sim.foot_arrival_target_handler(follower, &rules, ObjectAiCtx::default());
        let taken = sim
            .entities()
            .get(follower)
            .expect("follower")
            .archive_target()
            == Some(TargetKind::Entity(leader));
        assert_eq!(
            taken, include_air,
            "IncludeAir={include_air} decides whether the airborne leader is found"
        );
    }
}
