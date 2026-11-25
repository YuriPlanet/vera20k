//! Phobos `TechnoClass_ReceiveDamage_AllowBerzerkOnAllies` (hook `0x701D2E`,
//! `Ext/Techno/Hooks.ReceiveDamage.cpp:528-539`): the house a Psychedelic
//! hit's allied test consults is the live attacker's owner first, and
//! `[CombatDamage] AllowBerzerkOnAllies=yes` lets the allied hit through.
//! Stock (`0x701CD7..0x701CF8`) reads only the call's SourceHouse.
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::arena_fixture::{clear_ground_ini, rival_arena};
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

fn rules_with(allow_key: &str) -> RuleSet {
    RuleSet::from_ini(&clear_ground_ini(&format!(
        "[VehicleTypes]\n0=FIRER\n1=TARGET\n\
         [Warheads]\n0=WH\n\
         [WH]\nCellSpread=0\nDamage=30\nPsychedelic=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [FIRER]\nStrength=500\nArmor=none\n\
         [TARGET]\nStrength=1000\nArmor=none\nImmuneToPsionics=no\n\
         {allow_key}"
    )))
    .expect("psychedelic fixture")
}

/// The Phobos hook: `AllowBerzerkOnAllies=` (even at its default) marks the
/// game as a Phobos game, which is what installs it.
fn rules() -> RuleSet {
    rules_with("[CombatDamage]\nAllowBerzerkOnAllies=no\n")
}

/// One Psychedelic hit from `firer`'s own house, with a hostile `event`
/// SourceHouse, delivered to `target`.
fn hit(
    sim: &mut Simulation,
    rules: &RuleSet,
    firer: u64,
    target: u64,
    source_house: Option<&str>,
    attacker: bool,
) {
    let source = source_house.map(|name| sim.interner.intern(name));
    let event = EntityDamageEvent::direct_receiver(
        target,
        30,
        0,
        if attacker { firer } else { RAD_NO_ATTACKER },
        source,
        sim.interner.intern("WH"),
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    let _ = sim.commit_direct_damage_receiver(rules, None, event, &mut FrameEffects::default());
}

fn berserk(sim: &Simulation, id: u64) -> bool {
    sim.entities().get(id).unwrap().berserk.is_active()
}

/// The two firer and target in one house, so the attacker's own House is the
/// allied one; the event's SourceHouse names the rival.
fn paired(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = rival_arena(rules, 0x1C_AA);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, rules)
        .unwrap();
    let target = sim
        .spawn_object("TARGET", "Americans", 23, 20, 0, rules)
        .unwrap();
    (sim, firer, target)
}

/// Phobos' hook reads `Attacker->Owner` when an attacker is live, so an allied
/// firer rejects the hit even though the call names the enemy SourceHouse —
/// the priority the stock SourceHouse-only test does not have.
#[test]
fn an_allied_attacker_rejects_the_hit_whatever_the_source_house_names() {
    let mut rules = rules();
    let (mut sim, firer, target) = paired(&rules);
    hit(&mut sim, &rules, firer, target, Some("Russians"), true);
    assert!(
        !berserk(&sim, target),
        "AllowBerzerkOnAllies constructs no, so the allied attacker rejects the hit"
    );

    rules.phobos_globals.allow_berzerk_on_allies = true;
    let (mut sim, firer, target) = paired(&rules);
    hit(&mut sim, &rules, firer, target, Some("Russians"), true);
    assert!(
        berserk(&sim, target),
        "the key on lets the same allied hit make its target berserk"
    );
}

/// Without a live attacker Phobos' hook falls back to the call's SourceHouse,
/// so its allied value still rejects the hit.
#[test]
fn without_an_attacker_the_source_house_still_governs() {
    let mut rules = rules();
    let (mut sim, firer, target) = paired(&rules);
    hit(&mut sim, &rules, firer, target, Some("Americans"), false);
    assert!(!berserk(&sim, target));

    rules.phobos_globals.allow_berzerk_on_allies = true;
    let (mut sim, firer, target) = paired(&rules);
    hit(&mut sim, &rules, firer, target, Some("Americans"), false);
    assert!(berserk(&sim, target));
}

/// Stock reads only `[esp+0xE0]`, the SourceHouse, so the enemy name stands
/// and the allied attacker is never consulted. This is the case Phobos' hook
/// inverts.
#[test]
fn stock_reads_only_the_source_house_of_the_call() {
    let rules = rules_with("");
    let (mut sim, firer, target) = paired(&rules);
    hit(&mut sim, &rules, firer, target, Some("Russians"), true);
    assert!(
        berserk(&sim, target),
        "the stock test never looks at the attacker's house"
    );
}
