//! VERA's own rules switches, read from the `[VERA]` section.
//!
//! gamemd has no `[VERA]` section: the keys are VERA's own, and a modder or
//! a map sets them in the rules INI, where every peer of a game reads them
//! the same way. Retail data holds none of them, so a retail game plays as
//! `gamemd.exe` plays.
//!
//! The keys are read with the stock integer reader
//! ([`crate::rules::ini_value`]), so `$0F` and `0F` are the same fifteen and
//! a present but unreadable value is a zero, the way the stock reader reads
//! it.

use crate::rules::ini_parser::IniFile;

/// The `[VERA]` keys the simulation reads.
///
/// The switches live here and not in a build feature: a game turns them on
/// from its own data, and a build without the feature keeps every stock
/// path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VeraFeatures {
    /// `SharedGoalField=`: the path batch's shared goal field, which gives
    /// the movers of a mass order one reverse search for their common goal
    /// instead of one search each.
    ///
    /// The field is a guide over the terrain and the routes it produces are
    /// not the stock ones, so a game that turns this on plays differently
    /// from a game that does not.
    pub shared_goal_field: bool,
}

impl VeraFeatures {
    /// The `[VERA]` keys of the rules, an absent section included.
    pub fn from_ini(ini: &IniFile) -> Self {
        let Some(section) = ini.section("VERA") else {
            return Self::default();
        };
        Self {
            shared_goal_field: section.read_int("SharedGoalField", 0) != 0,
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn an_absent_section_leaves_every_switch_off() {
        let ini = IniFile::from_str("[General]\nBuildSpeed=1.0\n");
        assert_eq!(VeraFeatures::from_ini(&ini), VeraFeatures::default());
    }

    #[test]
    fn the_switch_reaches_the_rules_from_a_later_layer() {
        // A map carries VERA's keys the way it carries any other section,
        // so the merged projection is where the switch is read.
        let mut layers = crate::rules::native_processing::RulesLayerStack::new(IniFile::from_str(
            "[General]\nBuildSpeed=1.0\n",
        ));
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str("[VERA]\nSharedGoalField=1\n"),
        );
        let processed = layers
            .process_with_fixed_art(&IniFile::empty())
            .expect("the layers process");
        let rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed)
            .expect("the rules project");
        assert!(rules.shared_goal_field(), "the map's key reaches the rules");
    }

    #[test]
    fn the_shared_goal_field_switch_reads_its_key() {
        let ini = IniFile::from_str("[VERA]\nSharedGoalField=1\n");
        assert!(VeraFeatures::from_ini(&ini).shared_goal_field);
        let ini = IniFile::from_str("[VERA]\nSharedGoalField=0\n");
        assert!(!VeraFeatures::from_ini(&ini).shared_goal_field);
        let ini = IniFile::from_str("[VERA]\nSharedGoalField=off\n");
        assert!(!VeraFeatures::from_ini(&ini).shared_goal_field);
    }
}
