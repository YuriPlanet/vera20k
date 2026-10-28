//! Phobos "Flashing Technos on selecting".
//!
//! Sources: `src/Ext/Rules/Body.cpp` (`SelectionFlashDuration.Read(exINI,
//! GameStrings::AudioVisual, "SelectionFlashDuration")`, constructor default
//! 0) and `src/Ext/Techno/Hooks.Misc.cpp` (`ObjectClass_Select` hooks at
//! `0x5F46AE`/`0x5F4718`: `Phobos::Config::ShowFlashOnSelecting`, then
//! `SelectionFlashDuration > 0` and
//! `GetOwningHouse()->IsControlledByCurrentPlayer()`, then `Flash(duration)`).
//! The client switch `[Phobos] ShowFlashOnSelecting` is read from `RA2MD.INI`
//! by the app profile, not from rules.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on app/, render/, ui/, sim/ or audio/.

use crate::rules::ini_parser::IniFile;

/// `[AudioVisual] SelectionFlashDuration=`, default 0 (the constructor's
/// `Valueable<int>`: ReadString, then Phobos's `Parser<int>`). The reader
/// stores any signed value unchanged; the flash hook only accepts values
/// above 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelectionFlashGlobals {
    pub duration_frames: i32,
}

impl SelectionFlashGlobals {
    pub fn from_ini(ini: &IniFile) -> Self {
        Self {
            duration_frames: ini
                .section_or_empty("AudioVisual")
                .read_ares_int("SelectionFlashDuration", 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_key_or_section_stays_at_the_zero_default() {
        assert_eq!(
            SelectionFlashGlobals::from_ini(&IniFile::from_str("[General]\n")),
            SelectionFlashGlobals::default()
        );
        assert_eq!(
            SelectionFlashGlobals::from_ini(&IniFile::from_str(
                "[AudioVisual]\nSelectionFlashDuration=\n"
            )),
            SelectionFlashGlobals::default()
        );
    }

    #[test]
    fn the_duration_is_read_from_audiovisual_without_a_clamp() {
        let ini = IniFile::from_str("[AudioVisual]\nSelectionFlashDuration=12\n");
        assert_eq!(SelectionFlashGlobals::from_ini(&ini).duration_frames, 12);
        // The native Valueable<int> stores the reader's signed result raw; the
        // hook (not the reader) turns values <= 0 into "disabled".
        let negative = IniFile::from_str("[AudioVisual]\nSelectionFlashDuration=-3\n");
        assert_eq!(
            SelectionFlashGlobals::from_ini(&negative).duration_frames,
            -3
        );
        // Phobos's Parser<int> scans `$10` as decimal; stock ReadInteger
        // would read hex 16.
        let dollar = IniFile::from_str("[AudioVisual]\nSelectionFlashDuration=$10\n");
        assert_eq!(SelectionFlashGlobals::from_ini(&dollar).duration_frames, 10);
    }
}
