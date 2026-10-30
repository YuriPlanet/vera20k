//! Phobos `WarheadTypeExt` animation-list settings and their `[General]`
//! defaults.
//!
//! Source: Phobos b90ab4bc `src/Ext/WarheadType/Body.{h,cpp}` (reads and
//! constructor defaults), `src/Ext/Rules/Body.{h,cpp}` (global keys) and
//! `src/Ext/Bullet/Hooks.DetonateLogics.cpp` / `src/Ext/WarheadType/Hooks.cpp`
//! (consumers). `SplashList` is a `NullableVector<AnimTypeClass*>`: absent or
//! `<default>` inherits `[CombatDamage] SplashList`; a present value replaces
//! it. A value that resolves to no existing animation (`none`, unknown names)
//! is a present, empty list and does not fall back.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, audio/, etc.

use crate::rules::ini_parser::{IniFile, IniSection};

/// Phobos `WarheadTypeExt` fields consumed by the animation selection and
/// Bullet detonation animation tail.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhobosWarheadAnimRules {
    /// `SplashList=`, `NullableVector<AnimTypeClass*>`; `None` inherits the
    /// global list.
    pub splash_list: Option<Vec<String>>,
    /// `SplashList.PickRandom=`, `Valueable<bool>`, default false.
    pub splash_list_pick_random: bool,
    /// `SplashList.CreateAll=`, `Valueable<bool>`, default false.
    pub splash_list_create_all: bool,
    /// `SplashList.CreationInterval=`, `Valueable<int>`, default 0.
    pub splash_list_creation_interval: i32,
    /// `SplashList.ScatterMin=`, `Valueable<Leptons>` as cells, default -1.
    pub splash_list_scatter_min: i32,
    /// `SplashList.ScatterMax=`, `Valueable<Leptons>` as cells, default -1.
    pub splash_list_scatter_max: i32,
    /// `AnimList.PickRandom=`, `Valueable<bool>`, default false.
    pub anim_list_pick_random: bool,
    /// `AnimList.CreateAll=`, `Valueable<bool>`, default false.
    pub anim_list_create_all: bool,
    /// `AnimList.CreationInterval=`, `Valueable<int>`, default 0.
    pub anim_list_creation_interval: i32,
    /// `AnimList.ScatterMin=`, `Valueable<Leptons>` as cells, default -1.
    pub anim_list_scatter_min: i32,
    /// `AnimList.ScatterMax=`, `Valueable<Leptons>` as cells, default -1.
    pub anim_list_scatter_max: i32,
    /// `CreateAnimsOnZeroDamage=`, `Nullable<bool>`; `None` inherits
    /// `[General] CreateAnimsOnZeroDamage`.
    pub create_anims_on_zero_damage: Option<bool>,
    /// `Conventional.IgnoreUnits=`, `Nullable<bool>`; `None` inherits
    /// `[General] Conventional.IgnoreUnits`.
    pub conventional_ignore_units: Option<bool>,
    /// `PlayAnimAboveSurface=`, `Valueable<bool>`, default false. Raises the
    /// animation Z to the cell floor when the impact is below it.
    pub play_anim_above_surface: bool,
    /// `PlayAnimUnderground=`, `Valueable<bool>`, default true. A `no` value
    /// suppresses the animation when the impact Z is below the cell floor.
    pub play_anim_underground: bool,
    /// `AnimZAdjust=`, `Nullable<int>`; `None` inherits
    /// `[AudioVisual] WarheadAnimZAdjust`.
    pub anim_z_adjust: Option<i32>,
}

impl Default for PhobosWarheadAnimRules {
    fn default() -> Self {
        Self {
            splash_list: None,
            splash_list_pick_random: false,
            splash_list_create_all: false,
            splash_list_creation_interval: 0,
            splash_list_scatter_min: -1,
            splash_list_scatter_max: -1,
            anim_list_pick_random: false,
            anim_list_create_all: false,
            anim_list_creation_interval: 0,
            anim_list_scatter_min: -1,
            anim_list_scatter_max: -1,
            create_anims_on_zero_damage: None,
            conventional_ignore_units: None,
            play_anim_above_surface: false,
            play_anim_underground: true,
            anim_z_adjust: None,
        }
    }
}

impl PhobosWarheadAnimRules {
    pub fn from_section(section: &IniSection) -> Self {
        let mut value = Self::default();
        // Names resolve when the list is used, not at the read: an unknown
        // name stays in the list, where Phobos `parse_values` drops it.
        value.splash_list = crate::rules::native_processing::read_nullable_type_vector(
            section,
            "SplashList",
            value.splash_list.as_ref(),
            |name| Some(name.to_owned()),
        );
        value.splash_list_pick_random =
            section.read_ares_bool("SplashList.PickRandom", value.splash_list_pick_random);
        value.splash_list_create_all =
            section.read_ares_bool("SplashList.CreateAll", value.splash_list_create_all);
        value.splash_list_creation_interval = section.read_ares_int(
            "SplashList.CreationInterval",
            value.splash_list_creation_interval,
        );
        value.splash_list_scatter_min =
            section.read_phobos_leptons("SplashList.ScatterMin", value.splash_list_scatter_min);
        value.splash_list_scatter_max =
            section.read_phobos_leptons("SplashList.ScatterMax", value.splash_list_scatter_max);
        value.anim_list_pick_random =
            section.read_ares_bool("AnimList.PickRandom", value.anim_list_pick_random);
        value.anim_list_create_all =
            section.read_ares_bool("AnimList.CreateAll", value.anim_list_create_all);
        value.anim_list_creation_interval = section.read_ares_int(
            "AnimList.CreationInterval",
            value.anim_list_creation_interval,
        );
        value.anim_list_scatter_min =
            section.read_phobos_leptons("AnimList.ScatterMin", value.anim_list_scatter_min);
        value.anim_list_scatter_max =
            section.read_phobos_leptons("AnimList.ScatterMax", value.anim_list_scatter_max);
        value.create_anims_on_zero_damage = section.read_phobos_nullable_bool(
            "CreateAnimsOnZeroDamage",
            value.create_anims_on_zero_damage,
        );
        value.conventional_ignore_units = section
            .read_phobos_nullable_bool("Conventional.IgnoreUnits", value.conventional_ignore_units);
        value.play_anim_above_surface =
            section.read_ares_bool("PlayAnimAboveSurface", value.play_anim_above_surface);
        value.play_anim_underground =
            section.read_ares_bool("PlayAnimUnderground", value.play_anim_underground);
        value.anim_z_adjust = section.read_phobos_nullable_int("AnimZAdjust", value.anim_z_adjust);
        value
    }

    pub fn create_anims_on_zero_damage(&self, globals: &PhobosWarheadAnimGlobals) -> bool {
        self.create_anims_on_zero_damage
            .unwrap_or(globals.create_anims_on_zero_damage)
    }

    pub fn conventional_ignore_units(&self, globals: &PhobosWarheadAnimGlobals) -> bool {
        self.conventional_ignore_units
            .unwrap_or(globals.conventional_ignore_units)
    }

    pub fn anim_z_adjust(&self, globals: &PhobosWarheadAnimGlobals) -> i32 {
        self.anim_z_adjust.unwrap_or(globals.warhead_anim_z_adjust)
    }

    /// The `SplashList` entries the native pointer lookup actually accepts.
    /// Phobos resolves each token with `AnimTypeClass::Find`; invalid names are
    /// skipped. A present empty list stays empty and does not fall back.
    pub fn splash_list_names<'r>(
        &'r self,
        rules: &'r crate::rules::ruleset::RuleSet,
    ) -> Vec<&'r str> {
        let Some(list) = self.splash_list.as_deref() else {
            // The global list is already resolved against the animation
            // registry by `native_processing`.
            return rules
                .combat_damage
                .splash_list
                .iter()
                .map(String::as_str)
                .collect();
        };
        list.iter()
            .filter(|name| {
                rules
                    .anim_type_names
                    .iter()
                    .any(|valid| valid.eq_ignore_ascii_case(name))
            })
            .map(String::as_str)
            .collect()
    }
}

/// `[General]` defaults for the two nullable warhead keys and the
/// `[AudioVisual]` animation Z adjust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhobosWarheadAnimGlobals {
    pub create_anims_on_zero_damage: bool,
    pub conventional_ignore_units: bool,
    /// `[AudioVisual] WarheadAnimZAdjust`, `Valueable<int>`, default -15.
    pub warhead_anim_z_adjust: i32,
}

impl Default for PhobosWarheadAnimGlobals {
    fn default() -> Self {
        Self {
            create_anims_on_zero_damage: false,
            conventional_ignore_units: false,
            warhead_anim_z_adjust: -15,
        }
    }
}

impl PhobosWarheadAnimGlobals {
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut value = Self::default();
        if let Some(section) = ini.section("General") {
            value.create_anims_on_zero_damage =
                section.read_ares_bool("CreateAnimsOnZeroDamage", false);
            value.conventional_ignore_units =
                section.read_ares_bool("Conventional.IgnoreUnits", false);
        }
        if let Some(section) = ini.section("AudioVisual") {
            value.warhead_anim_z_adjust =
                section.read_ares_int("WarheadAnimZAdjust", value.warhead_anim_z_adjust);
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_and_splash_list_absent_default_and_none() {
        let ini = IniFile::from_str(
            "[General]\nCreateAnimsOnZeroDamage=yes\nConventional.IgnoreUnits=yes\n\
             [AudioVisual]\nWarheadAnimZAdjust=-7\n\
             [WH]\nSplashList=A,B\nSplashList.PickRandom=yes\n\
             SplashList.CreateAll=yes\nSplashList.CreationInterval=3\n\
             SplashList.ScatterMin=1.5\nSplashList.ScatterMax=2\n\
             AnimList.PickRandom=yes\nAnimList.CreateAll=yes\n\
             AnimList.CreationInterval=4\nAnimList.ScatterMin=0.5\n\
             AnimList.ScatterMax=1\n\
             PlayAnimAboveSurface=yes\nPlayAnimUnderground=no\nAnimZAdjust=4\n\
             [EMPTY]\nKeep=1\nSplashList=\n\
             [DEFAULT]\nKeep=1\nSplashList=<default>\n\
             [NONE]\nKeep=1\nSplashList=none\n\
             [NOZ]\nKeep=1\nAnimZAdjust=<default>\n",
        );
        let value = PhobosWarheadAnimRules::from_section(ini.section("WH").unwrap());
        assert_eq!(value.splash_list, Some(vec!["A".into(), "B".into()]));
        assert!(value.splash_list_pick_random);
        assert!(value.splash_list_create_all);
        assert_eq!(value.splash_list_creation_interval, 3);
        assert_eq!(value.splash_list_scatter_min, 384);
        assert_eq!(value.splash_list_scatter_max, 512);
        assert!(value.anim_list_pick_random);
        assert!(value.anim_list_create_all);
        assert_eq!(value.anim_list_creation_interval, 4);
        assert_eq!(value.anim_list_scatter_min, 128);
        assert_eq!(value.anim_list_scatter_max, 256);
        assert!(value.play_anim_above_surface);
        assert!(!value.play_anim_underground);
        assert_eq!(value.anim_z_adjust, Some(4));
        let globals = PhobosWarheadAnimGlobals::from_ini(&ini);
        assert!(globals.create_anims_on_zero_damage);
        assert!(globals.conventional_ignore_units);
        assert_eq!(globals.warhead_anim_z_adjust, -7);
        assert_eq!(value.anim_z_adjust(&globals), 4);
        assert!(value.create_anims_on_zero_damage(&globals));
        assert!(value.conventional_ignore_units(&globals));
        assert_eq!(
            PhobosWarheadAnimRules::from_section(ini.section("EMPTY").unwrap()).splash_list,
            None
        );
        assert_eq!(
            PhobosWarheadAnimRules::from_section(ini.section("DEFAULT").unwrap()).splash_list,
            None
        );
        assert_eq!(
            PhobosWarheadAnimRules::from_section(ini.section("NONE").unwrap()).splash_list,
            Some(vec!["none".into()])
        );
        assert_eq!(
            PhobosWarheadAnimRules::from_section(ini.section("NOZ").unwrap()).anim_z_adjust,
            None
        );
    }

    #[test]
    fn animation_z_adjust_defaults_to_the_audio_visual_fallback() {
        let ini = IniFile::from_str("[General]\n[Warheads]\n0=WH\n[WH]\nAnimList=A\n");
        let globals = PhobosWarheadAnimGlobals::from_ini(&ini);
        assert_eq!(globals.warhead_anim_z_adjust, -15);
        let value = PhobosWarheadAnimRules::from_section(ini.section("WH").unwrap());
        assert_eq!(value.anim_z_adjust(&globals), -15);
    }
}
