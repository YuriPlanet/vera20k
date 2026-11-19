//! Phobos `WarheadTypeExt` per-target keys that no other warhead owner reads.
//!
//! Two groups, both consumed while a detonation walks the targets it reaches
//! (`WarheadTypeExt::DetonateOnOneUnit`, `Ext/WarheadType/Detonate.cpp:197-231`):
//!
//! ```ini
//! RemoveMindControl=            ; Valueable<bool>, no
//! RemoveMindControl.Silent=     ; Nullable<bool>, unset
//! ShakeIsLocal=                 ; Nullable<bool>, unset
//! BuildingSell=                 ; Valueable<bool>, no
//! BuildingSell.IgnoreUnsellable=; Valueable<bool>, no
//! BuildingUndeploy=             ; Valueable<bool>, no
//! BuildingUndeploy.Leave=       ; Valueable<bool>, no
//! ```
//!
//! plus `MergeBuildingDamage=`, a `Nullable<bool>` read at
//! `Ext/WarheadType/Body.cpp:339` and consumed by the area-damage sweep
//! (`Ext/WarheadType/Hooks.cpp:412-487`) rather than per target.
//!
//! Read order is `LoadFromINI`'s: `RemoveMindControl` and its `.Silent`
//! (`Body.cpp:178-179`, declarations `Body.h:49-50`, constructor
//! `Body.h:328-329`), `ShakeIsLocal` (`:184`, `Body.h:55`, constructor
//! `Body.h:334`), `MergeBuildingDamage` (`:339`, `Body.h:184`, constructor
//! `:463`) and the four sell/undeploy keys (`:362-365`, `Body.h:207-210`,
//! constructor `:486-489`).
//!
//! Three keys have `[Rules]` fallbacks:
//! - `MergeBuildingDamage` falls back to `[CombatDamage] MergeBuildingDamage`
//!   (`Ext/Rules/Body.cpp:503`, constructor `Ext/Rules/Body.h:918`, default
//!   `no`).
//! - `RemoveMindControl.Silent` falls back to
//!   `[AudioVisual] RemoveMindControl.Silent` (`Ext/Rules/Body.cpp:597`,
//!   constructor `Ext/Rules/Body.h:998`, default `no`).
//! - `ShakeIsLocal` falls back to `[General] ShakeIsLocal`
//!   (`Ext/Rules/Body.cpp:491`, constructor `Ext/Rules/Body.h:906`,
//!   default `no`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The eight keys owned here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PhobosWarheadExtras {
    /// `[WarheadType] Particle.AlphaImageIsLightFlash=`
    /// (`Ext/WarheadType/Body.cpp:345`, `Nullable<bool>`): the particles this
    /// warhead's `Particle=` system spawns are light-flash alpha images, gated by
    /// `LightFlashAlphaImageDetailLevel` at `0x005F5053` (`Ext/Anim/Hooks.cpp`
    /// neighbours, `Misc/Hooks.LightEffects.cpp:12-50`). Unset falls back to
    /// `[AudioVisual] WarheadParticleAlphaImageIsLightFlash=`.
    pub particle_alpha_image_is_light_flash: Option<bool>,
    /// `RemoveMindControl=` (`Body.cpp:178`), default `no`: a target this
    /// warhead reaches is freed from mind control before every other
    /// per-target effect (`Detonate.cpp:207-208`).
    pub remove_mind_control: bool,
    /// `RemoveMindControl.Silent=` (`Body.cpp:179`), `Nullable`: frees the
    /// target without its `MindClearedSound`. `None` reads the
    /// `[AudioVisual]` global.
    pub remove_mind_control_silent: Option<bool>,
    /// `ShakeIsLocal=` (`Body.cpp:184`), `Nullable<bool>`: a detonation
    /// outside the local tactical view skips its screen-shake writes.
    /// `None` reads the `[General]` global (`Ext/Rules/Body.cpp:491`).
    pub shake_is_local: Option<bool>,
    /// `MergeBuildingDamage=` (`Body.cpp:339`), `Nullable`: one building hit
    /// by several cells of the same area damage takes a single receiver call
    /// carrying the summed distance multipliers. `None` reads the
    /// `[CombatDamage]` global.
    pub merge_building_damage: Option<bool>,
    /// `BuildingSell=` (`Body.cpp:362`), default `no`: a hit building is sold
    /// (`Detonate.cpp:274-278`). It outranks `BuildingUndeploy=`.
    pub building_sell: bool,
    /// `BuildingSell.IgnoreUnsellable=` (`Body.cpp:363`), default `no`: sell
    /// the building even when `CanBeSold()` refuses or it is strange.
    pub building_sell_ignore_unsellable: bool,
    /// `BuildingUndeploy=` (`Body.cpp:364`), default `no`: a hit building
    /// packs up into its `UndeploysInto=` unit (`Detonate.cpp:304`).
    pub building_undeploy: bool,
    /// `BuildingUndeploy.Leave=` (`Body.cpp:365`), default `no`: the
    /// undeployment leaves a building behind at the cell instead of nothing.
    pub building_undeploy_leave: bool,
    /// `CLIsBlack=` (`Body.cpp:344`), default `no`: the combat light this
    /// warhead's detonation writes carries `SpotlightFlags::NoColor`
    /// (`Misc/Hooks.LightEffects.cpp:79-80`).
    pub cl_is_black: bool,
    /// `CombatLightDetailLevel.CheckColored=` (`Body.cpp:342`), `Nullable`:
    /// overrides the `[AudioVisual]` global for this warhead's flashes
    /// (`Misc/Hooks.LightEffects.cpp:77`).
    pub combat_light_detail_level_check_colored: Option<bool>,
}

impl PhobosWarheadExtras {
    /// Reads the seven keys in `LoadFromINI` order
    /// (`Ext/WarheadType/Body.cpp:178-179`, `:339`, `:362-365`).
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            remove_mind_control: section.read_bool("RemoveMindControl", false),
            remove_mind_control_silent: section
                .read_phobos_nullable_bool("RemoveMindControl.Silent", None),
            shake_is_local: section.read_phobos_nullable_bool("ShakeIsLocal", None),
            merge_building_damage: section.read_phobos_nullable_bool("MergeBuildingDamage", None),
            building_sell: section.read_bool("BuildingSell", false),
            building_sell_ignore_unsellable: section
                .read_bool("BuildingSell.IgnoreUnsellable", false),
            building_undeploy: section.read_bool("BuildingUndeploy", false),
            building_undeploy_leave: section.read_bool("BuildingUndeploy.Leave", false),
            // `Body.cpp:342-344`: the check-coloured override precedes
            // `CLIsBlack` in `LoadFromINI`.
            combat_light_detail_level_check_colored: section
                .read_phobos_nullable_bool("CombatLightDetailLevel.CheckColored", None),
            cl_is_black: section.read_bool("CLIsBlack", false),
            particle_alpha_image_is_light_flash: section
                .read_phobos_nullable_bool("Particle.AlphaImageIsLightFlash", None),
        }
    }

    /// `RemoveMindControl` or either sell/undeploy arm is set, so the
    /// per-target walk must visit this warhead.
    pub fn has_per_target_effect(&self) -> bool {
        self.remove_mind_control || self.building_sell || self.building_undeploy
    }

    /// `RemoveMindControl_Silent.Get(RulesExt::Global()->RemoveMindControl_Silent)`
    /// (`Detonate.cpp:478`).
    pub fn mind_control_silent(&self, global: bool) -> bool {
        self.remove_mind_control_silent.unwrap_or(global)
    }

    /// `MergeBuildingDamage.Get(RulesExt::Global()->MergeBuildingDamage)`
    /// (`Ext/WarheadType/Hooks.cpp:414`).
    pub fn merges_building_damage(&self, global: bool) -> bool {
        self.merge_building_damage.unwrap_or(global)
    }

    /// `ShakeIsLocal.Get(RulesExt::Global()->ShakeIsLocal)`
    /// (`Ext/Bullet/Hooks.DetonateLogics.cpp:38`).
    pub fn shake_is_local(&self, global: bool) -> bool {
        self.shake_is_local.unwrap_or(global)
    }

    /// `CombatLightDetailLevel_CheckColored.Get(Global->…)`
    /// (`Misc/Hooks.LightEffects.cpp:77`, `Ext/WarheadType/Body.cpp:342`).
    pub fn combat_light_check_colored(&self, global: bool) -> bool {
        self.combat_light_detail_level_check_colored
            .unwrap_or(global)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_warhead_extras_tests.rs"]
mod tests;
