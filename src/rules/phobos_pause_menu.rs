//! Phobos `uimd.ini [Sidebar] CenterPauseMenuBackground`.
//!
//! Sources: `src/Phobos.INI.cpp` (the `[Sidebar]` read, default `false`) and
//! the `InitSideRectangles_CenterBackground` hook at `0x72FCB5`
//! (`src/Ext/Sidebar/Hooks.cpp`). Original `0x72FCB5` is the
//! `mov [0x00B0FC30], eax` that publishes the background rectangle original
//! `0x0072FC60` fills; the hook rewrites that rectangle's X/Y at the centre of
//! the battlefield before the store runs. `ui/shell/in_game_shell.rs` owns the
//! resulting geometry.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on app/, render/, ui/, sim/ or audio/.

use crate::rules::ini_parser::{IniFile, IniSection};

/// `uimd.ini [Sidebar]` pause-menu values. Defaults are the Phobos defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PauseMenuUi {
    /// `CenterPauseMenuBackground`, default `no`.
    pub center_background: bool,
}

impl PauseMenuUi {
    pub fn from_uimd_bytes(bytes: &[u8]) -> Self {
        let Ok(ini) = IniFile::from_bytes(bytes) else {
            log::warn!("Could not parse UIMD.INI for pause menu settings");
            return Self::default();
        };
        Self::from_ini(&ini)
    }

    pub fn from_ini(ini: &IniFile) -> Self {
        ini.section("Sidebar")
            .map_or_else(Self::default, Self::from_section)
    }

    pub fn from_section(section: &IniSection) -> Self {
        Self {
            center_background: section.read_bool("CenterPauseMenuBackground", false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_key_and_section_keep_the_background_at_the_origin() {
        assert!(!PauseMenuUi::default().center_background);
        for text in ["[Sidebar]\nSuperWeaponSidebar=no\n", "[General]\n"] {
            assert!(!PauseMenuUi::from_uimd_bytes(text.as_bytes()).center_background);
        }
    }

    #[test]
    fn the_sidebar_key_enables_centering_through_the_uimd_reader() {
        let ui = PauseMenuUi::from_uimd_bytes(
            b"[Sidebar]\nCenterPauseMenuBackground=yes\nSuperWeaponSidebar=yes\n",
        );
        assert!(ui.center_background);
        let ui = PauseMenuUi::from_uimd_bytes(b"[Sidebar]\nCenterPauseMenuBackground=false\n");
        assert!(!ui.center_background);
    }
}
