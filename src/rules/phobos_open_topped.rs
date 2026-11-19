//! Phobos `OpenTopped.*` TechnoType keys and their `[General]` / YR
//! `[CombatDamage]` fallbacks.
//!
//! The eleven type keys are read by `TechnoTypeExt::LoadFromINIFile`
//! (`src/Ext/TechnoType/Body.cpp:879-889`), declared `Nullable` at
//! `Body.h:149-159` and constructed `{}` (unset) at `Body.h:546-556`. There
//! are two distinct fallback chains:
//!
//! * The three numeric keys fall back to the **YR engine** `[CombatDamage]`
//!   fields `OpenToppedRangeBonus`, `OpenToppedDamageMultiplier` and
//!   `OpenToppedWarpDistance` (`RulesClass::Instance`). Phobos reads no global
//!   for them and sets no default; `.Get(global)` is evaluated at the hook.
//! * The eight boolean keys fall back to the Phobos `[General]` `Valueable`
//!   globals of the same name (`Ext/Rules/Body.cpp:455-462`, declared
//!   `Rules/Body.h:365-372`, defaults `:870-877`). A `Valueable` global has
//!   no unset state and no clearing token.
//!
//! `UseTransportRangeModifiers` and `CheckTransportDisableWeapons` are both
//! type keys *and* `[General]` globals (`Ext/Rules/Body.cpp:459-460`). VERA
//! already reads those two globals into `AttachEffectGlobals` for earlier
//! features; the eight `PhobosGlobals` reads here are the authoritative
//! fallbacks for the new consumers, with identical `read_bool` defaults.
//!
//! ## Consumers (native addresses)
//!
//! * `RangeBonus` -> `TechnoClass_IsCloseEnoughToTarget_OpenTopped_RangeBonus
//!   @ 0x6F72D2` (`Ext/Techno/Hooks.Transport.cpp:167-183`).
//! * `DamageMultiplier` -> `TechnoClass_FireAt_OpenToppedDmgMult @ 0x6FE43B`
//!   (`Ext/Techno/Hooks.Firing.cpp:714-741`) and the separate
//!   `TechnoExt::GetCurrentFirepowerMultiplier` (`Ext/Techno/Body.cpp:208-236`).
//! * `WarpDistance` -> `TemporalClass_AI_Opentopped_WarpDistance @ 0x71A82C`
//!   (`Ext/Techno/Hooks.Transport.cpp:185-197`).
//! * `IgnoreRangefinding` -> `TechnoClass_WeaponRange @ 0x7012C2`
//!   (`Ext/Techno/Hooks.WeaponRange.cpp:4-49`).
//! * The five firing-gate keys -> `TechnoClass_CanFire_OpenTopped @ 0x6FC5C7`
//!   (`Ext/Techno/Hooks.Firing.cpp:439-482`).
//! * `ShareTransportTarget` ->
//!   `TechnoClass_SetOpenTransportCargoTarget_ShareTarget @ 0x710552`
//!   (`Ext/Techno/Hooks.Transport.cpp:199-215`).
//! * `DecloakToFire` ->
//!   `TechnoClass_CanFire_RevertAresOpenTopCloakFix @ 0x6FCA26` and
//!   `TechnoClass_CanFire_OpenTopCloakFix @ 0x6FCD1D`
//!   (`Ext/Techno/Hooks.Cloak.cpp:179-214`).
//! * `UseTransportRangeModifiers` is the already-ported
//!   `WeaponTypeExt::GetRangeWithModifiers` (`Ext/WeaponType/Body.cpp:398-416`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ruleset::RuleSet;

/// The eleven `TechnoTypeExt` OpenTopped fields, all `Nullable` (unset at
/// construction). `damage_multiplier` is `Nullable<float>` (`Body.h:150`):
/// the reader yields `f64` and narrows it to `f32`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PhobosOpenTopped {
    /// `OpenTopped.RangeBonus=` (`Body.cpp:879`), cells.
    pub range_bonus: Option<i32>,
    /// `OpenTopped.DamageMultiplier=` (`Body.cpp:880`).
    pub damage_multiplier: Option<f32>,
    /// `OpenTopped.WarpDistance=` (`Body.cpp:881`), cells.
    pub warp_distance: Option<i32>,
    /// `OpenTopped.IgnoreRangefinding=` (`Body.cpp:882`).
    pub ignore_rangefinding: Option<bool>,
    /// `OpenTopped.AllowFiringIfDeactivated=` (`Body.cpp:883`).
    pub allow_firing_if_deactivated: Option<bool>,
    /// `OpenTopped.AllowFiringIfAttackedByLocomotor=` (`Body.cpp:884`).
    pub allow_firing_if_attacked_by_locomotor: Option<bool>,
    /// `OpenTopped.ShareTransportTarget=` (`Body.cpp:885`).
    pub share_transport_target: Option<bool>,
    /// `OpenTopped.UseTransportRangeModifiers=` (`Body.cpp:886`).
    pub use_transport_range_modifiers: Option<bool>,
    /// `OpenTopped.CheckTransportDisableWeapons=` (`Body.cpp:887`).
    pub check_transport_disable_weapons: Option<bool>,
    /// `OpenTopped.DecloakToFire=` (`Body.cpp:888`).
    pub decloak_to_fire: Option<bool>,
    /// `OpenTopped.FireWhileMoving=` (`Body.cpp:889`).
    pub fire_while_moving: Option<bool>,
}

impl PhobosOpenTopped {
    /// The reads of `TechnoTypeExt::LoadFromINIFile`
    /// (`Ext/TechnoType/Body.cpp:879-889`). Every field is `Nullable`, so an
    /// absent key stays unset and `<default>`/`none`/`<none>` clear it.
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            range_bonus: section.read_phobos_nullable_int("OpenTopped.RangeBonus", None),
            damage_multiplier: section
                .read_phobos_nullable_double("OpenTopped.DamageMultiplier", None)
                .map(|value| value as f32),
            warp_distance: section.read_phobos_nullable_int("OpenTopped.WarpDistance", None),
            ignore_rangefinding: section
                .read_phobos_nullable_bool("OpenTopped.IgnoreRangefinding", None),
            allow_firing_if_deactivated: section
                .read_phobos_nullable_bool("OpenTopped.AllowFiringIfDeactivated", None),
            allow_firing_if_attacked_by_locomotor: section
                .read_phobos_nullable_bool("OpenTopped.AllowFiringIfAttackedByLocomotor", None),
            share_transport_target: section
                .read_phobos_nullable_bool("OpenTopped.ShareTransportTarget", None),
            use_transport_range_modifiers: section
                .read_phobos_nullable_bool("OpenTopped.UseTransportRangeModifiers", None),
            check_transport_disable_weapons: section
                .read_phobos_nullable_bool("OpenTopped.CheckTransportDisableWeapons", None),
            decloak_to_fire: section.read_phobos_nullable_bool("OpenTopped.DecloakToFire", None),
            fire_while_moving: section
                .read_phobos_nullable_bool("OpenTopped.FireWhileMoving", None),
        }
    }

    /// `Nullable<int>::Get(RulesClass::Instance->OpenToppedRangeBonus)` at
    /// `0x6F72D2` (`Hooks.Transport.cpp:176`). Falls back to the YR
    /// `[CombatDamage] OpenToppedRangeBonus=` (constructor default 2 cells).
    pub fn range_bonus(&self, rules: &RuleSet) -> i32 {
        self.range_bonus
            .unwrap_or(rules.garrison_rules.open_topped_range_bonus)
    }

    /// `Nullable<float>::Get(RulesClass::Instance->OpenToppedDamageMultiplier)`
    /// at `0x6FE43B` (`Hooks.Firing.cpp:724-731`). Falls back to the YR
    /// `[CombatDamage] OpenToppedDamageMultiplier=` (constructor default 1.0).
    pub fn damage_multiplier(&self, rules: &RuleSet) -> f32 {
        self.damage_multiplier
            .unwrap_or(rules.garrison_rules.open_topped_damage_multiplier)
    }

    /// `Nullable<int>::Get(RulesClass::Instance->OpenToppedWarpDistance)` at
    /// `0x71A82C` (`Hooks.Transport.cpp:192`). Falls back to the YR
    /// `[CombatDamage] OpenToppedWarpDistance=` (constructor default 5 cells).
    pub fn warp_distance(&self, rules: &RuleSet) -> i32 {
        self.warp_distance
            .unwrap_or(rules.combat_damage.open_topped_warp_distance)
    }

    /// `Nullable<bool>::Get(RulesExt::Global()->...)` for the eight `Valueable`
    /// `[General]` switches; the remaining helpers share this pattern.
    pub fn ignore_rangefinding(&self, rules: &RuleSet) -> bool {
        self.ignore_rangefinding
            .unwrap_or(rules.phobos_globals.open_topped_ignore_rangefinding)
    }

    pub fn allow_firing_if_deactivated(&self, rules: &RuleSet) -> bool {
        self.allow_firing_if_deactivated
            .unwrap_or(rules.phobos_globals.open_topped_allow_firing_if_deactivated)
    }

    pub fn allow_firing_if_attacked_by_locomotor(&self, rules: &RuleSet) -> bool {
        self.allow_firing_if_attacked_by_locomotor.unwrap_or(
            rules
                .phobos_globals
                .open_topped_allow_firing_if_attacked_by_locomotor,
        )
    }

    pub fn share_transport_target(&self, rules: &RuleSet) -> bool {
        self.share_transport_target
            .unwrap_or(rules.phobos_globals.open_topped_share_transport_target)
    }

    pub fn use_transport_range_modifiers(&self, rules: &RuleSet) -> bool {
        self.use_transport_range_modifiers.unwrap_or(
            rules
                .phobos_globals
                .open_topped_use_transport_range_modifiers,
        )
    }

    pub fn check_transport_disable_weapons(&self, rules: &RuleSet) -> bool {
        self.check_transport_disable_weapons.unwrap_or(
            rules
                .phobos_globals
                .open_topped_check_transport_disable_weapons,
        )
    }

    pub fn decloak_to_fire(&self, rules: &RuleSet) -> bool {
        self.decloak_to_fire
            .unwrap_or(rules.phobos_globals.open_topped_decloak_to_fire)
    }

    pub fn fire_while_moving(&self, rules: &RuleSet) -> bool {
        self.fire_while_moving
            .unwrap_or(rules.phobos_globals.open_topped_fire_while_moving)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_open_topped_tests.rs"]
mod tests;
