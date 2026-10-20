//! Ares3.0 custom-armor records through the existing production consumers.
//! Hook identities and executed cases: rules::armor and its native corpus.
//! These are Rust routing regressions, not a whole-match parity claim.

use super::combat_weapon::{attacker_facts, techno_target_facts, what_weapon_should_i_use};
use super::fire_error::FireError;
use super::fire_error_world::FireSubject;
use super::{
    EntityDamageEvent, ScanMission, TargetKind, combat_targeting, greatest_threat, world_fire,
};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::world::Simulation;
use crate::util::native_x87::MaskedX87Chop53;

fn rules(primary: &str, secondary: &str, flags: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[ArmorTypes]\npaper=steel\n\
         [Clear]\nTrack=100%\nBuildable=yes\n\
         [VehicleTypes]\n0=SCORER\n1=TARGET\n\
         [InfantryTypes]\n[BuildingTypes]\n[AircraftTypes]\n\
         [SCORER]\nStrength=300\nArmor=paper\nSpeed=6\nSight=8\n\
         Primary=PRIMARY\nSecondary=SECONDARY\nCanRetaliate=yes\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
         [TARGET]\nStrength=300\nArmor=paper\nSpeed=6\nSight=8\n\
         Primary=PRIMARY\nSecondary=SECONDARY\nCanRetaliate=yes\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
         [PRIMARY]\nDamage=101\nROF=30\nRange=6\nProjectile=SHOT\nWarhead=PWH\nOmniFire=yes\n\
         [SECONDARY]\nDamage=101\nROF=30\nRange=6\nProjectile=SHOT\nWarhead=SWH\nOmniFire=yes\n\
         [SHOT]\nInviso=yes\nAG=yes\nAA=no\n\
         [PWH]\nCellSpread=0\nVersus.paper={primary}\n{flags}\n\
         [SWH]\nCellSpread=0\nVersus.paper={secondary}\n{flags}\n"
    )))
    .expect("ordinary custom armor rules")
}

fn scene(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let scorer = sim
        .spawn_object("SCORER", "Russians", 10, 10, 0, rules)
        .unwrap();
    let target = sim
        .spawn_object("TARGET", "Americans", 12, 10, 0, rules)
        .unwrap();
    assert!(rules.object("TARGET").unwrap().armor_id().index() >= 11);
    (sim, scorer, target)
}

fn fire_error(sim: &Simulation, rules: &RuleSet, scorer: u64, target: u64) -> FireError {
    FireSubject {
        world: sim,
        rules,
        overlay_registry: None,
        fog: None,
        firer: sim.substrate.entities.get(scorer).unwrap(),
        obj: rules.object("SCORER").unwrap(),
        target: Some(TargetKind::Entity(target)),
        weapon_index: 0,
    }
    .fire_error(false)
}

#[test]
fn custom_armor_selection_preserves_numeric_zero_and_secondary_first_order() {
    // Installed hook10054360 checks secondary zero first, independently of
    // the ForceFire override which allows a zero record through CanFire.
    for (primary, secondary, expected) in [
        ("0%", "25%", 1),
        ("25%", "0%", 0),
        ("0%", "0%", 0),
        ("25%", "25%", 0),
    ] {
        let rules = rules(primary, secondary, "Versus.paper.ForceFire=yes");
        let (sim, scorer, target) = scene(&rules);
        let actor = sim.substrate.entities.get(scorer).unwrap();
        let object = rules.object("SCORER").unwrap();
        let target = techno_target_facts(
            sim.substrate.entities.get(target).unwrap(),
            rules.object("TARGET").unwrap(),
            sim.resolved_terrain.as_ref(),
            false,
            &rules,
            &sim.interner,
        );
        assert_eq!(
            what_weapon_should_i_use(
                &rules,
                object,
                &attacker_facts(actor, object),
                Some(&target)
            ),
            expected,
            "primary={primary}, secondary={secondary}",
        );
    }
}

#[test]
fn custom_armor_can_fire_uses_force_fire_or_nonzero_multiplier() {
    for (multiplier, force_fire, admitted) in [
        ("0%", "no", false),
        ("0%", "yes", true),
        ("25%", "no", true),
    ] {
        let rules = rules(
            multiplier,
            multiplier,
            &format!("Versus.paper.ForceFire={force_fire}"),
        );
        let (sim, scorer, target) = scene(&rules);
        assert_eq!(
            fire_error(&sim, &rules, scorer, target) != FireError::Illegal,
            admitted
        );
    }
}

#[test]
fn custom_armor_passive_and_retaliation_flags_are_independent_of_each_other_and_damage() {
    for (multiplier, passive, retaliate) in [
        ("25%", false, false),
        ("0%", true, false),
        ("0%", false, true),
        ("0%", true, true),
    ] {
        let flag = |value| if value { "yes" } else { "no" };
        let rules = rules(
            multiplier,
            multiplier,
            &format!(
                "Versus.paper.ForceFire=yes\nVersus.paper.PassiveAcquire={}\nVersus.paper.Retaliate={}",
                flag(passive),
                flag(retaliate),
            ),
        );
        let (mut sim, scorer, target) = scene(&rules);
        assert_eq!(
            combat_targeting::should_retaliate(&sim, &rules, scorer, target),
            retaliate,
            "multiplier={multiplier}, passive={passive}, retaliate={retaliate}",
        );
        assert_eq!(
            sim.greatest_threat_represented(&rules, None, scorer, ScanMission::Guard, None),
            passive.then_some(target),
            "multiplier={multiplier}, passive={passive}, retaliate={retaliate}",
        );
    }
}

#[test]
fn custom_armor_positive_receive_and_estimate_share_the_effective_record() {
    let rules = rules("25%", "25%", "");
    let (mut sim, scorer, target) = scene(&rules);
    let weapon = rules.weapon("PRIMARY").unwrap();
    // Complete native GetTotalDamage control:101 * custom25% produces25.
    assert_eq!(
        world_fire::estimated_damage_on(&sim, &rules, scorer, target, weapon),
        25
    );
    // A real damage emitter interns the selected warhead when building its
    // event; intern_rule_type_ids does not seed warhead identities.
    let warhead_ref = sim.interner.intern(weapon.warhead.as_deref().unwrap());
    let event = EntityDamageEvent::area(
        target,
        101,
        0,
        scorer,
        Some(sim.substrate.entities.get(scorer).unwrap().owner()),
        warhead_ref,
    );
    let resolved = super::receiver_call::resolve_receive_damage(
        &event,
        &sim.substrate.entities,
        &rules,
        &sim.interner,
        &sim.houses,
        sim.house_alliances.as_map(),
        false,
        0,
        sim.resolved_terrain.as_ref(),
    )
    .unwrap();
    assert_eq!(resolved.outcome.hp_delta, 25);
}

#[test]
fn custom_armor_threat_score_consumes_both_effectiveness_terms() {
    let score = |multiplier: &str, mine: f64, theirs: f64| {
        let rules = rules(multiplier, multiplier, "");
        let (sim, scorer, target) = scene(&rules);
        let result = greatest_threat::calculate_threat_score(
            &sim.substrate.entities,
            scorer,
            target,
            &rules,
            &sim.interner,
            sim.resolved_terrain.as_ref(),
            Some(sim.house_alliances.as_map()),
            greatest_threat::ThreatCoefficients {
                my_effectiveness: mine,
                target_effectiveness: theirs,
                target_special_threat: 0.0,
                target_strength: 0.0,
                target_distance: 0.0,
            },
            greatest_threat::ThreatReference::NullCoord,
            None,
        )
        .unwrap();
        MaskedX87Chop53::store_f64_masked_chop(result)
    };
    // Routing checks: these two terms must notice custom armor instead of
    // silently using the unchanged none/100% legacy entry in either direction.
    assert_ne!(score("25%", 100.0, 0.0), score("100%", 100.0, 0.0));
    assert_ne!(score("25%", 0.0, 100.0), score("100%", 0.0, 100.0));
}
