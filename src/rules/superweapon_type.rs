//! Superweapon type definitions parsed from rules(md).ini.
//!
//! Each superweapon has its own `[SwName]` section in rules.ini, listed under
//! `[SuperWeaponTypes]`. Defines charging time, cursor action, sidebar image,
//! and the dispatch kind that controls what happens on launch.
//!
//! ## rules.ini format
//! ```ini
//! [LightningStormSpecial]
//! UIName=Name:Storm
//! Type=LightningStorm
//! RechargeTime=10
//! SidebarImage=BOLTICON
//! Action=LightningStorm
//! ```
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ini_value::truncate_native_bytes;

/// `SuperWeaponTypeClass` constructor recharge, `0x1194` frames (5 minutes)
/// at `0x006CE5E9`.
const CTOR_RECHARGE_FRAMES: i32 = 4500;

/// Maps the INI `Type=` string to an enum for launch dispatch.
///
/// Index values match gamemd.exe's SuperWeaponType enum (0x006CEA20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuperWeaponKind {
    /// Type=MultiMissile (index 0) — Nuclear missile.
    MultiMissile,
    /// Type=IronCurtain (index 1) — Unit invulnerability.
    IronCurtain,
    /// Type=LightningStorm (index 2) — Weather storm.
    LightningStorm,
    /// Type=ChronoSphere (index 3) — Source cell selection.
    ChronoSphere,
    /// Type=ChronoWarp (index 4) — Destination warp.
    ChronoWarp,
    /// Type=ParaDrop (index 5) — Paratroop delivery.
    ParaDrop,
    /// Type=AmerParaDrop (index 6) — American paratroop variant.
    AmerParaDrop,
    /// Type=PsychicDominator (index 7) — Area mind control.
    PsychicDominator,
    /// Type=SpyPlane (index 8) — Recon flyover.
    SpyPlane,
    /// Type=GeneticConverter (index 9) — Infantry mutation.
    GeneticConverter,
    /// Type=ForceShield (index 10) — Building invulnerability.
    ForceShield,
    /// Type=PsychicReveal (index 11) — Shroud reveal.
    PsychicReveal,
}

impl SuperWeaponKind {
    /// The native `Type=` value (`SuperWeaponTypeClass+0xB4`).
    pub const fn native_index(self) -> i32 {
        match self {
            Self::MultiMissile => 0,
            Self::IronCurtain => 1,
            Self::LightningStorm => 2,
            Self::ChronoSphere => 3,
            Self::ChronoWarp => 4,
            Self::ParaDrop => 5,
            Self::AmerParaDrop => 6,
            Self::PsychicDominator => 7,
            Self::SpyPlane => 8,
            Self::GeneticConverter => 9,
            Self::ForceShield => 10,
            Self::PsychicReveal => 11,
        }
    }

    /// Parse from the INI `Type=` string value. Case-insensitive.
    pub fn from_ini_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "multimissile" => Some(Self::MultiMissile),
            "ironcurtain" => Some(Self::IronCurtain),
            "lightningstorm" => Some(Self::LightningStorm),
            "chronosphere" => Some(Self::ChronoSphere),
            "chronowarp" => Some(Self::ChronoWarp),
            "paradrop" => Some(Self::ParaDrop),
            "amerparadrop" => Some(Self::AmerParaDrop),
            "psychicdominator" => Some(Self::PsychicDominator),
            "spyplane" => Some(Self::SpyPlane),
            "geneticconverter" => Some(Self::GeneticConverter),
            "forceshield" => Some(Self::ForceShield),
            "psychicreveal" => Some(Self::PsychicReveal),
            _ => None,
        }
    }
}

/// Superweapon type definition parsed from a rules.ini section.
///
/// Each superweapon type defines charging behavior, sidebar presentation,
/// targeting cursor, and the launch dispatch kind. Runtime instances are
/// tracked per-house in `sim/superweapon/`.
#[derive(Debug, Clone)]
pub struct SuperWeaponType {
    /// Section name in rules.ini (e.g., "LightningStormSpecial").
    pub id: String,
    /// CSF label for the localized display name (`UIName=Name:LStorm`). This
    /// is what the sidebar cameo tooltip shows for a superweapon slot — the
    /// section name is an internal identifier and never reaches the player.
    pub ui_name: Option<String>,
    /// Launch dispatch type (determines what happens on fire).
    pub kind: SuperWeaponKind,
    /// Charge time in game frames. INI value is minutes × 900.
    pub recharge_time_frames: i32,
    /// Whether charging pauses when the owner has low power.
    pub is_powered: bool,
    /// Cursor action name for targeting (e.g., "LightningStorm").
    pub action: Option<String>,
    /// SHP filename for sidebar cameo (e.g., "INTICON").
    pub sidebar_image: Option<String>,
    /// Whether to show the charge timer countdown on the sidebar.
    pub show_timer: bool,
    /// Whether this SW can be disabled from the game lobby.
    pub disableable_from_shell: bool,
    /// Charge→drain cycling mode (used by Force Shield).
    pub use_charge_drain: bool,
    /// Two-click mode: first click selects source (ChronoSphere).
    pub pre_click: bool,
    /// Two-click mode: second click selects destination (ChronoWarp).
    pub post_click: bool,
    /// `PreDependent=` (`+0xF0`, constructor -1): the `Type=` name it reads
    /// matched case-insensitively (`_strcmpi @ 0x007C8D20`) against the 12
    /// Type names at `0x008425C0` (`0x006CEC98..0x006CECE9`); an empty or
    /// unknown name keeps the field. Fire_SW indexes the house's Supers with
    /// it, so it names the `[SuperWeaponTypes]` entry at that position
    /// (retail ChronoWarpSpecial: `ChronoSphere`, 3).
    pub pre_dependent: i32,
    /// `WeaponType=` (+9C), the WeaponType the ordered Rules processor
    /// resolved (e.g., NukeCarrier for the nuke).
    pub weapon_type: Option<String>,
    /// Required secondary building (e.g., NukeSilo for nuke).
    pub aux_building: Option<String>,
    /// `SpecialSound=` (`+0xC0`, `0x006CEB74`): the cue SuperClass::AI
    /// plays when a Force Shield's countdown runs out (`0x006CBCCF`). A read
    /// stores VocClass::FindIndex (`0x007514D0`) of the name; an absent key
    /// or an unknown name keeps the field (`0x006CEB96`), the constructor's
    /// -1 or an earlier layer's sound.
    /// RESIDUAL: the last non-empty name wins here, so a later layer naming
    /// an unknown sound silences a cue an earlier layer set, where native
    /// keeps it. Trigger: a mode INI or map naming an unknown sound after an
    /// earlier layer named a known one. Effect: the cue does not play.
    pub special_sound: Option<String>,
    /// `StartSound=` (`+0xC4`, `0x006CEBB0`, read as `SpecialSound=` with
    /// the same residual, `0x006CEBD8`): the cue the Force Shield's launch
    /// plays (`0x006CD176`).
    pub start_sound: Option<String>,
    /// `AIDefendAgainst=` (`+0xEC`, ReadBool at `0x006CEAF7`, constructor
    /// clear): a launch of this type alerts each computer house whose base is
    /// near the target (`HouseClass::Fire_SW @ 0x004FAE50`'s house loop).
    pub ai_defend_against: bool,
    /// When true, suspension doesn't auto-resume on power restore.
    pub manual_control: bool,
}

impl SuperWeaponType {
    /// Parse a SuperWeaponType from a rules.ini section
    /// (`SuperWeaponTypeClass::ReadINI`, reads `0x006CEA6D`-`0x006CEDB1`).
    pub fn from_ini_section(id: &str, section: &IniSection) -> Option<Self> {
        // ReadString 0x28 ahead of the kind lookup (`0x006CEC2D`).
        let kind = SuperWeaponKind::from_ini_str(section.read_name("Type", 0x28)?)?;
        Some(Self {
            id: id.to_string(),
            // AbstractTypeClass `UIName=`, ReadString 0x20 (`0x00410AFB`).
            ui_name: section.read_name("UIName", 0x20).map(str::to_string),
            kind,
            // `0x006CED6E` ReadDouble(0.0): zero keeps the field, anything
            // else stores `ftol(minutes * 900.0)` (`0x006CED80`). An `f32`
            // minute count times 900 is exact in `f64`, so the cast chops
            // the same product.
            recharge_time_frames: section.read_double_with(
                "RechargeTime",
                CTOR_RECHARGE_FRAMES,
                |frames, minutes| {
                    if minutes == 0.0 {
                        frames
                    } else {
                        (minutes * 900.0) as i32
                    }
                },
            ),
            is_powered: section.read_bool("IsPowered", true),
            // ReadAction's 0x20-byte ReadString (`0x00474EE0`).
            action: section.read_name("Action", 0x20).map(str::to_string),
            // ReadString 0x100, then a 24-byte copy (`0x006CEDB1`,
            // `0x006CEDD6`) that names the cameo.
            sidebar_image: section
                .read_name("SidebarImage", 0x100)
                .map(|name| truncate_native_bytes(name, 0x18).to_string()),
            show_timer: section.read_bool("ShowTimer", false),
            disableable_from_shell: section.read_bool("DisableableFromShell", false),
            use_charge_drain: section.read_bool("UseChargeDrain", false),
            pre_click: section.read_bool("PreClick", false),
            post_click: section.read_bool("PostClick", false),
            // ReadString 0x28 (`0x006CEC98`).
            pre_dependent: section
                .read_name("PreDependent", 0x28)
                .and_then(SuperWeaponKind::from_ini_str)
                .map_or(-1, SuperWeaponKind::native_index),
            // Constructor null; only the ordered Process owner reads it.
            weapon_type: None,
            // ReadString 0x80, then BuildingTypeClass::FindOrAllocate
            // (`0x006CED1F`), which answers `none` and `<none>` null.
            aux_building: section
                .read_type_name("AuxBuilding", 0x80)
                .map(str::to_string),
            special_sound: section.read_name("SpecialSound", 0x80).map(str::to_string),
            start_sound: section.read_name("StartSound", 0x80).map(str::to_string),
            ai_defend_against: section.read_bool("AIDefendAgainst", false),
            manual_control: section.read_bool("ManualControl", false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn parse_lightning_storm_special() {
        // Verbatim from `ini/rulesmd.ini` [LightningStormSpecial].
        let ini_text = "\
[LightningStormSpecial]
UIName=Name:Storm
Name=Lightning Storm
Type=LightningStorm
RechargeTime=10
SidebarImage=BOLTICON
Action=LightningStorm
IsPowered=yes
ShowTimer=yes
DisableableFromShell=yes
";
        let ini = IniFile::from_str(ini_text);
        let section = ini.section("LightningStormSpecial").unwrap();
        let sw = SuperWeaponType::from_ini_section("LightningStormSpecial", section).unwrap();
        assert_eq!(sw.kind, SuperWeaponKind::LightningStorm);
        assert_eq!(sw.recharge_time_frames, 9000); // 10 min × 900
        assert!(sw.is_powered);
        assert!(sw.show_timer);
        assert!(sw.disableable_from_shell);
        // The sidebar cameo tooltip shows the localized UIName, not `Name=`
        // and not the section id. Retail value, not a plausible-looking one.
        assert_eq!(sw.ui_name.as_deref(), Some("Name:Storm"));
        assert_eq!(sw.sidebar_image.as_deref(), Some("BOLTICON"));
    }

    #[test]
    fn ui_name_is_absent_when_the_section_omits_it() {
        let ini = IniFile::from_str("[NoNameSW]\nType=MultiMissile\n");
        let section = ini.section("NoNameSW").unwrap();
        let sw = SuperWeaponType::from_ini_section("NoNameSW", section).unwrap();
        assert_eq!(sw.ui_name, None);
    }

    #[test]
    fn parse_unknown_type_returns_none() {
        let ini_text = "\
[BogusWeapon]
Type=BogusType
";
        let ini = IniFile::from_str(ini_text);
        let section = ini.section("BogusWeapon").unwrap();
        assert!(SuperWeaponType::from_ini_section("BogusWeapon", section).is_none());
    }

    #[test]
    fn kind_from_ini_str_case_insensitive() {
        assert_eq!(
            SuperWeaponKind::from_ini_str("LightningStorm"),
            Some(SuperWeaponKind::LightningStorm)
        );
        assert_eq!(
            SuperWeaponKind::from_ini_str("lightningstorm"),
            Some(SuperWeaponKind::LightningStorm)
        );
        assert_eq!(
            SuperWeaponKind::from_ini_str("MULTIMISSILE"),
            Some(SuperWeaponKind::MultiMissile)
        );
        assert_eq!(SuperWeaponKind::from_ini_str("bogus"), None);
    }
}
