//! Ares 3.0 WarheadTypeExt damage modifiers through the production receiver,
//! area collector and death arm. The rules reader has its own tests in
//! `rules::ares_warhead`.

use super::combat_aoe::{self, AreaDamageReceiver};
use super::receiver_call::{EntityDamageEvent, ReceiverCallFlags, resolve_receive_damage};
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// A Strength-1000 vehicle and a warhead `HIT` with `verses` everywhere and
/// `extra` keys.
fn rules_with(verses: &str, extra: &str) -> RuleSet {
    let verses = [verses; 11].join(",");
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [AudioVisual]\nConditionYellow=0.5\nConditionRed=0.25\n\
         [VehicleTypes]\n0=TARGET\n[InfantryTypes]\n0=TROOP\n\
         [TARGET]\nStrength=1000\nArmor=light\n\
         [TROOP]\nStrength=100\nArmor=none\n\
         [Warheads]\n0=HIT\n\
         [HIT]\nVerses={verses}\n{extra}"
    )))
    .expect("warhead fixture rules")
}

fn rules(extra: &str) -> RuleSet {
    rules_with("100%", extra)
}

fn scene(rules: &RuleSet, kind: &str) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 17);
    let target = sim
        .spawn_object(kind, "Russians", 20, 20, 0, rules)
        .unwrap();
    let attacker = sim
        .spawn_object("TARGET", "Americans", 22, 20, 0, rules)
        .unwrap();
    (sim, target, attacker)
}

fn hit(sim: &mut Simulation, target: u64, attacker: u64, damage: i32) -> EntityDamageEvent {
    let wh = sim.interner.intern("HIT");
    EntityDamageEvent::direct_receiver(
        target,
        damage,
        0,
        attacker,
        None,
        wh,
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    )
}

fn resolved_delta(rules: &RuleSet, health: i32, damage: i32) -> i32 {
    let (mut sim, target, attacker) = scene(rules, "TARGET");
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .health
        .set_for_test(health);
    let event = hit(&mut sim, target, attacker, damage);
    resolve_receive_damage(
        &event,
        sim.entities(),
        rules,
        &sim.interner,
        &sim.houses,
        &sim.house_alliances,
        false,
        0,
        sim.resolved_terrain(),
        None,
    )
    .unwrap()
    .outcome
    .hp_delta
}

/// ObjectClass_ReceiveDamage_Relative (10053A80): a positive kind value is a
/// share of Strength, a negative one of the current Health, the packet's sign
/// is kept, and the Verses kernel still applies after it.
#[test]
fn relative_damage_replaces_the_packet_before_the_verses_kernel() {
    let relative = rules("RelativeDamage=yes\nRelativeDamage.Vehicles=25\n");
    assert_eq!(resolved_delta(&relative, 1000, 10), 250);
    assert_eq!(resolved_delta(&relative, 1000, -10), -250);
    let current = rules("RelativeDamage=yes\nRelativeDamage.Vehicles=-50\n");
    assert_eq!(resolved_delta(&current, 600, 10), 300);
    let other_kind = rules("RelativeDamage=yes\nRelativeDamage.Infantry=50\n");
    assert_eq!(resolved_delta(&other_kind, 1000, 10), 0);
    let halved = rules_with("50%", "RelativeDamage=yes\nRelativeDamage.Vehicles=25\n");
    assert_eq!(resolved_delta(&halved, 1000, 10), 125);
    assert_eq!(
        resolved_delta(&rules("RelativeDamage.Vehicles=25\n"), 1000, 10),
        10
    );
}

fn health_after_hit(rules: &RuleSet, health: i32, elite_attacker: bool) -> i32 {
    let (mut sim, target, attacker) = scene(rules, "TARGET");
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .health
        .set_for_test(health);
    if elite_attacker {
        sim.substrate
            .entities
            .get_mut(attacker)
            .unwrap()
            .veterancy
            .set_rank_for_test(200);
    }
    let event = hit(&mut sim, target, attacker, 10);
    let _ = sim.commit_direct_damage_receiver(
        rules,
        None,
        event,
        crate::sim::world::FrameEffects::default(),
    );
    sim.substrate
        .entities
        .get(target)
        .map_or(0, |target| target.health.current())
}

/// ObjectClass_ReceiveDamage_Culling (10053A10) with CanCull (10052590): a
/// Culling warhead takes a red target's whole Health; the elite default also
/// culls at yellow; BelowHealth and a zero chance decide by rank.
#[test]
fn culling_warheads_kill_by_health_status_and_rank() {
    let culling = rules("Culling=yes\n");
    assert_eq!(health_after_hit(&culling, 200, false), 0, "red culls");
    assert_eq!(health_after_hit(&culling, 400, false), 390, "yellow rookie");
    assert_eq!(health_after_hit(&culling, 400, true), 0, "yellow elite");
    let percent = rules("Culling=yes\nCulling.BelowHealth=60\n");
    assert_eq!(health_after_hit(&percent, 600, false), 0, "at 60%");
    // Chop rounding takes 610 / 1000 * 100 to 60.99..., which culls.
    assert_eq!(health_after_hit(&percent, 610, false), 0, "chopped to 60");
    assert_eq!(health_after_hit(&percent, 620, false), 610, "61 is above");
    let never = rules("Culling=yes\nCulling.Chance=0\n");
    assert_eq!(health_after_hit(&never, 200, false), 190);
    assert_eq!(health_after_hit(&rules(""), 200, false), 190);
}

/// InfantryClass_ReceiveDamage_DeployedDamage (100539A0): a deployed
/// infantryman's raw damage is scaled by Damage.Deployed, without the prone
/// head's minimum of one.
#[test]
fn deployed_infantry_take_the_deployed_damage_multiplier() {
    let rules = rules("Damage.Deployed=25%\nProneDamage=200%\n");
    let (mut sim, target, attacker) = scene(&rules, "TROOP");
    let event = hit(&mut sim, target, attacker, 3);
    let prepared = |sim: &Simulation| {
        super::receiver_call::prepare_infantry_receiver(
            &event,
            sim.entities(),
            &rules,
            &sim.interner,
            sim.resolved_terrain(),
        )
        .damage
    };
    assert_eq!(prepared(&sim), 3, "standing: neither head");
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(28)
        .unwrap();
    assert_eq!(prepared(&sim), 0, "ftol(3 * 0.25) with no minimum");
}

/// DamageArea_NullDamage (10053940): only an AllowZeroDamage warhead enters
/// Apply_area_damage with zero damage; an inert scenario never does.
#[test]
fn zero_damage_enters_the_area_only_when_allowed() {
    let plain = rules("");
    let allowed = rules("AllowZeroDamage=yes\n");
    let wh = |rules: &RuleSet| rules.warhead("HIT").unwrap().clone();
    assert!(!combat_aoe::area_damage_enters(0, &wh(&plain), false));
    assert!(combat_aoe::area_damage_enters(0, &wh(&allowed), false));
    assert!(!combat_aoe::area_damage_enters(0, &wh(&allowed), true));
    assert!(combat_aoe::area_damage_enters(5, &wh(&plain), false));
}

/// DamageArea_Damage_MaxAffect (10053400): each object keeps its nearest
/// records up to the limit, ties to the earlier record, in record order.
#[test]
fn max_affect_keeps_the_nearest_records_of_each_object() {
    let rules = rules("CellSpread.MaxAffect=2\n");
    let warhead = rules.warhead("HIT").unwrap();
    let record = |target: u64, distance: i32| {
        AreaDamageReceiver::Entity(EntityDamageEvent::area(
            target,
            10,
            distance,
            0,
            None,
            Default::default(),
        ))
    };
    let mut result = combat_aoe::AoEDamageResult {
        receivers: vec![
            record(1, 90),
            record(2, 50),
            record(1, 30),
            record(1, 30),
            record(1, 10),
        ],
        ..Default::default()
    };
    combat_aoe::limit_records_per_object(&mut result, warhead);
    let kept: Vec<(u64, i32)> = result
        .receivers
        .iter()
        .map(|receiver| match receiver {
            AreaDamageReceiver::Entity(event) => (event.target_id, event.distance_leptons),
            AreaDamageReceiver::Terrain(_) => unreachable!(),
        })
        .collect();
    assert_eq!(kept, vec![(2, 50), (1, 30), (1, 10)]);
}

fn unit_lost_cues(sim: &Simulation) -> usize {
    sim.sound_events
        .iter()
        .filter(|event| matches!(event, crate::sim::world::SimSoundEvent::UnitLost { .. }))
        .count()
}

/// A human house's vehicle killed through the receiver, and its flash.
fn kill_human_vehicle(rules: &RuleSet, damage: i32) -> (Simulation, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 17);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, rules)
        .unwrap();
    let attacker = sim
        .spawn_object("TARGET", "Russians", 22, 20, 0, rules)
        .unwrap();
    let event = hit(&mut sim, target, attacker, damage);
    let _ = sim.commit_direct_damage_receiver(
        rules,
        None,
        event,
        crate::sim::world::FrameEffects::default(),
    );
    (sim, target)
}

/// TechnoClass_ReceiveDamage_SuppressUnitLost (1004B020) silences the
/// victim's Death_Announcement; the aftermath (1004AB50) starts a longer
/// Flash.Duration on a survivor.
#[test]
fn unit_lost_suppress_and_flash_duration_follow_the_hit_warhead() {
    let (sim, _) = kill_human_vehicle(&rules(""), 5000);
    assert_eq!(unit_lost_cues(&sim), 1);
    let (sim, _) = kill_human_vehicle(&rules("UnitLost.Suppress=yes\n"), 5000);
    assert_eq!(unit_lost_cues(&sim), 0);
    let (sim, target) = kill_human_vehicle(&rules("Flash.Duration=12\n"), 10);
    assert_eq!(sim.entities().get(target).unwrap().flashing.frames(), 12);
    let (sim, target) = kill_human_vehicle(&rules(""), 10);
    assert_eq!(sim.entities().get(target).unwrap().flashing.frames(), 0);
}

/// HouseClass_BuildingUnderAttack_Malicious (10053980): a `Malicious=no`
/// hit skips the victim house's own alert, keeping the ally line and Tags.
#[test]
fn non_malicious_hits_raise_no_own_building_alert() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n[BuildingTypes]\n0=BASE\n\
         [VehicleTypes]\n0=TARGET\n[BASE]\nStrength=1000\n[TARGET]\nStrength=1000\n",
    ))
    .unwrap();
    let alerts = |malicious: bool| {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        let base = sim
            .spawn_object("BASE", "Americans", 20, 20, 0, &rules)
            .unwrap();
        let attacker = sim
            .spawn_object("TARGET", "Russians", 24, 20, 0, &rules)
            .unwrap();
        let ping = sim.building_hit_response(
            base,
            Some(attacker),
            super::damage::DamageState::Damaged,
            malicious,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert!(ping.is_some());
        sim.sound_events
            .iter()
            .filter(|event| matches!(event, crate::sim::world::SimSoundEvent::UnderAttack { .. }))
            .count()
    };
    assert_eq!(alerts(true), 1);
    assert_eq!(alerts(false), 0);
}
