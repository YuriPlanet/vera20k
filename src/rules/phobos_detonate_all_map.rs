//! Phobos `DetonateOnAllMapObjects.*` warhead keys
//! (`Ext/WarheadType/Body.cpp:304-319`; fields `Ext/WarheadType/Body.h:153-159`;
//! constructor defaults `Body.h:432-438`).
//!
//! Consumer: `BulletClass_Logics_DetonateOnAllMapObjects @ 0x4690C1`
//! (`Ext/Bullet/Hooks.DetonateLogics.cpp:78-168`) and
//! `WarheadTypeExt::EligibleForFullMapDetonation`
//! (`Ext/WarheadType/Body.cpp:110-134`). The bullet detonation owner is
//! `sim::combat::world_receiver::emit_detonation_receivers`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_crit::AffectedTarget;
use crate::rules::phobos_shields::AffectedHouse;

/// The seven `Valueable` keys (two pairs are deprecated aliases).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DetonateOnAllMapObjectsRules {
    /// `DetonateOnAllMapObjects=`, `Valueable<bool>`, default `no`.
    pub enabled: bool,
    /// `.Full=`, default **`yes`**: each eligible target takes a full
    /// detonation instead of a plain area-damage packet.
    pub full: bool,
    /// `.RequireVerses=`, default `no`: skip a target the warhead's Verses
    /// rate reduces to 0%.
    pub require_verses: bool,
    /// `.AffectsTarget=` (read after the deprecated `.AffectTargets=`),
    /// `Valueable<AffectedTarget>`, default `None`.
    pub affects_target: AffectedTarget,
    /// `.AffectsHouse=` (read after the deprecated `.AffectHouses=`),
    /// `Valueable<AffectedHouse>`, default `None`.
    pub affects_house: AffectedHouse,
    /// `.AffectTypes=`, `ValueableVector<TechnoTypeClass*>`.
    pub affect_types: Vec<String>,
    /// `.IgnoreTypes=`, `ValueableVector<TechnoTypeClass*>`.
    pub ignore_types: Vec<String>,
}

impl Default for DetonateOnAllMapObjectsRules {
    fn default() -> Self {
        Self {
            enabled: false,
            full: true,
            require_verses: false,
            affects_target: AffectedTarget::NONE,
            affects_house: AffectedHouse::NONE,
            affect_types: Vec::new(),
            ignore_types: Vec::new(),
        }
    }
}

impl DetonateOnAllMapObjectsRules {
    pub fn from_section(section: &IniSection) -> Self {
        use crate::rules::native_processing::read_nullable_type_vector;
        // `ValueableVector<TechnoTypeClass*>` drops unresolvable names at read
        // time natively; the names stay here until the consumer resolves them.
        let keep = |name: &str| Some(name.to_owned());
        let mut rules = Self {
            enabled: section.read_bool("DetonateOnAllMapObjects", false),
            full: section.read_bool("DetonateOnAllMapObjects.Full", true),
            require_verses: section.read_bool("DetonateOnAllMapObjects.RequireVerses", false),
            affects_target: AffectedTarget::NONE,
            affects_house: AffectedHouse::NONE,
            affect_types: read_nullable_type_vector(
                section,
                "DetonateOnAllMapObjects.AffectTypes",
                None,
                keep,
            )
            .unwrap_or_default(),
            ignore_types: read_nullable_type_vector(
                section,
                "DetonateOnAllMapObjects.IgnoreTypes",
                None,
                keep,
            )
            .unwrap_or_default(),
        };
        // The deprecated spellings are read first so a later real key wins
        // (`Body.cpp:307-319` reads both into the same field).
        rules.affects_target = crate::rules::phobos_crit::read_affected_target(
            section,
            "DetonateOnAllMapObjects.AffectTargets",
            rules.affects_target,
        );
        rules.affects_target = crate::rules::phobos_crit::read_affected_target(
            section,
            "DetonateOnAllMapObjects.AffectsTarget",
            rules.affects_target,
        );
        rules.affects_house = crate::rules::phobos_shields::read_affected_house(
            section,
            "DetonateOnAllMapObjects.AffectHouses",
            rules.affects_house,
        );
        rules.affects_house = crate::rules::phobos_shields::read_affected_house(
            section,
            "DetonateOnAllMapObjects.AffectsHouse",
            rules.affects_house,
        );
        rules
    }

    /// The gate at the head of the native hook (`Hooks.DetonateLogics.cpp:88-90`).
    pub fn claims_detonation(&self) -> bool {
        self.enabled
            && self.affects_target != AffectedTarget::NONE
            && self.affects_house != AffectedHouse::NONE
    }

    /// The per-category flag test used by the hook's four walks.
    pub fn affects(&self, category: AffectedTarget) -> bool {
        self.affects_target.contains(category)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_detonate_all_map_tests.rs"]
mod tests;
