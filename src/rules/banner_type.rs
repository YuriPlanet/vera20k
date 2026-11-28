//! Phobos `[BannerTypes]` catalog.
//!
//! Sources: `New/Type/BannerTypeClass.cpp:11-28` (`LoadFromINI`, the section
//! named like the banner), `New/Type/BannerTypeClass.h:10-39` (fields and
//! constructor defaults), `Utilities/Enumerable.h:52-67`
//! (`LoadFromINIList`, driven by `Ext/Rules/Body.cpp:64` with no hook) and
//! `New/Entity/BannerClass.cpp` (the runtime banners the trigger actions
//! 800-802 create and `0x6D4B25 TacticalClass_Render_Banner` draws).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ini_value::truncate_native_bytes;

/// `Enumerable`'s `PhobosFixedString<32> Name` payload (`Enumerable.h:122`).
const NAME_PAYLOAD_BYTES: usize = 0x1F;

/// `PhobosPCXFile`'s `FixedString<0x20> filename` payload
/// (`Utilities/Constructs.h:285`).
const PCX_NAME_PAYLOAD_BYTES: usize = 0x20;

/// Phobos `BannerNumberType` (`Utilities/Enum.h:358-364`): how the banner's
/// scenario variable joins the CSF text.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum BannerNumberType {
    #[default]
    None = 0,
    Variable = 1,
    Prefixed = 2,
    Suffixed = 3,
}

impl BannerNumberType {
    /// `Parser<BannerNumberType>` reads the enum name
    /// (`Utilities/TemplateDef.h:110-118`); unknown text retains the current.
    fn from_name(raw: &str) -> Option<Self> {
        if raw.eq_ignore_ascii_case("None") {
            Some(Self::None)
        } else if raw.eq_ignore_ascii_case("Variable") {
            Some(Self::Variable)
        } else if raw.eq_ignore_ascii_case("Prefixed") {
            Some(Self::Prefixed)
        } else if raw.eq_ignore_ascii_case("Suffixed") {
            Some(Self::Suffixed)
        } else {
            None
        }
    }
}

/// One `[BannerTypes]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BannerType {
    pub name: String,
    /// `PCX=` (`PhobosPCXFile`, `BannerTypeClass.cpp:15`): the image file
    /// name, 0x20 bytes at most.
    pub pcx: Option<String>,
    /// `SHP=` (`Valueable<SHPStruct*>`, `BannerTypeClass.cpp:17`): the shape
    /// file name; an absent or empty key retains the previous name.
    pub shape: Option<String>,
    /// `SHP.Palette=` (`CustomPalette`, `BannerTypeClass.cpp:18`): a palette
    /// file name; unset draws with `PALETTE.PAL`.
    pub palette: Option<String>,
    /// `CSF=` (`Valueable<CSFText>`, `BannerTypeClass.cpp:20`): the CSF
    /// label of the drawn text.
    pub csf: Option<String>,
    /// `CSF.Color=` (`Nullable<ColorStruct>`, `BannerTypeClass.cpp:21`):
    /// unset draws with the tooltip color at render (`BannerClass.cpp:186`).
    pub csf_color: Option<[u8; 3]>,
    /// `CSF.Background=` (`Valueable<bool>`, default false,
    /// `BannerTypeClass.h:26`).
    pub csf_background: bool,
    /// `CSF.VariableFormat=` (`Valueable<BannerNumberType>`, default None,
    /// `BannerTypeClass.h:27`).
    pub csf_variable_format: BannerNumberType,
    /// `Duration=` (`Valueable<int>`, default -1, `BannerTypeClass.h:30`):
    /// the shown frames of one blink period.
    pub duration: i32,
    /// `Delay=` (`Valueable<int>`, default -1, `BannerTypeClass.h:31`): the
    /// hidden frames between periods.
    pub delay: i32,
    /// `SHP.RefreshAfterDelay=` (`Valueable<bool>`, default false,
    /// `BannerTypeClass.h:32`): the shape restarts at frame 0 each period.
    pub shape_refresh_after_delay: bool,
    /// `ClampToScreen=` (`Valueable<bool>`, default **true**,
    /// `BannerTypeClass.h:34`): the banner keeps inside the tactical view.
    pub clamp_to_screen: bool,
}

impl Default for BannerType {
    /// `BannerTypeClass`'s constructor defaults (`BannerTypeClass.h:25-35`):
    /// `Duration`/`Delay` construct **-1**, `ClampToScreen` **true**; the
    /// images and the CSF label construct empty.
    fn default() -> Self {
        Self {
            name: String::new(),
            pcx: None,
            shape: None,
            palette: None,
            csf: None,
            csf_color: None,
            csf_background: false,
            csf_variable_format: BannerNumberType::None,
            duration: -1,
            delay: -1,
            shape_refresh_after_delay: false,
            clamp_to_screen: true,
        }
    }
}

impl BannerType {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Default::default()
        }
    }

    /// `BannerTypeClass::LoadFromINI` (`BannerTypeClass.cpp:11-28`) in its
    /// exact read order; a section named like nothing in `[BannerTypes]` is
    /// never reached.
    pub fn load_from_section(&mut self, section: &IniSection) {
        // `PhobosPCXFile::Read` (`Constructs.cpp`): an absent or empty key
        // retains the previous name; a present one truncates to 0x20 bytes.
        if let Some(name) = section.read_name("PCX", PCX_NAME_PAYLOAD_BYTES) {
            self.pcx = Some(truncate_native_bytes(name, PCX_NAME_PAYLOAD_BYTES).to_owned());
        }
        // `Valueable<SHPStruct*>::Read`: an absent or empty key retains the
        // previous pointer; a present name normalizes the `.shp` suffix.
        if let Some(name) = section.read_name("SHP", 0x100) {
            self.shape = Some(if name.to_ascii_lowercase().ends_with(".shp") {
                name.to_owned()
            } else {
                format!("{name}.shp")
            });
        }
        if let Some(palette) = section.read_name("SHP.Palette", 0x100) {
            self.palette = Some(palette.to_owned());
        }
        if let Some(label) = section.read_name("CSF", 0x20) {
            self.csf = Some(label.to_owned());
        }
        self.csf_color = section.read_phobos_nullable_color("CSF.Color", self.csf_color);
        self.csf_background = section.read_bool("CSF.Background", self.csf_background);
        self.csf_variable_format = section
            .read_name("CSF.VariableFormat", 0x20)
            .and_then(BannerNumberType::from_name)
            .unwrap_or(self.csf_variable_format);
        self.duration = section.read_int("Duration", self.duration);
        self.delay = section.read_int("Delay", self.delay);
        self.shape_refresh_after_delay =
            section.read_bool("SHP.RefreshAfterDelay", self.shape_refresh_after_delay);
        self.clamp_to_screen = section.read_bool("ClampToScreen", self.clamp_to_screen);
    }
}

/// Ordered Phobos `Enumerable<BannerTypeClass>::Array`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct BannerTypeCatalog {
    types: Vec<BannerType>,
}

impl BannerTypeCatalog {
    pub fn types(&self) -> &[BannerType] {
        &self.types
    }

    /// `BannerTypeClass::Find` (`Utilities/Enumerable.h:17-29`), the
    /// case-insensitive name lookup the trigger actions use.
    pub fn entry(&self, name: &str) -> Option<&BannerType> {
        self.types
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// Read one reached Rules pass: the pass's `[BannerTypes]` list appends
    /// entries, then every retained entry re-reads its own section
    /// (`Enumerable::LoadFromINIList`, `Utilities/Enumerable.h:52-67`).
    pub fn apply_pass(&mut self, pass: &IniFile) {
        if let Some(list) = pass.section("BannerTypes") {
            for key in list.physical_keys() {
                let Some(name) = list.read_name(key, 2048) else {
                    continue;
                };
                if self.entry(name).is_none() {
                    self.types.push(BannerType::named(&truncate_native_bytes(
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

#[cfg(all(test, tests_rules_a_n))]
mod tests {
    use super::*;

    /// `BannerTypeClass::LoadFromINI` (`BannerTypeClass.cpp:11-28`) reads the
    /// four bare keys with their constructor defaults
    /// (`BannerTypeClass.h:10-39`): `ClampToScreen` constructs **true**, the
    /// images and the CSF label empty.
    #[test]
    fn banner_types_read_their_sections_with_the_source_defaults() {
        let ini = IniFile::from_str(
            "[BannerTypes]\n0=INTRO\n\n\
             [INTRO]\nPCX=Title.pcx\nCSF=Name:Title\nClampToScreen=no\n",
        );
        let catalog = BannerTypeCatalog::from_ini(&ini);
        let intro = catalog.entry("intro").expect("the list names INTRO");
        assert_eq!(intro.pcx.as_deref(), Some("Title.pcx"));
        assert_eq!(intro.csf.as_deref(), Some("Name:Title"));
        assert_eq!(intro.shape, None);
        assert!(!intro.clamp_to_screen, "the key overrides the true default");
        assert_eq!(intro.csf_variable_format, BannerNumberType::None);
        assert_eq!(intro.duration, -1);
        assert_eq!(intro.delay, -1);
    }

    /// The SHP arm: the `.shp` suffix normalizes, the paired palette and
    /// refresh keys read, and `Duration`/`Delay` follow the blink period
    /// (`BannerClass.cpp:31-53`).
    #[test]
    fn the_shp_arm_reads_palette_and_period_keys() {
        let ini = IniFile::from_str(
            "[BannerTypes]\n0=WAVY\n\n\
             [WAVY]\nSHP=FLAG\nSHP.Palette=FLAG.PAL\nSHP.RefreshAfterDelay=yes\n\
             Duration=100\nDelay=50\nCSF.VariableFormat=Suffixed\nCSF.Background=yes\n\
             CSF.Color=255,128,0\n",
        );
        let catalog = BannerTypeCatalog::from_ini(&ini);
        let wavy = catalog.entry("WAVY").expect("the list names WAVY");
        assert_eq!(wavy.shape.as_deref(), Some("FLAG.shp"));
        assert_eq!(wavy.palette.as_deref(), Some("FLAG.PAL"));
        assert!(wavy.shape_refresh_after_delay);
        assert_eq!(wavy.duration, 100);
        assert_eq!(wavy.delay, 50);
        assert_eq!(wavy.csf_variable_format, BannerNumberType::Suffixed);
        assert!(wavy.csf_background);
        assert_eq!(wavy.csf_color, Some([255, 128, 0]));
    }
}
