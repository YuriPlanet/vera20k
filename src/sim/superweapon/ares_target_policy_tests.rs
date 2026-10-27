//! Small production-rule/command scenes for the shared Ares policy. No MO
//! names or assets, native-execution corpus, or duplicate selector in tests.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::superweapon_targeting::TargetReject;
use crate::sim::command::Command;

fn scene(extra: &str, late: &str) -> (RuleSet, Simulation, InternedId, InternedId, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=100\nArmor=light\nImmuneToPsionics=no\n\
         [BuildingTypes]\n0=DESIGNATOR\n[DESIGNATOR]\nStrength=500\nSight=3\n\
         DesignatorRange=3\nSuperWeapon=Strike\n[SuperWeaponTypes]\n0=Strike\n\
         [Strike]\nType=GenericWarhead\nIsPowered=no\nRechargeTime=1\n\
         SW.Warhead=Payload\nSW.Damage=25\nSW.UseAITargeting=yes\n\
         SW.AITargeting=PsychicDominator\nSW.Designators=DESIGNATOR\n\
         SW.RequiresHouse=owner\nSW.AIRequiresHouse=enemies\nSW.RangeMaximum=100.25\n\
         [Warheads]\n0=Payload\n[Payload]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let mut layers = RulesLayerStack::new(ini);
    layers.push(RulesLayerKind::Scenario, IniFile::from_str(extra));
    layers.push(RulesLayerKind::Scenario, IniFile::from_str(late));
    let processed = layers
        .process_with_fixed_art(&IniFile::from_str("[DESIGNATOR]\nFoundation=1x1\n"))
        .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.get("Americans").unwrap();
    let enemy = sim.interner.get("Russians").unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_enemy_house_for_test(Some(enemy));
    let source = sim
        .spawn_object_at_height("DESIGNATOR", "Americans", 12, 15, 0, 0, &rules)
        .unwrap();
    let target = sim
        .spawn_object_at_height("TARGET", "Russians", 15, 15, 0, 0, &rules)
        .unwrap();
    let sw = sim.interner.get("Strike").unwrap();
    let mut instance = SuperWeaponInstance::new(sw, owner, 0);
    instance.is_active = true;
    instance.is_ready = true;
    sim.super_weapons
        .map_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(sw, instance);
    (rules, sim, owner, sw, source, target)
}

#[test]
fn ares_refusal_view_obeys_native_readiness_money_and_constraint_priority() {
    let (rules, mut sim, owner, sw, _, target) = scene("[Strike]\nMoney.Amount=-1\n", "");
    crate::sim::credit_income::add_credits(&mut sim.houses, owner, 1);
    let view = |sim: &Simulation| {
        superweapon_views_for_owner(sim, &rules, &owner)
            .into_iter()
            .find(|view| view.type_id == sw)
            .unwrap()
    };
    assert!(view(&sim).can_fire);
    assert_eq!(view(&sim).ares_refusal, None);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_enemy_house_for_test(None);
    let before = (sim.state_hash(), sim.rng_state());
    let rejected = view(&sim);
    assert!(rejected.constraints_blocked && !rejected.sidebar_enabled());
    assert_eq!(rejected.ares_refusal, Some(AresCameoRefusal::CannotFire));
    assert_eq!(
        (sim.state_hash(), sim.rng_state()),
        before,
        "query is read-only"
    );
    // SPECIAL_PLACE can fail after a previously admitted click. Only that
    // event path emits this owner-scoped line, not background automatic AI.
    sim.sound_events.clear_for_test();
    let command = Command::LaunchSuperWeapon {
        sw_type_id: sw,
        target_rx: 0,
        target_ry: 0,
    };
    assert!(!sim.apply_command_with_overlays(
        "Americans",
        &command,
        &rules,
        None,
        FrameEffects::default(),
    ));
    assert!(
        matches!(sim.sound_events.iter().collect::<Vec<_>>().as_slice(),
            [crate::sim::world::SimSoundEvent::AresSuperWeaponRefused { owner: house, sw_type }]
                if *house == owner && *sw_type == sw
        )
    );
    assert!(sim.super_weapons[&owner][&sw].is_ready());
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .is_ready = false;
    assert_eq!(
        view(&sim).ares_refusal,
        None,
        "unready returns before warnings"
    );
    // Ares AutoFire10038373 returns native Fire_SW's1 after finding a
    // target, even when the previously queued click is no longer ready.
    let enemy = sim.interner.get("Russians").unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_enemy_house_for_test(Some(enemy));
    sim.sound_events.clear_for_test();
    let health = sim.entities().get(target).unwrap().health.current();
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &command,
        &rules,
        None,
        FrameEffects::default(),
    ));
    assert!(sim.sound_events.is_empty());
    assert!(!sim.super_weapons[&owner][&sw].is_ready());
    assert_eq!(sim.entities().get(target).unwrap().health.current(), health);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_enemy_house_for_test(None);
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .is_ready = true;
    let house = sim.houses.get_mut(&owner).unwrap();
    let available = house.economy().available_money();
    house.economy_mut().spend_money(available);
    assert_eq!(
        view(&sim).ares_refusal,
        Some(AresCameoRefusal::InsufficientFunds)
    );

    //10034E00: AutoFire without ManualFire skips the base readiness test,
    // then still checks warnings before its final silent selection refusal.
    let (rules, mut sim, owner, sw, _, _) =
        scene("[Strike]\nSW.AutoFire=yes\nSW.ManualFire=no\n", "");
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_enemy_house_for_test(None);
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .is_ready = false;
    let rejected = superweapon_views_for_owner(&sim, &rules, &owner)
        .into_iter()
        .find(|view| view.type_id == sw)
        .unwrap();
    assert!(!rejected.can_fire);
    assert_eq!(rejected.ares_refusal, Some(AresCameoRefusal::CannotFire));
}

#[test]
fn use_ai_place_ignores_click_and_keeps_first_eligible_maximum_after_invalid_later_rules() {
    let (rules, mut sim, owner, sw, _, first) = scene(
        "",
        "[Strike]\n\
        SW.UseAITargeting=?invalid\nSW.AITargeting=?invalid\n\
        SW.AIRequiresHouse=?invalid\nSW.RangeMaximum=invalid\n",
    );
    assert!(rules.super_weapon_uses_ai_targeting("Strike"));
    assert_eq!(
        rules
            .super_weapon_target_policy("Strike")
            .unwrap()
            .range()
            .1,
        100.25
    );
    let second = sim
        .spawn_object_at_height("TARGET", "Russians", 25, 15, 0, 0, &rules)
        .unwrap();
    sim.spawn_object_at_height("DESIGNATOR", "Americans", 22, 15, 0, 0, &rules)
        .unwrap();
    // A denser cluster cannot use a missing designator. The two eligible
    // isolated targets tie, so forward construction order must keep first.
    for (x, y) in [(25, 25), (25, 26), (24, 25)] {
        sim.spawn_object_at_height("TARGET", "Russians", x, y, 0, 0, &rules)
            .unwrap();
    }
    assert_eq!(
        sim.super_weapon_target_check(&rules, owner, sw, (15, 15), false),
        Err(TargetReject::TargetHouseMismatch)
    );
    let rng = sim.scenario_rng.logical_state();
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &Command::LaunchSuperWeapon {
            sw_type_id: sw,
            target_rx: 28,
            target_ry: 28,
        },
        &rules,
        None,
        FrameEffects::default()
    ));
    assert_eq!(sim.entities().get(first).unwrap().health.current(), 75);
    assert_eq!(sim.entities().get(second).unwrap().health.current(), 100);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        rng,
        "first maximum uses no random tie draw"
    );
    assert!(!sim.super_weapons[&owner][&sw].is_ready());
}

#[test]
fn human_auto_fire_uses_house_strategy_hook_and_manual_switch_requires_auto_fire() {
    for automatic in [false, true] {
        let (rules, mut sim, owner, sw, _, target) = scene(
            &format!(
                "[Strike]\nSW.AutoFire={}\nSW.ManualFire=no\n",
                if automatic { "yes" } else { "no" }
            ),
            "",
        );
        assert_eq!(
            superweapon_views_for_owner(&sim, &rules, &owner)[0].ares_refusal,
            automatic.then_some(AresCameoRefusal::ManualFireDisabled)
        );
        crate::sim::house_strategy::update_strategy(
            &mut sim,
            &rules,
            owner,
            None,
            FrameEffects::default(),
        );
        assert_eq!(
            sim.entities().get(target).unwrap().health.current(),
            if automatic { 75 } else { 100 }
        );
        assert_eq!(sim.super_weapons[&owner][&sw].is_ready(), !automatic);
    }
}

#[test]
fn no_target_selector_bypasses_designators_and_uses_empty_cell() {
    let (rules, mut sim, owner, sw, _, _) = scene(
        "[Strike]\nSW.AITargeting=NoTarget\nSW.Designators=TARGET\n",
        "",
    );
    assert_eq!(
        sim.super_weapon_target_check(&rules, owner, sw, (12, 15), false),
        Err(TargetReject::NoEligibleDesignator)
    );
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &Command::LaunchSuperWeapon {
            sw_type_id: sw,
            target_rx: 15,
            target_ry: 15,
        },
        &rules,
        None,
        FrameEffects::default()
    ));
    assert!(sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::SuperWeaponLaunched { world, .. } if world.to_cell_sub_z().0 == 0 && world.to_cell_sub_z().1 == 0)));
    assert!(!sim.super_weapons[&owner][&sw].is_ready());
}
