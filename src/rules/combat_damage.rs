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
    /// `Crush=` in LEPTONS (`RulesClass+0x1728`), read through `ReadRange` at
    /// `0x0066CDC1`; the constructor default written at `0x006675A6` is
    /// `0x180`, and stock `rulesmd.ini` authors `1.8` for 460. How near a
    /// computer unit charges a crushable attacker
    /// (`UnitClass::Should_Crush_It @ 0x007438F0`).
    pub crush: i32,
    /// `Incoming=` (`RulesClass+0x16C0`), read through `ReadSpeed @
    /// 0x00474810` at `0x0066CF24` with the current value as its default; the
    /// constructor writes 0 (`0x006674EA`). Stock `10` reads as 25. An
    /// infantryman whose weapon 0 is slower (`+0xA8`) makes the objects in
    /// its target's cell scatter away from it as it fires
    /// (`InfantryClass::Fire_At_Target @ 0x00520A61..0x00520AC7`).
    pub incoming: i32,
    /// `OpenToppedWarpDistance=` in CELLS (`RulesClass+0xF60`, ReadInt at
    /// `0x0066C79C`; constructor default 5 at `0x00666AB1`/`0x00666AE5`;
    /// stock 7). `TemporalClass::Update` multiplies it by 256 at its use.
    pub open_topped_warp_distance: i32,
    /// Global `DeathWeapon=` used only when a dying type has neither an
    /// explicit death weapon nor a live current-weapon fallback.
    pub death_weapon: Option<String>,
    /// `FallingDamageMultiplier=` (`RulesClass+0xF64`, ReadDouble at
    /// `0x0066C9CB` stored as a float; constructor 1.0): scales the damage a
    /// Unit dropped by a Magnetron deals where it lands
    /// (`UnitClass` notice `0x0074623E`).
    pub falling_damage_multiplier: f32,
    /// `CurrentStrengthDamage=` (`RulesClass+0xF68`, ReadBool at `0x0066C9EB`;
    /// constructor 1): that damage starts from the dropped Unit's current
    /// Health rather than its type's `Strength=` (`0x0074621D`).
    pub current_strength_damage: bool,
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
    /// Every name is ReadString 0x80 (`0x0066C514`-`0x0066CCCA`).
    pub fn from_ini_section(section: &IniSection) -> Self {
        Self {
            max_damage: section.read_int("MaxDamage", 1000),
            collapse_chance: section.read_int("CollapseChance", 100),
            ballistic_scatter: section.read_range("BallisticScatter", 0x100),
            crush: section.read_range("Crush", 0x180),
            incoming: section.read_speed("Incoming", 0),
            open_topped_warp_distance: section.read_int("OpenToppedWarpDistance", 5),
            death_weapon: section.read_name("DeathWeapon", 0x80).map(str::to_string),
            falling_damage_multiplier: section.read_float("FallingDamageMultiplier", 1.0),
            current_strength_damage: section.read_bool("CurrentStrengthDamage", true),
            ivan_warhead: section.read_name("IvanWarhead", 0x80).map(str::to_string),
            ivan_damage: section.read_int("IvanDamage", 100),
            ivan_timed_delay: section.read_int("IvanTimedDelay", 450),
            ivan_icon_flicker_rate: section.read_int("IvanIconFlickerRate", 8),
            splash_list: section
                .read_list("SplashList", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            default_large_grey_smoke_system: section
                .read_name("DefaultLargeGreySmokeSystem", 0x80)
                .map(str::to_string),
            default_small_grey_smoke_system: section
                .read_name("DefaultSmallGreySmokeSystem", 0x80)
                .map(str::to_string),
            default_spark_system: section
                .read_name("DefaultSparkSystem", 0x80)
                .map(str::to_string),
            default_large_red_smoke_system: section
                .read_name("DefaultLargeRedSmokeSystem", 0x80)
                .map(str::to_string),
            default_small_red_smoke_system: section
                .read_name("DefaultSmallRedSmokeSystem", 0x80)
                .map(str::to_string),
            default_debris_smoke_system: section
                .read_name("DefaultDebrisSmokeSystem", 0x80)
                .map(str::to_string),
            default_fire_stream_system: section
                .read_name("DefaultFireStreamSystem", 0x80)
                .map(str::to_string),
            default_test_particle_system: section
                .read_name("DefaultTestParticleSystem", 0x80)
                .map(str::to_string),
            default_repair_particle_system: section
                .read_name("DefaultRepairParticleSystem", 0x80)
                .map(str::to_string),
        }
    }
}

impl Default for CombatDamageDefaults {
    fn default() -> Self {
        Self {
            max_damage: 1000,
            collapse_chance: 100,
            ballistic_scatter: 0x100,
            crush: 0x180,
            incoming: 0,
            open_topped_warp_distance: 5,
            death_weapon: None,
            falling_damage_multiplier: 1.0,
            current_strength_damage: true,
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

    /// `Crush=` is cells read as leptons and chopped: stock `1.8` is 460, and
    /// a missing key keeps the constructor's `0x180`.
    #[test]
    fn crush_reads_cells_as_chopped_leptons() {
        let read = |text: &str| {
            let ini = IniFile::from_str(text);
            CombatDamageDefaults::from_ini_section(ini.section("CombatDamage").unwrap()).crush
        };
        assert_eq!(read("[CombatDamage]\nCrush=1.8\n"), 460);
        assert_eq!(read("[CombatDamage]\nFixtureOnly=1\n"), 0x180);
        assert_eq!(CombatDamageDefaults::default().crush, 0x180);
    }

    /// `Incoming=` is a percent read as a speed: stock `10` is 25, and a
    /// missing key keeps the constructor's 0.
    #[test]
    fn incoming_reads_through_read_speed() {
        let read = |text: &str| {
            let ini = IniFile::from_str(text);
            CombatDamageDefaults::from_ini_section(ini.section("CombatDamage").unwrap()).incoming
        };
        assert_eq!(read("[CombatDamage]\nIncoming=10\n"), 25);
        assert_eq!(read("[CombatDamage]\nFixtureOnly=1\n"), 0);
        assert_eq!(CombatDamageDefaults::default().incoming, 0);
    }
}
