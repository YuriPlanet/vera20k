//! Whether the game this process opened for play ships Ares's `ares.mix`.
//!
//! Ares.dll patches a whole launch: once it is loaded, every INI text load
//! takes its syntax and every Rules read its replacements. VERA has no DLL to
//! load. Its launch is an Ares launch when the game it opens ships `ares.mix`
//! (`AssetManager::ships_ares`), and the asset manager records that here when
//! it opens a game for play, the installed one or a mod chosen later. A thread
//! with no reading scope open reads its INIs and Rules by this answer
//! (`rules::ares_switch::ares_way`).
//!
//! The asset manager owns the answer; this is its copy for code that parses
//! INI text with no manager in reach. It changes only when a game is opened.

use std::sync::atomic::{AtomicBool, Ordering};

static SHIPS_ARES: AtomicBool = AtomicBool::new(false);

/// Record whether the game just opened for play ships `ares.mix`.
pub fn set_game_ships_ares(ships: bool) {
    SHIPS_ARES.store(ships, Ordering::Relaxed);
}

/// Whether the game last opened for play ships `ares.mix`.
pub fn game_ships_ares() -> bool {
    SHIPS_ARES.load(Ordering::Relaxed)
}
