//! The player's Kratos extension setting for a launch, `[VERA] Kratos=` in
//! VERA.INI and the Options screen's Extensions page.
//!
//! Kratos needs no build feature and no archive of its own: every build
//! carries it, and the game's data decides (`rules::kratos_switch`). This
//! setting overrides that answer for a whole launch, as the Ares setting does
//! for Ares: On plays Kratos in every game, its default values included, and
//! Off plays none. Auto, the default, leaves the answer to the game's rules
//! and art.
//!
//! The setting is read before the game's files open and holds until the next
//! start (`AssetManager::kratos_setting`); the processed rules carry it, so a
//! save, a replay and every peer reach the same answer.

use serde::{Deserialize, Serialize};

/// The player's Kratos extension setting for a launch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KratosSetting {
    /// The game's data decides (`rules::kratos_switch`): a Kratos key in its
    /// rules or art plays Kratos for that game.
    #[default]
    Auto,
    /// Every game plays Kratos, as if its mod shipped Kratos: the switch is on
    /// whatever the game's keys say, so Kratos' default values apply too.
    On,
    /// No game plays Kratos: its keys turn nothing on, so a Kratos mod plays
    /// as the original game reads it.
    Off,
}

impl KratosSetting {
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
        for setting in [KratosSetting::Auto, KratosSetting::On, KratosSetting::Off] {
            assert_eq!(KratosSetting::from_ini_text(setting.ini_value()), setting);
        }
        for text in ["yes", "TRUE", "1", " on "] {
            assert_eq!(KratosSetting::parse(text), Some(KratosSetting::On), "{text}");
        }
        for text in ["no", "False", "0", "off"] {
            assert_eq!(KratosSetting::parse(text), Some(KratosSetting::Off), "{text}");
        }
        assert_eq!(KratosSetting::from_ini_text("sometimes"), KratosSetting::Auto);
        assert_eq!(KratosSetting::parse(""), None);
    }
}
