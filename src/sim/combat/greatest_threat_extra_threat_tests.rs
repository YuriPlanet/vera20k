//! Phobos `ExtraThreat.*` / `ExtraThreatCoefficient.*` bonuses through the
//! production `TechnoClass::Calculate_Threat_Score @ 0x0070CD10` owner
//! (`TechnoClass_ThreatCoefficient_CanAttackMeThreatBonus @ 0x70CF87`,
//! `Ext/Techno/Hooks.TargetEvaluation.cpp:429-511`).
//!
//! Every score below is `calculate_threat_score`'s raw x87 result truncated by
//! `truncate_score`, exactly as `Evaluate_Candidate` reads it. The patched
//! loads differ from the unpatched one only by the keys each test authors, so
//! the score difference isolates the bonus under test.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::RetailBattleRules;
use crate::sim::combat::TargetKind;
use crate::sim::combat::extra_threat::ThreatBonusContext;
use crate::sim::world::Simulation;

/// A rival-house arena with an `Americans` scanner at (10, 10) and a
/// `Russians` candidate at `at`, on rules patched by `patch` (the map layer).
fn scene(
    patch: &str,
    scanner_type: &str,
    candidate_type: &str,
    at: (u16, u16),
) -> Option<(RetailBattleRules, Simulation, u64, u64)> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &IniFile::from_str(patch),
    )?;
    let mut sim = crate::sim::arena_fixture::rival_arena(&retail.rules, 1);
    sim.intern_rule_type_ids(&retail.rules);
    sim.resolve_type_handles(&retail.rules);
    let scanner = sim.spawn_object(scanner_type, "Americans", 10, 10, 0, &retail.rules)?;
    let candidate = sim.spawn_object(candidate_type, "Russians", at.0, at.1, 0, &retail.rules)?;
    Some((retail, sim, scanner, candidate))
}

/// The candidate's truncated threat score for this scanner, with the Phobos
/// bonus context the production scan supplies.
fn score_of(
    retail: &RetailBattleRules,
    sim: &Simulation,
    scanner: u64,
    candidate: u64,
    context: bool,
) -> i32 {
    let rules = &retail.rules;
    let scanner_type = sim
        .interner
        .object_type(
            sim.substrate.entities.get(scanner).unwrap().type_ref(),
            rules,
        )
        .unwrap();
    let coefficients =
        ThreatCoefficients::resolve(rules, scanner_type, HOUSE_SELECTS_OWN_COEFFICIENTS);
    let bonus = context
        .then(|| ThreatBonusContext::for_world(sim, None))
        .flatten();
    let score = calculate_threat_score(
        &sim.substrate.entities,
        scanner,
        candidate,
        rules,
        &sim.interner,
        sim.resolved_terrain.as_ref(),
        Some(&sim.house_alliances),
        coefficients,
        ThreatReference::NullCoord,
        None,
        bonus,
    )
    .unwrap();
    truncate_score(score)
}

/// Patched score minus the unpatched score for the same scene.
fn delta(patch: &str, scanner_type: &str, candidate_type: &str, at: (u16, u16)) -> Option<i32> {
    let (retail, sim, scanner, candidate) = scene(patch, scanner_type, candidate_type, at)?;
    let with_keys = score_of(&retail, &sim, scanner, candidate, true);
    let (base_retail, base_sim, base_scanner, base_candidate) =
        scene("", scanner_type, candidate_type, at)?;
    let base = score_of(&base_retail, &base_sim, base_scanner, base_candidate, false);
    Some(with_keys - base)
}

/// `ExtraThreat.IsThreat` (`Hooks.TargetEvaluation.cpp:441-453`): a candidate
/// that can fire at the scanner earns the bonus, `GetFireError(...,
/// ignoreRange=true)` keeps an out-of-range armed target, and an unarmed
/// target adds nothing (`IsAThreatToMe :393-394` returns false on a null
/// weapon).
#[test]
fn extra_threat_is_threat_needs_a_target_that_can_fire() {
    if crate::rules::retail_ini_fixture::retail_battle_rules().is_none() {
        return;
    }
    let patch = "[E1]\nExtraThreat.IsThreat=5\n";
    assert_eq!(
        delta(patch, "E1", "MTNK", (12, 10)),
        Some(5),
        "an armed target two cells away is a threat"
    );
    assert_eq!(
        delta(patch, "E1", "MTNK", (28, 28)),
        Some(5),
        "ignoreRange=true keeps an armed target far outside weapon range"
    );
    assert_eq!(
        delta(patch, "E1", "MCV", (12, 10)),
        Some(0),
        "an unarmed target adds nothing"
    );
}

/// `AlwaysConsideredThreat=yes` (`Hooks.TargetEvaluation.cpp:387-388`) admits
/// a candidate before the weapon and fire-error tests, so a weaponless MCV
/// earns the same `IsThreat` bonus.
#[test]
fn always_considered_threat_admits_an_unarmed_candidate() {
    if crate::rules::retail_ini_fixture::retail_battle_rules().is_none() {
        return;
    }
    let patch = "[E1]\nExtraThreat.IsThreat=5\n[MCV]\nAlwaysConsideredThreat=yes\n";
    assert_eq!(delta(patch, "E1", "MCV", (12, 10)), Some(5));
}

/// `ApplyInRangeBonus` (`Hooks.TargetEvaluation.cpp:455-470`): the bonus is
/// `ExtraThreat.InRange` plus `distanceCells * InRangeDistance`, and it only
/// applies while `IsCloseEnoughToAttack` holds.
#[test]
fn extra_threat_in_range_bonus_scales_with_distance() {
    if crate::rules::retail_ini_fixture::retail_battle_rules().is_none() {
        return;
    }
    let patch = "[MTNK]\nExtraThreat.InRange=4\nExtraThreatCoefficient.InRangeDistance=2\n";
    assert_eq!(
        delta(patch, "MTNK", "HTNK", (12, 10)),
        Some(8),
        "two cells: 4 + 2 * 2"
    );
    assert_eq!(
        delta(patch, "MTNK", "HTNK", (13, 10)),
        Some(10),
        "three cells: 4 + 3 * 2"
    );
    assert_eq!(
        delta(patch, "MTNK", "HTNK", (22, 10)),
        Some(0),
        "twelve cells is beyond the weapon's reach, so nothing is added"
    );
}

/// `ApplyFacingBonus` (`Hooks.TargetEvaluation.cpp:472-488`): `deltaFacing` is
/// 0 when the fire facing points at the target and 32768 when it points away.
/// An Infantry scanner has no fire facing (`GetFireFacing :420-421`).
#[test]
fn extra_threat_facing_bonus_grows_with_the_angular_delta() {
    let patch = "[MTNK]\nExtraThreatCoefficient.Facing=0.001\n";
    let Some((retail, mut sim, scanner, candidate)) = scene(patch, "MTNK", "HTNK", (12, 10)) else {
        return;
    };
    let aligned = crate::sim::movement::turret::facing_toward_target(
        sim.substrate.entities.get(scanner).unwrap(),
        &TargetKind::Entity(candidate),
        &sim,
    )
    .unwrap();
    let set_facing = |sim: &mut Simulation, value: u16| {
        let entity = sim.substrate.entities.get_mut(scanner).unwrap();
        entity.body_facing = crate::sim::movement::FacingClass::new(value, 0);
        entity.barrel_facing = Some(crate::sim::movement::FacingClass::new(value, 0));
    };
    set_facing(&mut sim, aligned);
    let toward = score_of(&retail, &sim, scanner, candidate, true);
    set_facing(&mut sim, aligned.wrapping_add(0x8000));
    let away = score_of(&retail, &sim, scanner, candidate, true);
    assert!(
        away > toward,
        "facing away scores higher than facing the target"
    );
    assert!(
        matches!(away - toward, 32 | 33),
        "32768 * 0.001 is 32.768, chopped with the base score's fraction ({})",
        away - toward
    );
    assert_eq!(
        delta(
            "[E1]\nExtraThreatCoefficient.Facing=0.001\n",
            "E1",
            "HTNK",
            (12, 10)
        ),
        Some(0),
        "an Infantry scanner has no fire facing"
    );
}

/// `ApplyLastTargetDistanceBonus` (`Hooks.TargetEvaluation.cpp:490-503`): the
/// coordinate skips the bonus while it is `CoordStruct::Empty`, and otherwise
/// scales with the candidate's distance from it.
#[test]
fn extra_threat_distance_to_last_target_ignores_empty_and_scales() {
    let patch = "[MTNK]\nExtraThreatCoefficient.DistanceToLastTarget=1\n";
    let Some((retail, mut sim, scanner, candidate)) = scene(patch, "MTNK", "HTNK", (12, 10)) else {
        return;
    };
    let (base_retail, base_sim, base_scanner, base_candidate) =
        scene("", "MTNK", "HTNK", (12, 10)).unwrap();
    let base = score_of(&base_retail, &base_sim, base_scanner, base_candidate, false);
    assert_eq!(
        score_of(&retail, &sim, scanner, candidate, true),
        base,
        "(0, 0, 0) skips the bonus"
    );
    let target = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(candidate).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    sim.substrate
        .entities
        .get_mut(scanner)
        .unwrap()
        .set_phobos_extra_threat_last_target_crd([target.x + 256, target.y, target.z]);
    assert_eq!(
        score_of(&retail, &sim, scanner, candidate, true) - base,
        1,
        "one cell from the last target is 1.0"
    );
    sim.substrate
        .entities
        .get_mut(scanner)
        .unwrap()
        .set_phobos_extra_threat_last_target_crd([target.x + 512, target.y, target.z]);
    assert_eq!(
        score_of(&retail, &sim, scanner, candidate, true) - base,
        2,
        "two cells from the last target is 2.0"
    );
}

/// The five type keys fall back per key to `[General]`
/// (`Ext/Rules/Body.cpp:535-539`), an explicit type value overrides it, and
/// all-zero values leave the score unchanged (`ExtraThreat_Enabled`,
/// `Ext/TechnoType/Body.cpp:1187-1191`).
#[test]
fn extra_threat_global_fallbacks_and_zero_values() {
    if crate::rules::retail_ini_fixture::retail_battle_rules().is_none() {
        return;
    }
    assert_eq!(
        delta(
            "[General]\nExtraThreat.IsThreat=5\n",
            "E1",
            "MTNK",
            (12, 10)
        ),
        Some(5),
        "a type with no key uses the General value"
    );
    assert_eq!(
        delta(
            "[General]\nExtraThreat.IsThreat=5\n[E1]\nExtraThreat.IsThreat=2\n",
            "E1",
            "MTNK",
            (12, 10)
        ),
        Some(2),
        "the type key overrides the General value"
    );
    assert_eq!(
        delta(
            "[General]\nExtraThreat.IsThreat=5\n[E1]\nExtraThreat.IsThreat=0\n",
            "E1",
            "MTNK",
            (12, 10)
        ),
        Some(0),
        "an explicit 0.0 disables the global fallback"
    );
    assert_eq!(
        delta("", "E1", "MTNK", (12, 10)),
        Some(0),
        "all-zero values keep the score"
    );
}

/// `TechnoExt::UpdateLastTargetCrd` (`Ext/Techno/Body.Update.cpp:1286-1309`):
/// with a target the retained coordinate follows it and the timer stops;
/// without one the timer runs 45 frames and then empties the coordinate.
/// Types whose keys all resolve to zero never run the body.
#[test]
fn the_last_target_crd_follows_the_target_and_clears_after_45_frames() {
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n1=SCAN\n\
         [MTNK]\nStrength=100\nSpeed=4\nSight=8\n\
         [SCAN]\nStrength=100\nSpeed=4\nSight=8\nExtraThreat.IsThreat=1\n",
    );
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::empty())
        .expect("extra threat rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(0);
    let scanner = sim
        .spawn_object("SCAN", "Americans", 10, 10, 0, &rules)
        .expect("scanner");
    let target = sim
        .spawn_object("MTNK", "Russians", 12, 10, 0, &rules)
        .expect("target");
    sim.substrate
        .entities
        .get_mut(scanner)
        .expect("scanner")
        .attack_target
        .set_for_test(Some(TargetKind::Entity(target)));
    sim.update_phobos_extra_threat_last_target(scanner, &rules);
    let retained = sim
        .substrate
        .entities
        .get(scanner)
        .expect("scanner")
        .phobos_extra_threat_last_target_crd();
    assert_ne!(retained, [0, 0, 0], "the target coordinate is stored");

    // Clearing the target starts the 45-frame timer and keeps the value.
    sim.substrate
        .entities
        .get_mut(scanner)
        .expect("scanner")
        .attack_target
        .set_for_test(None);
    sim.update_phobos_extra_threat_last_target(scanner, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(scanner)
            .expect("scanner")
            .phobos_extra_threat_last_target_crd(),
        retained
    );
    assert!(
        sim.substrate
            .entities
            .get(scanner)
            .expect("scanner")
            .phobos_extra_threat_last_target_clear_timer()
            .in_progress()
    );

    // The timer's expiry empties the coordinate and stops.
    sim.session.set_binary_frame_for_test(45);
    sim.update_phobos_extra_threat_last_target(scanner, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(scanner)
            .expect("scanner")
            .phobos_extra_threat_last_target_crd(),
        [0, 0, 0]
    );
    assert!(
        !sim.substrate
            .entities
            .get(scanner)
            .expect("scanner")
            .phobos_extra_threat_last_target_clear_timer()
            .in_progress()
    );
}
