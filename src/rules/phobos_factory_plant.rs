//! Phobos `FactoryPlant.AllowTypes` / `.DisallowTypes` / `.MaxCount`
//! (`Ext/BuildingType/Body.cpp:224-226`, declarations
//! `Ext/BuildingType/Body.h:74-76`, constructors `:199-201`).
//!
//! All three are `Valueable`: a present list key clears the old vector and
//! re-parses it, an absent key keeps the constructor value, and an unknown
//! type token is dropped by `Parser<TechnoTypeClass*>::Parse`
//! (`Utilities/TemplateDef.h:1728-1742`). `ValueableVector::Read` clears and
//! re-parses only when `INI_EX::ReadString` succeeds
//! (`Utilities/TemplateDef.h:1947-1955`), so `read_list`'s `None` is the
//! "keep the prior value" arm.

use crate::rules::ini_parser::IniSection;
use std::collections::HashSet;

/// The three BuildingType keys. `max_count` is the constructor's `-1`
/// (`Ext/BuildingType/Body.h:201`), not `Default`'s zero.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FactoryPlantRestrictions {
    /// `FactoryPlant.AllowTypes=`: an empty list means "no allow filter".
    pub allow: Vec<String>,
    /// `FactoryPlant.DisallowTypes=`: an empty list means "no deny filter".
    pub disallow: Vec<String>,
    /// `FactoryPlant.MaxCount=`: `-1` disables the per-type cap.
    pub max_count: i32,
}

impl Default for FactoryPlantRestrictions {
    fn default() -> Self {
        Self {
            allow: Vec::new(),
            disallow: Vec::new(),
            max_count: -1,
        }
    }
}

impl FactoryPlantRestrictions {
    /// `ValueableVector<TechnoTypeClass*>` reads with a 0x800 buffer; the
    /// resolved pointers are projected here as type names until the RuleSet
    /// post-read pass drops the unknown ones ([`Self::retain_known`]).
    pub fn from_section(section: &IniSection) -> Self {
        let mut value = Self::default();
        if let Some(tokens) = section.read_list("FactoryPlant.AllowTypes", 0x800) {
            value.allow = tokens.into_iter().map(str::to_owned).collect();
        }
        if let Some(tokens) = section.read_list("FactoryPlant.DisallowTypes", 0x800) {
            value.disallow = tokens.into_iter().map(str::to_owned).collect();
        }
        // `Valueable<int>`: a failed Parser<int> keeps the constructor -1.
        value.max_count = section.read_ares_int("FactoryPlant.MaxCount", value.max_count);
        value
    }

    /// `BuildingClass_Unlimbo_FactoryPlant @ 0x441501`
    /// (`Ext/Building/Hooks.cpp:437-454`): either non-empty list sends the
    /// building to the House's restricted vector instead of House+140.
    pub fn restricted(&self) -> bool {
        !self.allow.is_empty() || !self.disallow.is_empty()
    }

    /// `TechnoTypeClass::Find` resolves each token case-insensitively and
    /// drops an unknown one before `ValueableVector::size()` is ever asked.
    pub(crate) fn retain_known(&mut self, known: &HashSet<String>) {
        let resolves = |name: &str| known.contains(&name.to_ascii_uppercase());
        self.allow.retain(|name| resolves(name));
        self.disallow.retain(|name| resolves(name));
    }
}
