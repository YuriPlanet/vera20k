//! Control-link scalar readers through ordered production rules passes.
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::util::save_codec;

#[test]
fn drain_money_defaults_layers_and_saved_process_lifetime() {
    let pair = |rules: &RuleSet| {
        (
            rules.general.drain_money_frame_delay,
            rules.general.drain_money_amount,
        )
    };
    let defaults = RuleSet::from_ini(&IniFile::empty()).unwrap();
    assert_eq!(pair(&defaults), (100, 0));
    let wrong_keys = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nDrainMoneyFrameDelay=9\nDrainMoneyAmount=90\n\
         [combatdamage]\nDrainMoneyFrameDelay=8\n\
         [CombatDamage]\ndrainmoneyamount=80\n",
    ))
    .unwrap();
    assert_eq!(pair(&wrong_keys), (100, 0));

    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[CombatDamage]\nDrainMoneyFrameDelay=17tail\nDrainMoneyAmount=13\n",
    ));
    layers.push(
        RulesLayerKind::LangRule,
        IniFile::from_str("[CombatDamage]\nDrainMoneyAmount=-5\n"),
    );
    assert_eq!(
        pair(&RuleSet::from_rules_layers(&layers).unwrap()),
        (17, -5)
    );
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[CombatDamage]\nDrainMoneyFrameDelay=bad\n"),
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[CombatDamage]\nDrainMoneyFrameDelay=$Z\nDrainMoneyAmount=$Z\n"),
    );
    let processed = layers.process().unwrap();
    assert_eq!(
        pair(&RuleSet::from_processed_rules(&processed).unwrap()),
        (0, -5)
    );
    let restored: ProcessedRulesLayers =
        save_codec::deserialize(&save_codec::serialize(&processed).unwrap()).unwrap();
    let (_, trace) = restored.into_ini_and_native_type_construction_trace();
    let state = trace
        .into_registry_state_discarding_events()
        .destructive_reset();
    let next = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
        .unwrap();
    let retained = RuleSet::from_processed_rules(&next).unwrap();
    assert_eq!(pair(&retained), (0, -5));
    assert_eq!(defaults.source_ini_hash(), retained.source_ini_hash());
    assert_ne!(
        defaults.simulation_config_hash(),
        retained.simulation_config_hash()
    );
}

#[test]
fn control_line_defaults_exact_keys_and_signed_values() {
    for (text, frames) in [
        ("", 15),
        ("[General]\nMindControlAttackLineFrames=7\n", 15),
        ("[combatdamage]\nMindControlAttackLineFrames=7\n", 15),
        ("[CombatDamage]\nmindcontrolattacklineframes=7\n", 15),
        ("[CombatDamage]\nMindControlAttackLineFrames=24tail\n", 24),
        ("[CombatDamage]\nMindControlAttackLineFrames=-4\n", -4),
    ] {
        let rules = RuleSet::from_ini(&IniFile::from_str(text)).unwrap();
        assert_eq!(rules.mind_control.attack_line_frames, frames, "{text}");
    }
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MIND\n[MIND]\nStrength=100\n",
    ))
    .unwrap();
    assert_eq!(rules.object("MIND").unwrap().lepton_mind_control_offset, 70);
}

#[test]
fn control_line_rules_retain_values_over_layers_handoffs_and_saves() {
    let root = IniFile::from_str(
        "[VehicleTypes]\n0=MIND\n[MIND]\nStrength=100\nLeptonMindControlOffset=80\n[CombatDamage]\nMindControlAttackLineFrames=22\n",
    );
    let mut layers = RulesLayerStack::new(root);
    for (kind, text, frames, offset) in [
        (
            RulesLayerKind::LangRule,
            "[CombatDamage]\nMindControlAttackLineFrames=31\n[MIND]\nLeptonMindControlOffset=99\n",
            31,
            99,
        ),
        (
            RulesLayerKind::GameMode,
            "[CombatDamage]\nMindControlAttackLineFrames=bad\n[MIND]\nLeptonMindControlOffset=bad\n",
            0,
            0,
        ),
        (
            RulesLayerKind::Scenario,
            "[CombatDamage]\nmindcontrolattacklineframes=0\n[MIND]\nleptonmindcontroloffset=0\n",
            0,
            0,
        ),
        (
            RulesLayerKind::Scenario,
            "[CombatDamage]\nMindControlAttackLineFrames=-3\n[MIND]\nLeptonMindControlOffset=-17\n",
            -3,
            -17,
        ),
    ] {
        layers.push(kind, IniFile::from_str(text));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(rules.mind_control.attack_line_frames, frames);
        assert_eq!(
            rules.object("MIND").unwrap().lepton_mind_control_offset,
            offset
        );
    }
    let processed = layers.process().unwrap();
    let restored: ProcessedRulesLayers =
        save_codec::deserialize(&save_codec::serialize(&processed).unwrap()).unwrap();
    let (_, trace) = restored.into_ini_and_native_type_construction_trace();
    let mut state = trace.into_registry_state_discarding_events();
    for reset in [false, true] {
        if reset {
            state = state.destructive_reset();
        }
        let next = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=MIND\n[MIND]\nStrength=100\n",
        ))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
        .unwrap();
        let rules = RuleSet::from_processed_rules(&next).unwrap();
        assert_eq!(rules.mind_control.attack_line_frames, -3);
        assert_eq!(
            rules.object("MIND").unwrap().lepton_mind_control_offset,
            if reset { 70 } else { -17 }
        );
        let (_, trace) = next.into_ini_and_native_type_construction_trace();
        state = trace.into_registry_state_discarding_events();
    }
}

#[test]
fn retail_control_line_values_use_the_production_battle_readers() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Hills.mmx")
    else {
        return;
    };
    assert_eq!(battle.rules.mind_control.attack_line_frames, 20);
    assert_eq!(battle.rules.general.drain_money_frame_delay, 30);
    assert_eq!(battle.rules.general.drain_money_amount, 30);
    for name in ["YURI", "YURIPR", "MIND", "YAPSYT", "HTNK", "E1"] {
        assert_eq!(
            battle
                .rules
                .object(name)
                .unwrap()
                .lepton_mind_control_offset,
            70,
            "{name}"
        );
    }
}
