//! Source-level guards for current engine ownership and dependency boundaries.
//!
//! LAYER_RULES forbids reverse production dependencies and the exception list is
//! empty. The scanner excludes tests.rs, *_tests.rs and #[cfg(test)] items;
//! cfg(any(test, feature = "test-support")) items are test-only too, since only
//! the client crate's dev-dependency enables vera20k-core's test-support feature.
//! cfg(any(test, debug_assertions)) remains production because debug builds use it.
//! A separate guard checks the entire sim tree, including tests, for upper-layer
//! references. Other guards pin frame API and raw INI walk callers and
//! AppState owner boundaries, and keep test fixtures out of `include_str!` and
//! `include_bytes!`.
//!
//! The app pseudo-root matches crate::app paths and retired root app_* modules.
//! The ui rule also forbids the retired skirmish_scenarios root. These checks
//! preserve the current layout rather than describing future migration work.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Directory under `src/` -> reference roots its production code must not
/// name. The simulation direction contract is in AGENTS.md; the remaining
/// rules preserve the established lower-layer boundaries.
const LAYER_RULES: &[(&str, &[&str])] = &[
    ("assets", &["sim", "rules", "map", "render", "ui", "app"]),
    ("util", &["sim", "rules", "map", "render", "ui", "app"]),
    ("rules", &["sim", "map"]),
    ("map", &["sim", "render", "app"]),
    ("render", &["app"]),
    ("ui", &["app", "skirmish_scenarios"]),
    ("sim", &["render", "ui", "audio", "net"]),
];

/// No production exceptions remain. Keep this inventory empty; new reverse
/// edges must be fixed at their owner rather than exempted from the guard.
const FROZEN_EXCEPTIONS: &[(&str, &str)] = &[];

#[test]
fn production_dependency_edges_match_frozen_ledger_inventory() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let found = scan_forbidden_edges(&src_root);
    let expected: BTreeSet<(String, String)> = FROZEN_EXCEPTIONS
        .iter()
        .map(|(file, root)| (file.to_string(), root.to_string()))
        .collect();

    let unexpected: Vec<_> = found.difference(&expected).collect();
    let stale: Vec<_> = expected.difference(&found).collect();
    assert!(
        unexpected.is_empty(),
        "new forbidden production dependency edge(s): {unexpected:?}\n\
         The layer contract (AGENTS.md, architecture boundaries) forbids these \
         directions. Move the type/function to its owning layer instead of \
         importing upward; FROZEN_EXCEPTIONS only ever shrinks."
    );
    assert!(
        stale.is_empty(),
        "stale FROZEN_EXCEPTIONS entr(ies): {stale:?}\n\
         The edge no longer exists in production source. Delete the entry so \
         the ratchet tightens."
    );

    // The #1 invariant (AGENTS.md): sim never depends on presentation, audio,
    // or net in production. Zero exceptions, frozen or otherwise.
    assert!(
        !FROZEN_EXCEPTIONS.iter().any(|(f, _)| f.starts_with("sim/")),
        "sim/ may never appear in FROZEN_EXCEPTIONS"
    );
}

/// F09: `Simulation::advance_master_frame` and `advance_app_frame` accept
/// swappable rules/map/path resources, so their production call sites are
/// pinned — the master frame is reached only inside `sim/world/mod.rs`, the
/// app frame only there and in `SimRuntime::advance_frame`. Every other
/// caller must be `#[cfg(test)]` (the `advance_tick` fixture adapter and the
/// replay `run_fixture*` runners are compile-time gated already); production
/// and tooling advance exclusively through the bound-resource runtime.
#[test]
fn master_frame_adapters_have_pinned_production_callers() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed: &[(&str, &[&str])] = &[
        ("advance_master_frame", &["sim/world/mod.rs"]),
        ("advance_app_frame", &["sim/world/mod.rs", "sim/runtime.rs"]),
    ];
    let mut violations: Vec<String> = Vec::new();
    for file in source_files(&src_root) {
        let Some(production) = &file.production else {
            continue;
        };
        for (symbol, files) in allowed {
            if production.contains(symbol) && !files.contains(&file.rel.as_str()) {
                violations.push(format!("{}: {symbol}", file.rel));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "swappable-resource frame adapters referenced outside their pinned \
         production sites: {violations:?}\n\
         Production advances only through SimRuntime::advance_frame (F09); \
         gate new fixture callers with #[cfg(test)]."
    );
}

/// Stored INI text leaves `IniSection` through the `ini_value` readers and
/// two walks: `raw_entries` (copying or showing entries) and `registry_ids`
/// (registries `native_processing` rewrote). Their production call sites are
/// pinned by file and count; a new caller reads through a reader instead.
#[test]
fn raw_ini_walks_have_pinned_production_callers() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let pinned: &[(&str, &[(&str, usize)])] = &[
        (
            "raw_entries",
            &[
                ("rules/ini_parser.rs", 1), // the definition
                // Copy Hotkey entries into the separate writable profile.
                ("app/persistence/keyboard.rs", 1),
                ("app/persistence/profile_file.rs", 1),
                // `ini-get --reader raw` and the presence report.
                ("asset_tools/verb_ini.rs", 1),
                // `[Colors]` entries copied into the processed registry.
                ("rules/native_processing.rs", 1),
            ],
        ),
        (
            "registry_ids",
            &[
                ("rules/ini_parser.rs", 1), // the definition
                ("rules/overlay_types.rs", 1),
                // Includes parse_registry copying already resolved stored IDs.
                ("rules/ruleset.rs", 3),
                ("rules/smudge_type.rs", 1),
                ("rules/tiberium_type.rs", 1),
            ],
        ),
    ];
    let expected: BTreeSet<(String, &str, usize)> = pinned
        .iter()
        .flat_map(|&(symbol, files)| {
            files
                .iter()
                .map(move |&(file, count)| (file.to_string(), symbol, count))
        })
        .collect();
    let mut found = BTreeSet::new();
    for file in source_files(&src_root) {
        let Some(production) = &file.production else {
            continue;
        };
        for &(symbol, _) in pinned {
            let count = count_identifier(production, symbol);
            if count > 0 {
                found.insert((file.rel.clone(), symbol, count));
            }
        }
    }
    assert_eq!(
        found, expected,
        "raw INI walks called outside their pinned production sites, or a pin \
         is stale. Read values through the `ini_value` readers; pin a new \
         caller only to copy or show entries as stored."
    );
}

/// Occurrences of `ident` as a whole identifier.
fn count_identifier(text: &str, ident: &str) -> usize {
    let bytes = text.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    text.match_indices(ident)
        .filter(|&(at, _)| {
            (at == 0 || !is_ident(bytes[at - 1]))
                && bytes.get(at + ident.len()).is_none_or(|&b| !is_ident(b))
        })
        .count()
}

fn scan_forbidden_edges(src_root: &Path) -> BTreeSet<(String, String)> {
    let mut found = BTreeSet::new();
    for (layer, forbidden) in LAYER_RULES {
        let dir = src_root.join(layer);
        assert!(
            dir.is_dir(),
            "layer directory src/{layer} missing — update LAYER_RULES"
        );
        let prefix = format!("{layer}/");
        for file in source_files(src_root) {
            let Some(production) = &file.production else {
                continue;
            };
            if !file.rel.starts_with(&prefix) {
                continue;
            }
            for root in *forbidden {
                if contains_crate_ref(production, root) || group_contains_root(production, root) {
                    found.insert((file.rel.clone(), root.to_string()));
                }
            }
        }
    }
    found
}

/// One Rust file under `src/`, as the guards read it.
struct SourceFile {
    path: PathBuf,
    /// Path from `src/`, with `/` separators.
    rel: String,
    source: String,
    /// [`blank_comments_and_literals`] of `source`.
    blanked: String,
    /// [`strip_test_items`] of `source`; `None` for `tests.rs` and
    /// `*_tests.rs` files, which the production scans skip.
    production: Option<String>,
}

/// Every Rust file under `src_root`, in [`visit_rust_files`] order, read and
/// blanked once per test process: five guards scan the same 40 MB tree, and
/// blanking it is most of an unoptimized scan.
fn source_files(src_root: &Path) -> &'static [SourceFile] {
    static FILES: OnceLock<(PathBuf, Vec<SourceFile>)> = OnceLock::new();
    let (root, files) = FILES.get_or_init(|| {
        let mut paths = Vec::new();
        visit_rust_files(src_root, &mut |path| paths.push(path.to_path_buf()));
        let read = |path: PathBuf| {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let is_test_file = name == "tests.rs" || name.ends_with("_tests.rs");
            let blanked = blank_comments_and_literals(&source);
            let production = (!is_test_file).then(|| strip_test_items(&source, &blanked));
            let rel = path
                .strip_prefix(src_root)
                .expect("scanned file under src")
                .to_string_lossy()
                .replace('\\', "/");
            SourceFile {
                path,
                rel,
                source,
                blanked,
                production,
            }
        };
        // Blank the files on several threads; the order stays the walk's.
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let chunk = paths.len().div_ceil(threads).max(1);
        let mut chunks: Vec<Vec<PathBuf>> = Vec::new();
        let mut paths = paths.into_iter().peekable();
        while paths.peek().is_some() {
            chunks.push(paths.by_ref().take(chunk).collect());
        }
        let files = std::thread::scope(|scope| {
            let workers: Vec<_> = chunks
                .into_iter()
                .map(|chunk| scope.spawn(|| chunk.into_iter().map(read).collect::<Vec<_>>()))
                .collect();
            workers
                .into_iter()
                .flat_map(|worker| worker.join().expect("source scan worker"))
                .collect()
        });
        (src_root.to_path_buf(), files)
    });
    assert_eq!(root, src_root, "every guard scans the crate's src/");
    files
}

fn visit_rust_files(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    let mut paths: Vec<_> = entries.map(|e| e.expect("dir entry").path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            visit_rust_files(&path, visit);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            visit(&path);
        }
    }
}

/// Replace comment and string/char-literal contents with spaces, preserving
/// newlines and all code structure, so later brace counting and `crate::`
/// searches cannot be confused by literals or prose (doc links included).
fn blank_comments_and_literals(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out.push(b' ');
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut depth = 1u32;
                out.extend_from_slice(b"  ");
                i += 2;
                while i < bytes.len() && depth > 0 {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        out.extend_from_slice(b"  ");
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        out.extend_from_slice(b"  ");
                        i += 2;
                    } else {
                        out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                        i += 1;
                    }
                }
            }
            b'r' | b'b' if is_raw_string_start(bytes, i) => {
                // r"...", r#"..."#, br"..." etc.: no escapes, terminated by
                // a quote followed by the opening number of hashes.
                let mut j = i + 1;
                if bytes.get(j) == Some(&b'r') {
                    j += 1;
                }
                let mut hashes = 0usize;
                while bytes.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                // j is at the opening quote.
                out.resize(out.len() + (j - i) + 1, b' ');
                i = j + 1;
                while i < bytes.len() {
                    if bytes[i] == b'"'
                        && bytes[i + 1..]
                            .iter()
                            .take(hashes)
                            .filter(|&&c| c == b'#')
                            .count()
                            == hashes
                    {
                        out.resize(out.len() + 1 + hashes, b' ');
                        i += 1 + hashes;
                        break;
                    }
                    out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                    i += 1;
                }
            }
            b'"' => {
                out.push(b' ');
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        // Keep a line continuation's newline so lines stay aligned.
                        out.push(b' ');
                        out.push(if bytes.get(i + 1) == Some(&b'\n') {
                            b'\n'
                        } else {
                            b' '
                        });
                        i += 2;
                    } else if bytes[i] == b'"' {
                        out.push(b' ');
                        i += 1;
                        break;
                    } else {
                        out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                        i += 1;
                    }
                }
            }
            b'\'' => {
                // Distinguish char literals from lifetimes: a char literal
                // closes with a quote after one (possibly escaped) character.
                if bytes.get(i + 1) == Some(&b'\\') {
                    out.push(b' ');
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        out.push(b' ');
                        i += 1;
                    }
                    out.push(b' ');
                    i += 1;
                } else if bytes.get(i + 2) == Some(&b'\'') {
                    out.extend_from_slice(b"   ");
                    i += 3;
                } else {
                    // Lifetime — keep as-is.
                    out.push(b);
                    i += 1;
                }
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).expect("blanking preserves UTF-8 structure")
}

fn is_raw_string_start(bytes: &[u8], i: usize) -> bool {
    // Accept `r` / `br` prefixes; plain `b"…"` falls through to the ordinary
    // string arm on its quote.
    let mut j = i;
    if bytes[j] == b'b' {
        j += 1;
    }
    if bytes.get(j) != Some(&b'r') {
        return false;
    }
    j += 1;
    while bytes.get(j) == Some(&b'#') {
        j += 1;
    }
    bytes.get(j) == Some(&b'"')
}

/// `source` without its `#[cfg(test)]` items (attribute plus the following
/// item, braced or single-statement), as blanked text: `text` is `source` with
/// comments and literals blanked. `cfg(all(test, …))` and
/// `cfg(any(test, feature = "test-support"))` count as test-only;
/// `cfg(any(test, debug_assertions))` does not. The feature gate is read from
/// the source line, since blanking erases its string.
fn strip_test_items(source: &str, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth: i64 = 0;
    let mut skip_above: Option<i64> = None;
    let mut awaiting_item = false;
    for (line, source_line) in text.lines().zip(source.lines()) {
        let opens = line.bytes().filter(|&b| b == b'{').count() as i64;
        let closes = line.bytes().filter(|&b| b == b'}').count() as i64;
        let is_test_attr = line.contains("cfg(test")
            || line.contains("cfg(all(test")
            || (line.trim_start().starts_with("#[")
                && source_line.contains(r#"cfg(any(test, feature = "test-support"))"#));
        let mut emit = true;

        if skip_above.is_some() {
            emit = false;
        } else if is_test_attr {
            emit = false;
            if opens > 0 {
                // Attribute and item share the line.
                skip_above = Some(depth);
            } else {
                awaiting_item = true;
            }
        } else if awaiting_item {
            emit = false;
            let trimmed = line.trim_start();
            if trimmed.starts_with("#[") && opens == 0 {
                // Further attribute on the same item; keep waiting.
            } else if opens > 0 {
                skip_above = Some(depth);
                awaiting_item = false;
            } else if trimmed.trim_end().ends_with(';') {
                // Single-statement item (e.g. `use`), fully consumed.
                awaiting_item = false;
            }
        }

        depth += opens - closes;
        if let Some(above) = skip_above
            && depth <= above
        {
            skip_above = None;
        }
        if emit {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Token-boundary search for `crate::<root>`. The `app` pseudo-root also
/// matches root `crate::app_*` modules.
fn contains_crate_ref(text: &str, root: &str) -> bool {
    let needle = format!("crate::{root}");
    let bytes = text.as_bytes();
    let mut start = 0;
    while let Some(pos) = text[start..].find(&needle) {
        let abs = start + pos;
        let end = abs + needle.len();
        let prev_ok = abs == 0 || {
            let p = bytes[abs - 1];
            !(p.is_ascii_alphanumeric() || p == b'_' || p == b':')
        };
        let next = bytes.get(end).copied();
        let boundary_ok = match root {
            // ':' = crate::app::X, '_' = root app_* modules, ';'/' '/','/')' =
            // bare module imports (`use crate::app;`, `use crate::app as x;`).
            "app" => matches!(next, Some(b':' | b'_' | b';' | b' ' | b',' | b')') | None),
            _ => !matches!(next, Some(c) if c.is_ascii_alphanumeric() || c == b'_'),
        };
        if prev_ok && boundary_ok {
            return true;
        }
        start = abs + 1;
    }
    false
}

/// Brace-grouped `use crate::{...}` paths (`use crate::{app_init::X, ui::Y}`)
/// hide the root from `contains_crate_ref`; walk each group's top-level
/// segments. Found in the wild at ui/main_menu.rs before F06 closed it.
fn group_contains_root(text: &str, root: &str) -> bool {
    let bytes = text.as_bytes();
    let mut start = 0;
    while let Some(pos) = text[start..].find("crate::{") {
        let open = start + pos + "crate::".len();
        let mut depth = 0usize;
        let mut i = open;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let close = i.min(bytes.len());
        let group = &text[(open + 1).min(close)..close];
        let mut nest = 0usize;
        let mut segment = String::new();
        for ch in group.chars() {
            match ch {
                '{' => {
                    nest += 1;
                    segment.push(ch);
                }
                '}' => {
                    nest = nest.saturating_sub(1);
                    segment.push(ch);
                }
                ',' if nest == 0 => {
                    if segment_is_root(segment.trim(), root) {
                        return true;
                    }
                    segment.clear();
                }
                _ => segment.push(ch),
            }
        }
        if segment_is_root(segment.trim(), root) {
            return true;
        }
        start = close.max(start + pos + 1);
    }
    false
}

fn segment_is_root(segment: &str, root: &str) -> bool {
    let Some(rest) = segment.strip_prefix(root) else {
        return false;
    };
    match root {
        "app" => {
            rest.is_empty()
                || rest.starts_with(':')
                || rest.starts_with('_')
                || rest.starts_with(' ')
        }
        _ => rest.is_empty() || rest.starts_with(':') || rest.starts_with(' '),
    }
}

#[test]
fn brace_grouped_imports_cannot_evade_the_scan() {
    let evasive = "use crate::{app_init::MapMenuEntry, ui::client_theme};\n";
    assert!(group_contains_root(evasive, "app"));
    assert!(!contains_crate_ref(evasive, "app"));

    let clean = "use crate::{map::terrain, rules::ruleset};\n";
    assert!(!group_contains_root(clean, "app"));
    assert!(!group_contains_root(clean, "sim"));

    let nested = "use crate::{map::{terrain, overlay}, sim::rng};\n";
    assert!(group_contains_root(nested, "sim"));
    assert!(!group_contains_root(nested, "app"));

    let renamed = "use crate::{sim as s};\n";
    assert!(group_contains_root(renamed, "sim"));
}

#[test]
fn bare_and_aliased_app_imports_cannot_evade_the_scan() {
    assert!(contains_crate_ref("use crate::app;\n", "app"));
    assert!(contains_crate_ref("use crate::app as shell;\n", "app"));
    assert!(contains_crate_ref("use crate::app::AppState;\n", "app"));
    assert!(!contains_crate_ref("use crate::apple::pie;\n", "app"));
    assert!(group_contains_root(
        "use crate::{app as a, ui::x};\n",
        "app"
    ));
    assert!(!group_contains_root("use crate::{apple, ui::x};\n", "app"));
}

/// AppState holds exactly eight named owners; unrelated flat state must not return.
#[test]
fn app_state_contains_only_named_owners() {
    let state_rs = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app/state.rs");
    let source = fs::read_to_string(&state_rs).expect("read src/app/state.rs");
    let body_start = source
        .find("pub(crate) struct AppState {")
        .expect("AppState struct in src/app/state.rs");
    let body = &source[body_start..];
    let body_end = body.find("\n}").expect("AppState struct end");
    let body = &body[..body_end];

    let mut fields: Vec<&str> = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        // Any visibility (or none) counts — a private or `pub` flat field must
        // not evade the owner inventory.
        let stripped = line
            .strip_prefix("pub(crate) ")
            .or_else(|| line.strip_prefix("pub(super) "))
            .or_else(|| line.strip_prefix("pub "))
            .unwrap_or(line);
        if let Some((name, rest)) = stripped.split_once(':') {
            if rest.starts_with(':') {
                continue; // `::` — a path inside a wrapped type, not a field
            }
            let name = name.trim();
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                fields.push(name);
            }
        }
    }
    let expected = [
        "platform",
        "renderer",
        "diag",
        "frontend",
        "match_state",
        "process_assets",
        "audio",
        "persistence",
    ];
    assert_eq!(
        fields, expected,
        "AppState must contain exactly the eight F12 owners. A new per-match \
         fact belongs in MatchState (or one of its owners); a new process fact \
         belongs in the platform/process/renderer/audio/frontend/persistence/\
         diagnostics owner it is scoped to."
    );
}

/// The app inventory lives under `src/app/`;
/// `src/lib.rs` declares no root `app_*` module.
#[test]
fn no_root_app_modules_remain() {
    let lib_rs = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let source = fs::read_to_string(&lib_rs).expect("read src/lib.rs");
    let offenders: Vec<&str> = source
        .lines()
        .map(str::trim_start)
        .filter(|line| {
            line.strip_prefix("pub mod app_")
                .or_else(|| line.strip_prefix("pub(crate) mod app_"))
                .or_else(|| line.strip_prefix("mod app_"))
                .is_some()
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "root app_* module(s) declared in src/lib.rs: {offenders:?}\n\
         The app layer inventory lives under src/app/ (F12); add new app \
         modules there."
    );
}

/// The entire `sim/` tree names no presentation,
/// audio, net, or app root anywhere — production AND test code. Unlike the
/// layer scan above (which strips test items), this ratchet holds the whole
/// tree at zero so a new sim-side test cannot quietly reintroduce the edge.
#[test]
fn sim_names_no_upper_layer_root_even_in_tests() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders: Vec<String> = Vec::new();
    for file in source_files(&src_root) {
        if !file.rel.starts_with("sim/") {
            continue;
        }
        for root in ["render", "net", "ui", "audio", "app"] {
            if contains_crate_ref(&file.blanked, root) || group_contains_root(&file.blanked, root) {
                offenders.push(format!("{} -> {root}", file.path.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "sim/ references upper-layer root(s): {offenders:?}\n\
         Sim tests that need render/net live in those layers' boundary test \
         modules (e.g. render::locomotor_visual tests, \
         net::lockstep_sim_convergence_tests), never inside sim/."
    );
}

/// Tests read fixture files at run time through `crate::test_fixture`;
/// `include_str!`/`include_bytes!` stay for data the program itself needs.
/// Embedded fixtures made up most of the test executable and recompiled the
/// crate on every fixture edit. A literal fixture path must name a file, so a
/// removed fixture fails here even when only ignored tests read it.
#[test]
fn test_fixtures_are_read_at_run_time_from_existing_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (mut embedded, mut missing) = (Vec::new(), Vec::new());
    for file in source_files(&root.join("src")) {
        let rel = file
            .path
            .strip_prefix(root)
            .expect("scanned file under the crate");
        let (file_embedded, file_missing) = fixture_violations(
            root,
            &rel.display().to_string(),
            &file.source,
            &file.blanked,
        );
        embedded.extend(file_embedded);
        missing.extend(file_missing);
    }
    assert!(
        embedded.is_empty(),
        "test fixtures embedded in the build: {embedded:?}\n\
         Read them with crate::test_fixture::text or ::bytes and a path from \
         the crate root."
    );
    assert!(
        missing.is_empty(),
        "test fixture paths that name no file: {missing:?}"
    );
}

#[test]
fn fixture_scan_flags_embeds_and_missing_paths_but_not_program_data() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = r#"
        const SHADER: &str = include_str!("batch_shader.wgsl");
        // include_str!("../../tools/commented_out.json")
        fn a() -> &'static str { include_str!("../../tools/spatial_oracle/a.json") }
        fn b() -> &'static [u8] {
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/b.bin"))
        }
        fn c() -> &'static str { crate::test_fixture::text("tools/README.md") }
        fn d() -> &'static str {
            crate::test_fixture::text(
                "tools/no_such_fixture.json",
            )
        }
    "#;
    let (embedded, missing) =
        fixture_violations(root, "x.rs", source, &blank_comments_and_literals(source));
    assert_eq!(embedded.len(), 2, "{embedded:?}");
    assert_eq!(missing, ["x.rs: tools/no_such_fixture.json"]);
}

/// Fixture embeds, and literal fixture paths that name no file, in one source
/// file shown as `rel`.
fn fixture_violations(
    root: &Path,
    rel: &str,
    source: &str,
    code: &str,
) -> (Vec<String>, Vec<String>) {
    let mut embedded = Vec::new();
    for needle in ["include_str!(", "include_bytes!("] {
        for (at, _) in code.match_indices(needle) {
            let arguments = macro_arguments(source, code, at + needle.len());
            let fixture = arguments.split('"').skip(1).step_by(2).any(|literal| {
                literal
                    .split('/')
                    .any(|part| matches!(part, "tools" | "tests" | "fixtures"))
            });
            if fixture {
                embedded.push(format!("{rel}: {}", arguments.trim()));
            }
        }
    }
    let mut missing = Vec::new();
    for needle in [
        "test_fixture::text(",
        "test_fixture::bytes(",
        "test_fixture::json(",
    ] {
        for (at, _) in code.match_indices(needle) {
            let argument = macro_arguments(source, code, at + needle.len());
            let argument = argument.trim().trim_end_matches(',').trim_end();
            let literal = argument.strip_prefix('"').and_then(|a| a.strip_suffix('"'));
            if let Some(file) = literal.filter(|file| !file.contains('"'))
                && !root.join(file).is_file()
            {
                missing.push(format!("{rel}: {file}"));
            }
        }
    }
    (embedded, missing)
}

/// Source text of the argument list that opens just before `start`, ending
/// at its matching parenthesis. `code` is `source` with comments and
/// literals blanked, so literal parentheses do not count.
fn macro_arguments<'a>(source: &'a str, code: &str, start: usize) -> &'a str {
    let mut depth = 1;
    for (at, byte) in code.bytes().enumerate().skip(start) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..at];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed argument list at byte {start}")
}

/// The simulation's composition roots: structs of owner slots.
const ROOTS: &[(&str, &str)] = &[
    ("sim/world/mod.rs", "Simulation"),
    ("sim/world/substrate.rs", "ObjectSubstrate"),
    ("sim/game_entity.rs", "GameEntity"),
];

/// Each non-private slot of the simulation's composition roots is typed as an
/// owner: a struct whose fields are all private, so other modules change its
/// state only through its operations. A raw container, primitive, enum or
/// open struct in a root slot would let any module write that state directly.
/// `Option`, `Box`, `Arc` and `Rc` wrappers are looked through.
#[test]
fn composition_root_slots_are_owner_types() {
    // Root slots that are not owner structs, each with its reason.
    const PINNED: &[(&str, &str, &str)] = &[
        (
            "Simulation",
            "substrate",
            "the object root, whose own slots are checked as ObjectSubstrate",
        ),
        (
            "GameEntity",
            "mission_leaf",
            "an enum of owner leaves, built once at entity construction; only fixtures replace it",
        ),
    ];
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = source_files(&src_root);
    // Parsed from the blanked source: `strip_test_items` works per line and
    // does not end a gated struct field, so `cfg(test)` items and fields are
    // dropped here instead.
    let mut structs: BTreeMap<String, Vec<Vec<FieldDecl>>> = BTreeMap::new();
    for file in files.iter().filter(|file| file.production.is_some()) {
        for (name, fields) in struct_definitions(&file.blanked) {
            structs.entry(name).or_default().push(fields);
        }
    }
    let mut violations = Vec::new();
    let mut pins_used = BTreeSet::new();
    for &(rel, root) in ROOTS {
        let file = files
            .iter()
            .find(|file| file.rel == rel)
            .unwrap_or_else(|| panic!("{rel} exists"));
        let (_, fields) = struct_definitions(&file.blanked)
            .into_iter()
            .find(|(name, _)| name == root)
            .unwrap_or_else(|| panic!("struct {root} in {rel}"));
        for field in fields.iter().filter(|field| field.non_private) {
            if PINNED
                .iter()
                .any(|&(pinned_root, slot, _)| pinned_root == root && slot == field.name)
            {
                pins_used.insert((root, field.name.clone()));
                continue;
            }
            if let Err(why) = owner_type_verdict(&structs, &field.ty) {
                violations.push(format!("{root}.{}: `{}` {why}", field.name, field.ty));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "composition-root slots that are not owner types: {violations:#?}\n\
         Give the state an owner struct with private fields and rule-named \
         operations, or pin the slot here with its reason."
    );
    let stale: Vec<_> = PINNED
        .iter()
        .filter(|&&(root, slot, _)| !pins_used.contains(&(root, slot.to_string())))
        .collect();
    assert!(stale.is_empty(), "stale PINNED entr(ies): {stale:?}");
}

#[test]
fn slot_type_scan_reads_wrappers_tuples_and_visibility() {
    let code = "pub struct A<T> where T: Copy { pub(crate) x: Option<Arc<B>>, #[serde(skip)] y: Vec<(u8, u16)>,\n\
                #[cfg(test)] pub z: u8 }\n\
                struct B(pub u8, C);\n\
                pub(crate) struct C;\n\
                #[cfg(any(test, feature = \"x\"))]\n\
                struct D<P, Q> { pub open: P, q: Q }\n\
                #[cfg(test)]\n\
                mod tests { struct E { pub open: u8 } }\n";
    let defs = struct_definitions(code);
    let names: Vec<_> = defs.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["A", "B", "C"]);
    let a = &defs[0].1;
    assert_eq!(a.len(), 2, "the cfg(test) field is not a production slot");
    assert_eq!((a[0].non_private, a[0].name.as_str()), (true, "x"));
    assert_eq!(
        (a[1].non_private, a[1].ty.as_str()),
        (false, "Vec<(u8, u16)>")
    );
    assert_eq!(slot_base_type(&a[0].ty), "B");
    assert!(defs[1].1[0].non_private && !defs[1].1[1].non_private);
    assert!(defs[2].1.is_empty());
    let structs: BTreeMap<String, Vec<Vec<FieldDecl>>> = defs
        .into_iter()
        .map(|(name, fields)| (name, vec![fields]))
        .collect();
    assert!(owner_type_verdict(&structs, "Option<C>").is_ok());
    assert!(owner_type_verdict(&structs, "crate::x::B").is_err());
    assert!(owner_type_verdict(&structs, "BTreeMap<u8, C>").is_err());
    assert!(owner_type_verdict(&structs, "SomeEnum").is_err());
}

struct FieldDecl {
    non_private: bool,
    name: String,
    ty: String,
}

/// Whether a slot of type `ty` is an owner struct, or why not.
fn owner_type_verdict(
    structs: &BTreeMap<String, Vec<Vec<FieldDecl>>>,
    ty: &str,
) -> Result<(), String> {
    const RAW: &[&str] = &[
        "Vec", "VecDeque", "BTreeMap", "BTreeSet", "HashMap", "HashSet", "String", "bool", "char",
        "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
        "f32", "f64",
    ];
    let base = slot_base_type(ty);
    if base.is_empty() || base.starts_with(['(', '[', '&']) || RAW.contains(&base) {
        return Err("is a raw value or container".to_string());
    }
    let Some(definitions) = structs.get(base) else {
        return Err("is not a struct (an enum, alias or foreign type)".to_string());
    };
    let open: Vec<&str> = definitions
        .iter()
        .flatten()
        .filter(|field| field.non_private)
        .map(|field| field.name.as_str())
        .collect();
    if open.is_empty() {
        Ok(())
    } else {
        Err(format!("has non-private fields {open:?}"))
    }
}

/// The type a slot names, through `Option`/`Box`/`Arc`/`Rc` and paths.
fn slot_base_type(ty: &str) -> &str {
    let mut ty = ty.trim();
    loop {
        let (path, generics) = ty.find('<').map_or((ty, ""), |at| ty.split_at(at));
        let last = path.rsplit("::").next().unwrap_or(path).trim();
        if ["Option", "Box", "Arc", "Rc"].contains(&last)
            && generics.starts_with('<')
            && generics.ends_with('>')
        {
            ty = generics[1..generics.len() - 1].trim();
        } else {
            return last;
        }
    }
}

/// Every production `struct` item in blanked `code`, with its production
/// fields in order. Tuple fields are named by position; unit structs have
/// none. Items and fields gated on `cfg(test)` or `cfg(any(test, ..))` are
/// skipped.
fn struct_definitions(code: &str) -> Vec<(String, Vec<FieldDecl>)> {
    let bytes = code.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let gated = cfg_test_items(code);
    let mut out = Vec::new();
    for (at, _) in code.match_indices("struct") {
        let end = at + "struct".len();
        if gated.iter().any(|&(start, stop)| start <= at && at < stop)
            || (at > 0 && is_ident(bytes[at - 1]))
            || !bytes.get(end).is_some_and(u8::is_ascii_whitespace)
        {
            continue;
        }
        let name_start = end + code[end..].len() - code[end..].trim_start().len();
        let name_len = code[name_start..]
            .bytes()
            .take_while(|&b| is_ident(b))
            .count();
        if name_len == 0 {
            continue;
        }
        let name = &code[name_start..name_start + name_len];
        // The body opens at the first `{`, `(` or `;` outside the generics.
        let mut depth = 0i32;
        let mut open = None;
        let mut prev = 0u8;
        for (offset, b) in code[name_start + name_len..].bytes().enumerate() {
            match b {
                b'<' => depth += 1,
                b'>' if prev != b'-' => depth -= 1,
                b'{' | b'(' | b';' if depth == 0 => {
                    open = Some((name_start + name_len + offset, b));
                    break;
                }
                _ => {}
            }
            prev = b;
        }
        let Some((open_at, open_byte)) = open else {
            continue;
        };
        if open_byte == b';' {
            out.push((name.to_string(), Vec::new()));
            continue;
        }
        let close_byte = if open_byte == b'{' { b'}' } else { b')' };
        let mut nesting = 0i32;
        let mut close_at = None;
        for (offset, b) in code[open_at..].bytes().enumerate() {
            if b == open_byte {
                nesting += 1;
            } else if b == close_byte {
                nesting -= 1;
                if nesting == 0 {
                    close_at = Some(open_at + offset);
                    break;
                }
            }
        }
        let Some(close_at) = close_at else {
            continue;
        };
        let named = open_byte == b'{';
        let fields = split_top_level(&code[open_at + 1..close_at])
            .into_iter()
            .enumerate()
            .filter_map(|(index, part)| parse_field_decl(part, named, index))
            .collect();
        out.push((name.to_string(), fields));
    }
    out
}

/// `text` split at commas outside brackets, braces, parentheses and generics.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut prev = 0u8;
    for (at, b) in text.bytes().enumerate() {
        match b {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b'>' if prev == b'-' => {}
            b')' | b']' | b'}' | b'>' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
        prev = b;
    }
    parts.push(&text[start..]);
    parts
}

fn parse_field_decl(part: &str, named: bool, index: usize) -> Option<FieldDecl> {
    let mut rest = part.trim();
    // Attributes such as `#[serde(skip)]` precede the field.
    while let Some(after) = rest.strip_prefix("#[") {
        let mut depth = 1i32;
        let close = after.bytes().position(|b| {
            match b {
                b'[' => depth += 1,
                b']' => depth -= 1,
                _ => {}
            }
            depth == 0
        })?;
        if is_cfg_test(&after[..close]) {
            return None;
        }
        rest = after[close + 1..].trim_start();
    }
    if rest.is_empty() {
        return None;
    }
    let non_private = rest.starts_with("pub") && rest[3..].starts_with([' ', '\t', '\n', '(']);
    if non_private {
        rest = rest[3..].trim_start();
        if rest.starts_with('(') {
            rest = rest[rest.find(')')? + 1..].trim_start();
        }
    }
    let (name, ty) = if named {
        let (name, ty) = rest.split_once(':')?;
        (name.trim().to_string(), ty)
    } else {
        (index.to_string(), rest)
    };
    Some(FieldDecl {
        non_private,
        name,
        ty: ty.split_whitespace().collect::<Vec<_>>().join(" "),
    })
}

/// Whether an attribute's contents gate on tests.
fn is_cfg_test(attribute: &str) -> bool {
    ["cfg(test", "cfg(any(test", "cfg(all(test"]
        .iter()
        .any(|gate| attribute.trim_start().starts_with(gate))
}

/// Byte ranges of the items in blanked `code` gated on tests: from the
/// attribute to the item's `;` or `,`, or to the close of its block.
fn cfg_test_items(code: &str) -> Vec<(usize, usize)> {
    let mut items = Vec::new();
    for (at, _) in code.match_indices("#[") {
        let Some(close) = code[at..].find(']') else {
            continue;
        };
        if !is_cfg_test(&code[at + 2..at + close]) {
            continue;
        }
        let start = at + close + 1;
        let mut depth = 0i32;
        let mut prev = 0u8;
        for (offset, b) in code[start..].bytes().enumerate() {
            match b {
                b'(' | b'[' | b'<' => depth += 1,
                b'>' if prev == b'-' => {}
                b')' | b']' | b'>' => depth -= 1,
                b';' | b',' if depth == 0 => {
                    items.push((at, start + offset));
                    break;
                }
                b'{' if depth == 0 => {
                    let mut nesting = 0i32;
                    let block_end = code[start + offset..].bytes().position(|b| {
                        match b {
                            b'{' => nesting += 1,
                            b'}' => nesting -= 1,
                            _ => {}
                        }
                        nesting == 0
                    });
                    items.push((at, block_end.map_or(code.len(), |end| start + offset + end)));
                    break;
                }
                b'}' if depth == 0 => break,
                _ => {}
            }
            prev = b;
        }
    }
    items
}

/// Holding a root reaches only owner operations. No function another module
/// can call hands out mutable access to anything but an owner struct (one
/// whose fields are all private), and no method is a plain one-field setter,
/// except the pinned ones with their reasons. With
/// `composition_root_slots_are_owner_types`, this is what stops a
/// `&mut Simulation` signature from writing state past its owner: every slot
/// is an owner, and no owner opens its state through a handout or a setter.
/// Scans the engine's production code. Trait-impl and trait methods count as
/// callable from anywhere; `&mut Self` from `&mut self` reaches nothing new,
/// and a root's slots are checked by the guard above. A setter is a `set` or
/// `set_*` method; a single store under a rule's name is an operation.
#[test]
fn mutable_handouts_and_setters_are_owner_operations() {
    const ENGINE: &[&str] = &["sim/", "map/", "net/", "rules/", "util/"];
    // (file, function, reason): mutable access to something other than an
    // owner struct.
    const RMG: &str =
        "random-map generator scratch, consumed into map data before a scenario exists";
    const PAYLOAD: &str =
        "a command's payload bytes under construction, which the simulation decodes";
    const CELL_MAP: &str =
        "a generic cell-keyed container; every CellMap is a private field of its owner";
    const LASER: &str = "LaserDraw 0x0054FE60 returns the new laser for its caller's postprocessing \
                         (FireAt 0x006FF50A..0x006FF571); the caller finishes its own frame fact";
    const PINNED_HANDOUTS: &[(&str, &str, &str)] = &[
        ("map/rmg/grid.rs", "get_mut", RMG),
        ("map/rmg/grid.rs", "ref_cell_mut", RMG),
        ("map/rmg/grid.rs", "cell_native_mut", RMG),
        ("map/rmg/scratch.rs", "get_mut", RMG),
        ("map/rmg/scratch.rs", "cells_mut", RMG),
        (
            "map/tubes.rs",
            "install_receipt",
            "the map read builds the tube receipt in place once; binding takes it once",
        ),
        ("net/lockstep.rs", "payload_mut", PAYLOAD),
        (
            "net/udp.rs",
            "deref_mut",
            "UdpWriteGuard construction is private to LanUdpTransport::write; its only callers \
             project the existing LanConnections and GlobalChannel owners, whose fields are private",
        ),
        (
            "rules/art_data.rs",
            "get_mut",
            "art rules installed at load: the registry fills SHP frame sizes into its own \
             entries, and fixtures reach it through art_entry_mut_for_test",
        ),
        ("sim/command.rs", "payload_mut", PAYLOAD),
        ("sim/cell_map.rs", "get_mut", CELL_MAP),
        ("sim/cell_map.rs", "get_or_insert_with", CELL_MAP),
        ("sim/cell_map.rs", "values_mut", CELL_MAP),
        ("sim/combat/laser.rs", "create", LASER),
        ("sim/combat/laser.rs", "construct", LASER),
        (
            "sim/pathfinding/zone_hierarchy.rs",
            "cell_zone_ids_mut",
            "the zone build writes a level's cell zone ids (zone_build/hierarchy.rs)",
        ),
        (
            "sim/pathfinding/zone_hierarchy.rs",
            "levels_mut",
            "the zone build and bridge zone publication edit the levels; the handout drops \
             the precheck memo first",
        ),
        (
            "sim/pathfinding/zone_map.rs",
            "base_topology_mut",
            "zone repair rewrites base topology records in place, after the handout \
             invalidates navigation history",
        ),
        (
            "sim/world/frame_channel.rs",
            "emit_and_finish",
            "a producer finishes the fact it just emitted, before the frame tail reads the channel",
        ),
    ];
    // (file, method, reason): `set`/`set_*` methods whose body stores one
    // parameter. Each stores one native member; the decision to store it
    // lives in the cited native caller, which the Rust caller ports.
    const PINNED_SETTERS: &[(&str, &str, &str)] = &[
        (
            "net/global.rs",
            "set_timing",
            "Set_Timing (0x00540C60) forwards its timing to the global channel's +0x28..+0x30 \
             through vt+0x38 (0x00540D10) only when its bool argument is set",
        ),
        (
            "net/reliable.rs",
            "set_timing",
            "Set_Timing (0x00540C60) stores the manager's +0x58..+0x60 and copies them into \
             every private connection's +0x28..+0x30 (0x00540CBE..0x00540CE1)",
        ),
        (
            "rules/object_type.rs",
            "set_voxel_shadow_index",
            "runs only inside RuleSet's load",
        ),
        (
            "rules/object_type.rs",
            "set_secret_lab",
            "runs only inside RuleSet's load",
        ),
        (
            "rules/superweapon_type.rs",
            "set_tech_level",
            "runs only inside RuleSet's load: Phobos SW.TechLevel",
        ),
        (
            "rules/superweapon_type.rs",
            "set_aux_technos",
            "runs only inside RuleSet's load: Phobos SW.AuxTechnos",
        ),
        (
            "rules/superweapon_type.rs",
            "set_neg_technos",
            "runs only inside RuleSet's load: Phobos SW.NegTechnos",
        ),
        (
            "sim/ai_base_building.rs",
            "set_mode",
            "AI_Manage_Build_Queue's mode: 2 for a harvester (0x004FE109), 1 for a building (0x004FE3AB)",
        ),
        (
            "sim/ai_team_creation.rs",
            "set_trigger_ratio",
            "TAction 76 stores the signed +0x90 dword unchanged (0x006DF364)",
        ),
        (
            "sim/ammo.rs",
            "set_dock",
            "the +0x6CC writers and PointerExpired's clear",
        ),
        (
            "sim/combat/attack_target.rs",
            "set",
            "the TarCom class setter; callers port each native writer, such as \
             PointerExpired's NULL store (0x00707A33)",
        ),
        (
            "sim/combat/base_defense_response.rs",
            "set_team_recruitable",
            "TeamClass::Add_Member stores AreTeamMembersRecruitable into +0x422 (0x006EA500)",
        ),
        (
            "sim/combat/base_defense_response.rs",
            "set_archive_target",
            "TechnoClass::Set_ArchiveTarget (0x0070C610)",
        ),
        (
            "sim/combat/object_health.rs",
            "set",
            "the Strength store of the native death paths (Crush 0x007416A0, \
             KillPassengers 0x00707CB0, KillOccupants 0x004585C0)",
        ),
        (
            "sim/house_state.rs",
            "set_alternate_base",
            "trigger action 137 (0x0050DFE0) writes House+0x5494",
        ),
        (
            "sim/house_state.rs",
            "set_ui_name",
            "House+0x1602A: CreateHouses (0x00688092, 0x00688252) and UIName= (0x00500C5D)",
        ),
        (
            "sim/house_state.rs",
            "set_aircraft_team_ratio",
            "TAction 77 stores the signed Action+90 (0x006DF38E)",
        ),
        (
            "sim/house_state.rs",
            "set_infantry_team_ratio",
            "TAction 78 stores the signed Action+90 (0x006DF3B8)",
        ),
        (
            "sim/house_state.rs",
            "set_unit_team_ratio",
            "TAction 79 stores the signed Action+90 (0x006DF3E2)",
        ),
        (
            "sim/house_state.rs",
            "set_nuke_target",
            "SuperClass::Launch's MultiMissile arm (0x006CDDC4)",
        ),
        (
            "sim/house_state.rs",
            "set_preferred_super_weapon_target_kind",
            "TAction 35 stores the signed dword without clamping (0x006DE1F1)",
        ),
        (
            "sim/house_state.rs",
            "set_preferred_super_weapon_target",
            "House 0x0050DA00, reached by TAction 135 (0x006E43E0)",
        ),
        (
            "sim/house_state.rs",
            "set_ai_triggers_active",
            "TActions 74/75 store only House+1F2 (0x006DF2FA, 0x006DF339)",
        ),
        (
            "sim/mission/state.rs",
            "set_handler_state",
            "the handler-state dword native mission handlers write while executing",
        ),
        (
            "sim/movement/fly_height.rs",
            "set_flight_level",
            "Horizontal_Step's height target (0x004CF3F6, 0x004CF4B6, 0x004CF4CF)",
        ),
        (
            "sim/movement/navcom.rs",
            "set_nav_com_aux",
            "FootClass::Set_NavComAux (0x004D94A0) stores Foot+0x5A0; EventClass::Execute \
             calls it after Set_Destination (0x004C74BA)",
        ),
        (
            "sim/movement/teleport_movement.rs",
            "set_pending_state",
            "Techno+280, published by the reinforcement (0x0065F29F)",
        ),
        (
            "sim/movement/teleport_movement.rs",
            "set_state",
            "teleport +0x38, stored by the Process's warp states and TimerCheck \
             (0x007197CC..0x00719BF0)",
        ),
        (
            "sim/movement/teleport_movement.rs",
            "set_destination",
            "Techno+0x288, moved by the Process's blocked-landing search",
        ),
        (
            "sim/movement/teleport_movement.rs",
            "set_warping_in",
            "Techno+0x271, cleared by TimerCheck's expiry and state 7",
        ),
        (
            "sim/pathfinding/zone_map.rs",
            "set_bridge_redirect",
            "the zone grid's derived bridge redirect table, rebuilt at zone build and bridge repair",
        ),
        (
            "sim/rocking/mod.rs",
            "set_vel_sideways_native",
            "Techno+330, written by CaptureManager (0x00471C78)",
        ),
        (
            "sim/rocking/mod.rs",
            "set_vel_forwards_native",
            "Techno+334, replaced by Drive (0x004B19F7)",
        ),
        (
            "sim/scenario_session.rs",
            "set_fixed_alliance",
            "Scenario flag 0x400: Apply_Squad_Alliances clears it in GameMode 4 (0x0069B19B) \
             and sets it after the squad alliances (0x0069B4E2)",
        ),
        (
            "sim/scenario_session.rs",
            "set_campaign_map_stage",
            "Scenario+35A0: MapSelect writes the chosen stage (0x005AE1FA/0x005AE0B8); \
             carry-over restores that word separately from the incoming restart record",
        ),
        (
            "sim/snapshot/scenario_resources.rs",
            "set_elapsed_buckets",
            "ScenarioSave stops +0x614 into the +0x61C accumulator (0x00689360..0x0068936F) \
             before its stream write (0x00689378); the save boundary stores that value",
        ),
        (
            "sim/snapshot/saved_local_view.rs",
            "set_credits_counter",
            "MouseClass::Save (0x005BE6D0, store 0x005BE715) captures CreditsClass+4; \
             this snapshot field records the presentation owner's displayed counter",
        ),
        (
            "sim/stage.rs",
            "set_value",
            "the unload resets the dump counter without restarting its clock (0x0073E3xx)",
        ),
        (
            "sim/stage.rs",
            "set_rate",
            "Repair changes only the rate and keeps the timer (0x0044BCD6)",
        ),
        (
            "sim/superweapon/mod.rs",
            "set_recharge_time",
            "SuperClass::SetRechargeTime (0x006CC280, TAction 132) changes only Super+24",
        ),
        (
            "sim/world/mod.rs",
            "set_input_delay_ticks",
            "the match's configured input delay, installed by the app at match start",
        ),
    ];
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = source_files(&src_root);
    let gated = test_gated_files(files);
    let production = || {
        files
            .iter()
            .filter(|file| file.production.is_some() && !gated.contains(&file.rel))
    };
    let mut structs: BTreeMap<String, Vec<Vec<FieldDecl>>> = BTreeMap::new();
    for file in production() {
        for (name, fields) in struct_definitions(&file.blanked) {
            structs.entry(name).or_default().push(fields);
        }
    }
    let mut handouts = Vec::new();
    let mut setters = Vec::new();
    let mut pins_used = BTreeSet::new();
    let pinned = |pins: &[(&str, &str, &str)], rel: &str, name: &str| {
        pins.iter()
            .any(|&(file, function, _)| file == rel && function == name)
    };
    for file in production().filter(|file| ENGINE.iter().any(|dir| file.rel.starts_with(dir))) {
        for item in fn_items(&file.blanked) {
            if !item.callable_outside {
                continue;
            }
            let open = mutable_targets(&item.ret).into_iter().find(|target| {
                let base = slot_base_type(target);
                !(base == "Self" && item.receiver_mut
                    || target.starts_with("dyn ")
                    || target.starts_with("impl ")
                    || ROOTS.iter().any(|&(_, root)| root == base)
                    || owner_type_verdict(&structs, target).is_ok())
            });
            if let Some(target) = open {
                if pinned(PINNED_HANDOUTS, &file.rel, &item.name) {
                    pins_used.insert(("handout", file.rel.clone(), item.name.clone()));
                } else {
                    handouts.push(format!(
                        "{}: {} -> {} (`{target}`)",
                        file.rel, item.name, item.ret
                    ));
                }
            }
            if item.plain_setter() {
                if pinned(PINNED_SETTERS, &file.rel, &item.name) {
                    pins_used.insert(("setter", file.rel.clone(), item.name.clone()));
                } else {
                    setters.push(format!("{}: {}", file.rel, item.name));
                }
            }
        }
    }
    assert!(
        handouts.is_empty() && setters.is_empty(),
        "mutable handouts past an owner: {handouts:#?}\nplain setters: {setters:#?}\n\
         Hand out the owner struct or a rule-named operation instead, or pin the \
         function here with its reason."
    );
    let stale: Vec<_> = PINNED_HANDOUTS
        .iter()
        .map(|&(file, function, _)| ("handout", file, function))
        .chain(
            PINNED_SETTERS
                .iter()
                .map(|&(file, function, _)| ("setter", file, function)),
        )
        .filter(|&(kind, file, function)| {
            !pins_used.contains(&(kind, file.to_string(), function.to_string()))
        })
        .collect();
    assert!(stale.is_empty(), "stale pinned entr(ies): {stale:?}");
}

#[test]
fn handout_scan_reads_visibility_returns_and_setters() {
    let code = "pub struct Open { pub x: u8 }\n\
                pub(crate) struct Owned { x: u8 }\n\
                impl Owned {\n\
                    pub(crate) fn owned(&mut self) -> &mut Owned { self }\n\
                    pub fn open(&mut self) -> Option<&'_ mut Open> { None }\n\
                    fn private(&mut self) -> &mut Open { todo!() }\n\
                    pub const fn set_x(&mut self, value: u8) { self.x = value; }\n\
                    pub fn store(&mut self, value: u8) -> u8 { self.x = value; value }\n\
                    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Open> { todo!() }\n\
                    pub fn fluent(&mut self) -> &mut Self { self }\n\
                }\n\
                impl std::ops::IndexMut<usize> for Owned {\n\
                    fn index_mut(&mut self, i: usize) -> &mut u8 { &mut self.x }\n\
                }\n\
                #[cfg(test)]\n\
                impl Owned { pub fn test_only(&mut self) -> &mut u8 { &mut self.x } }\n";
    let items = fn_items(code);
    let names: Vec<_> = items.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "owned",
            "open",
            "private",
            "set_x",
            "store",
            "iter_mut",
            "fluent",
            "index_mut"
        ]
    );
    let callable: Vec<_> = items.iter().map(|item| item.callable_outside).collect();
    assert_eq!(callable, [true, true, false, true, true, true, true, true]);
    assert_eq!(mutable_targets(&items[0].ret), ["Owned"]);
    assert_eq!(mutable_targets(&items[1].ret), ["Open"]);
    assert_eq!(mutable_targets(&items[5].ret), ["Open"]);
    assert_eq!(mutable_targets(&items[6].ret), ["Self"]);
    assert_eq!(mutable_targets(&items[7].ret), ["u8"]);
    assert_eq!(
        mutable_targets("impl Iterator<Item = (&K, &mut V)> + '_"),
        ["V"]
    );
    let setters: Vec<_> = items.iter().map(FnItem::plain_setter).collect();
    assert_eq!(
        setters,
        [false, false, false, true, false, false, false, false]
    );
    let gated_mod = "#[cfg(any(test, feature = \"x\"))]\nmod fixture;\nmod live;\n";
    assert_eq!(gated_module_names(gated_mod), ["fixture"]);
}

/// One `fn` item as the handout guard reads it.
struct FnItem {
    name: String,
    /// Non-private, or a method of a trait or trait impl.
    callable_outside: bool,
    /// The first parameter is `&mut self`.
    receiver_mut: bool,
    params: String,
    /// The return type, empty for `()`.
    ret: String,
    body: Option<String>,
}

impl FnItem {
    /// `&mut self` and one more parameter, and a body that only stores that
    /// parameter in a field path of `self`.
    fn plain_setter(&self) -> bool {
        if self.name != "set" && !self.name.starts_with("set_") {
            return false;
        }
        let params = split_top_level(&self.params);
        let params: Vec<&str> = params
            .iter()
            .map(|part| part.trim())
            .filter(|part| !part.is_empty())
            .collect();
        let (true, [_, value], Some(body)) = (self.receiver_mut, params.as_slice(), &self.body)
        else {
            return false;
        };
        let Some((value, _)) = value.split_once(':') else {
            return false;
        };
        let value = value.trim().trim_start_matches("mut ").trim();
        let statement = body.trim().trim_end_matches(';').trim();
        let Some((lhs, rhs)) = statement.split_once('=') else {
            return false;
        };
        let path = lhs.trim();
        !rhs.starts_with('=')
            && !path.ends_with(['+', '-', '*', '/', '|', '&', '^', '!', '<', '>'])
            && rhs.trim() == value
            && path.strip_prefix("self.").is_some_and(|fields| {
                !fields.is_empty()
                    && fields
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
            })
    }
}

/// Every `fn` item in blanked `code` outside test-gated items.
fn fn_items(code: &str) -> Vec<FnItem> {
    let bytes = code.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let gated = cfg_test_items(code);
    let trait_blocks = trait_blocks(code);
    let mut out = Vec::new();
    for (at, _) in code.match_indices("fn") {
        let end = at + 2;
        if (at > 0 && is_ident(bytes[at - 1]))
            || !bytes.get(end).is_some_and(u8::is_ascii_whitespace)
            || gated.iter().any(|&(start, stop)| start <= at && at < stop)
        {
            continue;
        }
        let name_start = end + code[end..].len() - code[end..].trim_start().len();
        let name_len = code[name_start..]
            .bytes()
            .take_while(|&b| is_ident(b))
            .count();
        if name_len == 0 {
            continue;
        }
        let name = &code[name_start..name_start + name_len];
        let mut k = name_start + name_len;
        k += code[k..].len() - code[k..].trim_start().len();
        if code[k..].starts_with('<') {
            let Some(close) = closing(code, k, b'<', b'>') else {
                continue;
            };
            k = close + 1;
            k += code[k..].len() - code[k..].trim_start().len();
        }
        if !code[k..].starts_with('(') {
            continue;
        }
        let Some(params_end) = closing(code, k, b'(', b')') else {
            continue;
        };
        let params = &code[k + 1..params_end];
        // The signature ends at the first `{` or `;` outside brackets.
        let mut depth = 0i32;
        let mut prev = 0u8;
        let mut tail_end = None;
        for (offset, b) in code[params_end + 1..].bytes().enumerate() {
            match b {
                b'(' | b'[' | b'<' => depth += 1,
                b'>' if prev == b'-' => {}
                b')' | b']' | b'>' => depth -= 1,
                b'{' | b';' if depth == 0 => {
                    tail_end = Some(params_end + 1 + offset);
                    break;
                }
                _ => {}
            }
            prev = b;
        }
        let Some(tail_end) = tail_end else {
            continue;
        };
        let tail = code[params_end + 1..tail_end].trim();
        let ret = tail.strip_prefix("->").map_or("", |ret| {
            let ret = ret.trim();
            ret.match_indices("where")
                .find(|&(at, _)| {
                    (at == 0 || !is_ident(ret.as_bytes()[at - 1]))
                        && !ret.as_bytes().get(at + 5).is_some_and(|&b| is_ident(b))
                })
                .map_or(ret, |(at, _)| ret[..at].trim())
        });
        let body = (bytes[tail_end] == b'{')
            .then(|| closing(code, tail_end, b'{', b'}'))
            .flatten()
            .map(|close| code[tail_end + 1..close].to_string());
        let first = params.split(',').next().unwrap_or("").trim();
        let receiver_mut = first.starts_with('&')
            && first.ends_with("self")
            && first
                .split_whitespace()
                .any(|word| word == "mut" || word.ends_with("mut"));
        let in_trait = trait_blocks
            .iter()
            .any(|&(start, stop)| start < at && at < stop);
        out.push(FnItem {
            name: name.to_string(),
            callable_outside: in_trait || visibility_precedes(code, at),
            receiver_mut,
            params: params.to_string(),
            ret: ret.split_whitespace().collect::<Vec<_>>().join(" "),
            body,
        });
    }
    out
}

/// Whether a `pub` or `pub(..)` visibility opens the item whose `fn` keyword
/// is at `at`, past `const`, `async`, `unsafe` and `extern` qualifiers.
fn visibility_precedes(code: &str, at: usize) -> bool {
    let mut head = code[..at].trim_end();
    loop {
        let stripped = ["const", "async", "unsafe", "extern"]
            .iter()
            .find_map(|keyword| {
                head.strip_suffix(keyword)
                    .filter(|rest| rest.is_empty() || rest.ends_with(|c: char| c.is_whitespace()))
            });
        match stripped {
            Some(rest) => head = rest.trim_end(),
            None => break,
        }
    }
    let ident_end = |rest: &str| !rest.ends_with(|c: char| c.is_alphanumeric() || c == '_');
    if let Some(rest) = head.strip_suffix("pub") {
        return ident_end(rest);
    }
    if head.ends_with(')') {
        if let Some(open) = head.rfind('(') {
            if let Some(rest) = head[..open].trim_end().strip_suffix("pub") {
                return ident_end(rest);
            }
        }
    }
    false
}

/// Byte ranges of `trait` definitions and `impl .. for ..` blocks in blanked
/// `code`, whose methods any holder of the type can call.
fn trait_blocks(code: &str) -> Vec<(usize, usize)> {
    let bytes = code.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut blocks = Vec::new();
    for keyword in ["impl", "trait"] {
        for (at, _) in code.match_indices(keyword) {
            let end = at + keyword.len();
            if (at > 0 && is_ident(bytes[at - 1]))
                || !bytes
                    .get(end)
                    .is_some_and(|&b| b.is_ascii_whitespace() || b == b'<')
            {
                continue;
            }
            // The header runs to the first `{` or `;` outside generics. An
            // `impl Trait` type in a signature closes an enclosing bracket
            // first and is not a block.
            let mut depth = 0i32;
            let mut prev = 0u8;
            let mut open = None;
            for (offset, b) in code[end..].bytes().enumerate() {
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
                        open = (b == b'{').then_some(end + offset);
                        break;
                    }
                    _ => {}
                }
                prev = b;
            }
            let Some(open) = open else {
                continue;
            };
            let header = &code[end..open];
            let trait_impl =
                keyword == "trait" || header.split_whitespace().any(|word| word == "for");
            if trait_impl {
                if let Some(close) = closing(code, open, b'{', b'}') {
                    blocks.push((open, close));
                }
            }
        }
    }
    blocks
}

/// The type of each mutable borrow a return type hands out: `&mut T` and
/// `&'a mut T`, and the items of `IterMut`-style adapters and `RefMut`.
fn mutable_targets(ret: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, _) in ret.match_indices('&') {
        let mut rest = ret[at + 1..].trim_start();
        if let Some(lifetime) = rest.strip_prefix('\'') {
            let len = lifetime
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count();
            rest = lifetime[len..].trim_start();
        }
        let Some(after) = rest.strip_prefix("mut") else {
            continue;
        };
        if after.starts_with(|c: char| c.is_whitespace()) {
            out.push(leading_type(after.trim_start()).to_string());
        }
    }
    for adapter in [
        "IterMut<",
        "ValuesMut<",
        "RangeMut<",
        "ChunksMut<",
        "ChunksExactMut<",
        "RefMut<",
    ] {
        for (at, _) in ret.match_indices(adapter) {
            let open = at + adapter.len() - 1;
            let Some(close) = closing(ret, open, b'<', b'>') else {
                continue;
            };
            if let Some(item) = split_top_level(&ret[open + 1..close])
                .into_iter()
                .map(str::trim)
                .filter(|arg| !arg.starts_with('\''))
                .last()
            {
                out.push(item.to_string());
            }
        }
    }
    out
}

/// The type expression at the start of `text`: up to a `,`, `>`, `)`, `]`
/// or ` +` outside brackets.
fn leading_type(text: &str) -> &str {
    let mut depth = 0i32;
    let mut prev = 0u8;
    for (at, b) in text.bytes().enumerate() {
        match b {
            b'(' | b'[' | b'<' => depth += 1,
            b'>' if prev == b'-' => {}
            b')' | b']' | b'>' if depth == 0 => return text[..at].trim(),
            b')' | b']' | b'>' => depth -= 1,
            b',' | b'+' if depth == 0 => return text[..at].trim(),
            _ => {}
        }
        prev = b;
    }
    text.trim()
}

/// The index of the byte closing the bracket that opens at `open`.
fn closing(text: &str, open: usize, open_byte: u8, close_byte: u8) -> Option<usize> {
    let mut depth = 0i32;
    let mut prev = 0u8;
    for (offset, b) in text[open..].bytes().enumerate() {
        if b == open_byte {
            depth += 1;
        } else if b == close_byte && !(close_byte == b'>' && prev == b'-') {
            depth -= 1;
            if depth == 0 {
                return Some(open + offset);
            }
        }
        prev = b;
    }
    None
}

/// Modules declared by a `mod name;` item gated on tests in blanked `code`.
fn gated_module_names(code: &str) -> Vec<String> {
    cfg_test_items(code)
        .into_iter()
        .filter_map(|(start, stop)| {
            let item = code[start..=stop.min(code.len() - 1)].trim_end();
            let item = item.strip_suffix(';')?;
            let after = item.split("mod").nth(1)?;
            let name = after.trim();
            (!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
                .then(|| name.to_string())
        })
        .collect()
}

/// Files of modules declared under a test gate, with their submodules: a
/// `#[cfg(test)] mod x;` makes `x.rs` and everything under `x/` test code.
fn test_gated_files(files: &[SourceFile]) -> BTreeSet<String> {
    let mut gated_dirs = Vec::new();
    let mut gated = BTreeSet::new();
    for file in files {
        let names = gated_module_names(&file.blanked);
        if names.is_empty() {
            continue;
        }
        let stem = file.rel.strip_suffix(".rs").unwrap_or(&file.rel);
        let dir = if ["mod", "lib", "main"]
            .iter()
            .any(|root| stem == *root || stem.ends_with(&format!("/{root}")))
        {
            stem.rsplit_once('/').map_or("", |(dir, _)| dir).to_string()
        } else {
            stem.to_string()
        };
        for name in names {
            let base = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            gated.insert(format!("{base}.rs"));
            gated.insert(format!("{base}/mod.rs"));
            gated_dirs.push(format!("{base}/"));
        }
    }
    for file in files {
        if gated_dirs
            .iter()
            .any(|dir| file.rel.starts_with(dir.as_str()))
        {
            gated.insert(file.rel.clone());
        }
    }
    gated
}
