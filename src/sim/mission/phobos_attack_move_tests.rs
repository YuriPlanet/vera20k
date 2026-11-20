//! Phobos `AttackMove.*` through the production paths.
//!
//! `AttackMove.IgnoreWeaponCheck=` ([`Simulation::techno_can_attack_move`],
//! `0x00711E90`), `AttackMove.Aggressive=` (`TechnoClass::Evaluate_Candidate`,
//! `0x006F85AB`) and `AttackMove.UpdateTarget=` (`foot_arrival_target_handler`,
//! `0x4DF3A0`). No retail INI or ART is involved.
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{ObjectAiCtx, Simulation};

use super::MegaMission;

/// One `FIRER` vehicle over the arena's ground: `body` is the rest of its
/// section, so a caller adds `Primary=`, `PreventAttackMove=` or a Phobos key.
fn rules_with(firer_body: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[General]\n\
         [VehicleTypes]\n0=FIRER\n1=MTNK\n\
         [BuildingTypes]\n0=OILRE\n\
         [FIRER]\nStrength=1000\nArmor=heavy\nSpeed=4\nCanPassiveAquire=yes\n{firer_body}\
         [MTNK]\nStrength=500\nArmor=heavy\nSpeed=4\nPrimary=GUN\n\
         CanPassiveAquire=yes\nThreatPosed=40\n\
         [OILRE]\nStrength=1000\nArmor=wood\nThreatPosed=0\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [Warheads]\n0=HIT\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .expect("attack-move rules read")
}

/// The arena with an American `FIRER` at (10,10) and a Russian `OILRE` at
/// (12,10) — unarmed, no `ThreatPosed=` — un-limboed and discovered, with the
/// building's cell visible to the player so the human acquisition gate
/// (`0x006F81BE`) admits it. With `threat`, an armed Russian `MTNK` at (14,10)
/// joins it, likewise discovered.
fn arena(firer_body: &str, threat: bool) -> (Simulation, RuleSet, u64, u64, Option<u64>) {
    let rules = rules_with(firer_body);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 11);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    let firer = sim
        .spawn_object("FIRER", "Americans", 10, 10, 0, &rules)
        .expect("firer");
    let building = sim
        .spawn_object("OILRE", "Russians", 12, 10, 0, &rules)
        .expect("building");
    let mut placed = vec![firer, building];
    let tank = threat.then(|| {
        sim.spawn_object("MTNK", "Russians", 14, 10, 0, &rules)
            .expect("threat")
    });
    placed.extend(tank);
    for id in placed {
        sim.entities_mut_for_test()
            .get_mut(id)
            .expect("placed object")
            .lifecycle
            .set_in_limbo_for_test(false);
    }
    let (owner, cells) = {
        let actor = sim.entities().get(firer).expect("firer");
        let mut cells = Vec::new();
        for id in [building].into_iter().chain(tank) {
            let target = sim.entities().get(id).expect("target");
            cells.push((target.position.rx(), target.position.ry()));
        }
        (actor.owner(), cells)
    };
    for (rx, ry) in cells {
        sim.fog.mark_visible_for_owner(owner, rx, ry);
    }
    crate::sim::arena_fixture::discover(&mut sim, building);
    if let Some(tank) = tank {
        crate::sim::arena_fixture::discover(&mut sim, tank);
    }
    // The passive scan's targeting timer is armed for 45 frames at
    // construction (`0x006F2C33..0x006F2C39`); frame 100 leaves it run out, so
    // `TargetingTimer.InProgress()` is false as `CheckAttackMoveCanResetTarget`
    // requires.
    sim.session.set_binary_frame_for_test(100);
    (sim, rules, firer, building, tank)
}

/// `AttackMove.IgnoreWeaponCheck=` (`Ext/Techno/Hooks.cpp:1259-1264`): the
/// global drops the `Primary != null` half of `Can_Attack_Move @ 0x00711E90`
/// and nothing else.
#[test]
fn ignore_weapon_check_admits_a_weaponless_unit_but_not_prevent_attack_move() {
    // A type with no `Primary=` at all.
    let mut rules = rules_with("");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 3);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let id = sim
        .spawn_object("FIRER", "Americans", 10, 10, 0, &rules)
        .expect("weaponless unit");
    sim.entities_mut_for_test()
        .get_mut(id)
        .expect("unit")
        .lifecycle
        .set_in_limbo_for_test(false);

    assert!(
        !sim.techno_can_attack_move(id, &rules),
        "stock refuses a type whose Primary names no weapon"
    );
    rules.phobos_globals.attack_move_ignore_weapon_check = true;
    assert!(
        sim.techno_can_attack_move(id, &rules),
        "the global admits the weaponless type"
    );

    // `PreventAttackMove=yes` still refuses: the hook resumes at `0x00711E9A`,
    // past the weapon test only.
    let mut rules = rules_with("PreventAttackMove=yes\n");
    rules.phobos_globals.attack_move_ignore_weapon_check = true;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 3);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let id = sim
        .spawn_object("FIRER", "Americans", 10, 10, 0, &rules)
        .expect("prevented unit");
    sim.entities_mut_for_test()
        .get_mut(id)
        .expect("unit")
        .lifecycle
        .set_in_limbo_for_test(false);
    assert!(!sim.techno_can_attack_move(id, &rules));
}

/// `AttackMove.Aggressive=` (`0x006F85AB`) and `AttackMove.UpdateTarget=`
/// (`0x4DF3A0`): the two gates that change what an attack-mover shoots at.
#[test]
fn attack_move_reaches_unarmed_buildings_and_replaces_a_useless_target() {
    // 1. Aggressive. The unarmed building is refused to a human house's
    //    scanner by `0x006F85AB` unless the key is on.
    for (body, expected) in [("", false), ("AttackMove.Aggressive=yes\n", true)] {
        let (mut sim, rules, firer, building, _) = arena(&format!("Primary=GUN\n{body}"), false);
        sim.entities_mut_for_test()
            .get_mut(firer)
            .expect("firer")
            .mega_mission =
            MegaMission::for_test(true, Some(TargetKind::Cell(20, 10)), None, false);
        let pick = crate::sim::world::direct_greatest_threat(
            &mut sim,
            &rules,
            None,
            firer,
            crate::sim::combat::ScanMission::Guard,
        );
        assert_eq!(
            pick == Some(TargetKind::Entity(building)),
            expected,
            "Aggressive={body:?} decides whether the unarmed building is a target"
        );
    }

    // 2. UpdateTarget. The firer holds an unarmed building it cannot use while
    //    an armed `MTNK` stands two cells further out. Without the key the
    //    arrival handler holds the held target; with it, the hook drops the
    //    target and the handler rescans onto the threat.
    for (body, replaced) in [("", false), ("AttackMove.UpdateTarget=yes\n", true)] {
        let (mut sim, rules, firer, building, tank) = arena(&format!("Primary=GUN\n{body}"), true);
        let tank = tank.expect("armed threat");
        {
            let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
            // A real attack-mover holds Mission::Move or Attack; Phobos'
            // `FootClass_UpdateAttackMove_Follow @ 0x4DF3A6` resumes the mega
            // mission for any other mission, which would clear the fixture.
            actor
                .mission
                .set_current_for_test(MissionId::from_known(MissionType::Move), 0);
            // `HaveAttackMoveTarget=1` with no MegaTarget: the handler holds a
            // TarCom in reach and drops it only when the scan finds nothing.
            actor.mega_mission = MegaMission::for_test(true, None, None, true);
            actor.attack_target.set(Some(TargetKind::Entity(building)));
        }
        sim.foot_arrival_target_handler(firer, &rules, ObjectAiCtx::default());
        let held = sim
            .entities()
            .get(firer)
            .and_then(|entity| entity.attack_target.target());
        assert_eq!(
            held,
            Some(TargetKind::Entity(if replaced { tank } else { building })),
            "UpdateTarget={body:?} decides whether the useless target is replaced"
        );
    }
}
