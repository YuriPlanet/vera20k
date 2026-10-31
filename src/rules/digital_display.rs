//! Phobos DigitalDisplay types, retained by the Rules registry.
//!
//! Native identity: Phobos `DigitalDisplayTypeClass`
//! (`src/New/Type/DigitalDisplayTypeClass.{h,cpp}`), an
//! `Enumerable<DigitalDisplayTypeClass>` whose main section is
//! `[DigitalDisplayTypes]`. `DigitalDisplayTypeClass::LoadFromINI` re-reads
//! every retained entry's own section on each reached Rules pass, keeping the
//! previous value for an absent key. `TechnoTypeExt::DigitalDisplayTypes`
//! (`DigitalDisplayTypes=`), `DigitalDisplay.Disable=` and
//! `DigitalDisplay.Health.FakeAtDisguise=` bind a techno;
//! `RulesExt` owns the four `[AudioVisual]` default lists and the global
//! disguise-health default. `TechnoExt::ProcessDigitalDisplays` resolves an
//! explicit list first, then the category default, and draws through
//! `DigitalDisplayTypeClass::Draw`.
//!
//! This module owns the catalog reads and the immutable projection only.
//! Value extraction lives in `sim::digital_display`; positioning, visibility
//! and text/SHP emission live in `app::presentation::digital_display`.
//! `DigitalDisplay.Enable` is a client setting read from the imported
//! `VERA.INI` (seeded from `RA2MD.INI`) by
//! `app::persistence::phobos_settings`.

use crate::rules::building_select_bracket::BuildingSelectBracketPosition;
#[cfg(feature = "phobos")]
use crate::rules::ini_parser::IniFile;
#[cfg(feature = "phobos")]
use crate::rules::ini_value::truncate_native_bytes;
use crate::rules::phobos_shields::AffectedHouse;

/// `Enumerable` stores `Name[32]` with `_TRUNCATE`.
#[cfg(feature = "phobos")]
const NAME_PAYLOAD_BYTES: usize = 0x1F;

/// Phobos `DisplayInfoType` (`Utilities/Enum.h:320..343`), in declaration
/// order. `PassengerKill` and `AutoDeath` have no owner in VERA and therefore
/// never produce a displayed value.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum DigitalInfoType {
    #[default]
    Health,
    Shield,
    Ammo,
    MindControl,
    Spawns,
    Passengers,
    Tiberium,
    Experience,
    Occupants,
    GattlingStage,
    Rof,
    Reload,
    SpawnTimer,
    GattlingTimer,
    ProduceCash,
    PassengerKill,
    AutoDeath,
    SuperWeapon,
    IronCurtain,
    TemporalLife,
    FactoryProcess,
}

impl DigitalInfoType {
    #[cfg(feature = "phobos")]
    /// Phobos `read<DisplayInfoType>` (`TemplateDef.h:1504..1598`):
    /// case-insensitive exact spelling, otherwise the key retains its value.
    fn parse(raw: &str) -> Option<Self> {
        Some(match raw.to_ascii_lowercase().as_str() {
            "health" => Self::Health,
            "shield" => Self::Shield,
            "ammo" => Self::Ammo,
            "mindcontrol" => Self::MindControl,
            "spawns" => Self::Spawns,
            "passengers" => Self::Passengers,
            "tiberium" => Self::Tiberium,
            "experience" => Self::Experience,
            "occupants" => Self::Occupants,
            "gattlingstage" => Self::GattlingStage,
            "rof" => Self::Rof,
            "reload" => Self::Reload,
            "spawntimer" => Self::SpawnTimer,
            "gattlingtimer" => Self::GattlingTimer,
            "producecash" => Self::ProduceCash,
            "passengerkill" => Self::PassengerKill,
            "autodeath" => Self::AutoDeath,
            "superweapon" => Self::SuperWeapon,
            "ironcurtain" => Self::IronCurtain,
            "temporallife" => Self::TemporalLife,
            "factoryprocess" => Self::FactoryProcess,
            _ => return None,
        })
    }
}

/// Phobos `TextAlign` (`Enum.h:234..240`). `none` (`0xFFF`) sets both the
/// center and right bits, so the native `DrawTextA` arm sees center first.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum DigitalTextAlign {
    Left,
    Center,
    #[default]
    Right,
}

impl DigitalTextAlign {
    #[cfg(feature = "phobos")]
    fn parse(raw: &str) -> Option<Self> {
        Some(match raw.to_ascii_lowercase().as_str() {
            "left" => Self::Left,
            "center" | "centre" | "none" => Self::Center,
            "right" => Self::Right,
            _ => return None,
        })
    }
}

/// Phobos `HorizontalPosition` (`Enum.h:296..301`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum DigitalHorizontalAnchor {
    Left,
    Center,
    #[default]
    Right,
}

impl DigitalHorizontalAnchor {
    #[cfg(feature = "phobos")]
    fn parse(raw: &str) -> Option<Self> {
        Some(match raw.to_ascii_lowercase().as_str() {
            "left" => Self::Left,
            "center" | "centre" => Self::Center,
            "right" => Self::Right,
            _ => return None,
        })
    }

    /// `Anchor::GetRelativeOffsetHorizontal`: enum value / 2.
    pub fn offset(self) -> f32 {
        match self {
            Self::Left => 0.0,
            Self::Center => 0.5,
            Self::Right => 1.0,
        }
    }
}

/// Phobos `VerticalPosition` (`Enum.h:303..308`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum DigitalVerticalAnchor {
    #[default]
    Top,
    Center,
    Bottom,
}

impl DigitalVerticalAnchor {
    #[cfg(feature = "phobos")]
    fn parse(raw: &str) -> Option<Self> {
        Some(match raw.to_ascii_lowercase().as_str() {
            "top" => Self::Top,
            "center" | "centre" => Self::Center,
            "bottom" => Self::Bottom,
            _ => return None,
        })
    }

    pub fn offset(self) -> f32 {
        match self {
            Self::Top => 0.0,
            Self::Center => 0.5,
            Self::Bottom => 1.0,
        }
    }
}

/// Phobos `DisplayShowType` flags (`Enum.h:345..354`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DigitalShowType(u8);

impl DigitalShowType {
    pub const NONE: Self = Self(0x0);
    pub const CURSOR_HOVER: Self = Self(0x1);
    pub const SELECTED: Self = Self(0x2);
    pub const IDLE: Self = Self(0x4);
    /// The constructor default: `CursorHover | Selected`.
    pub const SELECT: Self = Self(Self::CURSOR_HOVER.0 | Self::SELECTED.0);
    pub const ALL: Self = Self(Self::SELECT.0 | Self::IDLE.0);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub fn bits(self) -> u8 {
        self.0
    }

    #[cfg(feature = "phobos")]
    /// Phobos `read<DisplayShowType>`: comma-separated flags, every token
    /// must match, and a failed token keeps the current value.
    fn parse(raw: &str) -> Option<Self> {
        let mut parsed = Self::NONE;
        for part in raw.split(',') {
            let token = part.trim();
            if token.is_empty() {
                // Phobos rejects an empty token and retains the previous value.
                return None;
            }
            let flag = match token.to_ascii_lowercase().as_str() {
                "cursorhover" => Self::CURSOR_HOVER,
                "selected" => Self::SELECTED,
                "idle" => Self::IDLE,
                "all" => Self::ALL,
                _ => return None,
            };
            parsed = Self(parsed.0 | flag.0);
        }
        Some(parsed)
    }
}

impl Default for DigitalShowType {
    fn default() -> Self {
        Self::SELECT
    }
}

/// `DigitalDisplayTypeClass`'s constructor values. Every field retains its
/// current value when the key is absent.
#[derive(Debug, Clone, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DigitalDisplayType {
    pub name: String,
    /// `Text.Color` (green), `Text.Color.ConditionYellow`, `Text.Color.ConditionRed`.
    pub text_color: [[u8; 3]; 3],
    pub text_background: bool,
    pub offset: [i32; 2],
    pub offset_shield_delta: Option<[i32; 2]>,
    pub align: DigitalTextAlign,
    pub anchor_horizontal: DigitalHorizontalAnchor,
    pub anchor_vertical: DigitalVerticalAnchor,
    pub anchor_building: BuildingSelectBracketPosition,
    pub shape: Option<String>,
    pub palette: Option<String>,
    pub shape_spacing: Option<[i32; 2]>,
    pub shape_percentage_frame: bool,
    pub percentage: bool,
    pub hide_max_value: Option<bool>,
    pub visible_to_houses: AffectedHouse,
    pub visible_to_houses_observer: bool,
    pub visible_in_special_state: bool,
    pub info_type: DigitalInfoType,
    pub info_index: i32,
    pub value_scale_divisor: Option<i32>,
    pub value_as_timer: bool,
    pub show_type: DigitalShowType,
}

impl DigitalDisplayType {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            text_color: [[0, 255, 0], [255, 255, 0], [255, 0, 0]],
            text_background: false,
            offset: [0, 0],
            offset_shield_delta: None,
            align: DigitalTextAlign::Right,
            anchor_horizontal: DigitalHorizontalAnchor::Right,
            anchor_vertical: DigitalVerticalAnchor::Top,
            anchor_building: BuildingSelectBracketPosition::Top,
            shape: None,
            palette: None,
            shape_spacing: None,
            shape_percentage_frame: false,
            percentage: false,
            hide_max_value: None,
            // `VisibleToHouses { AffectedHouse::All }` in the constructor; the
            // shipped documentation's `owner` example is not the default.
            visible_to_houses: AffectedHouse::ALL,
            visible_to_houses_observer: true,
            visible_in_special_state: true,
            info_type: DigitalInfoType::Health,
            info_index: 0,
            value_scale_divisor: None,
            value_as_timer: false,
            show_type: DigitalShowType::SELECT,
        }
    }

    /// `Nullable<bool>.Get(isInfantry)`: infantry hide the maximum by default.
    pub fn hide_max_value(&self, is_infantry: bool) -> bool {
        self.hide_max_value.unwrap_or(is_infantry)
    }

    /// `ValueScaleDivisor.Get(ValueAsTimer ? 15 : 1)`.
    pub fn divisor(&self) -> i32 {
        self.value_scale_divisor
            .unwrap_or(if self.value_as_timer { 15 } else { 1 })
    }

    /// `Shape.Spacing`, defaulting to `(4,-2)` for buildings and `(4,0)`.
    pub fn shape_spacing(&self, is_building: bool) -> [i32; 2] {
        self.shape_spacing
            .unwrap_or(if is_building { [4, -2] } else { [4, 0] })
    }

    /// `Damageable<ColorStruct>::Get(ratio)`: red at or below
    /// `ConditionRed`, yellow at or below `ConditionYellow`, else green.
    pub fn text_color_for(&self, ratio: f64, condition_yellow: f64, condition_red: f64) -> [u8; 3] {
        if ratio <= condition_red {
            self.text_color[2]
        } else if ratio <= condition_yellow {
            self.text_color[1]
        } else {
            self.text_color[0]
        }
    }

    /// `CustomPalette.GetOrDefaultConvert(FileSystem::PALETTE_PAL)`.
    pub fn palette_name(&self) -> &str {
        self.palette.as_deref().unwrap_or("palette.pal")
    }
}

/// Ordered Phobos `Enumerable<DigitalDisplayTypeClass>::Array`.
#[derive(Debug, Clone, PartialEq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct DigitalDisplayTypeCatalog {
    types: Vec<DigitalDisplayType>,
}

impl DigitalDisplayTypeCatalog {
    pub fn types(&self) -> &[DigitalDisplayType] {
        &self.types
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(|entry| entry.name.as_str())
    }

    pub fn entry(&self, name: &str) -> Option<&DigitalDisplayType> {
        self.types
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.types
            .iter()
            .position(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// Read one reached Rules pass: the pass's `[DigitalDisplayTypes]` list
    /// appends entries, then every retained entry re-reads its own section.
    #[cfg(feature = "phobos")]
    pub(crate) fn apply_pass(&mut self, pass: &IniFile) {
        if let Some(list) = pass.section("DigitalDisplayTypes") {
            // Phobos Enumerable::LoadFromINIList reads each physical key
            // through ReadString with Phobos::readBuffer[2048] before lookup.
            for key in list.physical_keys() {
                let Some(name) = list.read_name(key, 2048) else {
                    continue;
                };
                if self.entry(name).is_none() {
                    self.types
                        .push(DigitalDisplayType::named(&truncate_native_bytes(
                            name,
                            NAME_PAYLOAD_BYTES,
                        )));
                }
            }
        }
        for index in 0..self.types.len() {
            let name = self.types[index].name.clone();
            let Some(section) = pass.section(&name) else {
                continue;
            };
            let entry = &mut self.types[index];
            // ReadColorRGB and ReadMinMax only write on a present key; a
            // missing key retains the constructor/previous-pass value.
            for (key, slot) in [
                ("Text.Color", 0),
                ("Text.Color.ConditionYellow", 1),
                ("Text.Color.ConditionRed", 2),
            ] {
                if section.is_present(key) {
                    entry.text_color[slot] =
                        section.read_color_rgb(key, entry.text_color[slot], None);
                }
            }
            entry.text_background = section.read_bool("Text.Background", entry.text_background);
            entry.offset = section.read_phobos_int_pair("Offset", entry.offset);
            entry.offset_shield_delta = section
                .read_phobos_nullable_int_pair("Offset.ShieldDelta", entry.offset_shield_delta);
            entry.align = section.read_string_with("Align", 0x40, entry.align, |current, raw| {
                DigitalTextAlign::parse(raw).unwrap_or(current)
            });
            entry.anchor_horizontal = section.read_string_with(
                "Anchor.Horizontal",
                0x40,
                entry.anchor_horizontal,
                |current, raw| DigitalHorizontalAnchor::parse(raw).unwrap_or(current),
            );
            entry.anchor_vertical = section.read_string_with(
                "Anchor.Vertical",
                0x40,
                entry.anchor_vertical,
                |current, raw| DigitalVerticalAnchor::parse(raw).unwrap_or(current),
            );
            entry.anchor_building = section.read_string_with(
                "Anchor.Building",
                0x40,
                entry.anchor_building,
                |current, raw| BuildingSelectBracketPosition::parse(raw).unwrap_or(current),
            );
            // `Valueable<SHPStruct*>` retains the loaded pointer when the key
            // is absent; only a present key can set or clear it.
            // Phobos' SHP reader adds `.shp` when the name lacks it. Residual:
            // it also keeps the previous image when the named file fails to
            // load, which a text-only rules read cannot check.
            if section.is_present("Shape") {
                entry.shape = section.read_name("Shape", 0x100).map(|name| {
                    if name.to_ascii_lowercase().ends_with(".shp") {
                        name.to_owned()
                    } else {
                        format!("{name}.shp")
                    }
                });
            }
            if let Some(palette) = section.read_name("Palette", 0x100) {
                entry.palette = Some(palette.to_owned());
            }
            entry.shape_spacing =
                section.read_phobos_nullable_int_pair("Shape.Spacing", entry.shape_spacing);
            entry.shape_percentage_frame =
                section.read_bool("Shape.PercentageFrame", entry.shape_percentage_frame);
            entry.percentage = section.read_bool("Percentage", entry.percentage);
            entry.hide_max_value =
                section.read_phobos_nullable_bool("HideMaxValue", entry.hide_max_value);
            entry.visible_to_houses = section.read_string_with(
                "VisibleToHouses",
                0x100,
                entry.visible_to_houses,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            );
            entry.visible_to_houses_observer =
                section.read_bool("VisibleToHouses.Observer", entry.visible_to_houses_observer);
            entry.visible_in_special_state =
                section.read_bool("VisibleInSpecialState", entry.visible_in_special_state);
            entry.info_type =
                section.read_string_with("InfoType", 0x40, entry.info_type, |current, raw| {
                    DigitalInfoType::parse(raw).unwrap_or(current)
                });
            entry.info_index = section.read_int("InfoIndex", entry.info_index);
            entry.value_scale_divisor =
                section.read_phobos_nullable_int("ValueScaleDivisor", entry.value_scale_divisor);
            entry.value_as_timer = section.read_bool("ValueAsTimer", entry.value_as_timer);
            entry.show_type =
                section.read_string_with("ShowType", 0x100, entry.show_type, |current, raw| {
                    DigitalShowType::parse(raw).unwrap_or(current)
                });
        }
    }
}

/// `RulesExt`'s global DigitalDisplay fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DigitalDisplayGlobals {
    pub buildings_default: Vec<String>,
    pub infantry_default: Vec<String>,
    pub vehicles_default: Vec<String>,
    pub aircraft_default: Vec<String>,
    pub health_fake_at_disguise: bool,
}

impl Default for DigitalDisplayGlobals {
    fn default() -> Self {
        // RulesExt constructor: only the disguise-health default is true.
        Self {
            buildings_default: Vec::new(),
            infantry_default: Vec::new(),
            vehicles_default: Vec::new(),
            aircraft_default: Vec::new(),
            health_fake_at_disguise: true,
        }
    }
}

impl DigitalDisplayGlobals {
    /// `RulesExt::LoadFromINIFile` follows `[AudioVisual]`; the four lists are
    /// `ValueableVector<DigitalDisplayTypeClass*>` and the disguise default is
    /// `Valueable<bool>`.
    #[cfg(feature = "phobos")]
    pub(crate) fn from_ini(ini: &IniFile) -> Self {
        let section = ini.section_or_empty("AudioVisual");
        // Phobos `Parser<T>::Parse` skips leading spaces after each comma.
        let list = |key: &str| {
            section
                .read_list(key, 0x100)
                .map(|tokens| {
                    tokens
                        .into_iter()
                        .map(|token| token.trim().to_owned())
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            buildings_default: list("Buildings.DefaultDigitalDisplayTypes"),
            infantry_default: list("Infantry.DefaultDigitalDisplayTypes"),
            vehicles_default: list("Vehicles.DefaultDigitalDisplayTypes"),
            aircraft_default: list("Aircraft.DefaultDigitalDisplayTypes"),
            health_fake_at_disguise: section
                .read_bool("DigitalDisplay.Health.FakeAtDisguise", true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "phobos")]
    use crate::rules::ini_parser::IniFile;

    #[cfg(feature = "phobos")]
    #[test]
    fn catalog_reads_constructor_defaults_and_enum_spellings() {
        let ini = IniFile::from_str(
            "[DigitalDisplayTypes]\n0=HP\n\n[HP]\nInfoType=SHIELD\nInfoIndex=2\n\
             Align=centre\nAnchor.Horizontal=center\nAnchor.Building=RightBottom\n\
             HideMaxValue=no\nValueScaleDivisor=20\nShowType=cursorhover, idle\n\
             VisibleToHouses=owner, allies\nText.Color=1,2,3\n\
             Text.Color.ConditionYellow=4,5,6\nText.Color.ConditionRed=7,8,9\n",
        );
        let mut catalog = DigitalDisplayTypeCatalog::default();
        catalog.apply_pass(&ini);
        let entry = catalog.entry("hp").expect("case-insensitive entry");
        assert_eq!(entry.info_type, DigitalInfoType::Shield);
        assert_eq!(entry.info_index, 2);
        assert_eq!(entry.align, DigitalTextAlign::Center);
        assert_eq!(entry.anchor_horizontal, DigitalHorizontalAnchor::Center);
        assert_eq!(
            entry.anchor_building,
            BuildingSelectBracketPosition::RightBottom
        );
        assert_eq!(entry.hide_max_value, Some(false));
        assert_eq!(entry.divisor(), 20);
        assert!(entry.show_type.contains(DigitalShowType::CURSOR_HOVER));
        assert!(entry.show_type.contains(DigitalShowType::IDLE));
        assert!(!entry.show_type.contains(DigitalShowType::SELECTED));
        assert!(entry.visible_to_houses.contains(AffectedHouse::OWNER));
        assert!(entry.visible_to_houses.contains(AffectedHouse::ALLIES));
        assert!(!entry.visible_to_houses.contains(AffectedHouse::ENEMIES));
        assert_eq!(entry.text_color, [[1, 2, 3], [4, 5, 6], [7, 8, 9]]);
        assert_eq!(entry.shape_spacing(false), [4, 0]);
        assert_eq!(entry.shape_spacing(true), [4, -2]);
        assert_eq!(entry.anchor_vertical, DigitalVerticalAnchor::Top);
        assert_eq!(entry.palette_name(), "palette.pal");
        assert!(entry.visible_to_houses_observer);
        assert!(entry.visible_in_special_state);
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn absent_keys_retain_the_previous_pass_and_later_layers_append() {
        let first = IniFile::from_str(
            "[DigitalDisplayTypes]\n0=A\n1=B\n[A]\nInfoType=Ammo\nPercentage=yes\n",
        );
        let second = IniFile::from_str("[DigitalDisplayTypes]\n0=C\n[A]\nInfoIndex=3\n");
        let mut catalog = DigitalDisplayTypeCatalog::default();
        catalog.apply_pass(&first);
        catalog.apply_pass(&second);
        assert_eq!(catalog.types().len(), 3);
        let entry = catalog.entry("A").unwrap();
        assert_eq!(entry.info_type, DigitalInfoType::Ammo);
        assert!(entry.percentage);
        assert_eq!(entry.info_index, 3);
        assert_eq!(entry.info_type, DigitalInfoType::Ammo);
        assert_eq!(
            catalog.entry("B").unwrap().info_type,
            DigitalInfoType::Health
        );
    }

    #[test]
    fn text_colour_thresholds_are_inclusive_at_the_lower_colour() {
        let entry = DigitalDisplayType::named("X");
        assert_eq!(entry.text_color_for(1.0, 0.5, 0.25), [0, 255, 0]);
        assert_eq!(entry.text_color_for(0.5, 0.5, 0.25), [255, 255, 0]);
        assert_eq!(entry.text_color_for(0.25, 0.5, 0.25), [255, 0, 0]);
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn globals_default_the_disguise_health_switch_to_yes() {
        let ini = IniFile::from_str("[AudioVisual]\n");
        assert!(DigitalDisplayGlobals::from_ini(&ini).health_fake_at_disguise);
        assert!(DigitalDisplayGlobals::default().health_fake_at_disguise);
        let off = IniFile::from_str("[AudioVisual]\nDigitalDisplay.Health.FakeAtDisguise=no\n");
        assert!(!DigitalDisplayGlobals::from_ini(&off).health_fake_at_disguise);
    }

    #[test]
    fn value_as_timer_defaults_the_divisor_to_fifteen() {
        let mut entry = DigitalDisplayType::named("X");
        assert_eq!(entry.divisor(), 1);
        entry.value_as_timer = true;
        assert_eq!(entry.divisor(), 15);
        entry.value_scale_divisor = Some(20);
        assert_eq!(entry.divisor(), 20);
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn explicit_lists_win_and_unknown_names_are_skipped() {
        use crate::rules::ruleset::RuleSet;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[DigitalDisplayTypes]\n0=HP\n1=AMMO\n\
             [HP]\nInfoType=Health\n[AMMO]\nInfoType=Ammo\n\
             [AudioVisual]\nVehicles.DefaultDigitalDisplayTypes=HP, AMMO\n\
             [VehicleTypes]\n0=TEST\n1=EXPLICIT\n\
             [TEST]\nStrength=100\n\
             [EXPLICIT]\nStrength=100\nDigitalDisplayTypes=AMMO, NOPE\n",
        ))
        .unwrap();
        assert_eq!(
            rules
                .digital_display_indices(rules.object("TEST").unwrap())
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            rules
                .digital_display_indices(rules.object("EXPLICIT").unwrap())
                .collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn default_visibility_follows_the_constructor_not_the_documentation() {
        // DigitalDisplayTypeClass's constructor stores AffectedHouse::All.
        let entry = DigitalDisplayType::named("X");
        assert_eq!(entry.visible_to_houses, AffectedHouse::ALL);
        assert_eq!(entry.show_type, DigitalShowType::SELECT);
        assert_eq!(entry.align, DigitalTextAlign::Right);
    }

    /// Phobos `Read2Integers` trims spaces and takes `$`/`h` tokens, writes a
    /// one-token `Valueable<Point2D>` read into X only, and its SHP reader
    /// adds `.shp`; gamemd's `ReadMinMax` scan did neither.
    #[cfg(feature = "phobos")]
    #[test]
    fn pair_keys_use_the_phobos_scan_and_shape_gains_its_extension() {
        let ini = IniFile::from_str(
            "[DigitalDisplayTypes]\n0=A\n[A]\nOffset=5\nOffset.ShieldDelta= 1 , $2\n\
             Shape.Spacing=Ah,3\nShape=DIGITS\n",
        );
        let mut catalog = DigitalDisplayTypeCatalog::default();
        catalog.apply_pass(&ini);
        let entry = catalog.entry("A").unwrap();
        assert_eq!(entry.offset, [5, 0]);
        assert_eq!(entry.offset_shield_delta, Some([1, 2]));
        assert_eq!(entry.shape_spacing, Some([10, 3]));
        assert_eq!(entry.shape.as_deref(), Some("DIGITS.shp"));
    }
}
