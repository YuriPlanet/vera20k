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
//! Stored as raw INI strings here; resolved to interned `WarheadId`s at world
//! init time when the warhead registry is populated.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// Default warhead names for bridge-related combat from `[CombatDamage]`.
///
/// Each field is the section name of a `WarheadType` (resolved later against
/// `RuleSet::warhead_id_by_name`). IonCannon starts null (Rules ctor666BD5); other legacy defaults remain.
#[derive(Debug, Clone)]
pub struct BridgeWarheads {
    /// `[CombatDamage] IonCannonWarhead=` (constructor default null).
    /// Bypasses BridgeStrength RNG gate; enables retry loop.
    pub ion_cannon_name: String,
    /// `[CombatDamage] C4Warhead=` (default `"Super"`).
    /// Used as killing warhead in bridge-collapse ground kill.
    pub c4_name: String,
    /// `[CombatDamage] CrushWarhead=` (default `"Crush"`). Distinct from C4.
    pub crush_name: String,
}

impl Default for BridgeWarheads {
    fn default() -> Self {
        Self {
            ion_cannon_name: String::new(),
            c4_name: "Super".to_string(),
            crush_name: "Crush".to_string(),
        }
    }
}

impl BridgeWarheads {
    /// The ordered Process owner supplies the retained IonCannon reference.
    /// The legacy C4/Crush projections still read their CombatDamage keys.
    pub fn from_ini_section(section: &IniSection, ion_cannon_name: &str) -> Self {
        let default = Self::default();
        // ReadString0x80 before C4/Crush lookup (66C327/66C365).
        // IonCannon66CA96 belongs to NativeRulesRegistryState; do not
        // reread a final INI and erase its current-pointer default here.
        let name = |key: &str, default: String| {
            section.read_name(key, 0x80).map_or(default, str::to_string)
        };
        Self {
            ion_cannon_name: ion_cannon_name.to_string(),
            c4_name: name("C4Warhead", default.c4_name),
            crush_name: name("CrushWarhead", default.crush_name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn ion_default_is_native_null() {
        let bw = BridgeWarheads::default();
        assert!(bw.ion_cannon_name.is_empty());
        assert_eq!(bw.c4_name, "Super");
        assert_eq!(bw.crush_name, "Crush");
    }

    #[test]
    fn c4_and_crush_parse_while_ion_uses_the_process_projection() {
        let ini = IniFile::from_str(
            "[CombatDamage]\nIonCannonWarhead=CustomIon\nC4Warhead=CustomC4\nCrushWarhead=CustomCrush\n",
        );
        let section = ini.section("CombatDamage").unwrap();
        let bw = BridgeWarheads::from_ini_section(section, "RetainedIon");
        assert_eq!(bw.ion_cannon_name, "RetainedIon");
        assert_eq!(bw.c4_name, "CustomC4");
        assert_eq!(bw.crush_name, "CustomCrush");
    }

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let ini = IniFile::from_str("[CombatDamage]\nFixtureOnly=1\n");
        let section = ini.section("CombatDamage").unwrap();
        let bw = BridgeWarheads::from_ini_section(section, "");
        assert!(bw.ion_cannon_name.is_empty());
        assert_eq!(bw.c4_name, "Super");
        assert_eq!(bw.crush_name, "Crush");
    }
}
