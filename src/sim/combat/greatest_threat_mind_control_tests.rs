//! Phobos `TechnoClass_CanAutoTargetObject_AttackMindControlledDelay`
//! (`0x6F88BF`, `Ext/Techno/Hooks.Misc.cpp:631-641`): a candidate inside its
//! `MindControl.ThreatDelay` shield is not auto-targeted by a house allied
//! with its original owner; the boundary frame, a berserk scanner and a
//! controller without a manager admit it again.

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::world::{FrameEffects, Simulation};

fn scene(delay: Option<i32>) -> (Simulation, RuleSet, u64, u64) {
    let controller_key = delay
        .map(|delay| format!("MindControl.ThreatDelay={delay}\n"))
        .unwrap_or_default();
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[InfantryTypes]\n[BuildingTypes]\n[AircraftTypes]\n\
         [VehicleTypes]\n0=SCAN\n1=PAWN\n2=CATCH\n\
         [SCAN]\nStrength=300\nArmor=heavy\nPrimary=GUN\nSpeed=6\nSight=8\n\
         [PAWN]\nStrength=300\nArmor=heavy\n\
         [CATCH]\nStrength=300\nArmor=heavy\nPrimary=MC\nSpeed=6\nSight=8\n\
         [GUN]\nDamage=10\nROF=20\nRange=8\nWarhead=WH\n\
         [MC]\nDamage=1\nROF=10\nRange=8\nWarhead=CTRL\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CTRL]\nMindControl=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         {controller_key}"
    )))
    .expect("mind-control targeting rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6F88_BF00);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let scan = sim
        .spawn_object("SCAN", "Americans", 10, 10, 0, &rules)
        .expect("scanner");
    let pawn = sim
        .spawn_object("PAWN", "Americans", 12, 10, 0, &rules)
        .expect("candidate");
    let catcher = sim
        .spawn_object("CATCH", "Russians", 14, 14, 0, &rules)
        .expect("controller");
    let warhead = sim.interner.intern("CTRL");
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 0,
        source_id: catcher,
        target: crate::sim::projectile::ProjectileTarget::Entity(pawn),
        impact: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
        payload: crate::sim::projectile::ProjectilePayload::new(1, warhead, None, false),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    sim.mind_control_detonation(detonation, &rules, None, FrameEffects::default());
    assert_eq!(
        sim.substrate.entities.get(pawn).unwrap().owner(),
        sim.interner.get("Russians").unwrap(),
        "the capture changes the candidate's house"
    );
    crate::sim::arena_fixture::discover(&mut sim, pawn);
    (sim, rules, scan, pawn)
}

#[test]
fn a_fresh_capture_shield_keeps_the_candidate_out_of_the_scan_until_it_expires() {
    let pick = |sim: &mut Simulation, rules: &RuleSet, scan: u64| {
        sim.greatest_threat_represented(rules, None, scan, ScanMission::Guard, None)
            .and_then(crate::sim::combat::TargetKind::object_id)
    };
    // No delay: the captured candidate is an ordinary enemy.
    let (mut sim, rules, scan, pawn) = scene(None);
    assert_eq!(pick(&mut sim, &rules, scan), Some(pawn));
    // A positive delay refuses it while the frame is in the future.
    let (mut sim, rules, scan, pawn) = scene(Some(5));
    assert_eq!(
        pick(&mut sim, &rules, scan),
        None,
        "frame 0 is inside the shield"
    );
    sim.session.set_binary_frame_for_test(4);
    assert_eq!(pick(&mut sim, &rules, scan), None);
    sim.session.set_binary_frame_for_test(5);
    assert_eq!(
        pick(&mut sim, &rules, scan),
        Some(pawn),
        "the shield ends on its boundary frame"
    );
    // A berserk scanner ignores the shield (`pRetaliator->Berzerk`).
    sim.session.set_binary_frame_for_test(0);
    sim.substrate
        .entities
        .get_mut(scan)
        .unwrap()
        .berserk
        .set_active_for_test(true);
    assert_eq!(pick(&mut sim, &rules, scan), Some(pawn));
    // A controller without a manager ignores it (`!pManager`).
    let (mut sim, rules, scan, pawn) = scene(Some(5));
    let catcher = sim
        .substrate
        .entities
        .get(pawn)
        .unwrap()
        .mind_control
        .controller()
        .unwrap();
    sim.substrate
        .entities
        .get_mut(catcher)
        .unwrap()
        .capture_manager = None;
    assert_eq!(pick(&mut sim, &rules, scan), Some(pawn));
}
