//! Standalone source-level architecture guards for VMCE.
//!
//! Kept separate from upstream `architecture_guards.rs` so VMCE boundary checks
//! stay conflict-free and easy to drop or adapt alongside a plugin system.

use std::fs;
use std::path::Path;

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

fn is_raw_string_start(bytes: &[u8], i: usize) -> bool {
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

/// Blank comments (and optionally string/char literals) while preserving
/// newlines, UTF-8 validity and code structure.
fn blank_source(source: &str, blank_literals: bool) -> String {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
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
                let start = i;
                let mut j = i + 1;
                if bytes.get(j) == Some(&b'r') {
                    j += 1;
                }
                let mut hashes = 0usize;
                while bytes.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if blank_literals {
                    out.resize(out.len() + (j - i) + 1, b' ');
                } else {
                    out.extend_from_slice(&bytes[start..=j]);
                }
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
                        if blank_literals {
                            out.resize(out.len() + 1 + hashes, b' ');
                        } else {
                            out.extend_from_slice(&bytes[i..i + 1 + hashes]);
                        }
                        i += 1 + hashes;
                        break;
                    }
                    out.push(if !blank_literals || bytes[i] == b'\n' {
                        bytes[i]
                    } else {
                        b' '
                    });
                    i += 1;
                }
            }
            b'"' => {
                out.push(if blank_literals { b' ' } else { b'"' });
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        if blank_literals {
                            out.push(b' ');
                            out.push(if bytes.get(i + 1) == Some(&b'\n') {
                                b'\n'
                            } else {
                                b' '
                            });
                        } else {
                            out.push(bytes[i]);
                            if let Some(&next) = bytes.get(i + 1) {
                                out.push(next);
                            }
                        }
                        i += 2;
                    } else if bytes[i] == b'"' {
                        out.push(if blank_literals { b' ' } else { b'"' });
                        i += 1;
                        break;
                    } else {
                        out.push(if !blank_literals || bytes[i] == b'\n' {
                            bytes[i]
                        } else {
                            b' '
                        });
                        i += 1;
                    }
                }
            }
            b'\'' => {
                if bytes.get(i + 1) == Some(&b'\\') {
                    let start = i;
                    i += 2;
                    if i < bytes.len() {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i] != b'\'' && bytes[i] != b'\n' {
                        i += 1;
                    }
                    if bytes.get(i) == Some(&b'\'') {
                        i += 1;
                    }
                    if blank_literals {
                        out.resize(out.len() + (i - start), b' ');
                    } else {
                        out.extend_from_slice(&bytes[start..i]);
                    }
                } else if bytes.get(i + 2) == Some(&b'\'') {
                    if blank_literals {
                        out.extend_from_slice(b"   ");
                    } else {
                        out.extend_from_slice(&bytes[i..i + 3]);
                    }
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(out).expect("blanking preserves UTF-8 structure")
}

fn vmce_manifest_feature_violations(manifest_name: &str, manifest_text: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let mut in_features = false;
    for (idx, raw_line) in manifest_text.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_features = line == "[features]";
            continue;
        }
        if !in_features {
            continue;
        }
        let Some((lhs, rhs)) = line.split_once('=') else {
            continue;
        };
        if lhs.trim().eq_ignore_ascii_case("vmce")
            || rhs.contains("\"vmce\"")
            || rhs.contains("/vmce\"")
        {
            violations.push(format!("{manifest_name}:{}: {line}", idx + 1));
        }
    }
    violations
}

fn contains_vmce_feature_attr(code_with_strings: &str) -> bool {
    let bytes = code_with_strings.as_bytes();
    let kw = b"feature";
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(kw)
            && (i == 0 || !is_ident(bytes[i - 1]))
            && !bytes.get(i + kw.len()).is_some_and(|&b| is_ident(b))
        {
            let mut j = i + kw.len();
            while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                j += 1;
            }
            if bytes.get(j) == Some(&b'=') {
                j += 1;
                while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                    j += 1;
                }
                if bytes[j..].starts_with(b"\"vmce\"") {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Checks `crate::<root>`, `super::<root>`, and brace-grouped `crate::{...}` /
/// `super::{...}` paths in blanked code (simplified check dedicated to
/// isolated VMCE data modules).
fn references_layer_root(blanked: &str, root: &str) -> bool {
    for prefix in ["crate::", "super::"] {
        let mut rest = blanked;
        while let Some(pos) = rest.find(prefix) {
            let after = rest[pos + prefix.len()..].trim_start();
            if after.starts_with('{') {
                if let Some(end) = after.find('}') {
                    let group = &after[1..end];
                    for item in group.split(',') {
                        let first_seg = item
                            .trim()
                            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                            .next()
                            .unwrap_or("");
                        if first_seg == root {
                            return true;
                        }
                    }
                }
            } else {
                let first_seg = after
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("");
                if first_seg == root {
                    return true;
                }
            }
            rest = &rest[pos + prefix.len()..];
        }
    }
    false
}

fn vmce_source_violations(rel: &str, source: &str) -> Vec<String> {
    if rel == "architecture_guards.rs" || rel == "architecture_guards_vmce.rs" {
        return Vec::new();
    }
    let mut violations = Vec::new();
    let rel_lower = rel.to_ascii_lowercase();
    let is_vmce_file = rel_lower.contains("vmce");
    let is_coop_file = rel_lower.contains("cooperative");
    let is_vmce_map_data = is_vmce_file && rel_lower.starts_with("map/");

    if !is_vmce_file && (rel_lower.starts_with("app/") || rel_lower.starts_with("ui/")) {
        let shared = blank_source(source, false);
        if shared.lines().any(|line| {
            line.to_ascii_lowercase().contains("vmce")
                && !matches!(
                    (rel, line.trim()),
                    (
                        "app/shell_network_setup.rs",
                        "#[path = \"shell_network_setup/vmce_setup.rs\"]"
                    ) | ("app/match_runtime/mod.rs", "#[path = \"vmce_result.rs\"]")
                        | ("app/input/diplomacy.rs", "#[path = \"vmce_diplomacy.rs\"]")
                )
        }) {
            violations.push(format!(
                "{rel}: shared setup must use a mode-neutral interface"
            ));
        }
    }

    if is_vmce_file && rel_lower.contains("campaign") {
        violations.push(format!("{rel}: VMCE file path must not mix `campaign`"));
    }

    let stripped = blank_source(source, false);
    if contains_vmce_feature_attr(&stripped) {
        violations.push(format!(
            "{rel}: compile-time `feature = \"vmce\"` gate is forbidden"
        ));
    }

    let blanked = blank_source(source, true);
    if is_vmce_map_data {
        for forbidden in ["app", "sim", "net", "ui", "render", "audio"] {
            if references_layer_root(&blanked, forbidden) {
                violations.push(format!(
                    "{rel}: VMCE map data module must stay independent from `{forbidden}`"
                ));
            }
        }
    }

    let bytes = blanked.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = &blanked[start..i];
            let lower = ident.to_ascii_lowercase();
            if lower.contains("vmce") && lower.contains("campaign") {
                violations.push(format!(
                    "{rel}: identifier `{ident}` mixes `vmce` and `campaign`"
                ));
            }
            if is_vmce_file {
                if lower.contains("campaign") {
                    violations.push(format!(
                        "{rel}: VMCE module identifier `{ident}` must not use `campaign`"
                    ));
                }
                if lower.contains("cooperative") {
                    violations.push(format!("{rel}: VMCE module must not couple to `{ident}`"));
                }
            }
            if is_coop_file && lower.contains("vmce") {
                violations.push(format!(
                    "{rel}: Cooperative module must not couple to `{ident}`"
                ));
            }
        } else {
            i += 1;
        }
    }
    violations
}

#[test]
fn vmce_architecture_boundaries_hold() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_root = root.join("src");
    let mut violations = Vec::new();
    for manifest in ["Cargo.toml", "src/Cargo.toml"] {
        let text = fs::read_to_string(root.join(manifest))
            .unwrap_or_else(|e| panic!("read {manifest}: {e}"));
        violations.extend(vmce_manifest_feature_violations(manifest, &text));
    }
    let mut files = Vec::new();
    visit_rust_files(&src_root, &mut |path| files.push(path.to_path_buf()));
    for path in files {
        let rel = path
            .strip_prefix(&src_root)
            .expect("scanned file under src")
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        violations.extend(vmce_source_violations(&rel, &source));
    }
    assert!(
        violations.is_empty(),
        "VMCE boundary violations detected: {violations:?}"
    );
}

#[test]
fn vmce_guards_reject_feature_gates_campaign_names_and_cooperative_coupling() {
    let bad_manifest = "[features]\ndefault = []\nvmce = [\"vera20k-core/vmce\"]\n";
    assert_eq!(
        vmce_manifest_feature_violations("Cargo.toml", bad_manifest).len(),
        1
    );

    let bad_cfg = "#[cfg(feature = \"vmce\")]\nfn gated() {}\n";
    assert!(
        !vmce_source_violations("app/vmce/mod.rs", bad_cfg).is_empty(),
        "must reject cfg(feature = \"vmce\")"
    );

    let bad_name = "pub struct VmceCampaignConfig;\nfn call_coop() { cooperative_round(); }\n";
    let found = vmce_source_violations("app/vmce/lobby.rs", bad_name);
    assert!(
        found.iter().any(|v| v.contains("VmceCampaignConfig")),
        "must reject campaign identifier in VMCE module: {found:?}"
    );
    assert!(
        found.iter().any(|v| v.contains("cooperative_round")),
        "must reject cooperative reference in VMCE module: {found:?}"
    );

    assert!(
        !vmce_source_violations("app/vmce_campaign.rs", "fn ok() {}\n").is_empty(),
        "must reject vmce_campaign file path"
    );

    // Allowed: reading external `"campaign.ini"`, escaped char literals `'\''`,
    // and UTF-8 string literals and comments inside a cleanly named VMCE module.
    let valid_vmce = "// Reads external campaign.ini manifest.\nconst TITLE: &str = \"尤里的复仇\";\nconst FILE: &str = \"campaign.ini\";\nconst QUOTE: char = '\\'';\nconst BACKSLASH: char = '\\\\';\npub struct VmceManifest;\n";
    assert!(
        vmce_source_violations("app/vmce/manifest.rs", valid_vmce).is_empty(),
        "valid VMCE code reading \"campaign.ini\" and UTF-8 literals must pass"
    );

    assert!(
        vmce_source_violations(
            "app/cooperative_round.rs",
            "fn use_vmce() { vmce_setup(); }\n"
        )
        .iter()
        .any(|v| v.contains("Cooperative module must not couple")),
        "must reject cooperative -> vmce coupling"
    );

    let coupled_map = "use crate::{net::lan, sim::world};\n";
    let map_violations = vmce_source_violations("map/vmce_config.rs", coupled_map);
    assert!(
        map_violations.iter().any(|v| v.contains("`net`")),
        "must reject map/vmce -> net dependency: {map_violations:?}"
    );
    assert!(
        map_violations.iter().any(|v| v.contains("`sim`")),
        "must reject map/vmce -> sim dependency: {map_violations:?}"
    );
}
