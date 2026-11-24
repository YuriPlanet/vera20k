//! Phobos `SelfHealGainType=` on a TechnoType: which `InfantryGainSelfHeal` or
//! `UnitsGainSelfHeal` house capacity a techno draws on.
//!
//! ```ini
//! ; rules section of the TechnoType
//! SelfHealGainType=   ; Nullable<SelfHealGainType>, unset   Ext/TechnoType/Body.cpp:957
//! ```
//!
//! Declared `Ext/TechnoType/Body.h:201`, constructed unset `Body.h:629`. The
//! value table and case-insensitive comparison are
//! `Utilities/TemplateDef.h:1008-1030`; an unknown token is a parse failure
//! that leaves the nullable at its prior value.
//!
//! Consumer: `TechnoExt::ApplyGainedSelfHeal` (`Ext/Techno/Body.Update.cpp:
//! 591-705`), installed over the stock block at `TechnoClass_AI_SelfHealGain`
//! (`0x6FA793`, `Ext/Techno/Hooks.cpp:146-155`, skip to `0x6FA941`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

/// `SelfHealGainType` (`Utilities/Enum.h:210-215`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelfHealGainType {
    NoHeal,
    Infantry,
    Units,
}

impl SelfHealGainType {
    /// The fixed, case-insensitive name table; an unknown token is `None`.
    pub fn parse(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("noheal") {
            Some(Self::NoHeal)
        } else if name.eq_ignore_ascii_case("infantry") {
            Some(Self::Infantry)
        } else if name.eq_ignore_ascii_case("units") {
            Some(Self::Units)
        } else {
            None
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::SelfHealGainType;

    /// `Utilities/TemplateDef.h:1008-1030`: the fixed name table is
    /// case-insensitive; an unknown token is a parse failure.
    #[test]
    fn the_enum_names_are_case_insensitive_and_unknowns_fail() {
        assert_eq!(
            SelfHealGainType::parse("noheal"),
            Some(SelfHealGainType::NoHeal)
        );
        assert_eq!(
            SelfHealGainType::parse("INFANTRY"),
            Some(SelfHealGainType::Infantry)
        );
        assert_eq!(
            SelfHealGainType::parse("Units"),
            Some(SelfHealGainType::Units)
        );
        assert_eq!(SelfHealGainType::parse("bogus"), None);
        assert_eq!(SelfHealGainType::parse(""), None);
    }

    /// `Ext/TechnoType/Body.cpp:957`: the type key reads from its rules
    /// section; an unknown later layer keeps the earlier value.
    #[test]
    fn the_object_type_reads_the_key_and_keeps_unset() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[VehicleTypes]\n0=MTNK\n1=MTNK2\n\
                 [MTNK]\nStrength=100\nSelfHealGainType=Units\n\
                 [MTNK2]\nStrength=100\n",
            ))
            .unwrap();
        assert_eq!(
            rules.object("MTNK").unwrap().phobos_self_heal_gain_type,
            Some(SelfHealGainType::Units)
        );
        assert_eq!(
            rules.object("MTNK2").unwrap().phobos_self_heal_gain_type,
            None
        );
    }
}
