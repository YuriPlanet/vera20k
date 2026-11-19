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
        target.phobos_armor_multiplier(),
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
        target.phobos_armor_multiplier(),
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
        target.phobos_armor_multiplier(),
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
    assert!(state.broken_non_respawning(ty, 0));

    let respawning = rules("Respawn=0.5", "");
    let ty = respawning.shield_type("TestShield").unwrap();
    assert!(!state.broken_non_respawning(ty, 0));
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

/// Phobos `ShieldClass::ReceiveDamage` (`ShieldClass.cpp:249..255`) uncloaks
/// a target whose shield absorbed a positive hit, gated by the warhead's
/// `DecloakDamagedTargets`. Only `ares,phobos` isolates the shield call: there
/// Ares skips the native-slot uncloak for the unaffected hull (`1004AB68`),
/// while plain `phobos` also uncloaks in the native slot.
#[cfg(feature = "phobos")]
#[test]
fn phobos_shield_damage_decloaks_only_when_the_warhead_allows_it() {
    fn hit_cloaked(rules: &RuleSet) -> i32 {
        let (mut sim, _) = scene(rules);
        let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0);
        cloak.establish_unlimbo_fully_cloaked();
        sim.substrate.entities.get_mut(1).unwrap().cloak = Some(cloak);
        hit(&mut sim, rules, 40);
        assert_eq!(shield(&sim).hp(), 60, "the shield owns the hit");
        assert_eq!(health(&sim), 200, "no hull damage reaches the receiver");
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state()
    }

    assert_eq!(
        hit_cloaked(&rules("", "")),
        3,
        "an admitting warhead starts uncloaking"
    );
    assert_eq!(
        hit_cloaked(&rules("", "DecloakDamagedTargets=no")),
        2,
        "the per-warhead no keeps the shield owner hidden"
    );
}

// --- Warhead `Shield.*` (`WarheadTypeExt::ApplyShieldModifiers`) through the
// --- production detonation path (`sim::combat::crit::detonate`). ---

/// FIRER and TARGET on rival houses; TARGET binds `TestShield`. `warheads`
/// extends the `[Warheads]` registry, `wh_ini` the `[WH]` body, `sections`
/// further type bodies. `[Warheads]` rows end in the caller's `\n`.
fn detonation_scene(
    warheads: &str,
    wh_ini: &str,
    sections: &str,
) -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=TARGET\n2=NEIGHBOR\n\
         [WeaponTypes]\n0=GUN\n1=BRKWPN\n\
         [GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
         [BRKWPN]\nDamage=5\nRange=5\nROF=20\nWarhead=WHBRK\n\
         [Warheads]\n0=WH\n{warheads}\
         [WH]\nCellSpread=0\n{VERSES}\n{wh_ini}\n\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n\
         [TARGET]\nStrength=500\nArmor=none\nSpeed=6\nShieldType=TestShield\n\
         [NEIGHBOR]\nStrength=500\nArmor=none\nSpeed=6\n\
         [ShieldTypes]\n0=TestShield\n1=SecondShield\n2=ThirdShield\n3=OtherShield\n\
         [TestShield]\nStrength=100\nInitialStrength=100\nArmor=none\n\
         [SecondShield]\nStrength=100\nInitialStrength=100\nArmor=none\n\
         [ThirdShield]\nStrength=100\nInitialStrength=100\nArmor=none\n\
         [OtherShield]\nStrength=50\nInitialStrength=50\nArmor=none\n\
         [Animations]\n0=BRKANIM\n1=HITANIM\n\
         {sections}"
    )))
    .expect("detonation fixture rules");
    // The break/hit animation types spawn only with resolvable art.
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[BRKANIM]\nRate=1\n[HITANIM]\nRate=1\n",
    ));
    art.bind_anim_frame_count_for_test("BRKANIM", 20);
    art.bind_anim_frame_count_for_test("HITANIM", 20);
    let mut rules = rules;
    rules.replace_art_registry_for_test(art);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", "Russians", 22, 20, 0, &rules)
        .expect("target");
    (sim, rules, firer, target)
}

/// One `WarheadTypeExt::Detonate` of `warhead` on `target` with the bullet's
/// `damage`, through `commit_projectiles`.
fn shoot(
    sim: &mut Simulation,
    rules: &RuleSet,
    warhead: &str,
    firer: u64,
    target: u64,
    damage: i32,
) -> crate::sim::combat::LogicProjectileCommit {
    let coords = {
        let target = sim.substrate.entities.get(target).expect("target");
        crate::sim::movement::ground_pose::object_get_coords(target, sim.resolved_terrain.as_ref())
    };
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: target,
        source_id: firer,
        target: crate::sim::projectile::ProjectileTarget::Entity(target),
        impact: crate::sim::projectile::ProjectileCoord::new(coords.x, coords.y, coords.z),
        payload: crate::sim::projectile::ProjectilePayload::new(
            damage,
            sim.interner.intern(warhead),
            None,
            false,
        ),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
    let commit = crate::sim::combat::world_receiver::commit_projectiles(
        sim,
        &mut run,
        &[detonation],
        rules,
        None,
    );
    run.finish();
    commit
}

fn target_shield(sim: &Simulation, target: u64) -> &ShieldState {
    sim.substrate
        .entities
        .get(target)
        .expect("target")
        .shield
        .as_ref()
        .expect("shield")
}

fn set_target_shield_hp(sim: &mut Simulation, target: u64, hp: i32) {
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .shield
        .as_mut()
        .unwrap()
        .hp = hp;
}

fn target_health(sim: &Simulation, target: u64) -> i32 {
    sim.substrate.entities.get(target).unwrap().health.current()
}

fn anim_names(sim: &Simulation) -> Vec<String> {
    sim.anims()
        .map(|(_, anim)| sim.interner.resolve(anim.type_id()).to_owned())
        .collect()
}

/// `ApplyShieldModifiers` removes through `Shield.RemoveTypes` and attaches
/// the first `Shield.AttachTypes` entry at full strength
/// (`Detonate.cpp:375..430`).
#[test]
fn warhead_remove_then_attach_reconfigures_the_shield() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "",
        "Shield.RemoveTypes=TestShield\nShield.AttachTypes=SecondShield",
        "",
    );
    set_target_shield_hp(&mut sim, target, 40);
    shoot(&mut sim, &rules, "WH", firer, target, 0);

    let state = target_shield(&sim, target);
    assert_eq!(state.type_name(), "SecondShield");
    assert_eq!(
        state.hp(),
        100,
        "the attached shield starts at InitialStrength"
    );
}

/// `Shield.RemoveTypes` without an attach leaves no shield at all.
#[test]
fn warhead_remove_types_strips_the_shield() {
    let (mut sim, rules, firer, target) = detonation_scene("", "Shield.RemoveTypes=TestShield", "");
    shoot(&mut sim, &rules, "WH", firer, target, 0);

    assert!(
        sim.substrate.entities.get(target).unwrap().shield.is_none(),
        "the removed shield is gone"
    );
    assert_eq!(target_health(&sim, target), 500, "no damage rides the pass");
}

/// `ReplaceOnly` without a removal match attaches nothing
/// (`Detonate.cpp:395..401`).
#[test]
fn replace_only_without_a_remove_match_keeps_the_shield() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "",
        "Shield.ReplaceOnly=yes\nShield.AttachTypes=SecondShield",
        "",
    );
    set_target_shield_hp(&mut sim, target, 40);
    shoot(&mut sim, &rules, "WH", firer, target, 0);

    let state = target_shield(&sim, target);
    assert_eq!(state.type_name(), "TestShield");
    assert_eq!(state.hp(), 40);
}

/// `ReplaceOnly` attaches `AttachTypes[min(removed index, size - 1)]`
/// (`Detonate.cpp:397..398`).
#[test]
fn replace_only_maps_the_removed_index_into_the_attach_list() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "",
        "Shield.ReplaceOnly=yes\n\
         Shield.RemoveTypes=ThirdShield,TestShield\n\
         Shield.AttachTypes=SecondShield,ThirdShield",
        "",
    );
    shoot(&mut sim, &rules, "WH", firer, target, 0);

    let state = target_shield(&sim, target);
    assert_eq!(
        state.type_name(),
        "ThirdShield",
        "TestShield sits at RemoveTypes index 1, so AttachTypes[1] attaches"
    );
}

/// `InheritStateOnReplace` sets the new shield to
/// `(int)(strength * removed ratio)` (`Detonate.cpp:419..427`).
#[test]
fn inherit_state_on_replace_copies_the_health_ratio() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "",
        "Shield.RemoveTypes=TestShield\n\
         Shield.AttachTypes=SecondShield\n\
         Shield.ReplaceOnly=yes\n\
         Shield.InheritStateOnReplace=yes",
        "",
    );
    set_target_shield_hp(&mut sim, target, 40);
    shoot(&mut sim, &rules, "WH", firer, target, 0);

    let state = target_shield(&sim, target);
    assert_eq!(state.type_name(), "SecondShield");
    assert_eq!(state.hp(), 40, "0.4 * SecondShield's 100 strength");
}

/// `ReplaceNonRespawning` replaces a broken non-respawning shield only once
/// it has been down for `MinimumReplaceDelay` frames
/// (`Detonate.cpp:409..413`).
#[test]
fn replace_non_respawning_honors_the_minimum_replace_delay() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "1=PLAIN\n",
        "Shield.AttachTypes=SecondShield\n\
         Shield.ReplaceNonRespawning=yes\n\
         Shield.MinimumReplaceDelay=10",
        "[PLAIN]\nCellSpread=0\n",
    );
    // Break TestShield at frame 0: it has no Respawn, so it stays broken.
    shoot(&mut sim, &rules, "PLAIN", firer, target, 150);
    assert_eq!(target_shield(&sim, target).hp(), 0);

    sim.session.set_binary_frame_for_test(5);
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    assert_eq!(
        target_shield(&sim, target).type_name(),
        "TestShield",
        "5 frames since the break is below MinimumReplaceDelay"
    );

    sim.session.set_binary_frame_for_test(10);
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    let state = target_shield(&sim, target);
    assert_eq!(state.type_name(), "SecondShield");
    assert_eq!(state.hp(), 100);
}

/// `Shield.Break` breaks the eligible active shield and emits the warhead's
/// break animation and break weapon on the owner
/// (`Detonate.cpp:442..443`, `ShieldClass.cpp:836..860`).
#[test]
fn warhead_break_emits_the_break_anim_and_weapon() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "1=WHBRK\n",
        "Shield.Break=yes\nShield.BreakAnim=BRKANIM\nShield.BreakWeapon=BRKWPN",
        "[WHBRK]\nCellSpread=1.5\n{VERSES}\n\
         [BRKWPN]\nDamage=5\nRange=5\nROF=20\nWarhead=WHBRK\n",
    );
    let neighbor = sim
        .spawn_object("NEIGHBOR", "Russians", 23, 20, 0, &rules)
        .expect("neighbor");
    shoot(&mut sim, &rules, "WH", firer, target, 0);

    assert_eq!(target_shield(&sim, target).hp(), 0, "the shield broke");
    assert!(
        anim_names(&sim)
            .iter()
            .any(|name| name.eq_ignore_ascii_case("BRKANIM")),
        "the break animation spawned on the shield owner"
    );
    // `FireWeaponAtSelf` -> `DetonateAt(weapon, pThis, pThis)`: the break
    // weapon's shot detonates on the shield owner. `DamageArea`'s self
    // exclusion (`Explosion_Damage_DamageSelf @ 0x004896EC`) keeps the owner
    // out of its own weapon's ground list, so the damage assert runs on a
    // neighbor inside the break warhead's cell spread.
    assert_eq!(
        sim.substrate
            .entities
            .get(neighbor)
            .unwrap()
            .health
            .current(),
        495,
        "the break weapon's shot detonated"
    );
    assert_eq!(
        sim.substrate.entities.get(target).unwrap().health.current(),
        500,
        "the owner is excluded from its own weapon's area damage"
    );
}

/// `Shield.Break.Types` narrows the break to its parsed types
/// (`Detonate.cpp:437..443`).
#[test]
fn break_types_gate_the_warhead_break() {
    let outside = detonation_scene("", "Shield.Break=yes\nShield.Break.Types=OtherShield", "");
    let (mut sim, rules, firer, target) = outside;
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    assert_eq!(
        target_shield(&sim, target).hp(),
        100,
        "TestShield is outside Shield.Break.Types"
    );

    let (mut sim, rules, firer, target) =
        detonation_scene("", "Shield.Break=yes\nShield.Break.Types=TestShield", "");
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    assert_eq!(target_shield(&sim, target).hp(), 0);
}

/// `Shield.Respawn.*` installs the warhead modifier (`SetRespawn`,
/// `ShieldClass.cpp:930..963`): rate is minutes * 900, and the AI respawns
/// with the warhead's amount while the modifier timer runs.
#[test]
fn warhead_respawn_modifier_respawns_at_the_warhead_rate() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "1=PLAIN\n",
        "Shield.Respawn.Duration=50\nShield.Respawn.Amount=0.5\nShield.Respawn.Rate=0.05",
        "[PLAIN]\nCellSpread=0\n",
    );
    // Break at frame 0 (TestShield has no type respawn: it stays broken).
    shoot(&mut sim, &rules, "PLAIN", firer, target, 150);
    assert_eq!(target_shield(&sim, target).hp(), 0);

    // The warhead installs 50 frames of 0.5 amount at 0.05 minutes = 45 frames.
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    let state = target_shield(&sim, target);
    assert!(
        state.timers.respawn_wh_modifier.in_progress(0),
        "the modifier timer runs for its duration"
    );
    let modifier = state.respawn_modifier.as_ref().expect("modifier stored");
    assert_eq!(modifier.rate, 45, "0.05 minutes * 900");
    assert_eq!(modifier.amount.to_bits(), 0.5f64.to_bits());

    super::tick(&mut sim, target, &rules);
    assert_eq!(target_shield(&sim, target).hp(), 0, "45 frames out");

    sim.session.set_binary_frame_for_test(45);
    super::tick(&mut sim, target, &rules);
    assert_eq!(
        target_shield(&sim, target).hp(),
        50,
        "the warhead amount respawned the shield"
    );
}

/// `Shield.SelfHealing.*` installs the warhead modifier (`SetSelfHealing`,
/// `ShieldClass.cpp:987..1017`); the AI heals the warhead amount on the
/// warhead rate while the modifier runs.
#[test]
fn warhead_self_healing_modifier_heals_at_the_warhead_rate() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "",
        "Shield.SelfHealing.Duration=50\nShield.SelfHealing.Amount=0.25\nShield.SelfHealing.Rate=0.02",
        "",
    );
    set_target_shield_hp(&mut sim, target, 40);

    shoot(&mut sim, &rules, "WH", firer, target, 0);
    let state = target_shield(&sim, target);
    let modifier = state
        .self_healing_modifier
        .as_ref()
        .expect("modifier stored");
    // Native parses the minutes through float (`Parser<double>::TryParse`),
    // so 0.02f32 * 900 truncates to 17.
    assert_eq!(modifier.rate, 17);
    assert_eq!(modifier.amount.to_bits(), 0.25f64.to_bits());

    super::tick(&mut sim, target, &rules);
    assert_eq!(target_shield(&sim, target).hp(), 40, "17 frames out");

    sim.session.set_binary_frame_for_test(17);
    super::tick(&mut sim, target, &rules);
    assert_eq!(
        target_shield(&sim, target).hp(),
        65,
        "the warhead amount healed the active shield"
    );
}

/// `Shield.AffectTypes` gates the absorb/pass override
/// (`ShieldClass.cpp:230..232`): a type outside the list keeps the shield
/// type's own split; a type inside it takes the warhead's override.
#[test]
fn affect_types_gate_the_absorb_override() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "1=WH2\n",
        "Shield.AbsorbPercent=0\nShield.AffectTypes=OtherShield",
        "[WH2]\nCellSpread=0\nShield.AbsorbPercent=0\nShield.AffectTypes=TestShield\n",
    );
    shoot(&mut sim, &rules, "WH", firer, target, 40);
    assert_eq!(
        target_shield(&sim, target).hp(),
        60,
        "the override does not reach TestShield"
    );
    assert_eq!(target_health(&sim, target), 500);

    let (mut sim, rules, firer, target) = detonation_scene(
        "1=WH2\n",
        "Shield.AbsorbPercent=0\nShield.AffectTypes=OtherShield",
        "[WH2]\nCellSpread=0\nShield.AbsorbPercent=0\nShield.AffectTypes=TestShield\n",
    );
    shoot(&mut sim, &rules, "WH2", firer, target, 40);
    assert_eq!(
        target_shield(&sim, target).hp(),
        100,
        "zero absorption leaves the shield full"
    );
    assert_eq!(
        target_health(&sim, target),
        500,
        "the zero pass percent leaves the hull untouched"
    );
}

/// `Shield.Penetrate.Types` restricts which shield types a penetrating
/// warhead bypasses (`ShieldClass.cpp:379..395`).
#[test]
fn penetrate_types_restrict_which_shields_are_bypassed() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "",
        "Shield.Penetrate=yes\nShield.Penetrate.Types=OtherShield",
        "",
    );
    shoot(&mut sim, &rules, "WH", firer, target, 40);
    assert_eq!(
        target_shield(&sim, target).hp(),
        60,
        "TestShield is outside Shield.Penetrate.Types"
    );
    assert_eq!(target_health(&sim, target), 500);

    let (mut sim, rules, firer, target) = detonation_scene(
        "",
        "Shield.Penetrate=yes\nShield.Penetrate.Types=TestShield",
        "",
    );
    shoot(&mut sim, &rules, "WH", firer, target, 40);
    assert_eq!(
        target_shield(&sim, target).hp(),
        100,
        "the shield is untouched"
    );
    assert_eq!(target_health(&sim, target), 460, "the hit reached the hull");
}

/// `Shield.ReceivedDamage.MinMultiplier` scales the received floor
/// (`ShieldClass.cpp:240..244`).
#[test]
fn received_damage_minimum_multiplier_scales_the_floor() {
    let (mut sim, rules, firer, target) = detonation_scene(
        "",
        "Shield.ReceivedDamage.Minimum=10\nShield.ReceivedDamage.MinMultiplier=2",
        "",
    );
    shoot(&mut sim, &rules, "WH", firer, target, 5);
    assert_eq!(
        target_shield(&sim, target).hp(),
        80,
        "the 5-point hit clamps up to 10 * 2"
    );
    assert_eq!(target_health(&sim, target), 500);
}

/// `WeaponNullifyAnim` draws the warhead's `Shield.HitAnim` on an absorbed
/// hit unless `Shield.SkipHitAnim` (`ShieldClass.cpp:297..298`, `:359..366`).
#[test]
fn the_hit_anim_emits_unless_skip_hit_anim() {
    let (mut sim, rules, firer, target) = detonation_scene("", "Shield.HitAnim=HITANIM", "");
    shoot(&mut sim, &rules, "WH", firer, target, 40);
    assert_eq!(target_shield(&sim, target).hp(), 60, "the shield absorbed");
    assert!(
        anim_names(&sim)
            .iter()
            .any(|name| name.eq_ignore_ascii_case("HITANIM")),
        "the absorbed-hit animation spawned"
    );

    let (mut sim, rules, firer, target) =
        detonation_scene("", "Shield.HitAnim=HITANIM\nShield.SkipHitAnim=yes", "");
    shoot(&mut sim, &rules, "WH", firer, target, 40);
    assert!(
        !anim_names(&sim)
            .iter()
            .any(|name| name.eq_ignore_ascii_case("HITANIM")),
        "Shield.SkipHitAnim suppressed the animation"
    );
}

/// A shield type carrying idle-animation keys, with resolvable art.
///
/// `IdleAnim`/`IdleAnimDamaged` are `Damageable<AnimTypeClass*>` reads
/// (`ShieldTypeClass.cpp:96..97`); the two actions are
/// `Valueable<AttachedAnimFlag>` with the constructor's `Hides`
/// (`:56..57,92..93`).
#[cfg(feature = "phobos")]
fn idle_rules(shield: &str, art_extra: &str) -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "[VehicleTypes]\n0=TEST\n\
         [TEST]\nStrength=200\nArmor=heavy\nShieldType=TestShield\n\
         [ShieldTypes]\n0=TestShield\n\
         [TestShield]\nStrength=100\nInitialStrength=100\nArmor=none\n{shield}\n\
         [Animations]\n0=IDLE\n1=IDLE2\n"
    ));
    let mut rules = RuleSet::from_ini(&ini).expect("idle fixture rules");
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(&format!(
        "[IDLE]\nRate=1\n{art_extra}[IDLE2]\nRate=1\n"
    )));
    art.bind_anim_frame_count_for_test("IDLE", 20);
    art.bind_anim_frame_count_for_test("IDLE2", 20);
    rules.replace_art_registry_for_test(art);
    rules
}

#[cfg(feature = "phobos")]
fn idle_anims<'s>(sim: &'s Simulation, name: &str) -> Vec<&'s crate::sim::anim_class::AnimObject> {
    sim.anims()
        .filter(|(_, anim)| sim.interner.resolve(anim.type_id()) == name)
        .map(|(_, anim)| anim)
        .collect()
}

/// `ShieldClass::CreateAnim` (`ShieldClass.cpp:1041..1074`): the `AI` tail
/// constructs one idle anim at the owner's location, with `SetOwnerObject`,
/// the techno owner and invoker, and `RemainingIterations = 0xFF`.
#[cfg(feature = "phobos")]
#[test]
fn an_online_shield_creates_one_owned_idle_anim() {
    let rules = idle_rules("IdleAnim=IDLE", "");
    let (mut sim, _) = scene(&rules);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.substrate.entities.get(1).unwrap().owner();
    super::tick(&mut sim, 1, &rules);
    super::tick(&mut sim, 1, &rules);

    let anims = idle_anims(&sim, "IDLE");
    assert_eq!(anims.len(), 1, "the shield holds exactly one idle anim");
    let anim = anims[0];
    assert_eq!(anim.owner_entity(), Some(1), "SetOwnerObject");
    assert_eq!(anim.owner_house(), Some(owner));
    assert_eq!(anim.invoker(), Some(1), "AnimExt::SetInvoker");
    assert_eq!(anim.invoker_house(), Some(owner));
    assert_eq!(
        anim.runtime().loop_remaining,
        u8::MAX,
        "RemainingIterations = 0xFF holds the loops"
    );
}

/// `ShieldClass::UpdateIdleAnim`/`GetIdleAnimType` (`ShieldClass.cpp:1076..1125`):
/// `IdleAnimDamaged` replaces the animation while the techno is at or below
/// `[AudioVisual] ConditionYellow`, and the replaced anim is killed.
#[cfg(feature = "phobos")]
#[test]
fn a_damaged_techno_retypes_the_idle_anim() {
    let rules = idle_rules("IdleAnim=IDLE\nIdleAnimDamaged=IDLE2", "");
    let (mut sim, _) = scene(&rules);
    sim.intern_rule_type_ids(&rules);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(idle_anims(&sim, "IDLE").len(), 1);
    assert_eq!(idle_anims(&sim, "IDLE2").len(), 0);

    // ConditionYellow defaults to 0.5; 80/200 crosses the band.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .health
        .set_for_test(80);
    super::tick(&mut sim, 1, &rules);
    sim.flush_pending_delete();
    assert_eq!(
        idle_anims(&sim, "IDLE").len(),
        0,
        "the healthy anim was killed"
    );
    assert_eq!(idle_anims(&sim, "IDLE2").len(), 1);
}

/// `OnlineCheck` (`ShieldClass.cpp:529..591`): an offline powered shield
/// applies `IdleAnim.OfflineAction` every visit, and its return online
/// resumes and later recreates the animation.
#[cfg(feature = "phobos")]
#[test]
fn a_powered_shield_hides_and_restores_its_idle_anim() {
    let rules = idle_rules(
        "Powered=yes\nIdleAnim=IDLE\nIdleAnim.OfflineAction=Hides",
        "",
    );
    let (mut sim, _) = scene(&rules);
    sim.intern_rule_type_ids(&rules);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(idle_anims(&sim, "IDLE").len(), 1);

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .deactivated
        .set_for_test(true);
    super::tick(&mut sim, 1, &rules);
    sim.flush_pending_delete();
    assert_eq!(
        idle_anims(&sim, "IDLE").len(),
        0,
        "Hides kills the anim while offline"
    );

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .deactivated
        .set_for_test(false);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(
        idle_anims(&sim, "IDLE").len(),
        1,
        "the online visit recreates the anim"
    );
}

/// `AttachedAnimFlag::Paused` pauses the frame advance, and the online
/// branch's `Unpause` resumes it (`ShieldClass.cpp:566..568,585..589`).
#[cfg(feature = "phobos")]
#[test]
fn an_offline_paused_shield_anim_resumes_online() {
    let rules = idle_rules(
        "Powered=yes\nIdleAnim=IDLE\nIdleAnim.OfflineAction=Paused",
        "",
    );
    let (mut sim, _) = scene(&rules);
    sim.intern_rule_type_ids(&rules);
    super::tick(&mut sim, 1, &rules);

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .deactivated
        .set_for_test(true);
    super::tick(&mut sim, 1, &rules);
    assert!(idle_anims(&sim, "IDLE")[0].runtime().paused);

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .deactivated
        .set_for_test(false);
    super::tick(&mut sim, 1, &rules);
    assert!(!idle_anims(&sim, "IDLE")[0].runtime().paused);
}

/// `AI_Temporal` (`ShieldClass.cpp:398..437`) applies
/// `IdleAnim.TemporalAction` when the freeze starts; `TemporalCheck` clears
/// the freeze flags and the AI recreates the anim afterwards.
#[cfg(feature = "phobos")]
#[test]
fn a_temporal_freeze_hides_the_idle_anim() {
    let rules = idle_rules("IdleAnim=IDLE\nIdleAnim.TemporalAction=Hides", "");
    let (mut sim, _) = scene(&rules);
    sim.intern_rule_type_ids(&rules);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(idle_anims(&sim, "IDLE").len(), 1);

    let warped = crate::sim::temporal::TemporalState::warped_by_for_test(9);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_temporal_for_test(warped);
    super::tick(&mut sim, 1, &rules);
    sim.flush_pending_delete();
    assert_eq!(
        idle_anims(&sim, "IDLE").len(),
        0,
        "TemporalAction=Hides kills the anim under a temporal freeze"
    );

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_temporal_for_test(crate::sim::temporal::TemporalState::default());
    super::tick(&mut sim, 1, &rules);
    assert_eq!(
        idle_anims(&sim, "IDLE").len(),
        1,
        "TemporalCheck lets the AI recreate the anim"
    );
}

/// `AreAnimsHidden` (`ShieldClass.h:82-86`, `ShieldClass.cpp:481`) as
/// `FootExt::UpdateOnTunnelEnter/Exit` writes it from
/// `TunnelLocomotionClass_Process_KillAnims @ 0x728F74` and `RestoreAnims @
/// 0x728E5F` (`Ext/Techno/Hooks.Misc.cpp:487-527`): a sunk or travelling
/// tunnel owner kills its shield idle animation and blocks the create gate
/// until it starts to surface.
#[cfg(feature = "phobos")]
#[test]
fn a_tunnel_owner_hides_and_restores_the_shield_idle_anim() {
    let rules = idle_rules("IdleAnim=IDLE", "");
    let (mut sim, _) = scene(&rules);
    sim.intern_rule_type_ids(&rules);
    let arm = |sim: &mut Simulation, state: crate::sim::movement::TunnelState| {
        sim.substrate.entities.get_mut(1).unwrap().locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Tunnel,
            ),
        );
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .expect("the fixture installs a Tunnel locomotor")
            .tunnel_runtime_mut_for_test()
            .expect("for_test_kind built a Tunnel payload")
            .set_state_and_timer_for_test(state, 0, 10);
    };
    arm(&mut sim, crate::sim::movement::TunnelState::DiggingIn);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(
        idle_anims(&sim, "IDLE").len(),
        1,
        "digging in still shows the idle anim"
    );
    arm(&mut sim, crate::sim::movement::TunnelState::DugIn);
    super::tick(&mut sim, 1, &rules);
    sim.flush_pending_delete();
    assert_eq!(
        idle_anims(&sim, "IDLE").len(),
        0,
        "AreAnimsHidden kills the idle anim"
    );
    super::tick(&mut sim, 1, &rules);
    assert_eq!(
        idle_anims(&sim, "IDLE").len(),
        0,
        "the create gate stays closed while travelling"
    );
    arm(&mut sim, crate::sim::movement::TunnelState::PreDigOut);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(
        idle_anims(&sim, "IDLE").len(),
        1,
        "surfacing lets the AI recreate it"
    );
}

/// `CloakCheck` (`ShieldClass.cpp:493..500`): a cloaking object kills the
/// idle anim when the anim type's `DetachOnCloak` is set (the default).
#[cfg(feature = "phobos")]
#[test]
fn a_cloaking_shield_detaches_its_idle_anim_per_type_flag() {
    let cloak = |sim: &mut Simulation| {
        let mut runtime = crate::sim::cloak_disguise::CloakRuntime::new(0);
        runtime.establish_unlimbo_fully_cloaked();
        sim.substrate.entities.get_mut(1).unwrap().cloak = Some(runtime);
    };
    let rules = idle_rules("IdleAnim=IDLE", "");
    let (mut sim, _) = scene(&rules);
    sim.intern_rule_type_ids(&rules);
    super::tick(&mut sim, 1, &rules);
    cloak(&mut sim);
    super::tick(&mut sim, 1, &rules);
    sim.flush_pending_delete();
    assert_eq!(idle_anims(&sim, "IDLE").len(), 0, "the default detaches");

    let rules = idle_rules("IdleAnim=IDLE", "DetachOnCloak=no\n");
    let (mut sim, _) = scene(&rules);
    sim.intern_rule_type_ids(&rules);
    super::tick(&mut sim, 1, &rules);
    cloak(&mut sim);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(
        idle_anims(&sim, "IDLE").len(),
        1,
        "DetachOnCloak=no keeps the anim attached"
    );
}

/// `AttachedAnimFlag::Temporal`/`PausedTemporal` (`ShieldClass.cpp:410..427`,
/// `:562..573`) freeze the frame advance through Anim+11A `UnderTemporal`
/// without killing the anim; `TemporalCheck` clears both flags.
#[cfg(feature = "phobos")]
#[test]
fn a_temporal_paused_anim_freezes_and_resumes() {
    let rules = idle_rules("IdleAnim=IDLE\nIdleAnim.TemporalAction=PausedTemporal", "");
    let (mut sim, _) = scene(&rules);
    sim.intern_rule_type_ids(&rules);
    super::tick(&mut sim, 1, &rules);

    let warped = crate::sim::temporal::TemporalState::warped_by_for_test(9);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_temporal_for_test(warped);
    super::tick(&mut sim, 1, &rules);
    {
        let anim = idle_anims(&sim, "IDLE")[0];
        assert!(anim.runtime().paused, "PausedTemporal pauses");
        assert!(
            anim.runtime().temporal_paused,
            "PausedTemporal sets UnderTemporal"
        );
    }

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_temporal_for_test(crate::sim::temporal::TemporalState::default());
    super::tick(&mut sim, 1, &rules);
    let anim = idle_anims(&sim, "IDLE")[0];
    assert!(!anim.runtime().paused);
    assert!(
        !anim.runtime().temporal_paused,
        "TemporalCheck clears UnderTemporal"
    );
}

/// `ShieldTypeClass::GetIdleAnimType` compares the *shield's* ratio against
/// the shield type's `ConditionYellow`/`ConditionRed`, while `isDamaged` uses
/// the techno ratio (`ShieldClass.cpp:1111..1124`,
/// `New/Type/ShieldTypeClass.h:132..147`).
#[cfg(feature = "phobos")]
#[test]
fn the_shield_ratio_selects_the_idle_band() {
    let rules = idle_rules(
        "ConditionYellow=0.75\nIdleAnim=IDLE\nIdleAnim.ConditionYellow=IDLE2",
        "",
    );
    let (mut sim, _) = scene(&rules);
    sim.intern_rule_type_ids(&rules);
    super::tick(&mut sim, 1, &rules);
    assert_eq!(idle_anims(&sim, "IDLE").len(), 1, "100/100 is above 0.75");

    // The techno stays at full health; only the shield ratio (60/100)
    // crosses the shield type's ConditionYellow.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .shield
        .as_mut()
        .unwrap()
        .hp = 60;
    super::tick(&mut sim, 1, &rules);
    sim.flush_pending_delete();
    assert_eq!(idle_anims(&sim, "IDLE").len(), 0);
    assert_eq!(idle_anims(&sim, "IDLE2").len(), 1);
}

/// The committed `force_create` light request, i.e. the shield's
/// `FlashbangWarheadAt` call; the warhead's own detonation flash would carry
/// `force_create=false`.
#[cfg(feature = "phobos")]
fn shield_flash(sim: &Simulation) -> Option<(i32, u32, [i32; 3])> {
    sim.combat_light_requests
        .as_slice()
        .iter()
        .find_map(|request| match request {
            crate::sim::combat::CombatLightRequest::Impact {
                damage,
                coord,
                force_create,
                flags,
                ..
            } => (*force_create).then_some((*damage, *flags, [coord.x, coord.y, coord.z])),
            _ => None,
        })
}

/// `ShieldClass.cpp:283..297`: an absorbed, non-breaking hit whose type sets
/// `HitFlash=yes` and whose warhead leaves `Shield.HitFlash` on calls
/// `MapClass::FlashbangWarheadAt` at the owner's location with `Force=true`
/// and no channel disabled; the strength defaults to twice the clamped
/// shield damage.
#[cfg(feature = "phobos")]
#[test]
fn an_absorbed_hit_emits_the_shield_hit_flash() {
    let fixture = rules("HitFlash=yes", "");
    let (mut sim, _) = scene(&fixture);
    let location = {
        let entity = sim.substrate.entities.get(1).unwrap();
        let point = crate::sim::movement::ground_pose::object_location(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        [point.x, point.y, point.z]
    };
    hit(&mut sim, &fixture, 40);

    let (damage, flags, coord) = shield_flash(&sim).expect("the absorbed hit flashed");
    assert_eq!(
        damage, 80,
        "HitFlash.FixedSize defaults to 2 * shield damage"
    );
    assert_eq!(
        flags, 0x1,
        "the initialiser is NoColor even with every channel on"
    );
    assert_eq!(coord, location, "the light sits at the owner's Location");

    // The default doubles the *clamped* damage: 40 raw becomes 20 absorbed.
    let clamped = rules("HitFlash=yes\nAbsorbPercent=0.5", "");
    let (mut sim, _) = scene(&clamped);
    hit(&mut sim, &clamped, 40);
    let (damage, _, _) = shield_flash(&sim).expect("the absorbed hit flashed");
    assert_eq!(damage, 40, "2 * the clamped 20, not 2 * the raw 40");
}

/// `HitFlash.FixedSize` replaces the doubled damage; `HitFlash.Red/Green/Blue`
/// answer `SpotlightFlags::NoRed/NoGreen/NoBlue` and `HitFlash.Black` answers
/// `NoColor` (`ShieldClass.cpp:285..296`).
#[cfg(feature = "phobos")]
#[test]
fn the_hit_flash_reads_fixed_size_and_channel_flags() {
    let fixed = rules(
        "HitFlash=yes\nHitFlash.FixedSize=123\nHitFlash.Red=no\nHitFlash.Blue=no",
        "",
    );
    let (mut sim, _) = scene(&fixed);
    hit(&mut sim, &fixed, 40);
    let (damage, flags, _) = shield_flash(&sim).expect("the absorbed hit flashed");
    assert_eq!(damage, 123, "the fixed size replaces the doubled damage");
    assert_eq!(flags, 0xA, "NoRed | NoBlue");

    let black = rules("HitFlash=yes\nHitFlash.Black=yes", "");
    let (mut sim, _) = scene(&black);
    hit(&mut sim, &black, 40);
    let (_, flags, _) = shield_flash(&sim).expect("the absorbed hit flashed");
    assert_eq!(flags, 0x1, "Black is SpotlightFlags::NoColor");
}

/// The type gate, the warhead's `Shield.HitFlash` gate and the breaking-hit
/// arm all suppress the flash; the warhead's `CombatLightChance` still gates
/// it at commit time.
#[cfg(feature = "phobos")]
#[test]
fn the_hit_flash_gates_on_type_warhead_and_break() {
    let off = rules("", "");
    let (mut sim, _) = scene(&off);
    hit(&mut sim, &off, 40);
    assert!(
        shield_flash(&sim).is_none(),
        "HitFlash defaults to no on the type"
    );

    let warhead_off = rules("HitFlash=yes", "Shield.HitFlash=no");
    let (mut sim, _) = scene(&warhead_off);
    hit(&mut sim, &warhead_off, 40);
    assert!(
        shield_flash(&sim).is_none(),
        "the warhead's Shield.HitFlash=no suppresses it"
    );

    let breaking = rules("HitFlash=yes", "");
    let (mut sim, _) = scene(&breaking);
    hit(&mut sim, &breaking, 150);
    assert!(
        shield_flash(&sim).is_none(),
        "a shield-breaking hit does not flash"
    );

    let refused = rules("HitFlash=yes", "CombatLightChance=0");
    let (mut sim, _) = scene(&refused);
    hit(&mut sim, &refused, 40);
    assert!(
        shield_flash(&sim).is_none(),
        "the FlashbangWarheadAt hook's CombatLightChance gate refuses"
    );

    let healing = rules("HitFlash=yes", "");
    let (mut sim, _) = scene(&healing);
    hit(&mut sim, &healing, -20);
    assert!(
        shield_flash(&sim).is_none(),
        "a negative (healing) packet never reaches the flash arm"
    );
}
