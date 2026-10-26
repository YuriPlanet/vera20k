//! Phobos SuperWeaponSidebar settings.
//!
//! Sources: `src/Phobos.INI.cpp` (`[Sidebar]` reads at L204-L235),
//! `src/Ext/Rules/Body.cpp` (`SuperWeaponSidebarKeysEnabled`,
//! `SuperWeaponSidebar.AllowByDefault`), `src/Ext/Side/Body.cpp` (the five
//! side PCX names), and `src/Ext/SWType/Body.cpp` (per-type Allow,
//! PriorityHouses, RequiredHouses, Significance).
//!
//! `[Sidebar]` lives in `uimd.ini`; VERA reads it from the loaded archive and
//! stores the raw values here. `SuperWeaponSidebar.Max` depends on the game
//! screen height, so it is resolved at layout time instead of load time.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on app/, render/, ui/, sim/ or audio/.

use crate::rules::ini_parser::{IniFile, IniSection};

/// Phobos registers ten positional superweapon commands.
pub const SUPER_WEAPON_SIDEBAR_KEYS: usize = 10;

/// `uimd.ini [Sidebar]` values. Defaults are the Phobos source defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuperWeaponSidebarUi {
    pub enabled: bool,
    pub pyramid: bool,
    pub interval: i32,
    pub left_offset: i32,
    pub cameo_height: i32,
    /// `0` asks the layout to derive the column capacity from screen height.
    pub max: i32,
    pub max_columns: i32,
}

impl Default for SuperWeaponSidebarUi {
    fn default() -> Self {
        Self {
            enabled: false,
            pyramid: true,
            interval: 0,
            left_offset: 0,
            cameo_height: 48,
            max: 0,
            max_columns: i32::MAX,
        }
    }
}

impl SuperWeaponSidebarUi {
    pub fn from_uimd_bytes(bytes: &[u8]) -> Self {
        let Ok(ini) = IniFile::from_bytes(bytes) else {
            log::warn!("Could not parse UIMD.INI for SuperWeaponSidebar settings");
            return Self::default();
        };
        Self::from_ini(&ini)
    }

    pub fn from_ini(ini: &IniFile) -> Self {
        let Some(section) = ini.section("Sidebar") else {
            return Self::default();
        };
        Self::from_section(section)
    }

    pub fn from_section(section: &IniSection) -> Self {
        let interval = section.read_int("SuperWeaponSidebar.Interval", 0);
        let left_offset = section
            .read_int("SuperWeaponSidebar.LeftOffset", 0)
            .min(interval);
        let cameo_height = section
            .read_int("SuperWeaponSidebar.CameoHeight", 48)
            .max(48);
        Self {
            enabled: section.read_bool("SuperWeaponSidebar", false),
            pyramid: section.read_bool("SuperWeaponSidebar.Pyramid", true),
            interval,
            left_offset,
            cameo_height,
            max: section.read_int("SuperWeaponSidebar.Max", 0).max(0),
            max_columns: section.read_int("SuperWeaponSidebar.MaxColumns", i32::MAX),
        }
    }

    /// Phobos reserves 96 pixels above/below the columns and divides the rest
    /// by the cameo pitch (`Phobos.INI.cpp` L225-L231). An explicit positive
    /// `Max` is clamped to that resolution capacity.
    pub fn column_capacity(self, screen_height: i32) -> i32 {
        let usable = (screen_height - 96).max(0);
        let capacity = usable / self.cameo_height.max(48);
        if self.max > 0 {
            self.max.min(capacity)
        } else {
            capacity
        }
    }
}

/// `rulesmd.ini` switches that gate the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuperWeaponSidebarGlobals {
    /// `[GlobalControls] ToggleSuperWeaponSidebarKeyEnabled`, default yes.
    pub toggle_key_enabled: bool,
    /// `[GlobalControls] SuperWeaponSidebarKeysEnabled`, default no.
    pub keys_enabled: bool,
    /// `[AudioVisual] SuperWeaponSidebar.AllowByDefault`, default no.
    pub allow_by_default: bool,
}

impl Default for SuperWeaponSidebarGlobals {
    fn default() -> Self {
        Self {
            toggle_key_enabled: true,
            keys_enabled: false,
            allow_by_default: false,
        }
    }
}

impl SuperWeaponSidebarGlobals {
    pub fn from_ini(ini: &IniFile) -> Self {
        let globals = ini.section_or_empty("GlobalControls");
        let audio_visual = ini.section_or_empty("AudioVisual");
        Self {
            toggle_key_enabled: globals.read_bool("ToggleSuperWeaponSidebarKeyEnabled", true),
            keys_enabled: globals.read_bool("SuperWeaponSidebarKeysEnabled", false),
            allow_by_default: audio_visual.read_bool("SuperWeaponSidebar.AllowByDefault", false),
        }
    }
}

/// `[SIDE]` artwork. Missing names leave the corresponding column piece
/// undrawn, which keeps the sidebar usable with no extra graphics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SuperWeaponSidebarSideArt {
    pub on_pcx: Option<String>,
    pub off_pcx: Option<String>,
    pub top_pcx: Option<String>,
    pub center_pcx: Option<String>,
    pub bottom_pcx: Option<String>,
}

impl SuperWeaponSidebarSideArt {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            on_pcx: section
                .read_name("SuperWeaponSidebar.OnPCX", 0x100)
                .map(str::to_string),
            off_pcx: section
                .read_name("SuperWeaponSidebar.OffPCX", 0x100)
                .map(str::to_string),
            top_pcx: section
                .read_name("SuperWeaponSidebar.TopPCX", 0x100)
                .map(str::to_string),
            center_pcx: section
                .read_name("SuperWeaponSidebar.CenterPCX", 0x100)
                .map(str::to_string),
            bottom_pcx: section
                .read_name("SuperWeaponSidebar.BottomPCX", 0x100)
                .map(str::to_string),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.on_pcx.is_none()
            && self.off_pcx.is_none()
            && self.top_pcx.is_none()
            && self.center_pcx.is_none()
            && self.bottom_pcx.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_defaults_match_phobos_and_clamp_like_the_reader() {
        let defaults = SuperWeaponSidebarUi::default();
        assert!(!defaults.enabled);
        assert!(defaults.pyramid);
        assert_eq!(defaults.cameo_height, 48);
        assert_eq!(defaults.max_columns, i32::MAX);

        let ini = IniFile::from_str(
            "[Sidebar]\nSuperWeaponSidebar=yes\nSuperWeaponSidebar.Interval=10\n\
             SuperWeaponSidebar.LeftOffset=20\nSuperWeaponSidebar.CameoHeight=20\n\
             SuperWeaponSidebar.Max=8\nSuperWeaponSidebar.MaxColumns=3\n",
        );
        let ui = SuperWeaponSidebarUi::from_ini(&ini);
        assert!(ui.enabled);
        assert_eq!(ui.left_offset, 10, "LeftOffset is clamped to Interval");
        assert_eq!(ui.cameo_height, 48, "CameoHeight is at least 48");
        assert_eq!(ui.max, 8);
        assert_eq!(ui.max_columns, 3);
    }

    #[test]
    fn resolution_capacity_reserves_96_pixels_and_clamps_explicit_max() {
        let mut ui = SuperWeaponSidebarUi::default();
        assert_eq!(ui.column_capacity(600), 10); // (600-96)/48
        ui.max = 4;
        assert_eq!(ui.column_capacity(600), 4);
        ui.max = 99;
        assert_eq!(ui.column_capacity(600), 10);
        assert_eq!(ui.column_capacity(80), 0);
    }

    #[test]
    fn globals_use_their_own_sections_and_defaults() {
        let empty = SuperWeaponSidebarGlobals::from_ini(&IniFile::from_str("[General]\n"));
        assert_eq!(empty, SuperWeaponSidebarGlobals::default());
        let ini = IniFile::from_str(
            "[GlobalControls]\nSuperWeaponSidebarKeysEnabled=yes\n\
             ToggleSuperWeaponSidebarKeyEnabled=no\n\
             [AudioVisual]\nSuperWeaponSidebar.AllowByDefault=yes\n",
        );
        let globals = SuperWeaponSidebarGlobals::from_ini(&ini);
        assert!(!globals.toggle_key_enabled);
        assert!(globals.keys_enabled);
        assert!(globals.allow_by_default);
    }

    #[test]
    fn side_art_reads_all_five_pcx_names() {
        let ini = IniFile::from_str(
            "[SIDE]\nSuperWeaponSidebar.OnPCX=on.pcx\nSuperWeaponSidebar.OffPCX=off.pcx\n\
             SuperWeaponSidebar.TopPCX=top.pcx\nSuperWeaponSidebar.CenterPCX=center.pcx\n\
             SuperWeaponSidebar.BottomPCX=bottom.pcx\n",
        );
        let art = SuperWeaponSidebarSideArt::from_section(ini.section("SIDE").unwrap());
        assert_eq!(art.on_pcx.as_deref(), Some("on.pcx"));
        assert_eq!(art.off_pcx.as_deref(), Some("off.pcx"));
        assert_eq!(art.top_pcx.as_deref(), Some("top.pcx"));
        assert_eq!(art.center_pcx.as_deref(), Some("center.pcx"));
        assert_eq!(art.bottom_pcx.as_deref(), Some("bottom.pcx"));
        assert!(!art.is_empty());
    }
}
