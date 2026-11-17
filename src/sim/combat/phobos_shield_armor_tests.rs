//! Phobos `TechnoClass_ReplaceArmorWithShields` (`Ext/Techno/Hooks.cpp:480-515`)
//! at the four native armor loads it replaces, through the production consumers
//! that read those instruction positions: `TechnoClass_Calculate_Threat_Score`
//! (`0x70CF39`), `CanAutoTargetObject` (`0x6F7D31`), `GetFireError`
//! (`0x6FCB64`) and `ShouldRetaliate` (`0x708AEB`).
//!
//! Each test flips one object's shield between live and broken, so the Verses
//! column that answers moves between the shield's `none` armor and the
//! techno's `heavy` armor without any other state changing.

use super::fire_error::FireError;
use super::fire_error_world::FireSubject;
use super::{ScanMission, TargetKind, combat_targeting, greatest_threat};
use crate::rules::ruleset::RuleSet;
use crate::sim::arena_fixture;
use crate::sim::shield::ShieldState;
use crate::sim::world::Simulation;
use crate::util::native_x87::MaskedX87Chop53;

/// `[Weak]` shield strength; a broken shield reports the same strength with no
/// points left, which [`crate::sim::shield::ShieldState::is_active`] refuses.
const STRENGTH: i32 = 500;

/// Every techno here carries `heavy`; only `SHIELDED` carries a `[Weak]` shield
/// whose `none` armor answers instead. `warhead` names the warhead section the
/// single `Gun` fires, so each test decides which Verses columns differ.
fn shield_rules(warhead: &str) -> RuleSet {
    let ini = arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=SCORER\n1=PLAIN\n2=SHIELDED\n\
         [AircraftTypes]\n\
         [ShieldTypes]\n0=Weak\n\
         [Weak]\nStrength={STRENGTH}\nArmor=none\n\
         [SCORER]\nStrength=400\nCost=300\nSpeed=6\nSight=8\nArmor=heavy\n\
         CanRetaliate=yes\nPrimary=Gun\n\
         [PLAIN]\nStrength=400\nCost=300\nSpeed=6\nSight=8\nArmor=heavy\n\
         CanRetaliate=yes\nPrimary=Gun\n\
         [SHIELDED]\nStrength=400\nCost=300\nSpeed=6\nSight=8\nArmor=heavy\n\
         CanRetaliate=yes\nPrimary=Gun\nShieldType=Weak\n\
         [Gun]\nDamage=10\nROF=50\nRange=6\nWarhead={warhead}\nOmniFire=yes\n\
         [Blind]\nVerses=0%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Piercing]\nVerses=0%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         Shield.Penetrate=yes\n\
         [Reach]\nVerses=1%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ));
    RuleSet::from_ini(&ini).expect("shield armor rules")
}

/// A scorer beside `target_type`, which is the object carrying the shield.
/// The scorer is the arena's computer house and the target belongs to the
/// arena's human house, the arrangement `rival_arena` gives a scanner that
/// acquires without the extra `0x006F81BE` discovery gate.
fn arena(rules: &RuleSet, target_type: &str, seed: u64) -> (Simulation, u64, u64) {
    let mut sim = arena_fixture::rival_arena(rules, seed);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let scorer = sim
        .spawn_object("SCORER", "Russians", 8, 10, 0, rules)
        .expect("scorer spawns");
    let target = sim
        .spawn_object(target_type, "Americans", 12, 10, 0, rules)
        .expect("target spawns");
    (sim, scorer, target)
}

/// `ShieldClass::ChangeStrength` to zero: the shield keeps its type and
/// strength but is no longer `IsActive()`, as a shield burnt down in play.
fn break_shield(sim: &mut Simulation, id: u64) {
    sim.substrate
        .entities
        .get_mut(id)
        .expect("shielded object")
        .shield = Some(ShieldState::new("Weak".to_owned(), 0, STRENGTH));
}

fn fire_error(sim: &Simulation, rules: &RuleSet, scorer: u64, target: u64) -> FireError {
    FireSubject {
        world: sim,
        rules,
        overlay_registry: None,
        fog: None,
        firer: sim.substrate.entities.get(scorer).expect("firer"),
        obj: rules.object("SCORER").expect("scorer type"),
        target: Some(TargetKind::Entity(target)),
        weapon_index: 0,
    }
    .fire_error(false)
}

/// `0x70CF39`: the A term's Verses read the candidate's armor. `Blind` answers
/// 0% there and 100% against `heavy`, so a live shield removes exactly the
/// scorer's own effectiveness coefficient from the score.
#[test]
fn threat_score_reads_the_candidates_shield_armor() {
    let rules = shield_rules("Blind");
    let score = |target_type: &str| {
        let (sim, scorer, target) = arena(&rules, target_type, 31);
        let result = greatest_threat::calculate_threat_score(
            &sim.substrate.entities,
            scorer,
            target,
            &rules,
            &sim.interner,
            sim.resolved_terrain.as_ref(),
            Some(&sim.house_alliances),
            greatest_threat::ThreatCoefficients {
                my_effectiveness: 1.0,
                target_effectiveness: 0.0,
                target_special_threat: 0.0,
                target_strength: 0.0,
                target_distance: 0.0,
            },
            greatest_threat::ThreatReference::NullCoord,
            None,
            None,
        )
        .expect("a threat score");
        f64::from_bits(MaskedX87Chop53::store_f64_masked_chop(result).bits())
    };
    let plain = score("PLAIN");
    let shielded = score("SHIELDED");
    assert_eq!(
        plain - shielded,
        1.0,
        "plain={plain}, shielded={shielded}: the shield's none armor answers 0%"
    );
}

/// `0x6F7D31`: the candidate the scan may take passively. The same candidate
/// is refused while its shield absorbs the scanner's warhead and taken once
/// the shield is broken.
#[test]
fn candidate_acquisition_reads_the_candidates_shield_armor() {
    let rules = shield_rules("Blind");
    let (mut sim, scorer, target) = arena(&rules, "SHIELDED", 32);
    assert!(
        sim.entities()
            .get(target)
            .expect("target")
            .shield()
            .is_some(),
        "the shielded type carries a live shield"
    );
    let represented = |sim: &mut Simulation| {
        sim.greatest_threat_represented(&rules, None, scorer, ScanMission::Guard, None)
            .and_then(TargetKind::object_id)
    };
    assert_eq!(
        represented(&mut sim),
        None,
        "a live shield's 0% column refuses the candidate at 0x6F7D31"
    );
    break_shield(&mut sim, target);
    assert_eq!(
        represented(&mut sim),
        Some(target),
        "a broken shield falls back to the techno's heavy armor"
    );
}

/// `0x6FCB64`: the target armor `GetFireError` weighs. A shot the live shield
/// absorbs is illegal; the same shot against the broken shield is not, and a
/// `Shield.Penetrate` warhead never consults the shield at all.
#[test]
fn fire_error_reads_the_targets_shield_armor() {
    let rules = shield_rules("Blind");
    let (mut sim, scorer, target) = arena(&rules, "SHIELDED", 33);
    assert_eq!(
        fire_error(&sim, &rules, scorer, target),
        FireError::Illegal,
        "a live shield's 0% column refuses the shot at 0x6FCB64"
    );
    break_shield(&mut sim, target);
    assert_ne!(
        fire_error(&sim, &rules, scorer, target),
        FireError::Illegal,
        "a broken shield falls back to the techno's heavy armor"
    );

    let penetrating = shield_rules("Piercing");
    let (sim, scorer, target) = arena(&penetrating, "SHIELDED", 34);
    assert_ne!(
        fire_error(&sim, &penetrating, scorer, target),
        FireError::Illegal,
        "Shield.Penetrate keeps the techno's armor"
    );
}

/// `0x708AEB`: the armor the victim's Verses answer for the source. `1%`
/// against the shield's `none` armor still passes `CanFire`'s zero test but
/// misses the retaliation threshold, while the techno's `heavy` column answers
/// 100% — so only the armor source moves the answer between the two gates.
///
/// Only the Ares build separates the two gates: the stock Verses reader takes
/// whole percents, so without `Versus.<armor>.Retaliate` every column
/// `CanFire` admits (1% and up) also clears the widened `0.01f` threshold.
#[cfg(feature = "ares")]
#[test]
fn retaliation_reads_the_sources_shield_armor() {
    let rules = shield_rules("Reach");
    let (mut sim, victim, source) = arena(&rules, "SHIELDED", 35);
    let retaliates =
        |sim: &Simulation| combat_targeting::should_retaliate(sim, &rules, victim, source, None);
    assert!(
        !retaliates(&sim),
        "the live shield's 1% column misses the 0.01 threshold"
    );
    break_shield(&mut sim, source);
    assert!(
        retaliates(&sim),
        "a broken shield falls back to the techno's heavy armor and its 100%"
    );
}
