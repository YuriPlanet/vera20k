//! Phobos activation and linked-superweapon announcement facts emitted by the
//! shared launch tail (`Ext/SWType/FireSuperWeapon.cpp:40-42`, `:482-488`).
//!
//! The simulation owns only what it can see: which House launched, which type,
//! and whether a `SW.Link` action actually activated. The Owner/Allies/Enemies
//! column belongs to the listening client, so it is not decided here.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::superweapon::chronosphere_tests::{charge_super, click, world_with};
use crate::sim::world::SimSoundEvent;

const OWNER: &str = "Americans";
const ROOT: &str = "RootSpecial";
const LINK: &str = "LinkSpecial";
const TARGET: (u16, u16) = (30, 30);

const BASE: &str = "[InfantryTypes]\n0=E1\n[AircraftTypes]\n[VehicleTypes]\n[BuildingTypes]\n\
[E1]\nStrength=100\nSpeed=4\nSight=5\n\
[SuperWeaponTypes]\n0=RootSpecial\n1=LinkSpecial\n\
[RootSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
[LinkSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n";

fn rules_with(extra: &str) -> RuleSet {
    let mut ini = IniFile::from_str(BASE);
    ini.merge(&IniFile::from_str(extra));
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::empty())
        .expect("rules with the announcement keys parse")
}

fn world(rules: RuleSet) -> (RuleSet, Simulation, InternedId) {
    world_with(rules, 64, &[])
}

/// Every `SuperWeaponActivatedMessage` fact, in emission order.
fn activated_messages(sim: &Simulation) -> Vec<(InternedId, InternedId)> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::SuperWeaponActivatedMessage { owner, sw_type } => {
                Some((*owner, *sw_type))
            }
            _ => None,
        })
        .collect()
}

/// `ApplyActivatedMessage`/`ApplyActivatedEva` (`:40-42`) run for every launch,
/// whichever columns a type sets: the sim reports the launching House and the
/// launched type once, after the stock `SuperWeaponLaunched` fact.
#[test]
fn a_launch_reports_the_launcher_once_after_the_stock_fact() {
    let rules = rules_with(
        "[RootSpecial]\n\
         Message.Activated.Owner=Msg:Own\n\
         Message.Activated.Allies=Msg:Ally\n\
         Message.Activated.Enemies=Msg:Enemy\n\
         EVA.Activated.Owner=EVA_UnitSold\n\
         EVA.Activated.Allies=EVA_StructureSold\n\
         EVA.Activated.Enemies=EVA_UnitSold\n",
    );
    let (rules, mut sim, owner) = world(rules);
    let root = charge_super(&mut sim, owner, ROOT);
    click(&mut sim, &rules, owner, ROOT, TARGET);
    assert_eq!(
        activated_messages(&sim),
        vec![(owner, root)],
        "one fact carrying the launching House and the launched type"
    );
    let launched = sim
        .sound_events
        .iter()
        .position(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { .. }))
        .expect("the stock launch fact");
    let activated = sim
        .sound_events
        .iter()
        .position(|event| matches!(event, SimSoundEvent::SuperWeaponActivatedMessage { .. }))
        .expect("the Phobos announcement fact");
    assert!(
        launched < activated,
        "native runs the announcement at FireSuperWeaponExt's tail (:40-42)"
    );
}

/// A type with no Phobos keys still reports the launch: the app, not the sim,
/// decides that an empty column speaks nothing.
#[test]
fn a_launch_without_phobos_keys_still_reports_the_launcher() {
    let (rules, mut sim, owner) = world(rules_with(""));
    let root = charge_super(&mut sim, owner, ROOT);
    click(&mut sim, &rules, owner, ROOT, TARGET);
    assert_eq!(activated_messages(&sim), vec![(owner, root)]);
}

/// `SWTypeExt::ApplyLinkedSW`'s tail only fires when a link actually granted,
/// readied or reset something (`isActive`, `:482`). A present linked Super
/// with no `SW.Link.Grant`/`Ready`/`Reset` leaves the flag false.
#[test]
fn linked_announcement_needs_an_activated_link() {
    let idle = rules_with("[RootSpecial]\nSW.Link=LinkSpecial\n");
    let (idle, mut sim, owner) = world(idle);
    charge_super(&mut sim, owner, ROOT);
    charge_super(&mut sim, owner, LINK);
    click(&mut sim, &idle, owner, ROOT, TARGET);
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::SuperWeaponLinkedAcquired { .. })),
        "a present link that was neither readied nor reset is not active"
    );

    let granted = rules_with("[RootSpecial]\nSW.Link=LinkSpecial\nSW.Link.Grant=yes\n");
    let (granted, mut sim, owner) = world(granted);
    let root = charge_super(&mut sim, owner, ROOT);
    click(&mut sim, &granted, owner, ROOT, TARGET);
    let facts: Vec<_> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::SuperWeaponLinkedAcquired { owner, sw_type } => Some((*owner, *sw_type)),
            _ => None,
        })
        .collect();
    assert_eq!(
        facts,
        vec![(owner, root)],
        "a granted link reports the granting House and the granting type"
    );
}

/// Every EVA cue a sale emitted, whichever `PlayEVA` owner carried it.
fn sold_eva(sim: &Simulation) -> Vec<String> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::HouseEva { event, .. } => Some((*event).to_owned()),
            SimSoundEvent::HouseEvaNamed { event, .. } => Some(event.clone()),
            _ => None,
        })
        .collect()
}

/// Phobos `FootClass_Sell` hook 0x4D9F7B (`Ext/Building/Hooks.Selling.cpp:18-21`):
/// the unit's `EVA.Sold` replaces the stock `EVA_UnitSold`; an absent key
/// keeps the stock line. A blank key is dropped by the text parser, so it
/// reads as absent; the reader's explicit-none arm serves retained
/// projections that keep empty values. Unit selling is an Ares capability
/// (`production::foot_on_sold`), so this test needs both features.
#[test]
fn a_sold_unit_uses_the_types_eva_sold_override() {
    let named = rules_with("[E1]\nEVA.Sold=EVA_LinkedSW\n");
    let (named, mut sim, _owner) = world(named);
    let id = sim
        .spawn_object_at_height("E1", OWNER, 20, 20, 0, 0, &named)
        .expect("the unit spawns");
    assert!(crate::sim::production::foot_on_sold(
        &mut sim,
        &named,
        id,
        crate::sim::world::UninitContext::new(&named),
    ));
    assert_eq!(sold_eva(&sim), vec!["EVA_LinkedSW".to_string()]);

    let absent = rules_with("");
    let (absent, mut sim, _owner) = world(absent);
    let id = sim
        .spawn_object_at_height("E1", OWNER, 20, 20, 0, 0, &absent)
        .expect("the unit spawns");
    assert!(crate::sim::production::foot_on_sold(
        &mut sim,
        &absent,
        id,
        crate::sim::world::UninitContext::new(&absent),
    ));
    assert_eq!(
        sold_eva(&sim),
        vec!["EVA_UnitSold".to_string()],
        "an absent key keeps the stock line"
    );

    let blank = rules_with("[E1]\nEVA.Sold=\n");
    let (blank, mut sim, _owner) = world(blank);
    let id = sim
        .spawn_object_at_height("E1", OWNER, 20, 20, 0, 0, &blank)
        .expect("the unit spawns");
    assert!(crate::sim::production::foot_on_sold(
        &mut sim,
        &blank,
        id,
        crate::sim::world::UninitContext::new(&blank),
    ));
    assert_eq!(
        sold_eva(&sim),
        vec!["EVA_UnitSold".to_string()],
        "the text parser drops `EVA.Sold=`, so a blank key keeps the stock line"
    );
}

/// Phobos `FootClass`'s `DisplayIncome` label (`Hooks.Selling.cpp:26-27`):
/// a sold Foot prints its refund at its Location when the global is on. Unit
/// selling is the Ares capability (`production::foot_on_sold`).
#[test]
fn a_sold_unit_prints_the_display_income_label() {
    use crate::sim::flying_text::BountyText;
    use crate::sim::world::{FrameEffects, UninitContext};

    let rules = rules_with(
        "[E1]
Cost=200
[AudioVisual]
DisplayIncome=yes
",
    );
    let (rules, mut sim, _owner) = world(rules);
    let id = sim
        .spawn_object_at_height("E1", OWNER, 20, 20, 0, 0, &rules)
        .expect("the unit spawns");
    let refund = crate::sim::production::techno_refund(&sim, &rules, id).expect("refund");
    assert!(refund > 0, "the type has a cost and the refund percent");
    let texts = std::cell::RefCell::new(Vec::new());
    let observe = |_: &Simulation, _: &crate::rules::ruleset::RuleSet, text: BountyText| {
        texts.borrow_mut().push(text);
    };
    let effects = FrameEffects::empty().with_bounty_text(&observe);
    assert!(crate::sim::production::foot_on_sold(
        &mut sim,
        &rules,
        id,
        UninitContext::new(&rules).with_effects(effects),
    ));
    let texts = texts.into_inner();
    assert_eq!(texts.len(), 1, "one DisplayIncome label");
    assert_eq!(texts[0].entity_id, Some(id));
    assert_eq!(texts[0].amount, refund);
}
