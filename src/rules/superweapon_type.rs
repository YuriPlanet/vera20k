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
/// Stock values match gamemd.exe's SuperWeaponType enum (0x006CEA20).
/// Extensions use explicit tags outside its twelve dispatch slots.
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
    /// Ares extension: detonate `SW.Warhead` at the selected cell.
    GenericWarhead,
    /// Ares reinforcement delivery extension.
    UnitDelivery,
}

impl SuperWeaponKind {
    /// Retail Type value; extensions use identifiers outside 0..12.
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
            Self::UnitDelivery => 0x101,
            Self::GenericWarhead => -1,
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
            #[cfg(feature = "ares")]
            "unitdelivery" => Some(Self::UnitDelivery),
            #[cfg(feature = "ares")]
            "genericwarhead" => Some(Self::GenericWarhead),
            _ => None,
        }
    }
}

/// The supported Ares GenericWarhead payload and automatic targeting mode.
/// Ares sources: `src/Misc/SWTypes/GenericWarhead.cpp` and
/// `src/Ext/SWType/Body.cpp` in https://github.com/Ares-Developers/Ares.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericWarheadSettings {
    /// Bound by the ordered Rules processor, including otherwise unreferenced
    /// Warheads. An absent/null reference makes the launch unavailable.
    pub warhead: Option<String>,
    pub damage: i32,
    pub ai_targeting: GenericWarheadAiTargeting,
    pub targeting: crate::rules::superweapon_targeting::SuperWeaponTargeting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericWarheadAiTargeting {
    None,
    Offensive,
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
    /// `Range=` (`SuperWeaponTypeClass+0xF8`), a stored native float. The
    /// constructor initializes it to `0.0` (`0x006CE5B0`); `ReadINI @
    /// 0x006CEA20` reads the current value with `ReadDouble @ 0x005283D0`
    /// and stores the result as a float at `0x006CEC01`, with no later clamp.
    pub range: f32,
    /// `LineMultiplier=` (`SuperWeaponTypeClass+0xFC`), a signed integer.
    /// The constructor initializes it to `0` (`0x006CE5B0`); `ReadINI @
    /// 0x006CEA20` reads the current value with `ReadInt @ 0x005276D0` at
    /// `0x006CEC0E`, with no later clamp.
    pub line_multiplier: i32,
    /// Charge time in game frames. INI value is minutes × 900.
    pub recharge_time_frames: i32,
    /// Whether charging pauses when the owner has low power.
    pub is_powered: bool,
    /// Cursor action name for targeting (e.g., "LightningStorm").
    pub action: Option<String>,
    /// SHP filename for sidebar cameo (e.g., "INTICON").
    pub sidebar_image: Option<String>,
    /// Optional Ares PCX; the existing SHP remains the loading fallback.
    #[cfg(feature = "ares")]
    sidebar_pcx: Option<String>,
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
    /// `AuxBuilding=` (+C8), the BuildingType the ordered Rules processor
    /// resolved: a required secondary building (e.g., NukeSilo for nuke).
    pub aux_building: Option<String>,
    /// Resolved optional extension state; ordered Process owns its reads.
    pub unit_delivery: Option<UnitDeliveryRules>,
    /// Optional Ares building gates, bound by ordered rules processing.
    /// `None` leaves all stock grant/fire paths unchanged.
    pub building_requirements: Option<SuperWeaponBuildingRequirements>,
    /// Ares `Money.Amount`: signed credits settled once at accepted launch.
    /// Negative amounts are costs; zero leaves the stock path unchanged.
    pub money_amount: i32,
    /// Owner-local refusal cue resolved against the fixed EVAMD catalog.
    /// The process audio binder owns defaults and ordered unknown retention.
    pub insufficient_funds_eva: Option<String>,
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
    /// Present only for the explicitly selected Ares extension; stock types
    /// retain their existing readers and behavior.
    pub generic_warhead: Option<GenericWarheadSettings>,
}

impl SuperWeaponType {
    /// Ares3.0 SWType extension reader10033B50..10033B66, PCXFile100762C0.
    #[cfg(feature = "ares")]
    pub fn sidebar_pcx(&self) -> Option<&str> {
        self.sidebar_pcx.as_deref()
    }

    /// Parse a SuperWeaponType from a rules.ini section
    /// (`SuperWeaponTypeClass::ReadINI`, reads `0x006CEA6D`-`0x006CEDB1`).
    /// Ares extensions require the explicit `ares` build feature.
    pub fn from_ini_section(id: &str, section: &IniSection) -> Option<Self> {
        // Native reads Range and LineMultiplier before Type. The constructor
        // owns the zero defaults; keep the same current-value defaults here.
        let range = section.read_float("Range", 0.0);
        let line_multiplier = section.read_int("LineMultiplier", 0);
        // ReadString 0x28 ahead of the kind lookup (`0x006CEC2D`).
        let kind = SuperWeaponKind::from_ini_str(section.read_name("Type", 0x28)?)?;
        #[cfg(feature = "ares")]
        let extension_kind = matches!(
            kind,
            SuperWeaponKind::GenericWarhead | SuperWeaponKind::UnitDelivery
        );
        #[cfg(not(feature = "ares"))]
        let extension_kind = false;
        Some(Self {
            id: id.to_string(),
            // AbstractTypeClass `UIName=`, ReadString 0x20 (`0x00410AFB`).
            ui_name: section.read_name("UIName", 0x20).map(str::to_string),
            kind,
            range,
            line_multiplier,
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
            // Ares LoadFromRulesFile installs its custom action for new SWs;
            // InitializeConstants supplies an Attack cursor. Stock ReadAction
            // keeps its 0x20-byte ReadString (`0x00474EE0`).
            action: match kind {
                #[cfg(feature = "ares")]
                SuperWeaponKind::GenericWarhead => Some("Attack".to_string()),
                #[cfg(feature = "ares")]
                SuperWeaponKind::UnitDelivery => Some(
                    section
                        .read_name("Action", 0x20)
                        .filter(|action| !action.eq_ignore_ascii_case("Custom"))
                        .unwrap_or("ParaDrop")
                        .to_string(),
                ),
                _ => section.read_name("Action", 0x20).map(str::to_string),
            },
            // ReadString 0x100, then a 24-byte copy (`0x006CEDB1`,
            // `0x006CEDD6`) that names the cameo.
            sidebar_image: section
                .read_name("SidebarImage", 0x100)
                .map(|name| truncate_native_bytes(name, 0x18).to_string()),
            #[cfg(feature = "ares")]
            sidebar_pcx: section.read_ares_pcx_filename("SidebarPCX"),
            show_timer: section.read_bool("ShowTimer", false),
            disableable_from_shell: section.read_bool("DisableableFromShell", false),
            use_charge_drain: section.read_bool("UseChargeDrain", false),
            // Ares LoadFromINIFile derives these from the new type's Flags,
            // not user tags. GenericWarhead is an ordinary one-click type.
            pre_click: !extension_kind && section.read_bool("PreClick", false),
            post_click: !extension_kind && section.read_bool("PostClick", false),
            // ReadString 0x28 (`0x006CEC98`).
            pre_dependent: if extension_kind {
                -1
            } else {
                section
                    .read_name("PreDependent", 0x28)
                    .and_then(SuperWeaponKind::from_ini_str)
                    .filter(|kind| (0..12).contains(&kind.native_index()))
                    .map_or(-1, SuperWeaponKind::native_index)
            },
            // Constructor nulls; only the ordered Process owner reads them.
            weapon_type: None,
            aux_building: None,
            building_requirements: None,
            #[cfg(feature = "ares")]
            money_amount: section.read_ares_int("Money.Amount", 0),
            #[cfg(not(feature = "ares"))]
            money_amount: 0,
            insufficient_funds_eva: None,
            unit_delivery: None,
            special_sound: section.read_name("SpecialSound", 0x80).map(str::to_string),
            start_sound: section.read_name("StartSound", 0x80).map(str::to_string),
            ai_defend_against: section.read_bool("AIDefendAgainst", false),
            manual_control: section.read_bool("ManualControl", false),
            #[cfg(not(feature = "ares"))]
            generic_warhead: None,
            #[cfg(feature = "ares")]
            generic_warhead: (kind == SuperWeaponKind::GenericWarhead).then(|| {
                let ai_targeting = match section.read_name("SW.AITargeting", 0x40) {
                    None => GenericWarheadAiTargeting::Offensive,
                    Some(mode) if mode.eq_ignore_ascii_case("Offensive") => {
                        GenericWarheadAiTargeting::Offensive
                    }
                    Some(mode) => {
                        if !mode.eq_ignore_ascii_case("None") {
                            log::warn!("SuperWeapon '{id}': unsupported SW.AITargeting={mode}");
                        }
                        GenericWarheadAiTargeting::None
                    }
                };
                GenericWarheadSettings {
                    warhead: None,
                    damage: section.read_int("SW.Damage", 0),
                    ai_targeting,
                    targeting: crate::rules::superweapon_targeting::SuperWeaponTargeting::from_ini(
                        section,
                    ),
                }
            }),
        })
    }
}

/// Canonical, category-distinct TechnoType identity for a delivery payload.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DeliveryType {
    pub category: crate::rules::object_type::ObjectCategory,
    pub id: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum DeliveryOwner {
    #[default]
    Invoker,
    Neutral,
    Special,
    Civilian,
}

/// UnitDelivery settings. No setting changes stock superweapon behavior.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct UnitDeliveryRules {
    pub types: Vec<DeliveryType>,
    /// Effective frame delay; the Ares negative sentinel is normalized to 20.
    pub deferment: i32,
    pub owner: DeliveryOwner,
    pub base_normal: bool,
    pub activation_sound: Option<String>,
}

impl Default for UnitDeliveryRules {
    fn default() -> Self {
        Self {
            types: Vec::new(),
            deferment: 20,
            owner: DeliveryOwner::Invoker,
            base_normal: true,
            activation_sound: None,
        }
    }
}

/// Ares `SW.AuxBuildings` (any present) and `SW.NegBuildings` (none present).
///
/// Ported from the author's earlier ra2-mono work
/// (GameLogicManager.SuperweaponAvailability.cs,
/// IsSuperweaponBlockedByMissingAuxBuildings / IsSuperweaponBlockedByNegativeBuildings)
/// and contributed under VERA's GPLv3.
/// Presence is supplied by the host engine, not cached in a second world model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SuperWeaponBuildingRequirements {
    pub auxiliary: Vec<String>,
    pub negative: Vec<String>,
}

impl SuperWeaponBuildingRequirements {
    #[cfg(feature = "ares")]
    pub fn allows(&self, mut present: impl FnMut(&str) -> bool) -> bool {
        (self.auxiliary.is_empty() || self.auxiliary.iter().any(|id| present(id)))
            && !self.negative.iter().any(|id| present(id))
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
    fn retail_battle_nuke_range_fields_follow_production_rules_reader() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let nuke = retail.rules.super_weapon("NukeSpecial").unwrap();
        assert_eq!(nuke.range, 7.0);
        assert_eq!(nuke.line_multiplier, 2);
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

    #[cfg(feature = "ares")]
    #[test]
    fn parse_ares_generic_warhead() {
        let ini =
            IniFile::from_str("[Strike]\nType=GenericWarhead\nSW.Warhead=Payload\nSW.Damage=75\n");
        let sw = SuperWeaponType::from_ini_section("Strike", ini.section("Strike").unwrap())
            .expect("Ares GenericWarhead is a supported superweapon type");
        assert_eq!(
            sw.kind.native_index(),
            -1,
            "extensions have no retail Type index"
        );
        assert_eq!(sw.generic_warhead.as_ref().unwrap().damage, 75);
        assert_eq!(sw.action.as_deref(), Some("Attack"));
        let defaults = IniFile::from_str("[Default]\nType=gEnErIcWaRhEaD\n");
        let default_sw =
            SuperWeaponType::from_ini_section("Default", defaults.section("Default").unwrap())
                .unwrap();
        assert_eq!(default_sw.generic_warhead.as_ref().unwrap().damage, 0);
        assert_eq!(
            default_sw.generic_warhead.as_ref().unwrap().ai_targeting,
            GenericWarheadAiTargeting::Offensive
        );
    }

    #[cfg(not(feature = "ares"))]
    #[test]
    fn default_build_rejects_extension_kinds_and_ignores_money_tags() {
        let ini = IniFile::from_str(
            "[Strike]\nType=GenericWarhead\nSW.Warhead=Payload\nSW.Damage=75\n\
             [Delivery]\nType=UnitDelivery\nDeliver.Types=UNIT\n\
             [Reveal]\nType=PsychicReveal\nMoney.Amount=-500\nRechargeTime=2\n\
             Action=PsychicReveal\nRange=7\nLineMultiplier=2\nSW.RequiresTarget=unknown-mask\n",
        );
        for id in ["Strike", "Delivery"] {
            assert!(SuperWeaponType::from_ini_section(id, ini.section(id).unwrap()).is_none());
        }
        let stock = SuperWeaponType::from_ini_section("Reveal", ini.section("Reveal").unwrap())
            .expect("native types remain available with extension tags present");
        assert_eq!(stock.kind, SuperWeaponKind::PsychicReveal);
        assert_eq!(stock.recharge_time_frames, 1800);
        assert_eq!(stock.action.as_deref(), Some("PsychicReveal"));
        assert_eq!(stock.range, 7.0);
        assert_eq!(stock.line_multiplier, 2);
        assert_eq!(stock.money_amount, 0);
        assert!(stock.generic_warhead.is_none());
        assert!(stock.unit_delivery.is_none());
    }
}
