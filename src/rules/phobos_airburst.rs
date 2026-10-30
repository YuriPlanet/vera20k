//! Phobos `BulletTypeExt` airburst/splits settings and their
//! `[CombatDamage]` defaults.
//!
//! Source: Phobos b90ab4bc `src/Ext/BulletType/Body.{h,cpp}` (reads and
//! constructor defaults), `src/Ext/Rules/Body.{h,cpp}` (global keys) and
//! `src/Ext/Bullet/Hooks.DetonateLogics.cpp` (consumer). The consumer reads
//! `Type.Get(Global)` for every nullable key; `AroundTarget` defaults to the
//! type's own `Splits` instead.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, audio/, etc.

use crate::rules::ini_parser::{IniFile, IniSection};
use std::hash::Hash;

/// `BulletTypeExt` fields consumed by the airburst/splits emitter.
#[derive(Debug, Clone, PartialEq)]
pub struct PhobosAirburstType {
    /// `Splits=`, constructor default false.
    pub splits: bool,
    /// `AirburstSpread=`, `Valueable<double>` in cells, default 1.5.
    pub airburst_spread: f64,
    /// `RetargetAccuracy=`, `Valueable<double>`, default 0.0.
    pub retarget_accuracy: f64,
    /// `RetargetSelf=`, `Valueable<bool>`, default true.
    pub retarget_self: bool,
    /// `RetargetSelf.Probability=`, default 0.5.
    pub retarget_self_probability: f64,
    /// `AroundTarget=`, `Nullable<bool>`; unset defaults to `splits`.
    pub around_target: Option<bool>,
    /// `Airburst.UseCluster=`, `Nullable<bool>`; unset takes the global.
    pub airburst_use_cluster: Option<bool>,
    /// `Airburst.RandomClusters=`, `Valueable<bool>`, default false.
    pub airburst_random_clusters: bool,
    /// `Airburst.TargetAsSource=`, `Valueable<bool>`, default false.
    pub airburst_target_as_source: bool,
    /// `Airburst.TargetAsSource.SkipHeight=`, `Nullable<bool>`.
    pub airburst_target_as_source_skip_height: Option<bool>,
    /// `Splits.TargetingDistance=`, `Valueable<Leptons>` read as cells,
    /// constructor default 1280 leptons (5 cells).
    pub splits_targeting_distance: i32,
    /// `Splits.TargetingDistance.Cylindrical=`, `Nullable<bool>`.
    pub splits_targeting_distance_cylindrical: Option<bool>,
    /// `Splits.AllowRepeatTargets=`, `Nullable<bool>`.
    pub splits_allow_repeat_targets: Option<bool>,
    /// `Splits.TargetCellRange=`, `Valueable<int>`, default 3.
    pub splits_target_cell_range: i32,
    /// `Splits.UseWeaponTargeting=`, `Nullable<bool>`.
    pub splits_use_weapon_targeting: Option<bool>,
    /// `AirburstWeapon.ApplyFirepowerMult=`, `Nullable<bool>`.
    pub airburst_weapon_apply_firepower_mult: Option<bool>,
    /// `AirburstWeapon.SourceScatterMin=` (`Valueable<Leptons>` as cells),
    /// default 0.
    pub airburst_weapon_source_scatter_min: i32,
    /// `AirburstWeapon.SourceScatterMax=` (`Valueable<Leptons>` as cells),
    /// default 0.
    pub airburst_weapon_source_scatter_max: i32,
    /// `AirburstWeapon.UseFiringEffects=`, `Nullable<bool>`.
    pub airburst_weapon_use_firing_effects: Option<bool>,
    /// `AirburstWeapon.HeadToTarget=`, `Nullable<bool>`.
    pub airburst_weapon_head_to_target: Option<bool>,
    /// `AirburstWeapon.RadialFireSegments=`, `Valueable<int>`, default 0.
    pub airburst_weapon_radial_fire_segments: i32,
    /// Phobos `BulletTypeExt::AAOnly`, `Valueable<bool>`, default false.
    /// The split projectile check reads it for the fired split BulletType.
    pub aa_only: bool,
}

impl Default for PhobosAirburstType {
    fn default() -> Self {
        Self {
            splits: false,
            airburst_spread: 1.5,
            retarget_accuracy: 0.0,
            retarget_self: true,
            retarget_self_probability: 0.5,
            around_target: None,
            airburst_use_cluster: None,
            airburst_random_clusters: false,
            airburst_target_as_source: false,
            airburst_target_as_source_skip_height: None,
            splits_targeting_distance: 5 * crate::util::lepton::LEPTONS_PER_CELL_I32,
            splits_targeting_distance_cylindrical: None,
            splits_allow_repeat_targets: None,
            splits_target_cell_range: 3,
            splits_use_weapon_targeting: None,
            airburst_weapon_apply_firepower_mult: None,
            airburst_weapon_source_scatter_min: 0,
            airburst_weapon_source_scatter_max: 0,
            airburst_weapon_use_firing_effects: None,
            airburst_weapon_head_to_target: None,
            airburst_weapon_radial_fire_segments: 0,
            aa_only: false,
        }
    }
}

impl PhobosAirburstType {
    /// The keys `BulletTypeExt::LoadFromINIFile` reads for this feature, in
    /// its source order.
    pub fn from_section(section: &IniSection) -> Self {
        let mut value = Self::default();
        value.splits = section.read_ares_bool("Splits", value.splits);
        value.airburst_spread = section.read_phobos_double("AirburstSpread", value.airburst_spread);
        value.retarget_accuracy =
            section.read_phobos_double("RetargetAccuracy", value.retarget_accuracy);
        value.retarget_self = section.read_ares_bool("RetargetSelf", value.retarget_self);
        value.retarget_self_probability =
            section.read_phobos_double("RetargetSelf.Probability", value.retarget_self_probability);
        value.around_target =
            section.read_phobos_nullable_bool("AroundTarget", value.around_target);
        value.airburst_use_cluster =
            section.read_phobos_nullable_bool("Airburst.UseCluster", value.airburst_use_cluster);
        value.airburst_random_clusters =
            section.read_ares_bool("Airburst.RandomClusters", value.airburst_random_clusters);
        value.airburst_target_as_source =
            section.read_ares_bool("Airburst.TargetAsSource", value.airburst_target_as_source);
        value.airburst_target_as_source_skip_height = section.read_phobos_nullable_bool(
            "Airburst.TargetAsSource.SkipHeight",
            value.airburst_target_as_source_skip_height,
        );
        value.splits_targeting_distance = section
            .read_phobos_leptons("Splits.TargetingDistance", value.splits_targeting_distance);
        value.splits_targeting_distance_cylindrical = section.read_phobos_nullable_bool(
            "Splits.TargetingDistance.Cylindrical",
            value.splits_targeting_distance_cylindrical,
        );
        value.splits_allow_repeat_targets = section.read_phobos_nullable_bool(
            "Splits.AllowRepeatTargets",
            value.splits_allow_repeat_targets,
        );
        value.splits_target_cell_range =
            section.read_ares_int("Splits.TargetCellRange", value.splits_target_cell_range);
        value.splits_use_weapon_targeting = section.read_phobos_nullable_bool(
            "Splits.UseWeaponTargeting",
            value.splits_use_weapon_targeting,
        );
        value.airburst_weapon_apply_firepower_mult = section.read_phobos_nullable_bool(
            "AirburstWeapon.ApplyFirepowerMult",
            value.airburst_weapon_apply_firepower_mult,
        );
        value.airburst_weapon_source_scatter_min = section.read_phobos_leptons(
            "AirburstWeapon.SourceScatterMin",
            value.airburst_weapon_source_scatter_min,
        );
        value.airburst_weapon_source_scatter_max = section.read_phobos_leptons(
            "AirburstWeapon.SourceScatterMax",
            value.airburst_weapon_source_scatter_max,
        );
        value.airburst_weapon_use_firing_effects = section.read_phobos_nullable_bool(
            "AirburstWeapon.UseFiringEffects",
            value.airburst_weapon_use_firing_effects,
        );
        value.airburst_weapon_head_to_target = section.read_phobos_nullable_bool(
            "AirburstWeapon.HeadToTarget",
            value.airburst_weapon_head_to_target,
        );
        value.airburst_weapon_radial_fire_segments = section.read_ares_int(
            "AirburstWeapon.RadialFireSegments",
            value.airburst_weapon_radial_fire_segments,
        );
        value.aa_only = section.read_ares_bool("AAOnly", value.aa_only);
        value
    }

    /// `AroundTarget.Get(Splits)`.
    pub fn around_target(&self) -> bool {
        self.around_target.unwrap_or(self.splits)
    }

    pub fn airburst_use_cluster(&self, globals: &PhobosAirburstGlobals) -> bool {
        self.airburst_use_cluster
            .unwrap_or(globals.airburst_use_cluster)
    }

    pub fn skip_height(&self, globals: &PhobosAirburstGlobals) -> bool {
        self.airburst_target_as_source_skip_height
            .unwrap_or(globals.airburst_target_as_source_skip_height)
    }

    pub fn targeting_distance_cylindrical(&self, globals: &PhobosAirburstGlobals) -> bool {
        self.splits_targeting_distance_cylindrical
            .unwrap_or(globals.splits_targeting_distance_cylindrical)
    }

    pub fn allow_repeat_targets(&self, globals: &PhobosAirburstGlobals) -> bool {
        self.splits_allow_repeat_targets
            .unwrap_or(globals.splits_allow_repeat_targets)
    }

    pub fn use_weapon_targeting(&self, globals: &PhobosAirburstGlobals) -> bool {
        self.splits_use_weapon_targeting
            .unwrap_or(globals.splits_use_weapon_targeting)
    }

    pub fn apply_firepower_mult(&self, globals: &PhobosAirburstGlobals) -> bool {
        self.airburst_weapon_apply_firepower_mult
            .unwrap_or(globals.airburst_weapon_apply_firepower_mult)
    }

    pub fn use_firing_effects(&self, globals: &PhobosAirburstGlobals) -> bool {
        self.airburst_weapon_use_firing_effects
            .unwrap_or(globals.airburst_weapon_use_firing_effects)
    }

    pub fn head_to_target(&self, globals: &PhobosAirburstGlobals) -> bool {
        self.airburst_weapon_head_to_target
            .unwrap_or(globals.airburst_weapon_head_to_target)
    }
}

/// `[CombatDamage]` defaults for the nullable type keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PhobosAirburstGlobals {
    pub splits_targeting_distance_cylindrical: bool,
    pub splits_allow_repeat_targets: bool,
    pub splits_use_weapon_targeting: bool,
    pub airburst_use_cluster: bool,
    pub airburst_target_as_source_skip_height: bool,
    pub airburst_weapon_apply_firepower_mult: bool,
    pub airburst_weapon_use_firing_effects: bool,
    pub airburst_weapon_head_to_target: bool,
}

impl PhobosAirburstGlobals {
    /// `RulesExt::LoadFromINIFile` reads each key from `[CombatDamage]`.
    pub fn from_ini(ini: &IniFile) -> Self {
        let Some(section) = ini.section("CombatDamage") else {
            return Self::default();
        };
        Self {
            splits_targeting_distance_cylindrical: section
                .read_ares_bool("Splits.TargetingDistance.Cylindrical", false),
            splits_allow_repeat_targets: section.read_ares_bool("Splits.AllowRepeatTargets", false),
            splits_use_weapon_targeting: section.read_ares_bool("Splits.UseWeaponTargeting", false),
            airburst_use_cluster: section.read_ares_bool("Airburst.UseCluster", false),
            airburst_target_as_source_skip_height: section
                .read_ares_bool("Airburst.TargetAsSource.SkipHeight", false),
            airburst_weapon_apply_firepower_mult: section
                .read_ares_bool("AirburstWeapon.ApplyFirepowerMult", false),
            airburst_weapon_use_firing_effects: section
                .read_ares_bool("AirburstWeapon.UseFiringEffects", false),
            airburst_weapon_head_to_target: section
                .read_ares_bool("AirburstWeapon.HeadToTarget", false),
        }
    }
}

/// Phobos `AffectedVeterancy` flags (`Utilities/Enum.h:115..123`), the target
/// of `read<AffectedVeterancy>`: one token per comma-separated flag; an
/// unknown token fails the whole read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AffectedVeterancy(u8);

impl AffectedVeterancy {
    pub const NONE: Self = Self(0x0);
    pub const ROOKIE: Self = Self(0x1);
    pub const VETERAN: Self = Self(0x2);
    pub const ELITE: Self = Self(0x4);
    pub const ALL: Self = Self(Self::ROOKIE.0 | Self::VETERAN.0 | Self::ELITE.0);

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// `read<AffectedVeterancy>`: `None` keeps the caller's current value.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let mut parsed = Self::NONE;
        for part in raw.split(',') {
            let token = part.trim_matches(|c| matches!(c, ' ' | '\t' | '\r'));
            let flag = match token.to_ascii_lowercase().as_str() {
                "rookie" => Self::ROOKIE,
                "veteran" => Self::VETERAN,
                "elite" => Self::ELITE,
                "all" => Self::ALL,
                "none" => Self::NONE,
                _ => return None,
            };
            parsed = Self(parsed.0 | flag.0);
        }
        Some(parsed)
    }
}

impl Default for AffectedVeterancy {
    fn default() -> Self {
        Self::ALL
    }
}

/// Phobos `WeaponTypeExt`'s target filters (`CanTarget`, its health range
/// and `CanTargetVeterancy`), read for `Splits.UseWeaponTargeting`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhobosWeaponTargeting {
    pub can_target: crate::rules::phobos_crit::AffectedTarget,
    pub min_health: f64,
    pub max_health: f64,
    pub veterancy: AffectedVeterancy,
}

impl Default for PhobosWeaponTargeting {
    fn default() -> Self {
        Self {
            can_target: crate::rules::phobos_crit::AffectedTarget::ALL,
            min_health: 0.0,
            max_health: 1.0,
            veterancy: AffectedVeterancy::ALL,
        }
    }
}

impl PhobosWeaponTargeting {
    pub fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        self.can_target.bits().hash(hasher);
        self.min_health.to_bits().hash(hasher);
        self.max_health.to_bits().hash(hasher);
        self.veterancy.bits().hash(hasher);
    }
}

/// Phobos `WeaponTypeExt::LoadFromINIFile`'s target-filter block
/// (`src/Ext/WeaponType/Body.cpp:113..117`), in native read order.
pub(crate) fn read_weapon_targeting(section: &IniSection) -> PhobosWeaponTargeting {
    let mut value = PhobosWeaponTargeting::default();
    if let Some(raw) = section.read_name("CanTarget", 2048)
        && let Some(parsed) = crate::rules::phobos_crit::AffectedTarget::parse(raw)
    {
        value.can_target = parsed;
    }
    value.max_health = section.read_phobos_double("CanTarget.MaxHealth", value.max_health);
    value.min_health = section.read_phobos_double("CanTarget.MinHealth", value.min_health);
    if let Some(raw) = section.read_name("CanTargetVeterancy", 2048)
        && let Some(parsed) = AffectedVeterancy::parse(raw)
    {
        value.veterancy = parsed;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weapon_targeting_reads_and_unknown_tokens_keep_defaults() {
        use crate::rules::phobos_crit::AffectedTarget;

        let ini = IniFile::from_str(
            "[W]\nCanTarget=units,buildings\nCanTarget.MinHealth=.25\n\
             CanTarget.MaxHealth=.75\nCanTargetVeterancy=veteran,elite\n\
             [D]\nKeep=1\n\
             [BAD]\nCanTarget=units,typo\nCanTargetVeterancy=rookie,typo\n",
        );
        let value = read_weapon_targeting(ini.section("W").unwrap());
        assert_eq!(
            value.can_target.bits(),
            AffectedTarget::UNIT.bits() | AffectedTarget::BUILDING.bits()
        );
        assert_eq!(value.min_health, 0.25);
        assert_eq!(value.max_health, 0.75);
        assert_eq!(
            value.veterancy.bits(),
            AffectedVeterancy::VETERAN.bits() | AffectedVeterancy::ELITE.bits()
        );
        assert_eq!(
            read_weapon_targeting(ini.section("D").unwrap()),
            PhobosWeaponTargeting::default()
        );
        assert_eq!(
            read_weapon_targeting(ini.section("BAD").unwrap()),
            PhobosWeaponTargeting::default(),
            "one unknown token abandons the whole read"
        );
    }

    #[test]
    fn type_keys_read_as_cells_and_nullable_fall_back() {
        let ini = IniFile::from_str(
            "[CombatDamage]\nSplits.AllowRepeatTargets=yes\nAirburst.UseCluster=yes\n\
             Airburst.TargetAsSource.SkipHeight=yes\nAirburstWeapon.HeadToTarget=yes\n\
             [SHOT]\nSplits=yes\nRetargetAccuracy=50%\nRetargetSelf=no\n\
             RetargetSelf.Probability=25%\nSplits.TargetingDistance=2.5\n\
             AirburstSpread=3\nAirburst.RandomClusters=yes\n\
             Airburst.TargetAsSource=yes\nSplits.TargetCellRange=-1\n\
             AirburstWeapon.SourceScatterMin=1.5\nAirburstWeapon.SourceScatterMax=2\n\
             AirburstWeapon.RadialFireSegments=4\nAAOnly=yes\n",
        );
        let section = ini.section("SHOT").unwrap();
        let value = PhobosAirburstType::from_section(section);
        assert!(value.splits);
        assert_eq!(value.retarget_accuracy, 0.5);
        assert!(!value.retarget_self);
        assert_eq!(value.retarget_self_probability, 0.25);
        assert_eq!(value.splits_targeting_distance, 640);
        assert_eq!(value.airburst_spread, 3.0);
        assert!(value.airburst_random_clusters);
        assert!(value.airburst_target_as_source);
        assert_eq!(value.splits_target_cell_range, -1);
        assert_eq!(value.airburst_weapon_source_scatter_min, 384);
        assert_eq!(value.airburst_weapon_source_scatter_max, 512);
        assert_eq!(value.airburst_weapon_radial_fire_segments, 4);
        assert!(value.aa_only);
        // Unset nullable keys take the globals.
        let globals = PhobosAirburstGlobals::from_ini(&ini);
        assert!(globals.splits_allow_repeat_targets);
        assert!(value.allow_repeat_targets(&globals));
        assert!(value.airburst_use_cluster(&globals));
        assert!(value.skip_height(&globals));
        assert!(value.head_to_target(&globals));
        // AroundTarget has no global; unset takes the type's own Splits.
        assert!(value.around_target());
    }
}
