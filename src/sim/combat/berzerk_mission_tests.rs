//! Phobos `TechnoClass_ReceiveDamage_Berzerk` (`Ext/Techno/Hooks.ReceiveDamage.cpp:543-553`,
//! the six bytes at `0x701DAE`, returns `0x701DBA`): the berserk block queues
//! `[CombatDamage] BerzerkMission=` instead of stock's `0xF` Hunt, and runs
//! `SetDestination(NULL, false)` — vt+0x480 mode 0, which keeps the NavQueue —
//! before it.
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::arena_fixture::{clear_ground_ini, rival_arena};
use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

fn fixture(extra: &str) -> RuleSet {
    RuleSet::from_ini(&clear_ground_ini(&format!(
        "[CombatDamage]\nBerzerkTargeting=all\n{extra}\
         [VehicleTypes]\n0=FIRER\n1=TARGET\n\
         [Warheads]\n0=WH\n\
         [WH]\nCellSpread=0\nDamage=30\nPsychedelic=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [FIRER]\nStrength=500\nArmor=none\n\
         [TARGET]\nStrength=1000\nArmor=none\nImmuneToPsionics=no\n"
    )))
    .expect("psychedelic fixture")
}

/// An enemy shot, so the ally test never interferes.
fn berserk_hit(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64) {
    let event = EntityDamageEvent::direct_receiver(
        target,
        30,
        0,
        firer,
        None,
        sim.interner.intern("WH"),
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    let _ = sim.commit_direct_damage_receiver(rules, None, event, &mut FrameEffects::default());
}

fn arena(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = rival_arena(rules, 0x1C_AA);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let firer = sim
        .spawn_object("FIRER", "Russians", 20, 20, 0, rules)
        .unwrap();
    let target = sim
        .spawn_object("TARGET", "Americans", 23, 20, 0, rules)
        .unwrap();
    (sim, firer, target)
}

/// Stock queues Hunt (`0xF`), and so does Phobos with the key unauthored; an
/// authored name replaces it, while a name `MissionControlClass::FindIndex`
/// does not know keeps the previous value.
#[test]
fn the_berzerk_block_queues_the_authored_mission() {
    let rules = fixture("");
    assert_eq!(rules.phobos_globals.berzerk_mission, MissionType::Hunt);
    let (mut sim, firer, target) = arena(&rules);
    berserk_hit(&mut sim, &rules, firer, target);
    assert_eq!(
        sim.entities().get(target).unwrap().mission.queued(),
        MissionId::from_known(MissionType::Hunt),
        "the constructor is Hunt, the same mission stock pushes"
    );

    let rules = fixture("BerzerkMission=Area Guard\n");
    assert_eq!(rules.phobos_globals.berzerk_mission, MissionType::AreaGuard);
    let (mut sim, firer, target) = arena(&rules);
    berserk_hit(&mut sim, &rules, firer, target);
    assert_eq!(
        sim.entities().get(target).unwrap().mission.queued(),
        MissionId::from_known(MissionType::AreaGuard),
        "the queued mission is the authored one, whatever it is"
    );

    let rules = fixture("BerzerkMission=NotAMission\n");
    assert_eq!(
        rules.phobos_globals.berzerk_mission,
        MissionType::Hunt,
        "`Ext/Utilities/TemplateDef.h:508-526` keeps the value on an unknown name"
    );
}

/// The hook's extra `SetDestination(NULL, false)`: the target's own NavCom is
/// cleared before the queue call, where stock leaves it alone.
#[test]
fn the_berzerk_block_clears_the_destination() {
    let rules = fixture("");
    let (mut sim, firer, target) = arena(&rules);
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(9, 9)));
    berserk_hit(&mut sim, &rules, firer, target);
    let victim = sim.entities().get(target).unwrap();
    assert!(
        victim.berserk.is_active(),
        "the hit still reaches the berserk block"
    );
    assert_eq!(
        victim.navigation.nav_com(),
        None,
        "`0x701DAE` runs SetDestination(NULL, false) before QueueMission"
    );
}
