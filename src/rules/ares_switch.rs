//! Whether Ares plays in a game: the one per-game switch the run-time Ares
//! paths read, through [`RuleSet::ares`](crate::rules::ruleset::RuleSet::ares).
//!
//! An engine extension is in every build and switches on from the game's data
//! (AGENTS.md, "Stability, extensions and online play"), as a modder expects
//! from Ares.dll: adding Ares keys to an INI is all it takes. Ares plays in a
//! game when one of its Rules passes (RULESMD, LANGRULE, the mode INI, the
//! map, TMCJ4F) holds a key that an Ares reader finds and no stock reader
//! reads in that section, or when the installed game or the mod ships
//! `ares.mix` (Ares mounts it from `Load_Bootstrap_AresMIX`, hook
//! `0x0053029E`). Retail data holds neither, so a retail game plays as
//! gamemd. Every build carries the decision, but until the last Ares code
//! leaves the `ares` build feature only a build with that feature or a test
//! build lets it turn on ([`LIVE`]): the release build keeps Ares off, so a
//! player never gets half of Ares.
//!
//! Which keys count comes from the readers, not from a list. Every reader
//! lookup of a present key (`IniSection`'s reader path in `ini_parser.rs`)
//! is noted with whether an Ares reader asked: the `read_ares_*` parsers
//! always do, and an Ares reader body opens [`ares_reads`] for the stock
//! readers it calls. A key a stock reader also read in the same section does
//! not count, since Ares re-reads stock keys such as `Prerequisite`, `Armor`
//! or `Verses`. Reads through Ares' parsers that are not Ares keys open
//! [`not_ares_reads`]: an Ares reader that replaces a stock read or reads a
//! key retail data already holds, and the Phobos readers, which parse with
//! Ares' `Valueable<T>` readers, so a Phobos-only mod keeps Ares off. The
//! store's own merges and walks are not reader lookups.
//!
//! One game, one answer. The processing passes keep their Ares-only keys
//! with the processed rules ([`AresInputs`], saved with them); the RuleSet
//! projection adds the keys its own readers find and stores the answer, so a
//! save, a replay and every peer reach it again from the same rules, mode and
//! map. The shell's and the map stage's projections answer for their own
//! layers.
//!
//! How Rules are read follows the game too. Ares.dll replaces some stock
//! reads (list capacities, the `[WeaponTypes]` pre-allocation, the generic
//! prerequisites and armors) and adds its own; a game that ships `ares.mix`
//! has its Rules processed and projected the Ares way ([`ares_way`]), any
//! other game the stock way. The few Ares readers whose state only Ares code
//! uses run in both, so their keys still turn Ares on in a stock-way game;
//! the rest run only the Ares way. The mode belongs to the retained Rules
//! registry, and each Process call and projection opens it
//! ([`read_ares_way`]); the game's later readers (its fixed ART, the asset
//! closures, SOUNDMD and EVAMD) open it from the RuleSet
//! (`RuleSet::reads_ares_way`) or from whether the game ships `ares.mix`.
//! Until the `ares` feature is deleted, a build with it reads every game the
//! Ares way, as it always has ([`reading_mode`]).
//!
//! Still decided by the build feature, not this switch, until each moves to
//! it (AGENTS.md): every `cfg(feature = "ares")` site outside Rules
//! processing and projection, mounting `ares.mix` and the MIX order, the
//! rules agreement's `ares.mix` byte, the INI loader's Ares syntax and
//! `[#include]`, and the spawn and network launch paths. A game turned on by
//! its keys alone, without `ares.mix`, still reads its Rules the stock way
//! until loading reads them again the Ares way.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

/// Whether this build lets the switch turn on: a build with the `ares`
/// feature, or a test build. Until every Ares reader and path has left that
/// feature, a build without it (the release build) records nothing and keeps
/// Ares off, so a player never gets half of Ares, while tests run the code
/// already moved with the switch on in every build. The PR that deletes the
/// feature deletes this constant.
pub(crate) const LIVE: bool = cfg!(any(feature = "ares", test, feature = "test-support"));

/// The Rules-reading mode a scope that asks for `on` gets: the Ares way only
/// where the build lets Ares turn on ([`LIVE`]), and always in a build with
/// the `ares` feature, which keeps reading every game as Ares.dll does, as it
/// did before the switch. The PR that deletes the feature makes this `on`.
pub(crate) const fn reading_mode(on: bool) -> bool {
    cfg!(feature = "ares") || (on && LIVE)
}

/// Whether the Rules reads on this thread follow Ares.dll: its list
/// capacities, type pre-allocations and replacement readers. Each Process
/// call and RuleSet projection sets it from the retained Rules registry
/// ([`read_ares_way`]); Ares readers that only fill Ares state do not ask.
/// A fixture built without saying stands for a game that ships `ares.mix`
/// exactly when its thread reads the Ares way ([`game_ships_ares`]), so an
/// Ares test opens [`read_ares_way`] first.
pub(crate) fn ares_way() -> bool {
    ARES_WAY.get()
}

/// Whether the game ships `ares.mix`: startup mounted it
/// (`AssetManager::ships_ares`), or, in a test, the thread reads the Ares way,
/// so a fixture (a `RulesLayerStack`, a cold-started owner, the retail
/// install) stands for an Ares game.
pub(crate) fn game_ships_ares(mounted: bool) -> bool {
    mounted || (cfg!(any(test, feature = "test-support")) && ares_way())
}

/// Reads the Rules on this thread the Ares way (or not) while it lives, and
/// restores the outer mode when it drops.
#[must_use = "the scope ends when the guard drops"]
pub struct AresWay {
    outer: bool,
}

/// Open a Rules-reading scope in the given mode (see [`ares_way`] and
/// [`reading_mode`]).
pub fn read_ares_way(on: bool) -> AresWay {
    AresWay {
        outer: ARES_WAY.replace(reading_mode(on)),
    }
}

impl Drop for AresWay {
    fn drop(&mut self) {
        ARES_WAY.set(self.outer);
    }
}

/// Marks the reads made while it lives as an Ares reader's. Every Ares reader
/// body that reads through the stock readers opens one; the `read_ares_*`
/// parsers open their own.
#[must_use = "the scope ends when the guard drops"]
pub(crate) struct AresReads(());

/// Open an Ares reader scope (see [`AresReads`]).
pub(crate) fn ares_reads() -> AresReads {
    RECORDER.with_borrow_mut(|recorder| recorder.ares_depth += 1);
    AresReads(())
}

/// One Ares read written inline in a stock reader (a struct field or a call
/// argument), marked as [`ares_reads`] marks a body.
pub(crate) fn ares_read<T>(read: impl FnOnce() -> T) -> T {
    let _ares = ares_reads();
    read()
}

impl Drop for AresReads {
    fn drop(&mut self) {
        RECORDER.with_borrow_mut(|recorder| recorder.ares_depth -= 1);
    }
}

/// Marks the reads made while it lives as not Ares', even inside an Ares
/// reader: another extension's reader (Phobos), a stock key Ares re-parses in
/// place of the stock reader (`Armor`, `DebrisMaximums`), or an RA2 or TS key
/// retail data already holds (`TiberiumProof`, the IFV turret keys). They
/// count as stock reads.
#[must_use = "the scope ends when the guard drops"]
pub(crate) struct NotAresReads(());

/// Open a not-Ares reader scope (see [`NotAresReads`]).
pub(crate) fn not_ares_reads() -> NotAresReads {
    RECORDER.with_borrow_mut(|recorder| recorder.not_ares_depth += 1);
    NotAresReads(())
}

/// One read written inline, marked as [`not_ares_reads`] marks a body.
pub(crate) fn not_ares_read<T>(read: impl FnOnce() -> T) -> T {
    let _not_ares = not_ares_reads();
    read()
}

impl Drop for NotAresReads {
    fn drop(&mut self) {
        RECORDER.with_borrow_mut(|recorder| recorder.not_ares_depth -= 1);
    }
}

#[derive(Default)]
struct Recorder {
    ares_depth: u32,
    /// Open [`not_ares_reads`] scopes, which win over `ares_depth`.
    not_ares_depth: u32,
    /// The innermost [`record`] run, if one is open on this thread.
    active: Option<KeyReads>,
}

thread_local! {
    static RECORDER: RefCell<Recorder> = RefCell::default();
    static ARES_WAY: Cell<bool> = const { Cell::new(reading_mode(false)) };
}

/// A reader found `key` in `section`. Called by the INI store's reader
/// lookups only.
pub(crate) fn note_found(section: &str, key: &str) {
    if !LIVE {
        return;
    }
    RECORDER.with_borrow_mut(|recorder| {
        let ares = recorder.ares_depth > 0 && recorder.not_ares_depth == 0;
        if let Some(reads) = recorder.active.as_mut() {
            reads.note(ares, section, key);
        }
    });
}

/// The present keys one recorded run's readers found, split by who asked.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyReads {
    ares: SectionKeys,
    stock: SectionKeys,
}

/// Found keys by section name.
type SectionKeys = BTreeMap<String, BTreeSet<String>>;

impl KeyReads {
    fn note(&mut self, ares: bool, section: &str, key: &str) {
        let map = if ares {
            &mut self.ares
        } else {
            &mut self.stock
        };
        let keys = match map.get_mut(section) {
            Some(keys) => keys,
            None => map.entry(section.to_owned()).or_default(),
        };
        if !keys.contains(key) {
            keys.insert(key.to_owned());
        }
    }

    /// Fold a later run (the next Rules pass) into these reads.
    pub(crate) fn absorb(&mut self, later: KeyReads) {
        for (map, later) in [(&mut self.ares, later.ares), (&mut self.stock, later.stock)] {
            for (section, keys) in later {
                map.entry(section).or_default().extend(keys);
            }
        }
    }

    fn stock_read(&self, section: &str, key: &str) -> bool {
        self.stock
            .get(section)
            .is_some_and(|keys| keys.contains(key))
    }

    /// The Ares readers' keys that no stock reader of these reads saw.
    fn ares_only(&self) -> impl Iterator<Item = (&str, &str)> {
        self.ares
            .iter()
            .flat_map(|(section, keys)| {
                keys.iter().map(move |key| (section.as_str(), key.as_str()))
            })
            .filter(|(section, key)| !self.stock_read(section, key))
    }
}

/// Run `run` with its reader lookups recorded on this thread. A run inside
/// another keeps its reads to itself.
pub(crate) fn record<T>(run: impl FnOnce() -> T) -> (T, KeyReads) {
    let outer = RECORDER.with_borrow_mut(|recorder| recorder.active.replace(KeyReads::default()));
    let result = run();
    let reads = RECORDER.with_borrow_mut(|recorder| {
        std::mem::replace(&mut recorder.active, outer).unwrap_or_default()
    });
    (result, reads)
}

/// The Ares inputs a processed Rules result carries to its projections: the
/// Ares-only keys its passes found and whether the game ships `ares.mix`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresInputs {
    /// `(section, key)` pairs only an Ares reader of the passes found.
    keys: BTreeSet<(String, String)>,
    ships_ares: bool,
}

impl AresInputs {
    pub(crate) fn from_passes(reads: &KeyReads, ships_ares: bool) -> Self {
        Self {
            keys: reads
                .ares_only()
                .map(|(section, key)| (section.to_owned(), key.to_owned()))
                .collect(),
            ships_ares,
        }
    }

    /// Whether Ares plays once the projection's own reads are in: `ares.mix`
    /// ships, or a key only Ares readers found remains after the
    /// projection's stock reads. The deciding input goes to the log, so a
    /// modder can see why.
    pub(crate) fn decide(&self, projection: &KeyReads) -> bool {
        if self.ships_ares {
            log::info!("Ares plays in this game: ares.mix is installed");
            return true;
        }
        let key = self
            .keys
            .iter()
            .map(|(section, key)| (section.as_str(), key.as_str()))
            .filter(|(section, key)| !projection.stock_read(section, key))
            .chain(projection.ares_only())
            .min();
        if let Some((section, key)) = key {
            log::info!("Ares plays in this game: [{section}] {key} is an Ares key");
        }
        key.is_some()
    }
}
