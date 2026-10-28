//! Phobos power plant enhancer range display.
//!
//! Sources: `src/Ext/Rules/Body.{h,cpp}` (`ShowPowerPlantEnhancerRange`,
//! `[AudioVisual]`, constructor and reader default `true`),
//! `src/Misc/Hooks.UI.cpp` (`TacticalClass_DrawRadialIndicator_Building_Extras`
//! at `0x6DBEA3`: both switches, the current player's active enhancers, the
//! `1.5 * range` cell prefilter and `Game::DrawRadialIndicator` with
//! `draw_line=false`, `adjust_color=true`, `pixel_radius=false`,
//! `truncate_radius=true`) and `src/Ext/BuildingType/Body.cpp` (the
//! `PowerPlantEnhancer.*` per-type keys, read as `[SOMEBUILDING]`).
//!
//! Per-BuildingType fields live on `ObjectType`; the client
//! `[Phobos] ShowPowerPlantEnhancerRange` switch lives in
//! `app::persistence::phobos_settings`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on app/, render/, ui/, sim/ or audio/.

use crate::rules::ini_parser::IniFile;

/// `RulesExt`'s `[AudioVisual]` switches for this chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PowerPlantEnhancerGlobals {
    /// `[AudioVisual] ShowPowerPlantEnhancerRange=`, default `true`.
    pub show_range: bool,
}

impl Default for PowerPlantEnhancerGlobals {
    fn default() -> Self {
        Self { show_range: true }
    }
}

impl PowerPlantEnhancerGlobals {
    pub(crate) fn from_ini(ini: &IniFile) -> Self {
        Self {
            show_range: ini
                .section_or_empty("AudioVisual")
                .read_bool("ShowPowerPlantEnhancerRange", true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_global_switch_defaults_on_and_reads_audiovisual() {
        assert_eq!(
            PowerPlantEnhancerGlobals::from_ini(&IniFile::from_str("[General]\n")),
            PowerPlantEnhancerGlobals::default()
        );
        let off = IniFile::from_str("[AudioVisual]\nShowPowerPlantEnhancerRange=no\n");
        assert!(!PowerPlantEnhancerGlobals::from_ini(&off).show_range);
        let on = IniFile::from_str("[AudioVisual]\nShowPowerPlantEnhancerRange=yes\n");
        assert!(PowerPlantEnhancerGlobals::from_ini(&on).show_range);
    }
}
