//! Stock SUB's retained-target clause in CanAutoCloak6FBDC0.
//!
//! Original6FBEF4..6FBF01 asks the existing CanFireAtTarget range query;
//! ordinary human Attack can retain a legal target outside that range.
//! The water arena supplies terrain and visibility. Actor construction,
//! target admission and cloak AI remain ordinary production paths.

use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::vision::FogState;

fn placed_sub_with_attack(
    rules: &crate::rules::ruleset::RuleSet,
    target_rx: u16,
) -> (super::Simulation, u64, u64) {
    let sub_type = rules.object("SUB").expect("retail submarine");
    assert!(sub_type.cloakable);
    assert!(!sub_type.cloak_stop);
    assert_eq!(sub_type.primary(), Some("SubTorpedo"));
    assert!(sub_type.cloaking_speed > 0);
    assert!(!rules.weapon("SubTorpedo").unwrap().decloak_to_fire);

    let mut sim = crate::sim::arena_fixture::rival_water_arena(rules, 0xC10A_0028, 64);
    sim.fog = FogState::new(64, 64);
    let owner = sim.interner.intern("Americans");
    sim.session.current_house = Some(owner);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let sub = sim
        .spawn_object("SUB", "Americans", 24, 24, 64, rules)
        .expect("ordinary SUB Unlimbo on Water");
    let target = sim
        .spawn_object("DEST", "Russians", target_rx, 24, 192, rules)
        .expect("ordinary naval target Unlimbo on Water");
    // Supplied session knowledge, as on an unveiled map, before issuing the
    // command. No combat target, cloak, health or timer is assigned by hand.
    sim.fog.mark_visible_for_owner(owner, target_rx, 24);
    let actor = sim.substrate.entities.get(sub).unwrap();
    assert!(actor.in_playfield());
    assert!(actor.lifecycle.cell_marked());
    assert!(!actor.lifecycle.in_limbo());
    assert_eq!(actor.health.current(), sub_type.strength);
    assert_eq!(actor.cloak.as_ref().unwrap().state(), 0);
    let frame = sim.session.binary_frame as i32;
    assert!(actor.rearm_timer.timer().expired(frame));
    assert!(actor.cloak.as_ref().unwrap().recloak_delay().expired(frame));
    assert!(sim.houses[&owner].is_human);
    assert!(sim.apply_command(
        "Americans",
        &Command::Attack {
            attacker_id: sub,
            target_id: target,
        },
        Some(rules),
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(sub)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Entity(target)),
        "ordinary Attack must retain the naval target"
    );
    (sim, sub, target)
}

#[test]
fn retail_sub_distant_attack_keeps_target_and_starts_cloaking() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, sub, target) = placed_sub_with_attack(rules, 48);
    assert!(!sim.techno_can_fire_at_target(sub, TargetKind::Entity(target), rules, None));
    let grid = sim.path_grid_snapshot().unwrap();
    sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
    let actor = sim.substrate.entities.get(sub).unwrap();
    assert_eq!(
        actor.attack_target.target(),
        Some(TargetKind::Entity(target)),
        "cloak entry preserves the source's own distant attack"
    );
    assert_eq!(
        actor.cloak.as_ref().unwrap().state(),
        1,
        "native CanAutoCloak permits a retained target outside weapon range"
    );
    assert!(sim.fire_events.iter().all(|event| event.attacker_id != sub));
}

#[test]
fn retail_sub_in_range_attack_blocks_cloak_before_weapon_rearm() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, sub, target) = placed_sub_with_attack(rules, 29);
    assert!(sim.techno_can_fire_at_target(sub, TargetKind::Entity(target), rules, None));
    // Inspect the same producer facts before mission AI has an opportunity to
    // fire: this distinguishes the target-range gate from the rearm gate.
    assert!(
        !super::stock_cloak_tick_facts(&sim, sub, rules, None)
            .unwrap()
            .can_auto_cloak
    );
    let grid = sim.path_grid_snapshot().unwrap();
    sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
    assert_eq!(
        sim.substrate
            .entities
            .get(sub)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state(),
        0
    );
}

#[test]
fn retail_sub_finishes_cloak_with_distant_attack_retained() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, sub, target) = placed_sub_with_attack(rules, 48);
    let grid = sim.path_grid_snapshot().unwrap();
    // Advance the connected mission/movement/cloak phases. No state, timer,
    // health or position is rewritten to manufacture the completion.
    for _ in 0..30 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
    }
    let actor = sim.substrate.entities.get(sub).unwrap();
    assert_eq!(actor.cloak.as_ref().unwrap().state(), 2);
    assert_eq!(
        actor.attack_target.target(),
        Some(TargetKind::Entity(target))
    );
    assert!(!sim.techno_can_fire_at_target(sub, TargetKind::Entity(target), rules, None));
    assert!(sim.fire_events.iter().all(|event| event.attacker_id != sub));
}
