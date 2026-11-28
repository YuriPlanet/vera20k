//! Phobos `Detonate.*` superweapon keys (`Ext/SWType/Body.cpp:274-278`,
//! consumer `SWTypeExt::ApplyDetonation` `Ext/SWType/FireSuperWeapon.cpp:267-303`).
//!
//! `Detonate.Warhead=` and `Detonate.Weapon=` are `Valueable` type pointers
//! read with `FindOrAllocate` (`Read<true>`); this owner keeps the names and
//! resolves them at the call site. `.Damage` is a `Nullable<int>` whose unset
//! value is the weapon's damage (0 on the warhead arm), `.Warhead.Full` a
//! `Valueable<bool>` default true (a bullet explosion instead of
//! `MapClass::DamageArea`) and `.AtFirer` a `Valueable<bool>` default false
//! (detonate at the launching building's centre instead of the target cell).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PhobosSuperWeaponDetonate {
    warhead: Option<String>,
    weapon: Option<String>,
    damage: Option<i32>,
    warhead_full: bool,
    at_firer: bool,
}

impl PhobosSuperWeaponDetonate {
    /// One reached rules pass: an absent key keeps the earlier value
    /// (`Valueable`/`Nullable::Read` semantics); a present one replaces it.
    pub(crate) fn read_pass(&mut self, section: &IniSection) {
        if section.is_present("Detonate.Warhead") {
            self.warhead = section
                .read_type_name("Detonate.Warhead", 0x100)
                .map(str::to_owned);
        }
        if section.is_present("Detonate.Weapon") {
            self.weapon = section
                .read_type_name("Detonate.Weapon", 0x100)
                .map(str::to_owned);
        }
        if section.is_present("Detonate.Damage") {
            self.damage = section.read_phobos_nullable_int("Detonate.Damage", None);
        }
        self.warhead_full = section.read_bool("Detonate.Warhead.Full", self.warhead_full);
        self.at_firer = section.read_bool("Detonate.AtFirer", self.at_firer);
    }

    pub(crate) fn warhead(&self) -> Option<&str> {
        self.warhead.as_deref()
    }

    pub(crate) fn weapon(&self) -> Option<&str> {
        self.weapon.as_deref()
    }

    pub(crate) fn damage(&self) -> Option<i32> {
        self.damage
    }

    pub(crate) fn warhead_full(&self) -> bool {
        self.warhead_full
    }

    pub(crate) fn at_firer(&self) -> bool {
        self.at_firer
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.warhead.is_none() && self.weapon.is_none()
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// A later pass without the keys keeps the earlier group; a present
    /// `Detonate.Damage=<default>` clears only the damage.
    #[test]
    fn pass_reads_retain_absent_keys() {
        let first = IniFile::from_str(
            "[SW]\nDetonate.Warhead=WH\nDetonate.Weapon=GUN\nDetonate.Damage=40\n\
             Detonate.Warhead.Full=no\nDetonate.AtFirer=yes\n",
        );
        let mut rules = PhobosSuperWeaponDetonate::default();
        rules.read_pass(first.section_or_empty("SW"));
        assert_eq!(rules.warhead(), Some("WH"));
        assert_eq!(rules.weapon(), Some("GUN"));
        assert_eq!(rules.damage(), Some(40));
        assert!(!rules.warhead_full());
        assert!(rules.at_firer());

        let absent = IniFile::from_str("[SW]\nName=unrelated\n");
        rules.read_pass(absent.section_or_empty("SW"));
        assert_eq!(rules.warhead(), Some("WH"), "absent keeps the warhead");
        assert_eq!(rules.weapon(), Some("GUN"), "absent keeps the weapon");
        assert_eq!(rules.damage(), Some(40), "absent keeps the damage");
        assert!(!rules.warhead_full());
        assert!(rules.at_firer());

        let reset = IniFile::from_str("[SW]\nDetonate.Damage=<default>\n");
        rules.read_pass(reset.section_or_empty("SW"));
        assert_eq!(rules.damage(), None, "<default> clears the damage");
        assert_eq!(rules.warhead(), Some("WH"), "other keys stay");
    }
}
