//! Phobos `[SelectBoxTypes]` catalog and the two `[AudioVisual]` defaults.
//!
//! Sources: `New/Type/SelectBoxTypeClass.cpp:9-34` (read order, section
//! `SelectBoxTypes`), `New/Type/SelectBoxTypeClass.h:9-43` (fields and
//! constructor defaults), `Ext/TechnoType/Body.cpp:999-1000` (the TechnoType
//! `SelectBox=` / `HideSelectBox=` keys) and `Ext/Rules/Body.cpp:333-334`
//! (`[AudioVisual] DefaultInfantrySelectBox=` / `DefaultUnitSelectBox=`).
//! Consumer: `Ext/Techno/Body.Visuals.cpp:333-409` (`TechnoExt::DrawSelectBox`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ini_value::{PhobosDamageableColor, truncate_native_bytes};
use crate::rules::phobos_shields::AffectedHouse;

const NAME_PAYLOAD_BYTES: usize = 0x1F;

/// One `[SelectBoxTypes]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SelectBoxType {
    pub name: String,
    /// `Shape=` (`Valueable<SHPStruct*>`): the box SHP, `.shp` appended when
    /// the written name lacks it.
    pub shape: Option<String>,
    /// `Palette=` (`CustomPalette`): a palette file name; unset draws with
    /// `PALETTE.PAL`.
    pub palette: Option<String>,
    /// `Frames=` (`Nullable<Vector3D<int>>`): X/Y/Z health-band frames.
    pub frames: Option<[i32; 3]>,
    /// `Offset=` (`Valueable<Point2D>`): box offset from the techno draw point.
    pub offset: [i32; 2],
    /// `Translucency=` (`TranslucencyLevel`): the raw integer; only exactly
    /// 25, 50 and 75 select a translucent blitter, anything else is opaque.
    pub translucency: i32,
    /// `VisibleToHouses=` (`Valueable<AffectedHouse>`, default `All`).
    pub visible_to_houses: AffectedHouse,
    /// `VisibleToHouses.Observer=` (`Valueable<bool>`, default true).
    pub visible_to_houses_observer: bool,
    /// `DrawAboveTechno=` (`Valueable<bool>`, default true): draw after the
    /// techno body rather than before it.
    pub draw_above_techno: bool,
    /// `GroundShape=` (`Valueable<SHPStruct*>`).
    pub ground_shape: Option<String>,
    /// `GroundPalette=` (`CustomPalette`).
    pub ground_palette: Option<String>,
    /// `GroundFrames=` (`Nullable<Vector3D<int>>`).
    pub ground_frames: Option<[i32; 3]>,
    /// `GroundOffset=` (`Valueable<Point2D>`).
    pub ground_offset: [i32; 2],
    /// `Ground.AlwaysDraw=` (`Valueable<bool>`, default true).
    pub ground_always_draw: bool,
    /// `GroundLine=` (`Valueable<bool>`, default false).
    pub ground_line: bool,
    /// `GroundLineColor=` (`Damageable<ColorStruct>`, base default green).
    pub ground_line_color: PhobosDamageableColor,
    /// `GroundLine.Dashed=` (`Valueable<bool>`, default false).
    pub ground_line_dashed: bool,
}

impl SelectBoxType {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            shape: None,
            palette: None,
            frames: None,
            offset: [0, 0],
            translucency: 0,
            visible_to_houses: AffectedHouse::ALL,
            visible_to_houses_observer: true,
            draw_above_techno: true,
            ground_shape: None,
            ground_palette: None,
            ground_frames: None,
            ground_offset: [0, 0],
            ground_always_draw: true,
            ground_line: false,
            ground_line_color: PhobosDamageableColor {
                base: [0, 255, 0],
                condition_yellow: None,
                condition_red: None,
            },
            ground_line_dashed: false,
        }
    }

    /// `CustomPalette.GetOrDefaultConvert(FileSystem::PALETTE_PAL)`.
    pub fn palette_name(&self) -> &str {
        self.palette.as_deref().unwrap_or("palette.pal")
    }

    pub fn ground_palette_name(&self) -> &str {
        self.ground_palette.as_deref().unwrap_or("palette.pal")
    }

    /// `SelectBoxTypeClass::LoadFromINI` (`New/Type/SelectBoxTypeClass.cpp:18-33`),
    /// in its native read order. Absent keys retain the constructor value.
    pub fn load_from_section(&mut self, section: &IniSection) {
        // `Valueable<SHPStruct*>::Read`: an absent or empty key retains the
        // previous pointer; a present name normalizes the `.shp` suffix. A
        // failed file load also retains it, which this text owner cannot check.
        if let Some(name) = section.read_name("Shape", 0x100) {
            self.shape = Some(normalize_shp_name(name));
        }
        if let Some(palette) = section.read_name("Palette", 0x100) {
            self.palette = Some(palette.to_owned());
        }
        self.frames = section.read_phobos_nullable_vector3("Frames", self.frames);
        self.offset = section.read_phobos_int_pair("Offset", self.offset);
        // `TranslucencyLevel::Read` (`Constructs.cpp:232-242`): ReadInteger,
        // then `operator=(int)` maps anything but 0/25/50/75 to opaque.
        self.translucency = section.read_ares_int("Translucency", self.translucency);
        self.visible_to_houses = crate::rules::phobos_shields::read_affected_house(
            section,
            "VisibleToHouses",
            self.visible_to_houses,
        );
        self.visible_to_houses_observer =
            section.read_bool("VisibleToHouses.Observer", self.visible_to_houses_observer);
        self.draw_above_techno = section.read_bool("DrawAboveTechno", self.draw_above_techno);
        if let Some(name) = section.read_name("GroundShape", 0x100) {
            self.ground_shape = Some(normalize_shp_name(name));
        }
        if let Some(palette) = section.read_name("GroundPalette", 0x100) {
            self.ground_palette = Some(palette.to_owned());
        }
        self.ground_frames =
            section.read_phobos_nullable_vector3("GroundFrames", self.ground_frames);
        self.ground_offset = section.read_phobos_int_pair("GroundOffset", self.ground_offset);
        self.ground_always_draw = section.read_bool("Ground.AlwaysDraw", self.ground_always_draw);
        self.ground_line = section.read_bool("GroundLine", self.ground_line);
        self.ground_line_color =
            section.read_phobos_damageable_color("GroundLineColor", self.ground_line_color);
        self.ground_line_dashed = section.read_bool("GroundLine.Dashed", self.ground_line_dashed);
    }

    /// `BlitterFlags::TransLucent25/50/75`: the alpha the box draws with.
    pub fn translucency_alpha(&self) -> f32 {
        match self.translucency {
            25 => 0.75,
            50 => 0.5,
            75 => 0.25,
            _ => 1.0,
        }
    }
}

/// `Valueable<SHPStruct*>::Read` (`Utilities/TemplateDef.h:344-369`): append
/// `.shp` when the name does not already end in that suffix.
fn normalize_shp_name(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".shp") {
        name.to_owned()
    } else {
        format!("{name}.shp")
    }
}

/// Ordered Phobos `Enumerable<SelectBoxTypeClass>::Array`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SelectBoxTypeCatalog {
    types: Vec<SelectBoxType>,
}

impl SelectBoxTypeCatalog {
    pub fn types(&self) -> &[SelectBoxType] {
        &self.types
    }

    pub fn entry(&self, name: &str) -> Option<&SelectBoxType> {
        self.types
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.types
            .iter()
            .position(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// Read one reached Rules pass: the pass's `[SelectBoxTypes]` list appends
    /// entries, then every retained entry re-reads its own section
    /// (`Enumerable::LoadFromINIList`, `Utilities/Enumerable.h:53-66`).
    pub fn apply_pass(&mut self, pass: &IniFile) {
        if let Some(list) = pass.section("SelectBoxTypes") {
            for key in list.physical_keys() {
                let Some(name) = list.read_name(key, 2048) else {
                    continue;
                };
                if self.entry(name).is_none() {
                    self.types.push(SelectBoxType::named(&truncate_native_bytes(
                        name,
                        NAME_PAYLOAD_BYTES,
                    )));
                }
            }
        }
        for index in 0..self.types.len() {
            let name = self.types[index].name.clone();
            if let Some(section) = pass.section(&name) {
                self.types[index].load_from_section(section);
            }
        }
    }

    pub fn from_ini(ini: &IniFile) -> Self {
        let mut catalog = Self::default();
        catalog.apply_pass(ini);
        catalog
    }
}

/// The two `[AudioVisual] Default*SelectBox=` type references. Each is a
/// `Valueable<SelectBoxTypeClass*>` (pointer null by default); a present name
/// that resolves sets it, absent or invalid text retains it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SelectBoxGlobals {
    pub default_infantry: Option<String>,
    pub default_unit: Option<String>,
}

impl SelectBoxGlobals {
    /// `RulesExt::LoadFromINIFile` follows `[AudioVisual]`
    /// (`Ext/Rules/Body.cpp:333-334`). The catalog validates a preset name.
    pub fn from_ini(ini: &IniFile, catalog: &SelectBoxTypeCatalog) -> Self {
        let section = ini.section_or_empty("AudioVisual");
        let resolve = |key: &str, current: Option<String>| {
            let Some(name) = section.read_name(key, 0x100) else {
                return current;
            };
            if catalog.entry(name).is_some() {
                Some(name.to_owned())
            } else {
                current
            }
        };
        Self {
            default_infantry: resolve("DefaultInfantrySelectBox", None),
            default_unit: resolve("DefaultUnitSelectBox", None),
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "select_box_type_tests.rs"]
mod tests;
