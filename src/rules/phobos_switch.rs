//! Whether Phobos plays in a game: the one per-game switch the run-time
//! Phobos paths read, through
//! [`RuleSet::phobos`](crate::rules::ruleset::RuleSet::phobos).
//!
//! An engine extension is in every build and switches on from the game's data
//! (AGENTS.md, "Stability, extensions and online play"), as a modder expects
//! from Phobos.dll: adding Phobos keys to an INI is all it takes. Phobos plays
//! in a game when its Rules projection finds a key that a reader in
//! [`phobos_read`] asked for and no stock or Ares reader read in that
//! section. Only the readers of the Phobos pieces that run in every build
//! open that scope: the parallel AI queue limits
//! (`rules::phobos_parallel_ai_queues`), the last-legs sale, hunt and
//! regroup switches (`[General] AIFireSale`, `AIFireSaleDelay`, `AIAllToHunt`,
//! `GatherWhenMCVDeploy`), the AI's spare power (`[AI] EnablePowerSurplus`,
//! `PowerSurplus.ScaleToDrainAmount`) and the target zone scan (a TechnoType's
//! `TargetZoneScanType`). `AllowParallelAIQueues`, which Ares reads too,
//! turns neither switch on. Retail data holds none of these keys, so a retail
//! game plays as gamemd.
//!
//! A mod can also declare Phobos in its `mod.ini`, as it declares Ares:
//! `[Extensions] Phobos=yes` turns Phobos on for every game it loads, even
//! when its rules carry no Phobos key of their own. The declaration is saved
//! with the processed rules, so a save, a replay and every peer agree. No
//! player setting forces Phobos off; the game's data still decides when the
//! key is `no` or absent.
//!
//! Only the projection's reads count. The processing passes' reads reach
//! Ares through `AresInputs`, which keeps no Phobos or stock reads, so a
//! Phobos reader in a pass, or a Phobos key a pass reader also reads, needs
//! those reads passed here first.
//!
//! The answer comes from the recorded reads that also decide Ares
//! (`ares_switch::record`), and the RuleSet stores it, so a save, a replay
//! and every peer reach it again from the same rules, mode and map. A game
//! whose data names none of those keys plays as gamemd even though every
//! build carries the Phobos code.

use crate::rules::ares_switch::KeyReads;

/// One Phobos read written inline (a struct field), marked as a Phobos
/// reader's, so its key turns Phobos on (see the module notes). The scope is
/// noted by the reader recorder both switches share
/// (`ares_switch::phobos_reads`).
pub(crate) fn phobos_read<T>(read: impl FnOnce() -> T) -> T {
    let _phobos = crate::rules::ares_switch::phobos_reads();
    read()
}

/// Whether Phobos plays in the game whose Rules projection made these reads:
/// the selected mod declared `[Extensions] Phobos=yes`, or a key only Phobos
/// readers found. What decided goes to the log, so a modder can see why.
pub(super) fn decide(projection: &KeyReads, required: bool) -> bool {
    if required {
        log::info!("Phobos plays in this game: the selected mod declares [Extensions] Phobos=yes");
        return true;
    }
    let key = projection.phobos_only().min();
    if let Some((section, key)) = key {
        log::info!("Phobos plays in this game: [{section}] {key} is a Phobos key");
    }
    key.is_some()
}
