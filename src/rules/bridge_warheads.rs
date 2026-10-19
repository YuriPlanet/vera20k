//! `[CombatDamage]` fixed warhead references used by bridge/combat routing.
//!
//! Bridge damage gating + collapse fallout cite two specific warhead names by
//! [CombatDamage] key:
//! - IonCannonWarhead: bypasses BridgeStrength RNG gate; enables 3-retry loop in
//!   Apply_area_damage (gamemd Rules+0xFF0).
//! - C4Warhead: used as the killing warhead in BlowUpBridge ground-occupant
//!   force_kill (gamemd Rules+0xFA8).
//! - CrushWarhead: admits the source object to Apply_area_damage's ground
//!   receiver list even when its ObjectType has DamageSelf=no (Rules+0xFAC).
//!
//! Each is the stored ID the ordered Rules processor's WarheadType factory
//! answered (C4 and Crush in `rules_type_references::RulesTypeReferences`,
//! IonCannon with the ion blast inputs), which `RuleSet` publishes; resolved
//! to interned `WarheadId`s at world init time when the warhead registry is
//! populated.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

/// Warhead names for bridge-related combat from `[CombatDamage]`.
///
/// Each field is the stored ID of a `WarheadType` (resolved later against
/// `RuleSet::warhead_id_by_name`). The RulesClass constructor leaves each
/// pointer null (`0x00666B52`, `0x00666B58`, `0x00666BD5`), an empty name
/// here, as does a `none` read; retail names all three.
#[derive(Debug, Clone, Default)]
pub struct BridgeWarheads {
    /// `[CombatDamage] IonCannonWarhead=` (retail `IonCannonWH`).
    /// Bypasses BridgeStrength RNG gate; enables retry loop.
    pub ion_cannon_name: String,
    /// `[CombatDamage] C4Warhead=` (retail `Super`).
    /// Used as killing warhead in bridge-collapse ground kill.
    pub c4_name: String,
    /// `[CombatDamage] CrushWarhead=` (retail `Crush`). Distinct from C4.
    pub crush_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    fn read(text: &str) -> BridgeWarheads {
        crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(text))
            .unwrap()
            .bridge_warheads
    }

    #[test]
    fn parses_keys_from_combat_damage_section() {
        let bw = read(
            "[CombatDamage]\nIonCannonWarhead=CustomIon\nC4Warhead=CustomC4\nCrushWarhead=CustomCrush\n",
        );
        assert_eq!(bw.ion_cannon_name, "CustomIon");
        assert_eq!(bw.c4_name, "CustomC4");
        assert_eq!(bw.crush_name, "CustomCrush");
    }

    /// The RulesClass constructor's nulls stay null without the keys, and
    /// the WarheadType factory answers `none` null: VERA no longer invents
    /// `IonCannonWH`, `Super` and `Crush`.
    #[test]
    fn missing_or_none_keys_stay_null() {
        for text in [
            "[CombatDamage]\nFixtureOnly=1\n",
            "[CombatDamage]\nIonCannonWarhead=none\nC4Warhead=<none>\nCrushWarhead=NONE\n",
        ] {
            let bw = read(text);
            assert_eq!(bw.ion_cannon_name, "", "{text}");
            assert_eq!(bw.c4_name, "", "{text}");
            assert_eq!(bw.crush_name, "", "{text}");
        }
    }
}
