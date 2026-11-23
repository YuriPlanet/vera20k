//! Whether VeraX plays in a game: the one per-game switch the run-time
//! VeraX paths read, through
//! [`RuleSet::verax`](crate::rules::ruleset::RuleSet::verax).
//!
//! An engine extension is in every build and switches on from the game's data
//! (AGENTS.md, "Stability, extensions and online play"), as a modder expects
//! from an extension: adding the extension's own key to an INI is all it
//! takes. VeraX is VERA20K's own first-party extension namespace, so its
//! switch is a VERA key in the game's own Rules: `[VeraX] Enabled=` in
//! RULESMD.INI, in the selected mode INI or in a map's rules pass. Retail
//! data holds no such key, so a retail game plays as gamemd, and every build
//! holds the VeraX code.
//!
//! The key is VeraX's own, so no gamemd section spells it; the value is read
//! by the game's own boolean reader (`CCINIClass::ReadBool` 0x005295F0, the
//! first-byte verdict at 0x0052976B: `1`/`T`/`Y` true, `0`/`F`/`N` false, else
//! the reader's default). An absent section or key leaves VeraX off, and a
//! present `Enabled=no` turns it off again, so a mod that ships the key can
//! also refuse it.
//!
//! The answer comes from the game's own Rules and is stored on the RuleSet,
//! so a save, a replay and every peer reach the same answer from the same
//! rules, mode and map.

use crate::rules::ini_parser::IniFile;

/// The section VeraX's own keys live in.
pub const SECTION: &str = "VeraX";

/// The key that turns VeraX on in a game.
pub const KEY: &str = "Enabled";

/// Whether VeraX plays in the game these Rules describe. The deciding key
/// goes to the log, so a modder can see why.
pub(super) fn decide(ini: &IniFile) -> bool {
    let plays = ini
        .section(SECTION)
        .is_some_and(|section| section.read_bool(KEY, false));
    if plays {
        log::info!("VeraX plays in this game: [{SECTION}] {KEY} is set");
    }
    plays
}
