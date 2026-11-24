//! Phobos `WeaponTypeExt::LaserZAdjust` / `EBoltZAdjust`
//! (`src/Ext/WeaponType/Body.cpp:130-131`, `Body.h:60-61`, constructor leaves
//! both unset at `Body.cpp:155-156`) and the `[AudioVisual]` defaults they
//! inherit from `RulesExt` (`src/Ext/Rules/Body.cpp:277-278`, `Body.h:195-196`,
//! constructor `0` at `Body.h:718-719`).
//!
//! Both adjust the beam's initial draw depth when it is created:
//! `LaserZAdjust` in the LaserDraw hook (`Misc/Hooks.LaserDraw.cpp:49-57`) and
//! `EBoltZAdjust` in `TechnoClass::FireEBolt`
//! (`Ext/EBolt/Hooks.cpp:208-220`) and in the simulated bolt
//! (`Ext/Bullet/Body.cpp:270-280`).
//!
//! VERA stores the adjustment on the created beam (`sim::combat::laser` /
//! `sim::combat::electric_bolt`), which is the same value the draw would read
//! one frame later; the per-draw hook site is not a separate owner.

use crate::rules::ini_parser::IniFile;

/// `RulesExt`'s `[AudioVisual] LaserZAdjust=` / `EBoltZAdjust=`
/// (`Valueable<int>`, constructor default 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BeamZAdjustGlobals {
    pub laser: i32,
    pub ebolt: i32,
    /// `[AudioVisual] EBoltZAdjust.ClampInitialDepthForBuilding=`
    /// (`Ext/Rules/Body.cpp:279`, constructor **true** at
    /// `Ext/Rules/Body.h:197,720`): with the key off, a building's bolt keeps
    /// the positive initial-depth delta that stock clamps to zero
    /// (`0x6FD4E4..0x6FD4ED`, Phobos hook `TechnoClass_FireEBolt_Building_ClampPositive`).
    pub ebolt_clamp_initial_depth_for_building: bool,
}

impl BeamZAdjustGlobals {
    pub fn from_ini(ini: &IniFile) -> Self {
        let audio_visual = ini.section_or_empty("AudioVisual");
        Self {
            laser: audio_visual.read_int("LaserZAdjust", 0),
            ebolt: audio_visual.read_int("EBoltZAdjust", 0),
            ebolt_clamp_initial_depth_for_building: audio_visual
                .read_bool("EBoltZAdjust.ClampInitialDepthForBuilding", true),
        }
    }
}

/// `Nullable<int>::Get(global)`: the weapon's own value, else the global one.
pub fn resolved(weapon: Option<i32>, global: i32) -> i32 {
    weapon.unwrap_or(global)
}

/// `Nullable<bool>::Get(global)` for `EBoltZAdjust.ClampInitialDepthForBuilding`:
/// the weapon's own value, else the `[AudioVisual]` default.
pub fn resolved_clamp(weapon: Option<bool>, global: bool) -> bool {
    weapon.unwrap_or(global)
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_beam_z_adjust_tests.rs"]
mod tests;
