//! Whether the game this process opened for play is an Ares game: it ships
//! Ares's `ares.mix`, or the player's Ares setting says so.
//!
//! Ares.dll patches a whole launch: once it is loaded, every INI text load
//! takes its syntax and every Rules read its replacements. VERA has no DLL to
//! load. Its launch is an Ares launch when the game it opens for play ships
//! `ares.mix` (`AssetManager::ships_ares`). With no mod selected, the
//! installation's `ares.mix` is ignored unless the player's Ares setting is
//! On; a selected mod can use that copy or its own.
//!
//! The player's Ares setting ([`AresSetting`], `[VERA] Ares=` in VERA.INI or
//! `[Settings] Ares=` in a `-SPAWN` launch file) can override that answer for
//! the whole launch: On plays every game as an Ares game, Off none. The asset
//! manager holds the setting (`AssetManager::ares_setting`), and the Rules
//! owner reads it from there.
//!
//! The asset manager owns the answer, and code with a manager or a RuleSet in
//! reach asks it, or opens a reading scope from it
//! (`rules::ares_switch::read_ares_way`). This holds the launch's copy for the
//! one reader with neither: a thread outside every reading scope, which loads
//! its INI text and reads its Rules by it (`rules::ares_switch::ares_way`).
//! Only a launch publishes it ([`open_for_play`]), once its game's archives
//! are open: the game's start and the rebuild of its manager, and the
//! commands that are a launch of their own (`--probe`, `--generate-map` and
//! their browser functions, the asset tool). An asset manager opened for
//! anything else (a test, a read-only INI probe) leaves it alone. A process
//! that opened no game for play reads the stock way.

use std::sync::atomic::{AtomicBool, Ordering};

/// The launch's answer: a process-wide value because Ares.dll is loaded for a
/// whole process, and read from threads that hold no manager. Written only by
/// [`open_for_play`].
static SHIPS_ARES: AtomicBool = AtomicBool::new(false);

/// VERA: the player's Ares extension setting for a launch. Like a mod, it is
/// chosen before the game's files open and holds until the next start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AresSetting {
    /// The game's data decides (`rules::ares_switch`): a selected mod that
    /// mounts `ares.mix` makes an Ares game, and Ares keys in its rules or
    /// map turn Ares on.
    #[default]
    Auto,
    /// Every game is an Ares game, as if its mod shipped `ares.mix`: its INIs
    /// and Rules are read the Ares way and Ares plays, the original game too.
    /// The installation's `ares.mix` mounts even with no mod selected.
    On,
    /// No game is an Ares game: `ares.mix` stays unmounted and Ares keys turn
    /// nothing on, so an Ares mod plays as the original game reads it.
    Off,
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

    /// VERA: this setting where it decides which game opens, else
    /// [`Self::Auto`]: On unless Auto mounts `ares.mix` too (`auto_mounts`,
    /// `AssetManager::auto_mounts_ares`), and Off only when Auto would mount
    /// it or the game's own Rules hold an Ares key that Off keeps from
    /// turning Ares on (`keys_kept_off`, the startup Rules'
    /// `RuleSet::ares_kept_off`). Players' games compare and name this
    /// (`ProcessAssets::agreed_ares_setting`), so a setting that opens the
    /// game Auto opens keeps no one out. Off also keeps a mode's or a map's
    /// Ares key off; whether Ares plays is each match's own answer
    /// (`RuleSet::ares`), which its start compares.
    pub const fn where_it_decides(self, auto_mounts: bool, keys_kept_off: bool) -> Self {
        match self {
            Self::On if !auto_mounts => Self::On,
            Self::Off if auto_mounts || keys_kept_off => Self::Off,
            _ => Self::Auto,
        }
    }
}

/// Publish the game this launch opened for play: whether it ships `ares.mix`
/// (`AssetManager::ships_ares`), through `AssetManager::open_for_play`.
pub fn open_for_play(ships_ares: bool) {
    SHIPS_ARES.store(ships_ares, Ordering::Relaxed);
}

/// Whether the game this launch opened for play ships `ares.mix`; false when
/// the process opened none.
pub fn game_ships_ares() -> bool {
    SHIPS_ARES.load(Ordering::Relaxed)
}
