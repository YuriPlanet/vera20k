//! Guards for state shared across threads or changed through `&`: every
//! entry into the worker pool says what its tasks do about the shared cell
//! Dummy, simulation state behind a lock, a cell or an atomic is pinned with
//! its reason, and `Deref` stays for smart pointers and guards.
//!
//! The scans read production text: [`strip_test_items`] drops `#[cfg(test)]`
//! items and `#[cfg(any(test, feature = "test-support"))]` ones, since only
//! the client crate's dev-dependency turns test-support on and no game build
//! compiles those seams. Items gated otherwise stay scanned.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{
    blank_comments_and_literals, closing, count_identifier, engine_production_files, source_files,
    strip_test_items, test_gated_files,
};

/// Identifiers that enter the worker pool: the `sim::parallel` module (any
/// path to it names it), the entity store's pooled walks, rayon itself and
/// `std::thread`.
const POOL_ENTRIES: &[&str] = &[
    "parallel",
    "par_select",
    "par_map",
    "par_filter_map",
    "rayon",
    "thread",
];

/// Mentions of [`POOL_ENTRIES`] in production text `code`.
fn pool_entry_count(code: &str) -> usize {
    POOL_ENTRIES
        .iter()
        .map(|entry| count_identifier(code, entry))
        .sum()
}

/// Every engine production file that enters the worker pool, with its count
/// of entry mentions and what its pooled tasks do about the shared cell
/// Dummy (`map::resolved_terrain::SharedCellDummy`). No pooled task writes
/// the Dummy's other words: every writer of them takes `&mut` of the grid,
/// an overlay, smudge or occupation owner or the Simulation, or runs at map
/// load on a Dummy no task holds, and a task holds only `&` borrows of the
/// world (`for_each_mut` hands out its own items).
const POOL_ENTRY_PINS: &[(&str, usize, &str)] = &[
    (
        "assets/mix_archive.rs",
        1,
        "never looks a cell up: a read-ahead thread touches a mapped file's pages",
    ),
    (
        "map/rmg/build.rs",
        1,
        "never looks a cell up: Fair Starts' candidate maps generate on random-map \
         scratch before any terrain grid exists",
    ),
    (
        "net/udp.rs",
        7,
        "never looks a cell up: the LAN socket's I/O worker moves packets",
    ),
    (
        "sim/combat/greatest_threat.rs",
        1,
        "detached: `flat_scores` scores each candidate inside `detached_stamps` and \
         scores the list again one by one when any candidate stamped",
    ),
    (
        "sim/entity_store.rs",
        7,
        "defines `par_select`, `par_map` and `par_filter_map`, whose closures are \
         their callers' (pinned here); its own debug check reads held Anims only",
    ),
    (
        "sim/house_threat.rs",
        1,
        "detached: each threat contribution runs inside `detached_stamps`, and the \
         census walks one by one when any stamped",
    ),
    ("sim/mod.rs", 1, "declares the `parallel` module"),
    (
        "sim/movement/path_batch.rs",
        1,
        "detached: each batched search runs inside `detached`; outside it a task \
         reads only its step counters",
    ),
    (
        "sim/parallel.rs",
        5,
        "the pool's entries themselves; what their tasks do is their callers' pin",
    ),
    (
        "sim/pathfinding/zone_build.rs",
        2,
        "never looks a cell up: zone rows build over plain arrays, and the bridge \
         redirect reads cells with `cell()`, which answers `None` off the map",
    ),
    (
        "sim/pathfinding/zone_map.rs",
        1,
        "never looks a cell up: rows project the base topology's arrays",
    ),
    (
        "sim/secret_lab.rs",
        1,
        "never looks a cell up: the filter reads each building's type in the rules",
    ),
    (
        "sim/state_hasher.rs",
        3,
        "never looks a cell up: digests of the items its callers hash",
    ),
    (
        "sim/vision/planes.rs",
        1,
        "never looks a cell up: a plane's waiting writes are interior ones, \
         applied with no `NativeCellQuery`",
    ),
    (
        "sim/world/expiry_listeners.rs",
        2,
        "never looks a cell up: re-reads the ids an object record names",
    ),
    (
        "sim/world/mod.rs",
        1,
        "never looks a cell up: syncs the presentation copy's parts side by side",
    ),
    (
        "sim/world/sim_construction.rs",
        3,
        "never looks a cell up: the presentation copy's syncs, fog copy, clone and \
         the old copy's drop; the copy's Dummy is then forked",
    ),
    (
        "sim/world/techno_ai/scan_batch.rs",
        1,
        "detached: each target scan runs inside `detached_stamps` and one that \
         stamped is dropped for its turn; the due check before it reads the object",
    ),
    (
        "sim/world/world_hash.rs",
        5,
        "never looks a cell up: the fog and entity digests and the retained-Dummy \
         scans, which resolve cells with `native_fixed_cell_index`",
    ),
];

#[test]
fn pool_entries_say_how_their_tasks_treat_the_cell_dummy() {
    let found: BTreeMap<&str, usize> = engine_production_files()
        .into_iter()
        .map(|(file, production)| (file, pool_entry_count(production)))
        .filter(|&(_, count)| count > 0)
        .collect();
    assert_pins(
        &found,
        POOL_ENTRY_PINS,
        "A pooled task may look a cell up only inside `SharedCellDummy::detached` \
         (or `detached_stamps`) or through `NativeCellQuery::isolated`, and must \
         never write the Dummy's other words; see the `SharedCellDummy` doc in \
         src/map/resolved_terrain/native_cell.rs. Pin the file with its entry count \
         and the reason its tasks keep that rule.",
    );
}

/// `found` (file -> count) against `pins` (file, count, reason).
fn assert_pins(found: &BTreeMap<&str, usize>, pins: &[(&str, usize, &str)], rule: &str) {
    let pinned: BTreeMap<&str, usize> =
        pins.iter().map(|&(file, count, _)| (file, count)).collect();
    let over: Vec<String> = found
        .iter()
        .filter(|&(file, count)| pinned.get(file).is_none_or(|pin| count > pin))
        .map(|(file, count)| format!("(\"{file}\", {count}, ...)"))
        .collect();
    let stale: Vec<String> = pinned
        .iter()
        .filter(|&(file, pin)| found.get(file).is_none_or(|count| count < pin))
        .map(|(file, pin)| format!("{file}: pinned {pin}, found {:?}", found.get(file)))
        .collect();
    assert!(
        over.is_empty(),
        "new or grown site(s), unpinned: {over:#?}\n{rule}"
    );
    assert!(
        stale.is_empty(),
        "stale pin(s): {stale:#?}\nLower or delete the pin so the guard tightens."
    );
}

/// Mentions of each kind of shared mutable state in production text `code`:
/// locks, `RefCell`, `std::cell::Cell`, `thread_local!` and every `Atomic*`
/// type, wherever it appears (a static, a field, a local or an import).
fn shared_state_counts(code: &str) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for kind in ["Mutex", "RwLock", "RefCell", "thread_local"] {
        counts.insert(kind, count_identifier(code, kind));
    }
    counts.insert("Atomic*", count_identifier_prefix(code, "Atomic"));
    counts.insert("Cell", std_cell_count(code));
    counts.retain(|_, count| *count > 0);
    counts
}

/// Identifiers in `code` that start with `prefix`.
fn count_identifier_prefix(code: &str, prefix: &str) -> usize {
    let bytes = code.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    code.match_indices(prefix)
        .filter(|&(at, _)| at == 0 || !is_ident(bytes[at - 1]))
        .count()
}

/// Uses of `std::cell::Cell`: a `Cell` followed by `<` or by `::` and a
/// lowercase item (`Cell::new`, `Cell::get`), a `Cell` named by a `cell::`
/// path, or one in a `cell::{..}` import group. A `TargetKind::Cell(..)`
/// variant, or `Cell::Real` through an alias of `NativeCellIdentity`, is none
/// of these.
fn std_cell_count(code: &str) -> usize {
    let bytes = code.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let groups: Vec<(usize, usize)> = code
        .match_indices("cell::{")
        .filter_map(|(at, _)| {
            let open = at + "cell::".len();
            closing(code, open, b'{', b'}').map(|close| (open, close))
        })
        .collect();
    code.match_indices("Cell")
        .filter(|&(at, _)| {
            let end = at + "Cell".len();
            if (at > 0 && is_ident(bytes[at - 1])) || bytes.get(end).is_some_and(|&b| is_ident(b)) {
                return false;
            }
            let after = code[end..].trim_start();
            let item = after.strip_prefix("::").map(str::trim_start);
            after.starts_with('<')
                || item.is_some_and(|item| !item.starts_with(|c: char| c.is_ascii_uppercase()))
                || code[..at].trim_end().ends_with("cell::")
                || groups.iter().any(|&(open, close)| open < at && at < close)
        })
        .count()
}

/// Every production use in simulation state code (`sim/`, `rules/`, `map/`)
/// of a lock, `RefCell`, `std::cell::Cell`, `thread_local!` or an atomic,
/// by file, with its count of mentions and why that state is shared or
/// changed through `&`. `sim/parallel.rs`, the pool itself, is not scanned.
const SHARED_STATE_PINS: &[(&str, usize, &str)] = &[
    (
        "map/resolved_terrain.rs",
        5,
        "imports the atomic word types `SharedCellDummy` is made of",
    ),
    (
        "map/resolved_terrain/native_cell.rs",
        27,
        "`SharedCellDummy`, native's one aliased fallback CellClass: lookups through \
         `&` stamp it, and pooled stamps go to a thread-local copy (`detached`); \
         see its doc",
    ),
    (
        "map/rmg/build.rs",
        1,
        "one generation's VERA urban draw stream, shared by its two closures",
    ),
    (
        "map/source.rs",
        2,
        "the mod folder in use, a process setting each start sets; not scenario state",
    ),
    (
        "map/source/cache/browser_pins.rs",
        2,
        "the map cache's weak pins on files in use, shared by every loader",
    ),
    (
        "rules/ares_switch.rs",
        4,
        "this thread's reader recorder: every INI reader lookup reports its key and \
         the kind of reader that asked, about 3,600 calls that take only the section; \
         its scope guards are `!Send` and `ares_switch_tests` keeps threads out of \
         `src/rules`",
    ),
    (
        "rules/art_data.rs",
        2,
        "a process-unique serial per AnimType config layout, never hashed or saved, so a \
         handle is never read against another layout",
    ),
    (
        "rules/ruleset.rs",
        2,
        "a process-unique serial per RuleSet, never hashed or saved",
    ),
    (
        "sim/frame_profile.rs",
        4,
        "the opt-in frame profiler's clock for the running object pass; wall time \
         that no simulation state reads",
    ),
    (
        "sim/intern.rs",
        2,
        "a process-unique serial per interner, never hashed or saved",
    ),
    (
        "sim/movement/foot_path.rs",
        3,
        "one search's own-cell mark and last real cell: A* and path finishing \
         hold the entry's marker overlay by `&` across every class question, \
         so the entry is shared, never lent `&mut`",
    ),
    (
        "sim/movement/path_batch.rs",
        6,
        "a batch's per-search expansion counts, read across workers by the re-path \
         budget cut, and one search's stop flag",
    ),
    (
        "sim/pathfinding/core.rs",
        9,
        "this thread's A* workspace and search-step count: scratch reset before each \
         search, never hashed",
    ),
    (
        "sim/pathfinding/zone_hierarchy.rs",
        7,
        "the precheck memo path-batch workers share, whose answers are exact whatever \
         order fills it, and this thread's precheck scratch",
    ),
    (
        "sim/ra2_all02_diagnostic.rs",
        6,
        "the RA2 Mode ALL02S trace's written-once flag and its last line, so it logs a \
         change once; log output only, which no simulation state reads",
    ),
    (
        "sim/rng.rs",
        4,
        "`MainRng`, the app's process cell of the unsynced stream outside the hash \
         and saves, drawn on the main and audio output threads; the simulation owns \
         its Main as a plain `SimRng`",
    ),
    (
        "sim/world/entry_facts.rs",
        8,
        "a path batch's facts table, each entry kept once and read by every worker; \
         emptied each batch, never saved or hashed",
    ),
];

#[test]
fn simulation_state_behind_locks_cells_or_atomics_is_pinned() {
    let mut kinds = BTreeMap::new();
    let found: BTreeMap<&str, usize> = engine_production_files()
        .into_iter()
        .filter(|(file, _)| {
            ["sim/", "rules/", "map/"]
                .iter()
                .any(|root| file.starts_with(root))
                && *file != "sim/parallel.rs"
        })
        .filter_map(|(file, production)| {
            let counts = shared_state_counts(production);
            let total = counts.values().sum::<usize>();
            kinds.insert(file, counts);
            (total > 0).then_some((file, total))
        })
        .collect();
    kinds.retain(|_, counts| !counts.is_empty());
    assert_pins(
        &found,
        SHARED_STATE_PINS,
        &format!(
            "Simulation state has one owner and is changed through `&mut`. If this \
             is a real exception, add the pin with its reason.\nMentions by kind: {kinds:#?}"
        ),
    );
}

/// The types whose `Deref` (and `DerefMut`) impls stand: smart pointers and
/// guards, which own or borrow exactly their target and add no field a
/// caller reads through the deref. (type, file, reason).
const DEREF_TYPES: &[(&str, &str, &str)] = &[
    (
        "SharedBytes",
        "assets/mix_archive.rs",
        "a file's bytes holding a share of their storage, read as `[u8]`",
    ),
    (
        "UdpReadGuard",
        "net/udp.rs",
        "a projection of the LAN I/O owner's mutex guard",
    ),
    (
        "UdpWriteGuard",
        "net/udp.rs",
        "a projection of the LAN I/O owner's mutex guard; its drop wakes the I/O worker",
    ),
    (
        "StashedLocomotor",
        "sim/movement/locomotion/piggyback.rs",
        "an owning box of the one nested locomotor, read only",
    ),
    (
        "FrameChannel",
        "sim/world/frame_channel.rs",
        "read-only slice view of one frame's facts",
    ),
    (
        "HumanPlayerClients",
        "sim/world/shroud_refresh.rs",
        "no client, one or a shared list, read as one slice of clients",
    ),
    (
        "ModFolder",
        "util/mod_package.rs",
        "a mod folder's path, which owns the prepared directory it names while it lives",
    ),
    (
        "Sparse",
        "util/sparse.rs",
        "an out-of-line `T` that reads a shared default until its first write",
    ),
    (
        "Synced",
        "util/synced.rs",
        "a change-tracking cell: `deref_mut` marks the value for the presentation copy",
    ),
    (
        "DerivedCache",
        "util/synced.rs",
        "a `OnceLock` whose clone starts empty; it forwards the lock",
    ),
    (
        "HandoffQueue",
        "render/frame_handoff.rs",
        "a guard: reaching the GPU queue waits for the frame on its way to the driver",
    ),
    (
        "RecordsMut",
        "app/scenario_catalog.rs",
        "a mutation guard over the catalogue's records; its drop re-projects the menu",
    ),
];

#[test]
fn deref_impls_are_smart_pointers_or_guards() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = source_files(&src_root);
    let gated = test_gated_files(files);
    let found: BTreeSet<(String, &str)> = files
        .iter()
        .filter(|file| !gated.contains(&file.rel))
        .filter_map(|file| Some((file.rel.as_str(), file.production.as_deref()?)))
        .flat_map(|(rel, production)| {
            deref_impl_types(production)
                .into_iter()
                .map(move |ty| (ty, rel))
        })
        .collect();
    let allowed: BTreeSet<(String, &str)> = DEREF_TYPES
        .iter()
        .map(|&(ty, file, _)| (ty.to_string(), file))
        .collect();
    let unexpected: Vec<_> = found.difference(&allowed).collect();
    let stale: Vec<_> = allowed.difference(&found).collect();
    assert!(
        unexpected.is_empty(),
        "`Deref`/`DerefMut` impls outside the allow-list: {unexpected:?}\n\
         Deref is for smart pointers and guards, not for reaching another type's \
         fields as if inherited: name the field (`x.part.field`) or give the owner \
         an operation. A new smart pointer or guard goes in DEREF_TYPES with its reason."
    );
    assert!(stale.is_empty(), "stale DEREF_TYPES entr(ies): {stale:?}");
}

/// The engine boxes no iterator behind `IntoIterator`: a `for` loop over it
/// would allocate and dispatch through a vtable per element. Name the
/// concrete iterator type instead.
#[test]
fn engine_into_iterators_are_not_boxed() {
    let boxed: Vec<&str> = engine_production_files()
        .into_iter()
        .filter(|(_, production)| boxes_into_iter(production))
        .map(|(file, _)| file)
        .collect();
    assert!(
        boxed.is_empty(),
        "`type IntoIter = Box<dyn ..>` in engine code: {boxed:?}\n\
         Name the concrete iterator (for example `Flatten<slice::Iter<'a, Vec<T>>>`)."
    );
}

fn boxes_into_iter(code: &str) -> bool {
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains("typeIntoIter=Box<dyn") || compact.contains("typeIntoIter=std::boxed::Box<dyn")
}

/// The implementing type of each `impl .. Deref for T` and `impl .. DerefMut
/// for T` in blanked `code`, also through a renamed import (`use
/// std::ops::Deref as D`).
fn deref_impl_types(code: &str) -> Vec<String> {
    let bytes = code.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut traits = vec!["Deref".to_string(), "DerefMut".to_string()];
    let words: Vec<&str> = code.split_whitespace().collect();
    for window in words.windows(3) {
        if let [name, "as", alias] = window
            && matches!(name.rsplit("::").next(), Some("Deref" | "DerefMut"))
        {
            let alias: String = alias
                .bytes()
                .take_while(|&b| is_ident(b))
                .map(char::from)
                .collect();
            traits.push(alias);
        }
    }
    let mut out = Vec::new();
    for (at, _) in code.match_indices("impl") {
        let end = at + "impl".len();
        if (at > 0 && is_ident(bytes[at - 1]))
            || !bytes
                .get(end)
                .is_some_and(|&b| b.is_ascii_whitespace() || b == b'<')
        {
            continue;
        }
        let mut header_start = end + code[end..].len() - code[end..].trim_start().len();
        if code[header_start..].starts_with('<') {
            let Some(close) = closing(code, header_start, b'<', b'>') else {
                continue;
            };
            header_start = close + 1;
        }
        // The header runs to the first `{` or `;` outside brackets; an
        // `impl Trait` in a signature closes an enclosing bracket first.
        let mut depth = 0i32;
        let mut prev = 0u8;
        let mut header_end = None;
        for (offset, b) in code[header_start..].bytes().enumerate() {
            match b {
                b'(' | b'[' | b'<' => depth += 1,
                b'>' if prev == b'-' => {}
                b')' | b']' | b'>' => {
                    depth -= 1;
                    if depth < 0 {
                        break;
                    }
                }
                b'{' | b';' if depth == 0 => {
                    header_end = Some(header_start + offset);
                    break;
                }
                _ => {}
            }
            prev = b;
        }
        let Some(header_end) = header_end else {
            continue;
        };
        let header = code[header_start..header_end]
            .split_whitespace()
            .collect::<Vec<_>>();
        let Some(for_at) = header.iter().position(|&word| word == "for") else {
            continue;
        };
        let trait_path = header[..for_at].join(" ");
        let trait_name = trait_path
            .split('<')
            .next()
            .unwrap_or("")
            .rsplit("::")
            .next()
            .unwrap_or("")
            .trim();
        if !traits.iter().any(|name| name == trait_name) {
            continue;
        }
        let ty = header[for_at + 1..].join(" ");
        let ty = ty.split(" where").next().unwrap_or("").trim();
        let ty = ty.trim_start_matches('&').trim_start();
        let ty = match ty.strip_prefix('\'') {
            Some(lifetime) => {
                lifetime.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_')
            }
            None => ty,
        };
        let ty = ty.trim_start();
        let ty = ty.strip_prefix("mut ").unwrap_or(ty);
        // The type's own name: before its generics, after its path.
        let name = ty.split('<').next().unwrap_or("").rsplit("::").next();
        out.push(name.unwrap_or("").trim().to_string());
    }
    out
}

#[test]
fn pool_entry_scan_counts_renamed_and_imported_entries() {
    let source = r#"
use crate::sim::parallel as pool;
use std::thread::spawn as go;
// parallel::map(&items, work) in a comment is no entry
fn a() { pool::map(&items, |item| item.len()); }
fn b() { rayon::join(|| (), || ()); }
fn c(store: &EntityStore) { store.par_filter_map(|id, entity| Some(id)); }
fn d() { let parallel_ai_queues = 1; reserve_parallel(); }
#[cfg(test)]
fn e() { std::thread::scope(|scope| ()); }
"#;
    let production = strip_test_items(source, &blank_comments_and_literals(source));
    // `parallel`, `thread`, `rayon` and `par_filter_map`.
    assert_eq!(pool_entry_count(&production), 4, "{production}");
}

#[test]
fn shared_state_scan_counts_evading_spellings() {
    let source = r#"
use std::cell::RefCell as R;
use std::cell::{Cell as C, OnceCell};
static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
struct S { seen: core::cell::Cell<bool>, last: Option<AtomicI64> }
fn f() { let m = std::sync::Mutex::<u8>::new(0); let l: RwLock<u8>; }
thread_local! { static T: u8 = 0; }
fn g(target: TargetKind) { let _ = TargetKind::Cell(1, 2); let _ = Cell::Real(3); R::new(0); }
// let commented = Mutex::new(0);
#[cfg(any(test, feature = "test-support"))]
static SEAM: std::sync::Mutex<u8> = std::sync::Mutex::new(0);
"#;
    let production = strip_test_items(source, &blank_comments_and_literals(source));
    let counts = shared_state_counts(&production);
    let expected = BTreeMap::from([
        ("Atomic*", 3),
        ("Cell", 2),
        ("Mutex", 1),
        ("RefCell", 1),
        ("RwLock", 1),
        ("thread_local", 1),
    ]);
    assert_eq!(counts, expected, "{production}");
}

#[test]
fn deref_scan_reads_multiline_renamed_and_generic_impls() {
    let source = r#"
use std::ops::DerefMut as Dm;
impl<T>
    Deref
    for Spread<T> { type Target = T; }
impl Dm for Renamed { }
impl<T: SparseDefault> DerefMut for Sparse<T> { }
impl<'a, T: Clone> std::ops::Deref for &'a Borrowed<'a, T> where T: Copy { }
impl<T: Deref> Display for Wrapper<T> { }
fn takes(x: impl Deref<Target = u8>) { }
impl std::fmt::Debug for Plain { }
"#;
    let code = blank_comments_and_literals(source);
    assert_eq!(
        deref_impl_types(&code),
        ["Spread", "Renamed", "Sparse", "Borrowed"]
    );
    assert!(boxes_into_iter(
        "impl X for Y { type IntoIter =\n    Box<dyn Iterator<Item = u8>>; }"
    ));
    assert!(!boxes_into_iter(
        "type IntoIter = std::iter::Flatten<std::slice::Iter<'a, Vec<T>>>;"
    ));
}
