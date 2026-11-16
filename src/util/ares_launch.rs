//! Whether the game this process opened for play is an Ares game: it ships
//! Ares's `ares.mix`, or the player's Ares setting says so.
//!
//! Ares.dll patches a whole launch: once it is loaded, every INI text load
//! takes its syntax and every Rules read its replacements. VERA has no DLL to
//! load. Its launch is an Ares launch when the game it opens ships `ares.mix`
//! (`AssetManager::ships_ares`), and the asset manager records that here when
//! it opens a game for play. With no mod selected, the installation's
//! `ares.mix` is ignored unless the player's Ares setting is On; a selected
//! mod can use that copy or its own. A
//! thread with no reading scope open reads its INIs and Rules by this answer
//! (`rules::ares_switch::ares_way`).
//!
//! The player's Ares setting ([`AresSetting`], `[VERA] Ares=` in VERA.INI or
//! `[Settings] Ares=` in a `-SPAWN` launch file) can override that answer for
//! the whole launch: On plays every game as an Ares game, Off none.
//!
//! The asset manager owns the answer; this is its copy for code that parses
//! INI text with no manager in reach. It changes only when a game is opened.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

static SHIPS_ARES: AtomicBool = AtomicBool::new(false);
static SETTING: AtomicU8 = AtomicU8::new(AresSetting::Auto as u8);

/// VERA: the player's Ares extension setting for a launch. Like a mod, it is
/// chosen before the game's files open and holds until the next start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AresSetting {
    /// The game's data decides (`rules::ares_switch`): a selected mod that
    /// mounts `ares.mix` makes an Ares game, and Ares keys in its rules or
    /// map turn Ares on.
    #[default]
    Auto = 0,
    /// Every game is an Ares game, as if its mod shipped `ares.mix`: its INIs
    /// and Rules are read the Ares way and Ares plays, the original game too.
    /// The installation's `ares.mix` mounts even with no mod selected.
    On = 1,
    /// No game is an Ares game: `ares.mix` stays unmounted and Ares keys turn
    /// nothing on, so an Ares mod plays as the original game reads it.
    Off = 2,
}

impl AresSetting {
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

    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::On,
            2 => Self::Off,
            _ => Self::Auto,
        }
    }
}

/// Record the Ares setting of the game just opened for play.
pub fn set_setting(setting: AresSetting) {
    SETTING.store(setting as u8, Ordering::Relaxed);
}

/// The Ares setting of the game last opened for play.
pub fn setting() -> AresSetting {
    AresSetting::from_u8(SETTING.load(Ordering::Relaxed))
}

/// Record whether the game just opened for play ships `ares.mix`.
pub fn set_game_ships_ares(ships: bool) {
    SHIPS_ARES.store(ships, Ordering::Relaxed);
}

/// Whether the game last opened for play ships `ares.mix`.
pub fn game_ships_ares() -> bool {
    SHIPS_ARES.load(Ordering::Relaxed)
}
