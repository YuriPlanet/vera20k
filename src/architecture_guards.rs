//! Source-level guards for current engine ownership and dependency boundaries.
//!
//! LAYER_RULES forbids reverse production dependencies and the exception list is
//! empty. The scanner excludes tests.rs, *_tests.rs and #[cfg(test)] items;
//! cfg(any(test, debug_assertions)) remains production because debug builds use it.
//! A separate guard checks the entire sim tree, including tests, for upper-layer
//! references. Other guards pin frame API callers and AppState owner boundaries.
//!
//! The app pseudo-root matches crate::app paths and retired root app_* modules.
//! The ui rule also forbids the retired skirmish_scenarios root. These checks
//! preserve the current layout rather than describing future migration work.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Directory under `src/` -> reference roots its production code must not
/// name. The simulation direction contract is in AGENTS.md; the remaining
/// rules preserve the established lower-layer boundaries.
const LAYER_RULES: &[(&str, &[&str])] = &[
    ("assets", &["sim", "rules", "map", "render", "sidebar", "ui", "app"]),
    ("util", &["sim", "rules", "map", "render", "sidebar", "ui", "app"]),
    ("rules", &["sim", "map"]),
    ("map", &["sim", "render", "app"]),
    ("render", &["app"]),
    ("sidebar", &["app"]),
    ("ui", &["app", "skirmish_scenarios"]),
    ("sim", &["render", "sidebar", "ui", "audio", "net"]),
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
    visit_rust_files(&src_root, &mut |path| {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name == "tests.rs" || name.ends_with("_tests.rs") {
            return;
        }
        let rel = path
            .strip_prefix(&src_root)
            .expect("scanned file under src")
            .to_string_lossy()
            .replace('\\', "/");
        let source =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let production = strip_test_items(&blank_comments_and_literals(&source));
        for (symbol, files) in allowed {
            if production.contains(symbol) && !files.contains(&rel.as_str()) {
                violations.push(format!("{rel}: {symbol}"));
            }
        }
    });
    assert!(
        violations.is_empty(),
        "swappable-resource frame adapters referenced outside their pinned \
         production sites: {violations:?}\n\
         Production advances only through SimRuntime::advance_frame (F09); \
         gate new fixture callers with #[cfg(test)]."
    );
}

fn scan_forbidden_edges(src_root: &Path) -> BTreeSet<(String, String)> {
    let mut found = BTreeSet::new();
    for (layer, forbidden) in LAYER_RULES {
        let dir = src_root.join(layer);
        assert!(
            dir.is_dir(),
            "layer directory src/{layer} missing — update LAYER_RULES"
        );
        visit_rust_files(&dir, &mut |path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if name == "tests.rs" || name.ends_with("_tests.rs") {
                return;
            }
            let source = fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let production = strip_test_items(&blank_comments_and_literals(&source));
            for root in *forbidden {
                if contains_crate_ref(&production, root)
                    || group_contains_root(&production, root)
                {
                    let rel = path
                        .strip_prefix(src_root)
                        .expect("scanned file under src")
                        .to_string_lossy()
                        .replace('\\', "/");
                    found.insert((rel, root.to_string()));
                }
            }
        });
    }
    found
}

fn visit_rust_files(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    let mut paths: Vec<_> = entries
        .map(|e| e.expect("dir entry").path())
        .collect();
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
                    if bytes[i] == b'"' && bytes[i + 1..].iter().take(hashes).filter(|&&c| c == b'#').count() == hashes {
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
                        out.extend_from_slice(b"  ");
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

/// Remove `#[cfg(test)]` items (attribute plus the following item, braced or
/// single-statement) from already-blanked source. `cfg(all(test, …))` counts
/// as test-only; `cfg(any(test, debug_assertions))` does not.
fn strip_test_items(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth: i64 = 0;
    let mut skip_above: Option<i64> = None;
    let mut awaiting_item = false;
    for line in text.lines() {
        let opens = line.bytes().filter(|&b| b == b'{').count() as i64;
        let closes = line.bytes().filter(|&b| b == b'}').count() as i64;
        let is_test_attr = line.contains("cfg(test") || line.contains("cfg(all(test");
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
        if let Some(above) = skip_above {
            if depth <= above {
                skip_above = None;
            }
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
    assert!(group_contains_root("use crate::{app as a, ui::x};\n", "app"));
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
        fields,
        expected,
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
    let sim_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sim");
    let mut offenders: Vec<String> = Vec::new();
    visit_rust_files(&sim_root, &mut |path| {
        let source =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let blanked = blank_comments_and_literals(&source);
        for root in ["render", "net", "sidebar", "ui", "audio", "app"] {
            if contains_crate_ref(&blanked, root) || group_contains_root(&blanked, root) {
                offenders.push(format!("{} -> {root}", path.display()));
            }
        }
    });
    assert!(
        offenders.is_empty(),
        "sim/ references upper-layer root(s): {offenders:?}\n\
         Sim tests that need render/net live in those layers' boundary test \
         modules (e.g. render::locomotor_visual tests, \
         net::lockstep_sim_convergence_tests), never inside sim/."
    );
}
