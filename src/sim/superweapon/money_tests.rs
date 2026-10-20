//! Scenarios adapted from the author's ra2-mono paid-superweapon/preflight
//! tests and contributed under VERA's GPLv3; these use VERA's actual commands.

use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::superweapon;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};
use crate::util::save_codec;

const SOURCE: &str = "[Countries]\n0=Americans\n1=Russians\n\
    [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
    [SuperWeaponTypes]\n0=Reveal\n1=Storm\n2=Dom\n3=Sphere\n4=Warp\n\
    [Reveal]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\nAction=PsychicReveal\n\
    [Storm]\nType=LightningStorm\nRechargeTime=1\nIsPowered=no\n\
    [Dom]\nType=PsychicDominator\nRechargeTime=1\nIsPowered=no\n\
    [Sphere]\nType=ChronoSphere\nRechargeTime=1\nPreClick=yes\nIsPowered=no\n\
    [Warp]\nType=ChronoWarp\nPostClick=yes\nPreDependent=ChronoSphere\nIsPowered=no\n";

fn rules(extra: &str) -> RuleSet {
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(SOURCE);
    ini.merge(&IniFile::from_str(extra));
    let processed = RulesLayerStack::new(ini)
        .process_with_fixed_art(&IniFile::empty())
        .unwrap();
    bound_rules(&processed)
}

fn bound_rules(processed: &ProcessedRulesLayers) -> RuleSet {
    let mut rules = RuleSet::from_processed_rules(processed).unwrap();
    let eva = crate::rules::sound_ini::EvaRegistry::from_ini(&IniFile::from_str(
        "[DialogList]\n0=EVA_InsufficientFunds\n1=CustomFunds\n",
    ));
    rules.bind_type_eva_references(processed.ini(), &eva);
    rules
}

fn world(rules: &RuleSet, credits: i32, human: bool) -> (Simulation, InternedId) {
    let mut sim = Simulation::with_seed(0x4D4F_4E45);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session.set_binary_frame_for_test(100);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, human, credits, 10));
    sim.session.set_current_house_for_test(Some(owner));
    sim.session.game_options_mut_for_test().super_weapons = true;
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    superweapon::construct_house_supers(&mut sim, rules, owner, false);
    (sim, owner)
}

fn ready(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    name: &str,
    once: bool,
) -> InternedId {
    let id = sim.interner.get(name).unwrap();
    assert!(superweapon::trigger_grant_super(
        sim, rules, owner, id, once
    ));
    sim.super_weapons
        .get_mut(&owner)
        .unwrap()
        .get_mut(&id)
        .unwrap()
        .set_charged(
            rules.super_weapon(name).unwrap(),
            sim.session.binary_frame(),
        );
    sim.sound_events.clear();
    id
}

fn fire(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    id: InternedId,
    cell: (u16, u16),
) -> bool {
    let owner = sim.interner.resolve(owner).to_owned();
    sim.apply_command_with_overlays(
        &owner,
        &Command::LaunchSuperWeapon {
            sw_type_id: id,
            target_rx: cell.0,
            target_ry: cell.1,
        },
        Some(rules),
        None,
        FrameEffects::default(),
    )
}

#[test]
fn superweapon_money_ordered_layers_and_saved_rules_keep_the_effective_value() {
    let mut base = IniFile::from_str(SOURCE);
    base.merge(&IniFile::from_str(
        "[Reveal]\nMoney.Amount=-500\nEVA.InsufficientFunds=CustomFunds\n",
    ));
    let mut layers = RulesLayerStack::new(base);
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Reveal]\nRechargeTime=2\n"),
    );
    let processed = layers.process_with_fixed_art(&IniFile::empty()).unwrap();
    let bytes = save_codec::serialize(&processed).unwrap();
    let restored: ProcessedRulesLayers = save_codec::deserialize(&bytes).unwrap();
    let before = bound_rules(&processed);
    let after = bound_rules(&restored);
    assert_eq!(after.super_weapon("Reveal").unwrap().money_amount, -500);
    assert_eq!(
        after
            .super_weapon("Reveal")
            .unwrap()
            .insufficient_funds_eva
            .as_deref(),
        Some("CustomFunds")
    );
    assert_eq!(
        before.simulation_config_hash(),
        after.simulation_config_hash()
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Reveal]\nMoney.Amount=0\nEVA.InsufficientFunds=none\n"),
    );
    let cleared = bound_rules(&layers.process_with_fixed_art(&IniFile::empty()).unwrap());
    assert_eq!(cleared.super_weapon("Reveal").unwrap().money_amount, 0);
    assert_eq!(
        cleared
            .super_weapon("Reveal")
            .unwrap()
            .insufficient_funds_eva,
        None
    );
}

#[test]
fn superweapon_money_insufficient_funds_preserve_charge_wallet_hash_and_effects() {
    let rules = rules("[Reveal]\nMoney.Amount=-500\n");
    let (mut sim, owner) = world(&rules, 499, true);
    let id = ready(&mut sim, &rules, owner, "Reveal", false);
    let before = sim.state_hash();
    assert!(!fire(&mut sim, &rules, owner, id, (10, 10)));
    assert_eq!(sim.state_hash(), before);
    assert!(sim.super_weapons[&owner][&id].is_ready);
    assert!(
        matches!(sim.sound_events.as_slice(), [SimSoundEvent::SuperWeaponInsufficientFunds { owner: actual_owner, sw_type }] if *actual_owner == owner && *sw_type == id)
    );
}

#[test]
fn superweapon_money_exact_funds_pay_once_through_house_wallet() {
    let rules = rules("[Reveal]\nMoney.Amount=-500\n");
    let (mut sim, owner) = world(&rules, 500, true);
    let id = ready(&mut sim, &rules, owner, "Reveal", false);
    assert!(fire(&mut sim, &rules, owner, id, (10, 10)));
    assert_eq!(sim.houses[&owner].economy().credits(), 0);
    assert_eq!(sim.houses[&owner].economy().spent_credits(), 500);
    assert!(!sim.super_weapons[&owner][&id].is_ready);
    assert!(sim.sound_events.iter().any(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { sw_type, .. } if *sw_type == id)));
    assert!(!fire(&mut sim, &rules, owner, id, (10, 10)));
    assert_eq!(sim.houses[&owner].economy().spent_credits(), 500);
}

#[test]
fn superweapon_money_positive_grant_changes_cash_not_spending_or_score() {
    let rules = rules("[Reveal]\nMoney.Amount=400\n");
    let (mut sim, owner) = world(&rules, 50, true);
    let id = ready(&mut sim, &rules, owner, "Reveal", false);
    assert!(fire(&mut sim, &rules, owner, id, (10, 10)));
    assert_eq!(sim.houses[&owner].economy().credits(), 450);
    assert_eq!(sim.houses[&owner].economy().spent_credits(), 0);
    assert_eq!(sim.houses[&owner].economy().score(), 0);
    assert!(!fire(&mut sim, &rules, owner, id, (10, 10)));
    assert_eq!(sim.houses[&owner].economy().credits(), 450);
}

#[test]
fn superweapon_money_positive_grant_preserves_host_wrapping_add_credits() {
    let rules = rules("[Reveal]\nMoney.Amount=1\n");
    let (mut sim, owner) = world(&rules, i32::MAX, true);
    let id = ready(&mut sim, &rules, owner, "Reveal", false);
    assert!(fire(&mut sim, &rules, owner, id, (10, 10)));
    assert_eq!(sim.houses[&owner].economy().credits(), i32::MIN);
}

#[test]
fn superweapon_money_minimum_signed_cost_cannot_overflow_into_a_reward() {
    let rules = rules("[Reveal]\nMoney.Amount=-2147483648\n");
    let (mut sim, owner) = world(&rules, i32::MAX, true);
    let id = ready(&mut sim, &rules, owner, "Reveal", false);
    let before = sim.state_hash();
    assert!(!fire(&mut sim, &rules, owner, id, (10, 10)));
    assert_eq!(sim.state_hash(), before);
}

#[test]
fn superweapon_money_unready_or_charge_drain_does_not_pay() {
    for extra in ["", "UseChargeDrain=yes\n"] {
        let rules = rules(&format!("[Reveal]\nMoney.Amount=-500\n{extra}"));
        let (mut sim, owner) = world(&rules, 1000, true);
        let id = ready(&mut sim, &rules, owner, "Reveal", false);
        if extra.is_empty() {
            sim.super_weapons
                .get_mut(&owner)
                .unwrap()
                .get_mut(&id)
                .unwrap()
                .is_ready = false;
        }
        let before = sim.state_hash();
        assert!(!fire(&mut sim, &rules, owner, id, (10, 10)));
        assert_eq!(sim.state_hash(), before);
        assert!(sim.sound_events.is_empty());
    }
}

#[test]
fn superweapon_money_busy_storm_refuses_without_a_second_charge() {
    let rules = rules("[Storm]\nMoney.Amount=-500\n");
    let (mut sim, owner) = world(&rules, 1000, true);
    let id = ready(&mut sim, &rules, owner, "Storm", false);
    superweapon::lightning_storm::start(&mut sim, &rules, 180, 250, (3, 3), Some(owner));
    sim.sound_events.clear();
    let before = sim.state_hash();
    assert!(!fire(&mut sim, &rules, owner, id, (10, 10)));
    assert_eq!(sim.state_hash(), before);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::LightningStormRefused { .. }]
    ));
}

#[test]
fn superweapon_money_one_time_grant_cannot_pay_or_reward_twice() {
    for amount in [-500, 500] {
        let rules = rules(&format!("[Reveal]\nMoney.Amount={amount}\n"));
        let (mut sim, owner) = world(&rules, 1000, true);
        let id = ready(&mut sim, &rules, owner, "Reveal", true);
        assert!(fire(&mut sim, &rules, owner, id, (10, 10)));
        assert_eq!(sim.houses[&owner].economy().credits(), 1000 + amount);
        assert!(!sim.super_weapons[&owner][&id].is_active);
        assert!(!fire(&mut sim, &rules, owner, id, (10, 10)));
        assert_eq!(sim.houses[&owner].economy().credits(), 1000 + amount);
    }
}

#[test]
fn superweapon_money_ui_affordability_is_live_and_independent_of_charge() {
    let rules = rules("[Reveal]\nMoney.Amount=-500\n");
    let (mut sim, owner) = world(&rules, 499, true);
    let id = ready(&mut sim, &rules, owner, "Reveal", false);
    let view = |sim: &Simulation| {
        superweapon::superweapon_views_for_owner(sim, &rules, &owner)
            .into_iter()
            .find(|view| view.type_id == id)
            .unwrap()
    };
    assert!(view(&sim).is_ready);
    assert!(!view(&sim).can_afford);
    crate::sim::credit_income::add_credits(&mut sim, owner, 1);
    assert!(view(&sim).can_afford);
    crate::sim::credit_income::spend_money(&mut sim, owner, 1);
    assert!(!view(&sim).can_afford);
    assert!(view(&sim).is_ready);
}

#[test]
fn superweapon_money_ai_refuses_before_target_search_or_rng() {
    use superweapon::ai_fire::{AI_FIRE_LOG, AiFireEvent};
    let rules = rules("[Reveal]\nMoney.Amount=-500\n");
    let (mut sim, owner) = world(&rules, 499, false);
    ready(&mut sim, &rules, owner, "Reveal", false);
    let before = sim.state_hash();
    AI_FIRE_LOG.with(|log| *log.borrow_mut() = Some(Vec::new()));
    superweapon::ai_fire::try_fire(&mut sim, &rules, owner, None, FrameEffects::default());
    let events = AI_FIRE_LOG.with(|log| log.borrow_mut().take().unwrap());
    assert_eq!(events, vec![AiFireEvent::TryFire]);
    assert_eq!(sim.state_hash(), before);
    assert!(sim.sound_events.is_empty());
}

#[test]
fn superweapon_money_zero_cost_chronosphere_keeps_native_source_and_warp() {
    let rules = rules("");
    let (mut sim, owner) = world(&rules, 1000, true);
    let sphere = ready(&mut sim, &rules, owner, "Sphere", false);
    let warp = sim.interner.get("Warp").unwrap();
    assert!(fire(&mut sim, &rules, owner, sphere, (10, 10)));
    assert!(!fire(&mut sim, &rules, owner, sphere, (10, 10)));
    assert!(fire(&mut sim, &rules, owner, warp, (15, 15)));
    assert_eq!(sim.houses[&owner].economy().credits(), 1000);
    assert_eq!(sim.houses[&owner].economy().spent_credits(), 0);
}

#[test]
fn superweapon_money_resolved_amount_participates_in_configuration_hash() {
    let mut rules = rules("");
    let baseline = rules.simulation_config_hash();
    rules.super_weapons.get_mut("Reveal").unwrap().money_amount = -500;
    assert_ne!(rules.simulation_config_hash(), baseline);
    rules.super_weapons.get_mut("Reveal").unwrap().money_amount = 0;
    assert_eq!(rules.simulation_config_hash(), baseline);
}

#[test]
fn superweapon_money_saved_wallet_and_charge_do_not_repeat_settlement() {
    use crate::sim::snapshot::GameSnapshot;
    let rules = rules("[Reveal]\nMoney.Amount=-500\n");
    let (mut sim, owner) = world(&rules, 1000, true);
    let id = ready(&mut sim, &rules, owner, "Reveal", true);
    assert!(fire(&mut sim, &rules, owner, id, (10, 10)));
    let bytes = GameSnapshot::save_validated(&sim, 0, rules.simulation_config_hash(), "paid", 0);
    let saved: GameSnapshot = save_codec::deserialize(&bytes).unwrap();
    let mut restored = saved.sim;
    // Headless synthetic fixture: restore the same map/rules resources through
    // their normal owners before rebuilding the skipped simulation indexes.
    crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
    restored.resolve_type_handles(&rules);
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.houses[&owner].economy().credits(), 500);
    assert_eq!(restored.houses[&owner].economy().spent_credits(), 500);
    assert!(!fire(&mut restored, &rules, owner, id, (10, 10)));
    assert_eq!(restored.houses[&owner].economy().credits(), 500);
}

#[test]
fn superweapon_money_funded_ai_uses_the_same_single_settlement() {
    let rules = rules("[Reveal]\nMoney.Amount=-500\n");
    let (mut sim, owner) = world(&rules, 1000, false);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let id = ready(&mut sim, &rules, owner, "Reveal", false);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_preferred_super_weapon_target((10, 10));
    superweapon::ai_fire::try_fire(&mut sim, &rules, owner, None, FrameEffects::default());
    assert_eq!(sim.houses[&owner].economy().credits(), 500);
    assert_eq!(sim.houses[&owner].economy().spent_credits(), 500);
    assert!(!sim.super_weapons[&owner][&id].is_ready);
    superweapon::ai_fire::try_fire(&mut sim, &rules, owner, None, FrameEffects::default());
    assert_eq!(sim.houses[&owner].economy().credits(), 500);
}

#[test]
fn superweapon_money_action_none_settles_the_native_direct_launch_event() {
    for amount in [-500, 500] {
        let rules = rules(&format!("[Reveal]\nAction=none\nMoney.Amount={amount}\n"));
        let (mut sim, owner) = world(&rules, 1000, true);
        let id = ready(&mut sim, &rules, owner, "Reveal", false);
        // The sidebar's Action=None branch intentionally supplies no target.
        assert!(fire(&mut sim, &rules, owner, id, (0, 0)));
        assert_eq!(sim.houses[&owner].economy().credits(), 1000 + amount);
        assert!(!fire(&mut sim, &rules, owner, id, (0, 0)));
        assert_eq!(sim.houses[&owner].economy().credits(), 1000 + amount);
    }
}

#[test]
fn superweapon_money_generic_payload_and_target_preflight_precede_settlement() {
    for (override_tags, accepted) in [
        ("", true),
        ("SW.RequiresTarget=infantry\n", false),
        ("SW.Warhead=none\n", false),
    ] {
        let mut ini = crate::sim::arena_fixture::clear_ground_ini(SOURCE);
        ini.merge(&IniFile::from_str("[BuildingTypes]\n0=LAUNCHER\n\
            [LAUNCHER]\nStrength=200\nSuperWeapon=Reveal\n\
            [Reveal]\nType=GenericWarhead\nAction=Attack\nSW.Warhead=Payload\nSW.Damage=25\nMoney.Amount=-500\n\
            [Payload]\nCellSpread=1\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"));
        ini.merge(&IniFile::from_str(&format!("[Reveal]\n{override_tags}")));
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &ini,
            &IniFile::from_str("[LAUNCHER]\nFoundation=1x1\n"),
        )
        .unwrap();
        let (mut sim, owner) = world(&rules, 1000, true);
        sim.spawn_object("LAUNCHER", "Americans", 6, 6, 0, &rules)
            .unwrap();
        let id = ready(&mut sim, &rules, owner, "Reveal", false);
        let before = sim.state_hash();
        assert_eq!(
            fire(&mut sim, &rules, owner, id, (10, 10)),
            accepted,
            "{override_tags}"
        );
        assert_eq!(
            sim.houses[&owner].economy().credits(),
            if accepted { 500 } else { 1000 }
        );
        if !accepted {
            assert_eq!(sim.state_hash(), before);
            assert!(sim.sound_events.is_empty());
        }
    }
}

#[test]
fn superweapon_money_two_stage_source_never_changes_wallet_across_ticks() {
    for amount in [-500, 500] {
        let rules = rules(&format!("[Sphere]\nMoney.Amount={amount}\n"));
        let (mut sim, owner) = world(&rules, 10_000, true);
        let source = ready(&mut sim, &rules, owner, "Sphere", false);
        let mut admitted = Vec::new();
        for _ in 0..3 {
            let before = sim.state_hash();
            sim.sound_events.clear();
            admitted.push(fire(&mut sim, &rules, owner, source, (10, 10)));
            assert_eq!(
                sim.state_hash(),
                before,
                "refusal preserves source and pair"
            );
            assert!(
                sim.sound_events.is_empty(),
                "refusal has no activation effects"
            );
            sim.session
                .set_binary_frame_for_test(sim.session.binary_frame() + 1);
            superweapon::tick_house_superweapon_instances(&mut sim, &rules, owner);
        }
        assert_eq!(
            sim.houses[&owner].economy().credits(),
            10_000,
            "unsupported paid source must not settle on a repeatable selection"
        );
        assert_eq!(admitted, vec![false; 3]);
        assert!(sim.super_weapons[&owner][&source].is_ready);
    }
}

#[test]
fn superweapon_money_two_stage_kinds_cannot_bypass_exclusion_by_clearing_flags() {
    for (name, flags) in [("Sphere", "PreClick=no"), ("Warp", "PostClick=no")] {
        let rules = rules(&format!("[{name}]\n{flags}\nMoney.Amount=500\n"));
        let (mut sim, owner) = world(&rules, 1000, true);
        let id = ready(&mut sim, &rules, owner, name, false);
        let before = sim.state_hash();
        assert!(!fire(&mut sim, &rules, owner, id, (10, 10)));
        assert_eq!(sim.state_hash(), before);
        assert!(sim.sound_events.is_empty());
    }
}

#[test]
fn superweapon_money_postclick_rejects_paid_predecessor_with_single_stage_flags() {
    for amount in [-500, 500] {
        let rules = rules(&format!(
            "[Sphere]\nType=PsychicReveal\nPreClick=no\nMoney.Amount={amount}\n"
        ));
        let (mut sim, owner) = world(&rules, 1000, true);
        let source = ready(&mut sim, &rules, owner, "Sphere", false);
        let destination = ready(&mut sim, &rules, owner, "Warp", false);
        let before = sim.state_hash();
        assert!(!fire(&mut sim, &rules, owner, destination, (15, 15)));
        assert_eq!(sim.state_hash(), before);
        assert!(sim.super_weapons[&owner][&source].is_ready);
        assert!(sim.sound_events.is_empty());
    }
}
