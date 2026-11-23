//! The per-game VeraX switch (`rules::verax_switch`) through the production
//! Rules owner: a game without VeraX's key plays as gamemd, and a game whose
//! own data holds the key plays VeraX.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

/// A ruleset from one INI, read by the production reader.
fn rules(ini: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(ini)).expect("a ruleset")
}

/// A game's own `[VeraX] Enabled=` key turns VeraX on; a game without that
/// key leaves VeraX off.
#[test]
fn a_games_own_key_turns_verax_on() {
    assert!(!rules("[General]\nBuildSpeed=.7\n").verax());
    assert!(rules("[VeraX]\nEnabled=1\n").verax());
    assert!(rules("[VeraX]\nEnabled=yes\n").verax());
    assert!(!rules("[VeraX]\nEnabled=no\n").verax());
    assert!(!rules("[VeraX]\nEnabled=\n").verax());
}

/// A map's own `[VeraX] Enabled=` key turns VeraX on for that game, and the
/// retail game of the same install keeps it off: the switch follows the
/// game's data, not the build.
#[test]
fn a_maps_own_key_turns_verax_on() {
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut owner: NativeRulesProcessOwner =
        crate::rules::retail_ini_fixture::retail_rules_owner(&assets).with_ares_mix(false);
    let map = IniFile::from_str("[VeraX]\nEnabled=1\n");
    let game = owner
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &map)
        .unwrap()
        .into_parts()
        .0;
    assert!(game.verax());
    let mut owner: NativeRulesProcessOwner =
        crate::rules::retail_ini_fixture::retail_rules_owner(&assets).with_ares_mix(false);
    let retail = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts()
        .0;
    assert!(!retail.verax());
}
