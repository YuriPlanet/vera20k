//! Phobos `SuppressRevengeWeapons` warhead keys
//! (`Ext/WarheadType/Body.cpp:356-357`, `Body.h:201-202,480-481`).
//!
//! Consumer: `TechnoExt::ApplyRevengeWeapon` (`Ext/Techno/WeaponHelpers.cpp:254-282`),
//! called from `TechnoClass_ReceiveDamage` (`0x702672`). VERA's ported arm is
//! the attach-effect one, `sim::attach_effect::apply_attach_effect_revenge_weapons`;
//! the TechnoType-level `RevengeWeapon` arm is not ported.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// `SuppressRevengeWeapons=` and `SuppressRevengeWeapons.Types=`: the warhead
/// stops the victim's revenge weapons, all of them without a list, or only the
/// listed weapons. Constructor: false and an empty list (`Body.h:480-481`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SuppressRevengeWeaponsRules {
    pub suppress: bool,
    /// Weapon names as the INI spells them; the native check is on the
    /// resolved `WeaponTypeClass*`, so names compare case-insensitively.
    pub types: Vec<String>,
}

impl SuppressRevengeWeaponsRules {
    /// `WarheadTypeExt::LoadFromINI`: the boolean, then the type list.
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            suppress: section.read_bool("SuppressRevengeWeapons", false),
            types: section
                .read_phobos_nullable_type_list("SuppressRevengeWeapons.Types")
                .unwrap_or_default(),
        }
    }

    /// `TechnoExt::ApplyRevengeWeapon`'s gate for one revenge weapon:
    /// `suppress && (!hasFilters || filters.Contains(weapon))`.
    pub fn suppresses(&self, weapon: Option<&str>) -> bool {
        if !self.suppress {
            return false;
        }
        if self.types.is_empty() {
            return true;
        }
        weapon.is_some_and(|name| {
            self.types
                .iter()
                .any(|listed| listed.eq_ignore_ascii_case(name))
        })
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// The two keys and their gate: no filters suppresses every weapon, a
    /// filter list narrows it, and the boolean is the switch.
    #[test]
    fn suppress_revenge_weapons_reads_the_switch_and_the_filter_list() {
        let read = |ini: &str| {
            // An empty section is dropped, so `[WH]` needs one real key.
            let file = IniFile::from_str(ini);
            SuppressRevengeWeaponsRules::from_section(file.section("WH").unwrap())
        };
        let off = read("[WH]\nCellSpread=0\n");
        assert!(!off.suppresses(Some("REVW")));
        let all = read("[WH]\nSuppressRevengeWeapons=yes\n");
        assert!(all.suppresses(Some("REVW")));
        let listed = read("[WH]\nSuppressRevengeWeapons=yes\nSuppressRevengeWeapons.Types=REVW\n");
        assert_eq!(listed.types, vec!["REVW".to_owned()]);
        assert!(listed.suppresses(Some("revw")));
        assert!(!listed.suppresses(Some("OTHERW")));
        // The list alone, without the boolean, suppresses nothing.
        assert!(!read("[WH]\nSuppressRevengeWeapons.Types=REVW\n").suppresses(Some("REVW")));
    }
}
