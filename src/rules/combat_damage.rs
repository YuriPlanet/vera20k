//! `[CombatDamage]` section parser — global default particle systems used
//! by various combat effects (smoke plumes, sparks, fire streams, debris).
//!
//! Fields hold unresolved particle-system section names; ID resolution
//! against the particle-system registry is deferred (matches the same
//! pattern used by ParticleType.warhead, ParticleSystemType.holds_what,
//! ObjectType.damage_particle_systems, and GeneralRules.barrel_particle).
//!
//! The particle fields below mirror the fixed RulesClass slots at +0x1018..+0x1038;
//! retail rulesmd.ini ships a 10th key (`DefaultFirestormExplosionSystem=`)
//! that is not present in the verified RulesClass::ReadCombatDamage layout,
//! so we don't parse it. The independent global DeathWeapon pointer is at
//! RulesClass +0xFDC.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// Default particle-system fallbacks read from `[CombatDamage]`.
///
/// Each field is the section name of a `ParticleSystemType` (resolved later
/// against `RuleSet::ps_type_id_by_name`). `None` means the key was absent
/// or empty — consumers must supply their own fallback in that case.
#[derive(Debug, Clone)]
pub struct CombatDamageDefaults {
    /// Signed post-Verses cap used by ApplyWarheadDamage. The executable's
    /// constructor default is 1000; stock rulesmd.ini overrides it to 10000.
    pub max_damage: i32,
    /// Signed, unclamped destroyable-cliff chance compared against one
    /// Scenario RandomRanged(0,99) draw. Native constructor default is 100.
    pub collapse_chance: i32,
    /// `BallisticScatter=` in LEPTONS. `RulesClass::ReadCombatDamage
    /// @ 0x0066CD5B` reads it through `CCINIClass::ReadRange @ 0x00474620`
    /// into `RulesClass+0x1734`, so the authored cell count is multiplied by
    /// 256; the constructor default written at `0x006675C4` is `0x100`, and
    /// stock `rulesmd.ini:815` authors `1.0` for the same 256.
    pub ballistic_scatter: i32,
    /// `OpenToppedWarpDistance=` in CELLS (`RulesClass+0xF60`, ReadInt at
    /// `0x0066C79C`; constructor default 5 at `0x00666AB1`/`0x00666AE5`;
    /// stock 7). `TemporalClass::Update` multiplies it by 256 at its use.
    pub open_topped_warp_distance: i32,
    /// Global `DeathWeapon=` used only when a dying type has neither an
    /// explicit death weapon nor a live current-weapon fallback.
    pub death_weapon: Option<String>,
    /// `IvanWarhead=` (`RulesClass+0xFC8`, ReadINI `0x0066C52A`; constructor
    /// null): the warhead of an Ivan bomb's blast (`BombClass::Detonate @
    /// 0x00438720`). Stock `IvanWH`.
    pub ivan_warhead: Option<String>,
    /// `IvanDamage=` (`RulesClass+0xFCC`, ReadINI `0x0066C5C8`; constructor
    /// 100 at `0x00666B88`): the blast's damage. Stock 450.
    pub ivan_damage: i32,
    /// `IvanTimedDelay=` in frames (`RulesClass+0xFD0`, ReadINI `0x0066C5EE`;
    /// constructor 450 at `0x00666B92`): the fuse, added to the attach frame.
    pub ivan_timed_delay: i32,
    /// `IvanIconFlickerRate=` in frames (`RulesClass+0xFD8`, ReadINI
    /// `0x0066C62B`; constructor 8 at `0x00666BA8`): the bomb clock's blink.
    pub ivan_icon_flicker_rate: i32,
    /// `SplashList=` (`RulesClass+0xBC4`, count `+0xBD0`; constructor empty):
    /// the water explosions. A bouncing debris chunk landing in water below
    /// the deck plays the first (`AnimClass::AI 0x00423DD2`).
    pub splash_list: Vec<String>,
    /// Large grey smoke plume — buildings under heavy damage.
    pub default_large_grey_smoke_system: Option<String>,
    /// Small grey smoke plume.
    pub default_small_grey_smoke_system: Option<String>,
    /// Spark shower — used by capture / warp-attach / electric bolt impact.
    pub default_spark_system: Option<String>,
    /// Large red smoke plume.
    pub default_large_red_smoke_system: Option<String>,
    /// Small red smoke plume.
    pub default_small_red_smoke_system: Option<String>,
    /// Debris dust kicked up when wreckage hits the ground.
    pub default_debris_smoke_system: Option<String>,
    /// Flamethrower fire stream particle system.
    pub default_fire_stream_system: Option<String>,
    /// Hidden test particle system — never used in retail YR.
    pub default_test_particle_system: Option<String>,
    /// Sparks emitted when a unit gets repaired by a service depot.
    pub default_repair_particle_system: Option<String>,
}

impl CombatDamageDefaults {
    /// Parse from a `[CombatDamage]` `IniSection`. Missing keys become `None`.
    pub fn from_ini_section(section: &IniSection) -> Self {
        Self {
            max_damage: section.get_i32("MaxDamage").unwrap_or(1000),
            collapse_chance: section.get_i32("CollapseChance").unwrap_or(100),
            ballistic_scatter: section.read_range("BallisticScatter", 0x100),
            open_topped_warp_distance: section.get_i32("OpenToppedWarpDistance").unwrap_or(5),
            death_weapon: read_name(section, "DeathWeapon"),
            ivan_warhead: read_name(section, "IvanWarhead"),
            ivan_damage: section.get_i32("IvanDamage").unwrap_or(100),
            ivan_timed_delay: section.get_i32("IvanTimedDelay").unwrap_or(450),
            ivan_icon_flicker_rate: section.get_i32("IvanIconFlickerRate").unwrap_or(8),
            splash_list: crate::rules::object_type::parse_csv_string_list(
                section.get("SplashList"),
            ),
            default_large_grey_smoke_system: read_name(section, "DefaultLargeGreySmokeSystem"),
            default_small_grey_smoke_system: read_name(section, "DefaultSmallGreySmokeSystem"),
            default_spark_system: read_name(section, "DefaultSparkSystem"),
            default_large_red_smoke_system: read_name(section, "DefaultLargeRedSmokeSystem"),
            default_small_red_smoke_system: read_name(section, "DefaultSmallRedSmokeSystem"),
            default_debris_smoke_system: read_name(section, "DefaultDebrisSmokeSystem"),
            default_fire_stream_system: read_name(section, "DefaultFireStreamSystem"),
            default_test_particle_system: read_name(section, "DefaultTestParticleSystem"),
            default_repair_particle_system: read_name(section, "DefaultRepairParticleSystem"),
        }
    }
}

impl Default for CombatDamageDefaults {
    fn default() -> Self {
        Self {
            max_damage: 1000,
            collapse_chance: 100,
            ballistic_scatter: 0x100,
            open_topped_warp_distance: 5,
            death_weapon: None,
            ivan_warhead: None,
            ivan_damage: 100,
            ivan_timed_delay: 450,
            ivan_icon_flicker_rate: 8,
            splash_list: Vec::new(),
            default_large_grey_smoke_system: None,
            default_small_grey_smoke_system: None,
            default_spark_system: None,
            default_large_red_smoke_system: None,
            default_small_red_smoke_system: None,
            default_debris_smoke_system: None,
            default_fire_stream_system: None,
            default_test_particle_system: None,
            default_repair_particle_system: None,
        }
    }
}

fn read_name(section: &IniSection, key: &str) -> Option<String> {
    section
        .get(key)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn parses_full_combat_damage_section() {
        let ini = IniFile::from_str(
            "[CombatDamage]\n\
             DefaultLargeGreySmokeSystem=BigGreySmokeSys\n\
             DefaultSmallGreySmokeSystem=SmallGreySSys\n\
             DefaultSparkSystem=SparkSys\n\
             DefaultLargeRedSmokeSystem=BigGreySmokeSys\n\
             DefaultSmallRedSmokeSystem=SmallGreySSys\n\
             DefaultDebrisSmokeSystem=SmallGreySSys\n\
             DefaultFireStreamSystem=FireStreamSys\n\
             DefaultTestParticleSystem=TestSmokeSys\n\
             DefaultRepairParticleSystem=WeldingSys\n",
        );
        let section = ini.section("CombatDamage").unwrap();
        let cd = CombatDamageDefaults::from_ini_section(section);

        assert_eq!(
            cd.default_large_grey_smoke_system.as_deref(),
            Some("BigGreySmokeSys")
        );
        assert_eq!(
            cd.default_small_grey_smoke_system.as_deref(),
            Some("SmallGreySSys")
        );
        assert_eq!(cd.default_spark_system.as_deref(), Some("SparkSys"));
        assert_eq!(
            cd.default_large_red_smoke_system.as_deref(),
            Some("BigGreySmokeSys")
        );
        assert_eq!(
            cd.default_small_red_smoke_system.as_deref(),
            Some("SmallGreySSys")
        );
        assert_eq!(
            cd.default_debris_smoke_system.as_deref(),
            Some("SmallGreySSys")
        );
        assert_eq!(
            cd.default_fire_stream_system.as_deref(),
            Some("FireStreamSys")
        );
        assert_eq!(
            cd.default_test_particle_system.as_deref(),
            Some("TestSmokeSys")
        );
        assert_eq!(
            cd.default_repair_particle_system.as_deref(),
            Some("WeldingSys")
        );
    }

    #[test]
    fn section_without_recognized_keys_yields_all_none() {
        let ini = IniFile::from_str("[CombatDamage]\nFixtureOnly=1\n");
        let section = ini.section("CombatDamage").unwrap();
        let cd = CombatDamageDefaults::from_ini_section(section);
        assert!(cd.default_large_grey_smoke_system.is_none());
        assert!(cd.default_spark_system.is_none());
        assert!(cd.default_fire_stream_system.is_none());
        assert!(cd.default_repair_particle_system.is_none());
    }

    #[test]
    fn whitespace_only_value_treated_as_none() {
        let ini = IniFile::from_str("[CombatDamage]\nFixtureOnly=1\nDefaultSparkSystem=   \n");
        let section = ini.section("CombatDamage").unwrap();
        let cd = CombatDamageDefaults::from_ini_section(section);
        assert!(cd.default_spark_system.is_none());
    }

    #[test]
    fn trims_whitespace_around_value() {
        let ini = IniFile::from_str("[CombatDamage]\nDefaultSparkSystem=  SparkSys  \n");
        let section = ini.section("CombatDamage").unwrap();
        let cd = CombatDamageDefaults::from_ini_section(section);
        assert_eq!(cd.default_spark_system.as_deref(), Some("SparkSys"));
    }
}
