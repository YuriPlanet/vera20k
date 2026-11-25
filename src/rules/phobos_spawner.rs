//! Phobos `Spawner.*` type keys (`Ext/TechnoType/Body.cpp:790-794`,
//! `:1120-1123`; fields `Body.h:60-66`, `:336-339`; constructor defaults
//! `Body.h:498-504`, `:770-773`) and their four `[General]` fallbacks
//! (`Ext/Rules/Body.cpp:208-211`, `Body.h:115-118,652-655`).
//!
//! Consumers live in `sim::spawn_manager` (delay, destination, recycle,
//! launch facing), the spawner-range unit AI check (`TechnoExt::
//! ApplySpawnLimitRange`, `Ext/Techno/Body.Update.cpp:445-461`) and the
//! per-type range cache (`TechnoTypeExt::CalculateSpawnerRange`,
//! `Ext/TechnoType/Body.cpp:539-563`, run from `Ext/Rules/Body.cpp:77`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::native_processing::read_nullable_type_vector;

/// The nine `Spawner.*` keys on one TechnoType. `SpawnerRange` /
/// `EliteSpawnerRange` are computed at use time from the type's weapons plus
/// `extra_limit_range` (the native post-pass runs after every type is known).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PhobosSpawner {
    /// `Spawner.LimitRange=`, `Valueable<bool>`, default `no`.
    pub limit_range: bool,
    /// `Spawner.ExtraLimitRange=`, `Valueable<int>`, default 0 (cells).
    pub extra_limit_range: i32,
    /// `Spawner.DelayFrames=`, `Nullable<int>`: the launch spacing.
    pub delay_frames: Option<i32>,
    /// `Spawner.AttackImmediately=`, `Nullable<bool>`.
    pub attack_immediately: Option<bool>,
    /// `Spawner.UseTurretFacing=`, `Nullable<bool>`.
    pub use_turret_facing: Option<bool>,
    /// `Spawner.RecycleRange=`, `Nullable<Leptons>` (INI cells × 256).
    pub recycle_range: Option<i32>,
    /// `Spawner.RecycleAnim=`, `ValueableVector<AnimTypeClass*>`.
    pub recycle_anim: Vec<String>,
    /// `Spawner.RecycleCoord=`, `Valueable<CoordStruct>`, default `0,0,0`.
    pub recycle_coord: [i32; 3],
    /// `Spawner.RecycleOnTurret=`, `Nullable<bool>`.
    pub recycle_on_turret: Option<bool>,
}

impl Default for PhobosSpawner {
    fn default() -> Self {
        Self {
            limit_range: false,
            extra_limit_range: 0,
            delay_frames: None,
            attack_immediately: None,
            use_turret_facing: None,
            recycle_range: None,
            recycle_anim: Vec::new(),
            recycle_coord: [0; 3],
            recycle_on_turret: None,
        }
    }
}

impl PhobosSpawner {
    pub fn from_section(section: &IniSection) -> Self {
        // `ValueableVector<AnimTypeClass*>` drops unresolvable names at read
        // time natively; the names stay here until a RuleSet resolves them,
        // and the consumer drops the ones it cannot resolve.
        let keep = |name: &str| Some(name.to_owned());
        Self {
            limit_range: section.read_ares_bool("Spawner.LimitRange", false),
            extra_limit_range: section.read_ares_int("Spawner.ExtraLimitRange", 0),
            delay_frames: section.read_phobos_nullable_int("Spawner.DelayFrames", None),
            attack_immediately: section
                .read_phobos_nullable_bool("Spawner.AttackImmediately", None),
            use_turret_facing: section.read_phobos_nullable_bool("Spawner.UseTurretFacing", None),
            recycle_range: section.read_phobos_nullable_leptons("Spawner.RecycleRange", None),
            recycle_anim: read_nullable_type_vector(section, "Spawner.RecycleAnim", None, keep)
                .unwrap_or_default(),
            // `Valueable<CoordStruct>`'s `Read3Integers` writes a parsed
            // prefix, so a one- or two-token read updates only those axes.
            recycle_coord: section.read_int_fields("Spawner.RecycleCoord", 64, [0; 3]),
            recycle_on_turret: section.read_phobos_nullable_bool("Spawner.RecycleOnTurret", None),
        }
    }

    pub fn attack_immediately(&self, global: bool) -> bool {
        self.attack_immediately.unwrap_or(global)
    }

    pub fn use_turret_facing(&self, global: bool) -> bool {
        self.use_turret_facing.unwrap_or(global)
    }

    pub fn recycle_range(&self, global: i32) -> i32 {
        self.recycle_range.unwrap_or(global)
    }

    pub fn recycle_on_turret(&self, global: bool) -> bool {
        self.recycle_on_turret.unwrap_or(global)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_spawner_tests.rs"]
mod tests;
