//! Phobos `SW.Next` chains and `SW.Link` groups through the production rules
//! reader and the shared launch tail
//! (`Ext/SWType/FireSuperWeapon.cpp:305-486`).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::superweapon::chronosphere_tests::{charge_super, click, world_with};
use crate::sim::world::SimSoundEvent;

const OWNER: &str = "Americans";
const ROOT: &str = "RootSpecial";
const NEXT: &str = "NextSpecial";
const OTHER: &str = "OtherSpecial";
const LINK: &str = "LinkSpecial";
const TARGET: (u16, u16) = (30, 30);

const BASE: &str = "[General]\nAIFireSale=yes\n[InfantryTypes]\n0=E1\n1=INHIB\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
[E1]\nStrength=100\nSpeed=4\n\
[INHIB]\nStrength=100\nSpeed=4\nSight=5\n\
[SuperWeaponTypes]\n0=RootSpecial\n1=NextSpecial\n2=OtherSpecial\n3=LinkSpecial\n\
[RootSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
[NextSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
[OtherSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
[LinkSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n";

fn rules_with(extra: &str) -> RuleSet {
    // `SW.Next` and `SW.Inhibitors` are Ares keys.
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let mut ini = IniFile::from_str(BASE);
    ini.merge(&IniFile::from_str(extra));
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::empty())
        .expect("rules with the chaining keys parse")
}

fn world(rules: RuleSet) -> (RuleSet, Simulation, InternedId) {
    world_with(rules, 64, &[])
}

/// How many `SuperWeaponLaunched` facts the launch tail emitted for `sw`.
fn launches(sim: &Simulation, sw: InternedId) -> usize {
    sim.sound_events
        .iter()
        .filter(|event| {
            matches!(
                event,
                SimSoundEvent::SuperWeaponLaunched { sw_type, .. } if *sw_type == sw
            )
        })
        .count()
}

fn instance<'a>(sim: &'a Simulation, owner: InternedId, sw: &str) -> &'a SuperWeaponInstance {
    let id = sim.interner.get(sw).expect("interned type");
    &sim.super_weapons[&owner][&id]
}

/// `SW.Next=` launches each listed, charged super after the root, consuming
/// its charge (`pSuper->SetReadiness(true); Launch; Reset`).
#[test]
fn next_launches_the_listed_charged_supers() {
    let rules = rules_with("[RootSpecial]\nSW.Next=NextSpecial,OtherSpecial\n");
    let (rules, mut sim, owner) = world(rules);
    let root = charge_super(&mut sim, owner, ROOT);
    let next = charge_super(&mut sim, owner, NEXT);
    let other = charge_super(&mut sim, owner, OTHER);
    let old_start = instance(&sim, owner, NEXT).charge_start_tick();
    click(&mut sim, &rules, owner, ROOT, TARGET);
    assert_eq!(launches(&sim, root), 1, "the root fired once");
    assert_eq!(launches(&sim, next), 1, "the first listed super launched");
    assert_eq!(launches(&sim, other), 1, "the second listed super launched");
    let after = instance(&sim, owner, NEXT);
    assert!(!after.is_ready(), "the chained launch consumed its charge");
    assert!(
        after.charge_start_tick() > old_start,
        "and restarted the recharge timer"
    );
}

/// `SW.Next.RealLaunch` default yes refuses an absent/unready/suspended next
/// type; `no` launches it forcibly and writes the saved timer back.
#[test]
fn real_launch_controls_the_next_types_charge() {
    let rules = rules_with("[RootSpecial]\nSW.Next=NextSpecial\n");
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    let next = charge_super(&mut sim, owner, NEXT);
    {
        let entity = sim
            .super_weapons
            .instance_mut(&owner, &next)
            .expect("retained");
        entity.set_is_ready_for_test(false);
        entity.set_charge_start_tick_for_test(100);
        entity.set_charge_duration_for_test(50);
    }
    click(&mut sim, &rules, owner, ROOT, TARGET);
    assert_eq!(
        launches(&sim, next),
        0,
        "RealLaunch=yes needs a charged next type"
    );
    let untouched = instance(&sim, owner, NEXT);
    assert_eq!(untouched.charge_start_tick(), 100);
    assert_eq!(untouched.charge_duration(), 50);

    let forced = rules_with("[RootSpecial]\nSW.Next=NextSpecial\nSW.Next.RealLaunch=no\n");
    let (forced, mut sim, owner) = world(forced);
    charge_super(&mut sim, owner, ROOT);
    let next = charge_super(&mut sim, owner, NEXT);
    {
        let entity = sim
            .super_weapons
            .instance_mut(&owner, &next)
            .expect("retained");
        entity.set_is_ready_for_test(false);
        entity.set_charge_start_tick_for_test(100);
        entity.set_charge_duration_for_test(50);
    }
    click(&mut sim, &forced, owner, ROOT, TARGET);
    assert_eq!(
        launches(&sim, next),
        1,
        "RealLaunch=no launches regardless of readiness"
    );
    let restored = instance(&sim, owner, NEXT);
    assert_eq!(restored.charge_start_tick(), 100, "the timer is restored");
    assert_eq!(restored.charge_duration(), 50);
    assert!(
        !restored.is_ready(),
        "Reset still cleared readiness; the timer refills it"
    );
}

/// Random mode starts at the first `SW.Next.RandomWeights` list, not at
/// `SW.Next.RollChances`: with weights present, each chance draws one Scenario
/// `RandomDouble` and a zero chance never fires; with one sure roll the weights
/// pick the index.
#[test]
fn next_rolls_and_weights_select_the_chained_supers() {
    let never = rules_with(
        "[RootSpecial]\nSW.Next=NextSpecial\nSW.Next.RandomWeights=1\n\
         SW.Next.RollChances=0\n",
    );
    let (never, mut sim, owner) = world(never);
    charge_super(&mut sim, owner, ROOT);
    let next = charge_super(&mut sim, owner, NEXT);
    click(&mut sim, &never, owner, ROOT, TARGET);
    assert_eq!(launches(&sim, next), 0, "a zero roll chance never fires");

    let weighted =
        rules_with("[RootSpecial]\nSW.Next=NextSpecial,OtherSpecial\nSW.Next.RandomWeights=0,1\n");
    let (weighted, mut sim, owner) = world(weighted);
    charge_super(&mut sim, owner, ROOT);
    let next = charge_super(&mut sim, owner, NEXT);
    let other = charge_super(&mut sim, owner, OTHER);
    click(&mut sim, &weighted, owner, ROOT, TARGET);
    assert_eq!(launches(&sim, next), 0, "weight 0 never picks the first");
    assert_eq!(launches(&sim, other), 1, "weight 1 always picks the second");
}

/// A next type's `SW.Inhibitors` blocks the chain unless
/// `SW.Next.IgnoreInhibitors=yes` (the lists are the Ares-owned keys).
#[test]
fn next_inhibitors_block_the_chain_unless_ignored() {
    let blocked = rules_with(
        "[RootSpecial]\nSW.Next=NextSpecial\n\
         [NextSpecial]\nSW.Inhibitors=INHIB\n",
    );
    let (blocked, mut sim, owner) = world(blocked);
    charge_super(&mut sim, owner, ROOT);
    let next = charge_super(&mut sim, owner, NEXT);
    sim.spawn_object_at_height("INHIB", "Russians", TARGET.0, TARGET.1, 0, 0, &blocked)
        .expect("the inhibitor spawns");
    click(&mut sim, &blocked, owner, ROOT, TARGET);
    assert_eq!(
        launches(&sim, next),
        0,
        "an enemy inhibitor in range refuses the chained launch"
    );

    let ignored = rules_with(
        "[RootSpecial]\nSW.Next=NextSpecial\nSW.Next.IgnoreInhibitors=yes\n\
         [NextSpecial]\nSW.Inhibitors=INHIB\n",
    );
    let (ignored, mut sim, owner) = world(ignored);
    charge_super(&mut sim, owner, ROOT);
    let next = charge_super(&mut sim, owner, NEXT);
    sim.spawn_object_at_height("INHIB", "Russians", TARGET.0, TARGET.1, 0, 0, &ignored)
        .expect("the inhibitor spawns");
    click(&mut sim, &ignored, owner, ROOT, TARGET);
    assert_eq!(
        launches(&sim, next),
        1,
        "IgnoreInhibitors bypasses the inhibitor list"
    );
}

/// `SW.Link.Grant` makes a missing linked super present (a one-time grant);
/// `SW.Link.Ready` then leaves it charged, and the weighted list selects one.
#[test]
fn link_grant_and_ready_use_the_weighted_selection() {
    let rules = rules_with(
        "[RootSpecial]\nSW.Link=NextSpecial,OtherSpecial\nSW.Link.Grant=yes\n\
         SW.Link.Ready=yes\nSW.Link.RandomWeights=0,1\n",
    );
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    assert!(!instance(&sim, owner, NEXT).is_active());
    click(&mut sim, &rules, owner, ROOT, TARGET);
    assert!(
        !instance(&sim, owner, NEXT).is_active(),
        "weight 0 skips the first linked super"
    );
    let linked = instance(&sim, owner, OTHER);
    assert!(linked.is_active(), "the second linked super is granted");
    assert!(linked.is_ready(), "SW.Link.Ready charges it");
    assert_eq!(linked.charge_duration(), 0, "TimeLeft was written to 0");
}

/// `SW.Link.Reset` restarts a present link's recharge; without it a bare
/// grant stays charging (the granted `Reset` arm).
#[test]
fn link_reset_restarts_the_charge() {
    let reset = rules_with("[RootSpecial]\nSW.Link=LinkSpecial\nSW.Link.Reset=yes\n");
    let (reset, mut sim, owner) = world(reset);
    charge_super(&mut sim, owner, ROOT);
    charge_super(&mut sim, owner, LINK);
    let old_start = instance(&sim, owner, LINK).charge_start_tick();
    click(&mut sim, &reset, owner, ROOT, TARGET);
    let after = instance(&sim, owner, LINK);
    assert!(!after.is_ready(), "Reset cleared readiness");
    assert!(
        after.charge_start_tick() > old_start,
        "and restarted the recharge timer"
    );

    let grant_only = rules_with("[RootSpecial]\nSW.Link=LinkSpecial\nSW.Link.Grant=yes\n");
    let (grant_only, mut sim, owner) = world(grant_only);
    charge_super(&mut sim, owner, ROOT);
    click(&mut sim, &grant_only, owner, ROOT, TARGET);
    let granted = instance(&sim, owner, LINK);
    assert!(granted.is_active(), "the grant made it present");
    assert!(
        !granted.is_ready(),
        "a bare grant is reset to a normal recharge"
    );
}
