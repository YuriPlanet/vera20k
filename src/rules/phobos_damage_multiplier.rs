//! Phobos `WarheadTypeExt`'s damage-multiplier block.
//!
//! `WarheadTypeExt::LoadFromINI` reads the eight keys in this order
//! (`Ext/WarheadType/Body.cpp:347-354`); the declarations live at
//! `Ext/WarheadType/Body.h:192-199` and their constructor values at
//! `Body.h:471-478`:
//!
//! ```ini
//! DamageOwnerMultiplier=             ; Nullable<double>, unset
//! DamageAlliesMultiplier=            ; Nullable<double>, unset
//! DamageEnemiesMultiplier=           ; Nullable<double>, unset
//! DamageOwnerMultiplier.Berzerk=     ; Nullable<double>, unset
//! DamageAlliesMultiplier.Berzerk=    ; Nullable<double>, unset
//! DamageEnemiesMultiplier.Berzerk=   ; Nullable<double>, unset
//! DamageSourceHealthMultiplier=0.0   ; Valueable<double>
//! DamageTargetHealthMultiplier=0.0   ; Valueable<double>
//! ```
//!
//! Every nullable falls back to the matching `[CombatDamage]` global, which is
//! `Valueable<double>` default 1.0 (`Ext/Rules/Body.cpp:340-347`,
//! `Ext/Rules/Body.h:248-255`); the owner/ally globals also have
//! `.NotAffectsEnemies` (nullable) and all three have `.Berzerk` (nullable)
//! variants. The consumer is
//! `Ext/Techno/Hooks.ReceiveDamage.cpp:48-82`, a single block inside
//! `TechnoClass_ReceiveDamage_Shield @ 0x701900` that runs after the
//! per-target effects and before the shield stage.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// Which relation between the damage source's House and the victim's House
/// selects the multiplier column (`Ext/Techno/Hooks.ReceiveDamage.cpp:60-74`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamageRelation {
    /// Same House.
    Owner,
    /// Allied Houses.
    Allies,
    /// Everything else, including a hit with no source House at all.
    Enemies,
}

impl DamageRelation {
    /// `!pSourceHouse || !pTargetHouse || !pSourceHouse->IsAlliedWith(pTargetHouse)`
    /// picks the enemy column first, `pSourceHouse != pTargetHouse` the ally
    /// column, and the remainder is the owner column.
    /// `HouseClass::IsAlliedWith` is reflexive, so a same-House hit reaches
    /// the owner test through the allied branch.
    pub fn classify(source_house: Option<u32>, target_house: u32, allied: bool) -> Self {
        match source_house {
            Some(source) if allied && source != target_house => Self::Allies,
            Some(source) if allied && source == target_house => Self::Owner,
            _ => Self::Enemies,
        }
    }
}

/// The eight `WarheadTypeExt` damage-multiplier keys.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DamageMultiplierRules {
    /// `DamageOwnerMultiplier=`: the multiplier for damage to the shooter's
    /// own House (`Hooks.ReceiveDamage.cpp:73-74`).
    pub owner: Option<f64>,
    /// `DamageAlliesMultiplier=`: the multiplier for an allied victim
    /// (`:71-72`).
    pub allies: Option<f64>,
    /// `DamageEnemiesMultiplier=`: the multiplier for an enemy victim, and
    /// for every victim when the shot has no source House (`:69-70`).
    pub enemies: Option<f64>,
    /// `DamageOwnerMultiplier.Berzerk=`: the owner column while the shooter
    /// is berserk (`:66-67`); its fallback chain is the global
    /// `.Berzerk`, then the global plain value.
    pub owner_berzerk: Option<f64>,
    /// `DamageAlliesMultiplier.Berzerk=`: the ally column while the shooter
    /// is berserk (`:64-65`).
    pub allies_berzerk: Option<f64>,
    /// `DamageEnemiesMultiplier.Berzerk=`: the enemy column while the
    /// shooter is berserk (`:62-63`); it has no `NotAffectsEnemies` layer.
    pub enemies_berzerk: Option<f64>,
    /// `DamageSourceHealthMultiplier=` (`Valueable<double>`, default 0.0):
    /// **added** to the multiplier, scaled by the shooter's health fraction
    /// (`:76-77`). A zero value skips the term.
    pub source_health: f64,
    /// `DamageTargetHealthMultiplier=` (`Valueable<double>`, default 0.0):
    /// **added** to the multiplier, scaled by the victim's health fraction
    /// (`:79-80`). A zero value skips the term.
    pub target_health: f64,
}

impl DamageMultiplierRules {
    /// Reads the eight keys in `LoadFromINI` order
    /// (`Ext/WarheadType/Body.cpp:347-354`).
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            owner: section.read_phobos_nullable_double("DamageOwnerMultiplier", None),
            allies: section.read_phobos_nullable_double("DamageAlliesMultiplier", None),
            enemies: section.read_phobos_nullable_double("DamageEnemiesMultiplier", None),
            owner_berzerk: section
                .read_phobos_nullable_double("DamageOwnerMultiplier.Berzerk", None),
            allies_berzerk: section
                .read_phobos_nullable_double("DamageAlliesMultiplier.Berzerk", None),
            enemies_berzerk: section
                .read_phobos_nullable_double("DamageEnemiesMultiplier.Berzerk", None),
            source_health: section.read_phobos_double("DamageSourceHealthMultiplier", 0.0),
            target_health: section.read_phobos_double("DamageTargetHealthMultiplier", 0.0),
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_damage_multiplier_tests.rs"]
mod tests;
