//! The player's NPatch extension setting for a launch, `[VERA] NPatch=` in
//! VERA.INI and the Options screen's Extensions page.
//!
//! NPatch needs no build feature and no archive of its own: every build
//! carries it, and the game's data decides (`rules::npatch_switch`). This
//! setting overrides that answer for a whole launch, as the Ares and Kratos
//! settings do for theirs: On plays every NPatch feature and every NPExt fix,
//! its default values included, and Off plays none. Auto, the default, leaves
//! the answer to the game's rules and map. `[General] NPatch=` stays the data
//! master switch; the launch setting is the stronger, outer answer.
//!
//! The setting is read before the game's files open and holds until the next
//! start (`AssetManager::npatch_setting`); the processed rules carry it, so a
//! save, a replay and every peer reach the same answer.

use serde::{Deserialize, Serialize};

/// The player's NPatch extension setting for a launch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NpatchSetting {
    /// The game's data decides (`rules::npatch_switch`): the `[General]
    /// NPatch=` master switch, or an NPatch-only key in its rules or map.
    #[default]
    Auto,
    /// Every game plays NPatch: both the feature paths and the NPExt fix
    /// chains are on whatever the game's keys say, so a retail game plays
    /// NPatch's defaults too.
    On,
    /// No game plays NPatch: neither the global key nor a feature key turns
    /// anything on, so an NPatch mod plays as the original game reads it.
    Off,
}

impl NpatchSetting {
    /// The value as written: `Auto`, `On` or `Off`.
    pub const fn ini_value(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::On => "On",
            Self::Off => "Off",
        }
    }

    /// Read a written value: `Auto`, `On`/`Yes`/`True`/`1` or
    /// `Off`/`No`/`False`/`0`, in any case. `None` for anything else, an empty
    /// value included.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "on" | "yes" | "true" | "1" => Some(Self::On),
            "off" | "no" | "false" | "0" => Some(Self::Off),
            _ => None,
        }
    }

    /// [`Self::parse`], where a value it does not know reads as
    /// [`Self::Auto`], so an unknown value never changes the game.
    pub fn from_ini_text(text: &str) -> Self {
        Self::parse(text).unwrap_or_default()
    }
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::*;

    #[test]
    fn the_written_values_round_trip_and_unknown_reads_auto() {
        for setting in [NpatchSetting::Auto, NpatchSetting::On, NpatchSetting::Off] {
            assert_eq!(NpatchSetting::from_ini_text(setting.ini_value()), setting);
        }
        for text in ["yes", "TRUE", "1", " on "] {
            assert_eq!(
                NpatchSetting::parse(text),
                Some(NpatchSetting::On),
                "{text}"
            );
        }
        for text in ["no", "False", "0", "off"] {
            assert_eq!(
                NpatchSetting::parse(text),
                Some(NpatchSetting::Off),
                "{text}"
            );
        }
        assert_eq!(
            NpatchSetting::from_ini_text("sometimes"),
            NpatchSetting::Auto
        );
        assert_eq!(NpatchSetting::parse(""), None);
    }
}
