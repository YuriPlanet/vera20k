//! Normal Full_Init ClearClasses686B65 and generated RMG saved-DL admission.
//! These regressions exercise the existing retained process owner. Native clear
//! ordering and TypeReset lifetime rest on installed caller instructions;
//! field parsing has the separate executed global_controls corpus.

use super::*;

fn owner() -> NativeRulesProcessOwner {
    NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::empty(),
        None,
        IniFile::empty(),
        Arc::new(SoundRegistry::default()),
        Arc::new(EvaRegistry::default()),
    )
    .unwrap()
}

fn flags(rules: &RuleSet) -> [bool; 3] {
    std::array::from_fn(|index| rules.ares_ai_bypasses_build_limit(index))
}

fn load(
    owner: &mut NativeRulesProcessOwner,
    prefix: NativeScenarioRulesPrefix<'_>,
    map: &str,
) -> RuleSet {
    owner
        .load_scenario(prefix, &IniFile::from_str(map))
        .unwrap()
        .into_parts()
        .0
}

#[test]
fn ordinary_new_scenario_clears_previous_map_flags_before_early_campaign_process() {
    let mut owner = owner();
    let first = load(
        &mut owner,
        NativeScenarioRulesPrefix::NonCampaign(None),
        "[GlobalControls]\nAllowBypassBuildLimit=yes,yes,yes\n",
    );
    assert_eq!(flags(&first), [true; 3]);
    let named = IniFile::from_str("[GlobalControls]\nAllowBypassBuildLimit=no,yes\n");
    let second = load(
        &mut owner,
        NativeScenarioRulesPrefix::Campaign(Some(&named)),
        "",
    );
    assert_eq!(
        flags(&second),
        [false, true, false],
        "ClearClasses precedes named early Process; its new flags survive TypeReset",
    );
    let third = load(
        &mut owner,
        NativeScenarioRulesPrefix::NonCampaign(None),
        "[Basic]\nRandomMap=yes\n",
    );
    assert_eq!(
        flags(&third),
        [false; 3],
        "an authored INI flag cannot impersonate the generated Full_Init admission",
    );
}

#[test]
fn admitted_random_map_preserves_rules_extension_and_later_mode_prefix_writes() {
    let mut owner = owner();
    let first = load(
        &mut owner,
        NativeScenarioRulesPrefix::NonCampaign(None),
        "[GlobalControls]\nAllowBypassBuildLimit=yes,no,yes\n",
    );
    assert_eq!(flags(&first), [true, false, true]);
    let generated = load(&mut owner, NativeScenarioRulesPrefix::RandomMap(None), "");
    assert_eq!(
        flags(&generated),
        [true, false, true],
        "saved DL skips real ScenarioClear and TypeReset retains RulesExt",
    );
    let mode = IniFile::from_str("[GlobalControls]\nAllowBypassBuildLimit=no,yes\n");
    let generated = load(
        &mut owner,
        NativeScenarioRulesPrefix::RandomMap(Some(&mode)),
        "",
    );
    assert_eq!(flags(&generated), [true, true, false]);
    let ordinary = load(&mut owner, NativeScenarioRulesPrefix::NonCampaign(None), "");
    assert_eq!(flags(&ordinary), [false; 3]);
}
