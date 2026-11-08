//! Ares 3.0 `SelfHealing.*` through the production pulse: GetSelfHealAmount
//! (10044E80) in TechnoClass_Update_SelfHeal (1004B820, hook 0x6FA743) and
//! the combat delay the ReceiveDamage aftermath (1004AB50, hook 0x702819)
//! starts.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags};
use crate::sim::game_entity::GameEntity;

/// A Strength-300 `HEALER` with `extra` keys, owned by `A`, at frame 900.
fn scene(extra: &str) -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nRepairRate=1\n[BuildingTypes]\n0=HEALER\n\
         [HEALER]\nStrength=300\nSelfHealing=yes\n{extra}\n\
         [Warheads]\n0=AP\n[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .unwrap();
    let mut sim = Simulation::new();
    let id = sim.substrate.allocate_stable_id();
    let mut building =
        GameEntity::test_default_of_category(id, "HEALER", "A", 2, 2, EntityCategory::Structure);
    building.set_type_ref_for_test(sim.interner.intern("HEALER"));
    building.set_owner_for_test(sim.interner.intern("A"));
    sim.substrate.entities.insert(building);
    sim.session.set_binary_frame_for_test(900);
    (sim, rules)
}

fn pulse(sim: &mut Simulation, rules: &RuleSet, health: i32) -> i32 {
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .health
        .set_for_test(health);
    self_heal_step(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &sim.resolved_terrain,
        1,
        rules,
    );
    sim.substrate.entities.get(1).unwrap().health.current()
}

/// `SelfHealing.Rate=0` pulses every frame (`max(ftol(0 * 900), 1)`, where
/// the stock IDIV faults); the Promotable amount heals by rank up to
/// `ftol(Strength * SelfHealing.%sMax) + 1`, and a damaging hit holds the
/// pulse off for `SelfHealing.CombatDelay` frames.
#[test]
fn ares_self_heal_amount_max_rate_and_combat_delay() {
    let (mut sim, rules) = scene(
        "SelfHealing.Rate=0\nSelfHealing.Amount=10\nSelfHealing.EliteAmount=40\n\
         SelfHealing.Max=50%\nSelfHealing.CombatDelay=30\n",
    );
    assert_eq!(pulse(&mut sim, &rules, 100), 110);
    sim.session.set_binary_frame_for_test(901);
    assert_eq!(pulse(&mut sim, &rules, 148), 151, "capped at 150 + 1");
    assert_eq!(pulse(&mut sim, &rules, 151), 151);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .veterancy
        .set_elite();
    assert_eq!(pulse(&mut sim, &rules, 100), 140);

    let event = EntityDamageEvent::direct_receiver(
        1,
        20,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        sim.interner.intern("AP"),
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    let _ = sim.commit_direct_damage_receiver(&rules, None, event, FrameEffects::default());
    let damaged = sim.substrate.entities.get(1).unwrap().health.current();
    assert_eq!(damaged, 120);
    sim.session.set_binary_frame_for_test(930);
    assert_eq!(pulse(&mut sim, &rules, damaged), damaged, "in combat");
    sim.session.set_binary_frame_for_test(931);
    assert_eq!(pulse(&mut sim, &rules, damaged), 151);
}

/// TechnoClass_Update_SelfHeal (1004B820) sends a crashing techno past both
/// pulses (0x6FA941) in the whole Techno AI visit.
#[test]
fn ares_crashing_techno_does_not_self_heal() {
    let (mut sim, rules) = scene("");
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .health
        .set_for_test(150);
    sim.reveal(1);
    sim.object_ai_stage(Some(&rules));
    assert_eq!(sim.entities().get(1).unwrap().health.current(), 151);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .crash
        .set_crashing_for_test(true);
    sim.session.set_binary_frame_for_test(1800);
    sim.object_ai_stage(Some(&rules));
    assert_eq!(sim.entities().get(1).unwrap().health.current(), 151);
}
