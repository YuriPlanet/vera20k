//! Whether Ares plays in a game: the one per-game switch the run-time Ares
//! paths read, through [`RuleSet::ares`](crate::rules::ruleset::RuleSet::ares).
//!
//! An engine extension is in every build and switches on from the game's data
//! (AGENTS.md, "Stability, extensions and online play"), as a modder expects
//! from Ares.dll: adding Ares keys to an INI is all it takes. Ares plays in a
//! game when one of its Rules passes (RULESMD, LANGRULE, the mode INI, the
//! map, TMCJ4F) holds a key that an Ares reader finds and no stock reader
//! reads in that section, or when the selected mod mounts its own or the
//! installation's `ares.mix` (Ares mounts it from `Load_Bootstrap_AresMIX`, hook
//! `0x0053029E`). Retail data holds neither, so a retail game plays as
//! gamemd. The player's Ares setting (`util::ares_launch::AresSetting`) can
//! replace that answer for a whole launch: On makes every game an Ares game,
//! as a mounted `ares.mix` does, and Off keeps every key from turning Ares on.
//! The game's asset manager holds both answers (`AssetManager::ships_ares`,
//! `AssetManager::ares_setting`), and its Rules owner takes them from it
//! (`RetailRulesSources::select`). Every build lets the switch turn on, the
//! release build too.
//!
//! Which keys count comes from the readers, not from a list. Every reader
//! lookup of a present key (`IniSection`'s reader path in `ini_parser.rs`)
//! is noted with who asked: an Ares reader, a Phobos reader the Phobos
//! switch counts (`phobos_switch`), or a stock reader. The `read_ares_*`
//! parsers always ask as Ares, and an Ares reader body opens [`ares_reads`]
//! for the stock readers it calls. A key a stock or Phobos reader also read
//! in the same section does not count, since Ares re-reads stock keys such
//! as `Prerequisite`, `Armor` or `Verses`, and a key both extensions read
//! turns neither on. Reads through Ares' parsers that are not Ares keys open
//! [`not_ares_reads`]: an Ares reader that replaces a stock read or reads a
//! key retail data already holds, and the Phobos readers, which parse with
//! Ares' `Valueable<T>` readers, so a Phobos-only mod keeps Ares off. That
//! holds in a build with the `phobos` feature: without those readers the
//! three keys both extensions read (`LaserThickness`, `Beam.IsHouseColor`,
//! `Passengers.BySize`) count as Ares'. The store's own merges and walks are
//! not reader lookups.
//!
//! One game, one answer. The processing passes keep their Ares-only keys
//! with the processed rules ([`AresInputs`], saved with them); the RuleSet
//! projection adds the keys its own readers find and stores the answer, so a
//! save, a replay and every peer reach it again from the same rules, mode and
//! map. The shell's and the map stage's projections answer for their own
//! layers.
//!
//! How a game is read follows the game too. Ares.dll replaces some stock
//! reads (list capacities, the `[WeaponTypes]` pre-allocation, the generic
//! prerequisites and armors), adds its own, and hooks every INI text load
//! (its `+` keys, `[Child]:[Base]` sections, later-wins keys and
//! `[#include]`); a game that ships `ares.mix` has its INIs loaded and its
//! Rules processed and projected the Ares way ([`ares_way`]), any other game
//! the stock way. With no mod selected, an installed `ares.mix` is ignored
//! unless the player's Ares setting is On; a selected mod can mount its own
//! or the installation's copy, and the setting Off mounts none
//! (`AssetManager::ships_ares`). The few Ares readers whose state only Ares
//! code uses run in both, so their keys still turn Ares on in
//! a stock-way game; the rest run only the Ares way. The mode belongs to the
//! retained Rules registry, and each Process call and projection opens it
//! ([`read_ares_way`]); the game's later readers (its fixed ART, the asset
//! closures, SOUNDMD and EVAMD) open it from the RuleSet
//! (`RuleSet::reads_ares_way`) or from whether the game ships `ares.mix`.
//! Outside every scope a thread reads by its launch, the game the launch
//! opened for play (`util::ares_launch`), as Ares.dll patches a whole
//! launch: the INI text loads of a game that ships `ares.mix` take Ares'
//! syntax wherever they run. A game turned on by its keys alone, without
//! `ares.mix`, still loads its INIs and reads its Rules the stock way.
//!
//! Why the reads are recorded on the reading thread, not handed down: the
//! stock readers (`IniSection::read_*`, thousands of call sites) take only
//! the section, and which keys count depends on who asked, an Ares, a Phobos
//! or a stock reader, so neither a scan of the INI data nor a context in the
//! readers' signatures can say it without rewriting every reader. One
//! thread-local `Recorder` keeps the open scopes and the record instead.
//! Its invariant: the reads a [`record`] run counts, and the reads a
//! [`read_ares_way`] scope governs, run on the thread that opened them. The
//! scope guards are `!Send`, so a scope cannot leave its thread, and
//! `ares_switch_tests` checks that the Rules code under `src/rules` spawns no
//! thread and uses no thread pool. Code outside it that a scope covers and
//! that starts threads (the random map generator's workers) hands them their
//! inputs already read and passes `ares` explicitly: a thread outside every
//! scope reads by the launch's answer (`util::ares_launch`), never by this
//! thread's. Two loads on two threads keep their own records; a load nested
//! in another keeps its reads to itself.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

/// Makes a scope guard `!Send`: the scope it closes lives in this thread's
/// [`Recorder`], so it must drop on the thread that opened it.
type OnThisThread = PhantomData<*const ()>;

/// Whether the game this launch opened for play ships `ares.mix`
/// (`util::ares_launch`): the answer for a thread outside every scope. A
/// process that opened no game for play (a test, a tool before its game
/// opens) reads the stock way there.
fn launch_ships_ares() -> bool {
    crate::util::ares_launch::game_ships_ares()
}

/// Whether the reads on this thread follow Ares.dll: its INI syntax, list
/// capacities, type pre-allocations and replacement readers. Each Process
/// call and RuleSet projection sets it from the retained Rules registry
/// ([`read_ares_way`]); outside every scope it is the launch's answer
/// ([`launch_ships_ares`]). Ares readers that only fill Ares state do not
/// ask. A Rules fixture built without saying (`RulesLayerStack::new`, a
/// cold-started owner) stands for a game that ships `ares.mix` exactly when
/// its thread reads the Ares way, so an Ares test opens [`read_ares_way`]
/// first.
pub(crate) fn ares_way() -> bool {
    RECORDER
        .with_borrow(|recorder| recorder.ares_way)
        .unwrap_or_else(launch_ships_ares)
}

/// Reads the Rules on this thread the Ares way (or not) while it lives, and
/// restores the outer mode when it drops.
#[must_use = "the scope ends when the guard drops"]
pub struct AresWay {
    /// The scope this one opened inside, or none.
    outer: Option<bool>,
    _thread: OnThisThread,
}

/// Open a Rules-reading scope in the given mode (see [`ares_way`]).
pub fn read_ares_way(on: bool) -> AresWay {
    AresWay {
        outer: RECORDER.with_borrow_mut(|recorder| recorder.ares_way.replace(on)),
        _thread: PhantomData,
    }
}

impl Drop for AresWay {
    fn drop(&mut self) {
        RECORDER.with_borrow_mut(|recorder| recorder.ares_way = self.outer);
    }
}

/// Marks the reads made while it lives as an Ares reader's. Every Ares reader
/// body that reads through the stock readers opens one; the `read_ares_*`
/// parsers open their own.
#[must_use = "the scope ends when the guard drops"]
pub(crate) struct AresReads(OnThisThread);

/// Open an Ares reader scope (see [`AresReads`]).
pub(crate) fn ares_reads() -> AresReads {
    RECORDER.with_borrow_mut(|recorder| recorder.ares_depth += 1);
    AresReads(PhantomData)
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

/// Marks the reads made while it lives as not Ares', even inside an Ares or
/// Phobos reader: a Phobos reader the Phobos switch does not count yet, a
/// stock key Ares re-parses in place of the stock reader (`Armor`,
/// `DebrisMaximums`), or an RA2 or TS key retail data already holds
/// (`TiberiumProof`, the IFV turret keys). They count as stock reads.
#[must_use = "the scope ends when the guard drops"]
pub(crate) struct NotAresReads(OnThisThread);

/// Open a not-Ares reader scope (see [`NotAresReads`]).
pub(crate) fn not_ares_reads() -> NotAresReads {
    RECORDER.with_borrow_mut(|recorder| recorder.not_ares_depth += 1);
    NotAresReads(PhantomData)
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

/// Marks the reads made while it lives as a Phobos reader's
/// (`phobos_switch::phobos_read`).
#[must_use = "the scope ends when the guard drops"]
pub(crate) struct PhobosReads(OnThisThread);

/// Open a Phobos reader scope (see [`PhobosReads`]).
pub(crate) fn phobos_reads() -> PhobosReads {
    RECORDER.with_borrow_mut(|recorder| recorder.phobos_depth += 1);
    PhobosReads(PhantomData)
}

impl Drop for PhobosReads {
    fn drop(&mut self) {
        RECORDER.with_borrow_mut(|recorder| recorder.phobos_depth -= 1);
    }
}

/// This thread's reading state: the open scopes and the [`record`] run they
/// note into (see the module notes for why it is per thread).
#[derive(Default)]
struct Recorder {
    /// The open [`read_ares_way`] scope's mode, or none outside every scope.
    ares_way: Option<bool>,
    ares_depth: u32,
    /// Open [`not_ares_reads`] scopes, which win over the other two.
    not_ares_depth: u32,
    /// Open [`phobos_reads`] scopes, which win over `ares_depth`.
    phobos_depth: u32,
    /// The innermost [`record`] run, if one is open on this thread.
    active: Option<KeyReads>,
}

thread_local! {
    static RECORDER: RefCell<Recorder> = RefCell::default();
}

/// A reader found `key` in `section`. Called by the INI store's reader
/// lookups only.
pub(crate) fn note_found(section: &str, key: &str) {
    RECORDER.with_borrow_mut(|recorder| {
        // A not-Ares scope wins over the reader that opened it.
        let reader = if recorder.not_ares_depth > 0 {
            Reader::Stock
        } else if recorder.phobos_depth > 0 {
            Reader::Phobos
        } else if recorder.ares_depth > 0 {
            Reader::Ares
        } else {
            Reader::Stock
        };
        if let Some(reads) = recorder.active.as_mut() {
            reads.note(reader, section, key);
        }
    });
}

/// Who asked for a key [`note_found`] noted.
#[derive(Clone, Copy)]
enum Reader {
    Stock,
    Ares,
    Phobos,
}

/// The present keys one recorded run's readers found, split by who asked.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyReads {
    ares: SectionKeys,
    phobos: SectionKeys,
    stock: SectionKeys,
}

/// Found keys by section name.
type SectionKeys = BTreeMap<String, BTreeSet<String>>;

impl KeyReads {
    fn note(&mut self, reader: Reader, section: &str, key: &str) {
        let map = match reader {
            Reader::Stock => &mut self.stock,
            Reader::Ares => &mut self.ares,
            Reader::Phobos => &mut self.phobos,
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
        for (map, later) in [
            (&mut self.ares, later.ares),
            (&mut self.phobos, later.phobos),
            (&mut self.stock, later.stock),
        ] {
            for (section, keys) in later {
                map.entry(section).or_default().extend(keys);
            }
        }
    }

    fn found(map: &SectionKeys, section: &str, key: &str) -> bool {
        map.get(section).is_some_and(|keys| keys.contains(key))
    }

    fn keys(map: &SectionKeys) -> impl Iterator<Item = (&str, &str)> {
        map.iter().flat_map(|(section, keys)| {
            keys.iter().map(move |key| (section.as_str(), key.as_str()))
        })
    }

    /// Whether a stock or Phobos reader of these reads saw the key.
    fn read_outside_ares(&self, section: &str, key: &str) -> bool {
        Self::found(&self.stock, section, key) || Self::found(&self.phobos, section, key)
    }

    /// The Ares readers' keys that no stock or Phobos reader of these reads
    /// saw.
    fn ares_only(&self) -> impl Iterator<Item = (&str, &str)> {
        Self::keys(&self.ares).filter(|(section, key)| !self.read_outside_ares(section, key))
    }

    /// The Phobos readers' keys that no stock or Ares reader of these reads
    /// saw (`phobos_switch`).
    pub(super) fn phobos_only(&self) -> impl Iterator<Item = (&str, &str)> {
        Self::keys(&self.phobos).filter(|(section, key)| {
            !Self::found(&self.stock, section, key) && !Self::found(&self.ares, section, key)
        })
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
    /// VERA: the player's Ares setting was Off (`AssetManager::ares_setting`), so no
    /// key turns Ares on. Saved with the Rules, so a save plays as it was
    /// started.
    keys_off: bool,
}

impl AresInputs {
    pub(crate) fn from_passes(reads: &KeyReads, ships_ares: bool, keys_off: bool) -> Self {
        Self {
            // Kept under the setting Off too, which only stops them from
            // turning Ares on ([`Self::kept_off`]).
            keys: reads
                .ares_only()
                .map(|(section, key)| (section.to_owned(), key.to_owned()))
                .collect(),
            ships_ares,
            keys_off: keys_off && !ships_ares,
        }
    }

    /// The Ares key that turns Ares on once the projection's own reads are
    /// in: the first that only Ares readers found and that remains after the
    /// projection's stock and Phobos reads.
    fn deciding_key<'a>(&'a self, projection: &'a KeyReads) -> Option<(&'a str, &'a str)> {
        self.keys
            .iter()
            .map(|(section, key)| (section.as_str(), key.as_str()))
            .filter(|(section, key)| !projection.read_outside_ares(section, key))
            .chain(projection.ares_only())
            .min()
    }

    /// Whether Ares plays once the projection's own reads are in: `ares.mix`
    /// ships, or a key only Ares readers found remains
    /// ([`Self::deciding_key`]) and the player's Ares setting is not Off. The
    /// deciding input goes to the log, so a modder can see why.
    pub(crate) fn decide(&self, projection: &KeyReads) -> bool {
        if self.ships_ares {
            log::info!("Ares plays in this game: ares.mix is installed or the Ares setting is On");
            return true;
        }
        match self.deciding_key(projection) {
            Some((section, key)) if self.keys_off => {
                log::info!(
                    "Ares stays off in this game, as the Ares setting Off says: [{section}] {key} \
                     is an Ares key"
                );
                false
            }
            Some((section, key)) => {
                log::info!("Ares plays in this game: [{section}] {key} is an Ares key");
                true
            }
            None => false,
        }
    }

    /// VERA: whether the player's Ares setting Off is what keeps Ares from
    /// playing: the game holds a key that turns Ares on under Auto. Such a
    /// game is another than the one a player set to Auto plays from the same
    /// files; with no such key the two are the same game.
    pub(crate) fn kept_off(&self, projection: &KeyReads) -> bool {
        self.keys_off && self.deciding_key(projection).is_some()
    }
}
