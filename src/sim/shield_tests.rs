//! Phobos shield damage absorption and recovery regressions.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER};
use crate::sim::game_entity::GameEntity;
use crate::sim::world::{FrameEffects, ObjectAiCtx, Simulation};

const VERSES: &str = "Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%";

/// A vehicle whose `TEST` type binds `ShieldType=TestShield`.
fn rules(shield: &str, warhead: &str) -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "[VehicleTypes]\n0=TEST\n\
         [Warheads]\n0=WH\n\
         [TEST]\nStrength=200\nArmor=heavy\nShieldType=TestShield\n\
         [ShieldTypes]\n0=TestShield\n\
         [TestShield]\nStrength=100\nInitialStrength=100\nArmor=none\n{shield}\n\
         [WH]\nCellSpread=0\nPercentAtMax=1\n{VERSES}\n{warhead}\n"
    ));
    RuleSet::from_ini(&ini).expect("shield fixture rules")
}

fn scene(rules: &RuleSet) -> (Simulation, u64) {
    let mut sim = Simulation::with_seed(1);
    let owner = sim.interner.intern("Neutral");
    let type_ref = sim.interner.intern("TEST");
    let mut entity = GameEntity::test_default(1, "TEST", "Neutral", 10, 10);
    entity.set_owner_for_test(owner);
    entity.set_type_ref_for_test(type_ref);
    entity.health.set_for_test(200);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_cell_marked_for_test(true);
    let shield = rules.shield_type("TestShield").expect("fixture type");
    entity.shield = Some(ShieldState::new(
        "TestShield".to_owned(),
        shield.initial_strength(),
        shield.strength(),
    ));
    sim.substrate.entities.insert(entity);
    (sim, 1)
}

fn hit(sim: &mut Simulation, rules: &RuleSet, damage: i32) {
    let warhead = sim.interner.intern("WH");
    sim.commit_noncombat_aoe_hits(
        rules,
        None,
        &[EntityDamageEvent::area(
            1,
            damage,
            0,
            RAD_NO_ATTACKER,
            None,
            warhead,
        )],
        FrameEffects::default(),
    );
}

fn shield(sim: &Simulation) -> &ShieldState {
    sim.substrate
        .entities
        .get(1)
        .unwrap()
        .shield
        .as_ref()
        .unwrap()
}

fn health(sim: &Simulation) -> i32 {
    sim.substrate.entities.get(1).unwrap().health.current()
}

fn arm_chrono_warp(sim: &mut Simulation) {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::{locomotor::LocomotorState, teleport_movement::ChronoWarp};
    let frame = sim.session.binary_frame();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    let destination = crate::sim::movement::ground_pose::object_get_coords(entity, None);
    let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Teleport);
    locomotor
        .teleport_runtime_mut()
        .unwrap()
        .arm_chrono(ChronoWarp::new(destination, frame));
    entity.locomotor = Some(locomotor);
}

#[test]
fn absorbed_damage_leaves_health_untouched() {
    let rules = rules("AbsorbPercent=1.0", "");
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 40);

    assert_eq!(health(&sim), 200, "the shield took the whole hit");
    assert_eq!(shield(&sim).hp(), 60);
}

#[test]
fn overkill_breaks_the_shield_and_passes_the_residue() {
    let rules = rules("", "");
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 150);

    assert_eq!(shield(&sim).hp(), 0, "the shield broke");
    assert_eq!(
        health(&sim),
        150,
        "the 50 beyond the shield's 100 points reaches the hull"
    );
}

#[test]
fn absorb_over_damage_drops_the_residue() {
    let rules = rules("AbsorbOverDamage=yes", "");
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 150);

    assert_eq!(shield(&sim).hp(), 0);
    assert_eq!(health(&sim), 200, "over-damage is absorbed, not passed");
}

#[test]
fn pass_percent_reaches_health_while_the_shield_holds() {
    let rules = rules("PassPercent=0.25", "");
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 40);

    assert_eq!(shield(&sim).hp(), 60);
    assert_eq!(health(&sim), 190, "25% of the raw hit passes through");
}

/// Phobos `ShieldClass::ReceiveDamage` applies absorption after either sign
/// of `MapClass::GetTotalDamage` (hook `0x701900+0x6`).
#[test]
fn healing_uses_the_shield_absorption_and_warhead_override() {
    let rules = rules("AbsorbPercent=0.25", "");
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 160);
    assert_eq!(shield(&sim).hp(), 60);
    hit(&mut sim, &rules, -40);
    assert_eq!(
        shield(&sim).hp(),
        70,
        "the shield absorbs a quarter of the heal"
    );

    let overridden = self::rules("AbsorbPercent=0.25", "Shield.AbsorbPercent=0.5");
    hit(&mut sim, &overridden, -40);
    assert_eq!(
        shield(&sim).hp(),
        90,
        "the warhead overrides absorption for healing too"
    );
    assert_eq!(health(&sim), 200);
}

#[test]
fn a_negative_shield_return_keeps_the_original_healing_packet() {
    let rules = rules(
        "AbsorbPercent=1.0\nPassPercent=0",
        "Shield.AbsorbPercent=0\nShield.PassPercent=0.5",
    );
    let (mut sim, _) = scene(&rules);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .health
        .set_for_test(100);
    hit(&mut sim, &rules, -40);
    assert_eq!(
        shield(&sim).hp(),
        100,
        "zero absorption leaves the shield full"
    );
    assert_eq!(
        health(&sim),
        140,
        // Phobos b90ab4bc Hooks.ReceiveDamage.cpp:155..163: a -20 shield
        // return does not replace the -40 caller packet that heals the hull.
        "a negative shield return leaves the caller's healing packet intact"
    );
}

#[test]
fn penetrating_warhead_bypasses_the_shield() {
    let rules = rules("", "Shield.Penetrate=yes");
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 40);

    assert_eq!(shield(&sim).hp(), 100, "the shield is untouched");
    assert_eq!(health(&sim), 160, "the whole hit reaches the hull");
}

#[test]
fn self_healing_restores_points_on_its_rate() {
    let rules = rules("SelfHealing=0.25\nSelfHealing.Rate=0.2", "");
    let (mut sim, _) = scene(&rules);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .shield
        .as_mut()
        .unwrap()
        .hp = 40;
    sim.session.set_binary_frame_for_test(0);

    super::tick(&mut sim, 1, &rules);
    assert_eq!(shield(&sim).hp(), 40, "the first visit starts the timer");
    sim.session.set_binary_frame_for_test(180);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(shield(&sim).hp(), 65, "0.2 minutes = 180 frames adds 25");
}

#[test]
fn broken_shield_respawns_after_its_rate() {
    let rules = rules("Respawn=0.5\nRespawn.Rate=0.1", "");
    let (mut sim, _) = scene(&rules);
    sim.session.set_binary_frame_for_test(0);
    hit(&mut sim, &rules, 150);
    assert_eq!(shield(&sim).hp(), 0);

    super::tick(&mut sim, 1, &rules);
    assert_eq!(shield(&sim).hp(), 0, "the respawn timer is still running");
    sim.session.set_binary_frame_for_test(90);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(
        shield(&sim).hp(),
        50,
        "0.1 minutes = 90 frames restores half"
    );
}

/// TemporalClass::Update's Phobos hook calls `ShieldClass::AI_Temporal`;
/// the frozen leaf then skips Techno AI (`UnitClass::AI @ 0x736204`).
#[test]
fn temporal_freeze_preserves_recovery_time_until_the_object_resumes() {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=TEST\n1=WARPER\n\
         [Warheads]\n0=WH\n\
         [TEST]\nStrength=200\nArmor=heavy\nShieldType=TestShield\n\
         [WARPER]\nStrength=100\nPrimary=Beam\n\
         [Beam]\nDamage=1\nWarhead=WH\n\
         [WH]\nTemporal=yes\n{VERSES}\n\
         [ShieldTypes]\n0=TestShield\n\
         [TestShield]\nStrength=100\nSelfHealing=0.25\nSelfHealing.Rate=0.1\nRespawn=0.5\nRespawn.Rate=0.1\n"
    ))).unwrap();
    let mut sim = Simulation::with_seed(1);
    let target = sim
        .spawn_object("TEST", "Neutral", 10, 10, 0, &rules)
        .unwrap();
    let warper = sim
        .spawn_object("WARPER", "Neutral", 11, 10, 0, &rules)
        .unwrap();
    let points = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(target)
            .unwrap()
            .shield
            .as_ref()
            .unwrap()
            .hp()
    };
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .shield
        .as_mut()
        .unwrap()
        .hp = 40;
    super::tick(&mut sim, target, &rules);
    sim.session.set_binary_frame_for_test(30);
    sim.temporal_initiate_warp(warper, Some(target), &rules, None, FrameEffects::default());
    assert!(sim.substrate.entities.get(target).unwrap().ai_frozen());
    sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default());
    sim.session.set_binary_frame_for_test(300);
    sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default());
    assert_eq!(
        points(&sim),
        40,
        "recovery stayed paused throughout the warp"
    );
    Simulation::temporal_let_go(
        &mut sim.substrate,
        &mut sim.houses,
        &mut sim.power_states,
        warper,
    );
    sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default());
    sim.session.set_binary_frame_for_test(359);
    super::tick(&mut sim, target, &rules);
    assert_eq!(points(&sim), 40, "the saved recovery time has not elapsed");
    sim.session.set_binary_frame_for_test(360);
    super::tick(&mut sim, target, &rules);
    assert_eq!(points(&sim), 65, "recovery resumes at the saved deadline");
}

#[test]
fn chronosphere_launch_latch_suppresses_due_recovery_before_warp_out() {
    let rules = rules("SelfHealing=0.25\nSelfHealing.Rate=0.1", "");
    let (mut sim, _) = scene(&rules);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .shield
        .as_mut()
        .unwrap()
        .hp = 40;
    super::tick(&mut sim, 1, &rules);
    sim.session.set_binary_frame_for_test(90);
    arm_chrono_warp(&mut sim);
    assert!(sim.substrate.entities.get(1).unwrap().chrono_warp_latch());
    assert!(!sim.substrate.entities.get(1).unwrap().is_warped_out());
    super::tick(&mut sim, 1, &rules);
    assert_eq!(shield(&sim).hp(), 40, "IsImmobilized gates the due heal");
}

/// Phobos calls AI_Temporal only from TemporalClass, not Chronosphere.
/// The latter skips AI at +0x27C without pausing its global-frame timers.
#[test]
fn chronosphere_skips_recovery_but_keeps_the_original_countdown() {
    let rules = rules("SelfHealing=0.25\nSelfHealing.Rate=0.1", "");
    let (mut sim, _) = scene(&rules);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .shield
        .as_mut()
        .unwrap()
        .hp = 40;
    super::tick(&mut sim, 1, &rules);
    sim.session.set_binary_frame_for_test(30);
    arm_chrono_warp(&mut sim);
    sim.process_chrono_warp(1, &rules, ObjectAiCtx::default())
        .unwrap();
    assert!(sim.substrate.entities.get(1).unwrap().is_warped_out());
    super::tick(&mut sim, 1, &rules);
    sim.session.set_binary_frame_for_test(90);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(shield(&sim).hp(), 40, "a frozen leaf cannot heal yet");
    // State 2 (0x7198DA..0x7198EE) clears the latch and warp-out byte,
    // keeping WarpingIn set: normal Shield AI may now run.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .teleport_runtime_mut()
        .unwrap()
        .chrono_mut()
        .unwrap()
        .set_bytes(false, false, true);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(shield(&sim).hp(), 65, "the pre-warp deadline remains due");
}

#[test]
fn construction_binds_the_shield_type() {
    let rules = rules("", "");
    let mut sim = Simulation::with_seed(1);
    let id = sim
        .spawn_object("TEST", "Neutral", 10, 10, 0, &rules)
        .expect("the fixture unit spawns");
    let shield = sim
        .entities()
        .get(id)
        .and_then(|entity| entity.shield.as_ref())
        .expect("ShieldType= allocated the shield");
    assert_eq!(shield.hp(), 100);
}

#[test]
fn an_offline_shield_absorbs_nothing() {
    let rules = rules("", "");
    let (mut sim, _) = scene(&rules);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .shield
        .as_mut()
        .unwrap()
        .online = false;
    hit(&mut sim, &rules, 40);
    assert_eq!(shield(&sim).hp(), 100);
    assert_eq!(health(&sim), 160);
}

#[test]
fn ignore_defenses_skips_the_shield_stage() {
    let rules = rules("", "");
    let (mut sim, _) = scene(&rules);
    let warhead_ref = sim.interner.intern("WH");
    let mut event = EntityDamageEvent::area(1, 40, 0, RAD_NO_ATTACKER, None, warhead_ref);
    event.receiver_flags.ignore_defenses = true;
    let target = sim.substrate.entities.get(1).unwrap();
    // The Infantry head already ran before this stage; a non-Infantry target
    // leaves the caller's damage untouched.
    let input = event.damage;
    let target_type = sim.interner.object_type(target.type_ref(), &rules);
    let divisors = crate::sim::combat::receiver_divisors(
        target,
        target_type,
        &rules,
        &sim.interner,
        &sim.houses,
    );
    assert!(
        pre_receive_damage(&mut sim, &rules, &event, input, &divisors).is_none(),
        "the installed hook sits inside `if (!IgnoreDefenses)`"
    );
    assert_eq!(shield(&sim).hp(), 100);
}

#[test]
fn control_hit_reaches_health_without_a_shield() {
    let rules = rules("", "");
    let (mut sim, _) = scene(&rules);
    sim.substrate.entities.get_mut(1).unwrap().shield = None;
    hit(&mut sim, &rules, 40);
    assert_eq!(health(&sim), 160);
}

#[test]
fn pre_stage_absorbs_before_the_receiver() {
    let rules = rules("", "");
    let (mut sim, _) = scene(&rules);
    let warhead_ref = sim.interner.intern("WH");
    let event = EntityDamageEvent::area(1, 40, 0, RAD_NO_ATTACKER, None, warhead_ref);
    let target = sim.substrate.entities.get(1).unwrap();
    let input = event.damage;
    let target_type = sim.interner.object_type(target.type_ref(), &rules);
    let divisors = crate::sim::combat::receiver_divisors(
        target,
        target_type,
        &rules,
        &sim.interner,
        &sim.houses,
    );
    let receipt = pre_receive_damage(&mut sim, &rules, &event, input, &divisors)
        .expect("a shield owns the hit");
    assert_eq!(receipt.remaining, 0);
    assert_eq!(shield(&sim).hp(), 60);
}

#[test]
fn creation_clamps_initial_strength_to_the_type_strength() {
    let state = ShieldState::new("TestShield".to_owned(), 500, 100);
    assert_eq!(state.hp(), 100);
}

/// `Math::clamp` answers its minimum when `minimum > maximum`; `Ord::clamp`
/// would panic instead.
#[test]
fn a_reversed_received_damage_range_clamps_to_the_native_minimum() {
    let rules = rules("ReceivedDamage.Minimum=50\nReceivedDamage.Maximum=10", "");
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 40);

    assert_eq!(shield(&sim).hp(), 50);
    assert_eq!(health(&sim), 200);
}

/// A Temporal warhead initializes both calculated damage terms to zero;
/// the default received-damage bounds leave the shield and hull untouched.
#[test]
fn a_temporal_warhead_leaves_the_shield_and_health_untouched() {
    let rules = rules("", "Temporal=yes");
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 40);

    assert_eq!(shield(&sim).hp(), 100);
    assert_eq!(health(&sim), 200);
}

/// `WarheadTypeExt::CanTargetHouse`'s owner/ally/enemy gates.
#[test]
fn a_warhead_that_cannot_target_the_owners_house_skips_absorption() {
    let rules = rules("", "AffectsOwner=no\nAffectsAllies=no\nAffectsEnemies=yes");
    let (mut sim, _) = scene(&rules);
    let owner = sim.interner.intern("Neutral");
    let enemy = sim.interner.intern("Other");
    let warhead = rules.warhead("WH").unwrap();

    assert!(!can_target_house(warhead, Some(owner), owner, &sim));
    assert!(can_target_house(warhead, Some(enemy), owner, &sim));
}

/// The wave lookahead reads the surviving damage without consuming the
/// shield; the commit path then absorbs the hit exactly once.
#[test]
fn the_wave_preview_reports_the_surviving_damage_without_consuming_the_shield() {
    let rules = rules("", "");
    let (mut sim, _) = scene(&rules);
    let warhead_ref = sim.interner.intern("WH");
    let event = EntityDamageEvent::area(1, 40, 0, RAD_NO_ATTACKER, None, warhead_ref);

    assert_eq!(preview_receive_damage(&sim, &rules, &event, 40), Some(0));
    assert_eq!(shield(&sim).hp(), 100, "the preview wrote no state");

    let target = sim.substrate.entities.get(1).unwrap();
    let target_type = sim.interner.object_type(target.type_ref(), &rules);
    let divisors = crate::sim::combat::receiver_divisors(
        target,
        target_type,
        &rules,
        &sim.interner,
        &sim.houses,
    );
    let receipt =
        pre_receive_damage(&mut sim, &rules, &event, 40, &divisors).expect("a shield owns the hit");
    assert_eq!(receipt.remaining, 0);
    assert_eq!(shield(&sim).hp(), 60);
}

/// `IsBrokenAndNonRespawning` gates the shield bar: a broken shield with no
/// `Respawn=` draw is suppressed, a respawning one keeps its empty bar.
#[test]
fn a_broken_shield_draws_only_while_it_can_respawn() {
    let fixed = rules("Respawn=0", "");
    let ty = fixed.shield_type("TestShield").unwrap();
    let mut state = ShieldState::new("TestShield".to_owned(), 100, 100);
    state.hp = 0;
    assert!(state.broken_non_respawning(ty));

    let respawning = rules("Respawn=0.5", "");
    let ty = respawning.shield_type("TestShield").unwrap();
    assert!(!state.broken_non_respawning(ty));
}

/// ShieldClass::CanBePenetrated checks the Psychedelic arm before the
/// generic Shield.Penetrate flag (Phobos b90ab4bc, receive hook 0x701900).
#[test]
fn a_berserk_immune_shield_absorbs_a_normally_penetrating_psychedelic_hit() {
    let rules = rules(
        "ImmuneToBerserk=yes",
        "Psychedelic=yes\nShield.Penetrate=yes",
    );
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 40);
    assert_eq!(shield(&sim).hp(), 60, "the shield intercepts the packet");
    assert_eq!(health(&sim), 200);
}

/// Phobos's active-shield receive arm restarts both recovery timers before
/// the separate non-Psychedelic fallback (Hooks.ReceiveDamage.cpp:153..177).
#[test]
fn absorbed_psychedelic_damage_restarts_both_combat_delays() {
    let rules = rules(
        "ImmuneToBerserk=yes\nSelfHealing=0.25\nSelfHealing.Rate=0.1\n\
         SelfHealing.RestartInCombatDelay=10\nRespawn.RestartInCombatDelay=7",
        "Psychedelic=yes\nShield.Penetrate=yes",
    );
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 40);
    sim.session.set_binary_frame_for_test(30);
    hit(&mut sim, &rules, 20);
    assert_eq!(shield(&sim).hp(), 40);
    assert_eq!(
        shield(&sim)
            .timers
            .self_healing_combat_restart
            .timer
            .remaining(30),
        10,
    );
    assert_eq!(
        shield(&sim)
            .timers
            .respawn_combat_restart
            .timer
            .remaining(30),
        7,
    );
    sim.session.set_binary_frame_for_test(40);
    super::tick(&mut sim, 1, &rules);
    sim.session.set_binary_frame_for_test(129);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(
        shield(&sim).hp(),
        40,
        "the restarted healing rate is not due"
    );
    sim.session.set_binary_frame_for_test(130);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(shield(&sim).hp(), 65);
}

/// House/Temporal gates zero the calculated packet, but the shared bounds
/// still apply (Phobos ShieldClass.cpp:204..244, receive hook 0x701900).
#[test]
fn a_temporal_warhead_still_applies_the_minimum_shield_damage() {
    let rules = rules("ReceivedDamage.Minimum=10", "Temporal=yes");
    let (mut sim, _) = scene(&rules);
    hit(&mut sim, &rules, 40);
    assert_eq!(shield(&sim).hp(), 90);
    assert_eq!(health(&sim), 200);
}
