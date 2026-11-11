//! Script directory discovery.
//!
//! Convention: `<ra2_dir>/vera20k/scripts/` holds editable script sources, an
//! optional `scripts.json` manifest, and (later) compiled modules. This module
//! only *discovers* and validates; it never executes anything.
//!
//! The path is supplied by the caller rather than read from a global, so a
//! host can override the default without changing this module.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Default script root, relative to the retail asset (game) directory. It sits
/// beside VERA's own `<game>/vera20k/` profile folder.
pub const SCRIPT_DIR: &str = "vera20k/scripts";
pub const MANIFEST_NAME: &str = "scripts.json";
/// A compiled module. Reserved for a future loader; not loadable yet.
pub const MODULE_EXT: &str = "wasm";
/// An editable script source: plain text, re-read from disk when it changes.
pub const SOURCE_EXT: &str = "rhai";

/// `scripts.json`: the mapping between a `[Unit] Script=` value and a file in
/// this directory. Order is not significant for lookup, but is kept as written
/// for stable diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptManifest {
    #[serde(default)]
    pub scripts: Vec<ScriptEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptEntry {
    /// Value matched (case-insensitively) against `[Unit] Script=`.
    pub name: String,
    /// File name relative to the script directory.
    pub module: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptFileKind {
    /// A compiled module, reserved for a future loader.
    Module,
    /// An editable script source (`*.rhai`), loaded by `scripting::file`.
    Source,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFile {
    pub file_name: String,
    pub kind: ScriptFileKind,
}

/// A read-only snapshot of the script directory. Diagnostics are advisory and
/// must never abort startup: a missing or broken script directory leaves the
/// game running with no scripts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptDirectory {
    pub root: PathBuf,
    pub present: bool,
    pub manifest: Option<ScriptManifest>,
    pub files: Vec<ScriptFile>,
    pub diagnostics: Vec<String>,
}

impl ScriptDirectory {
    /// Discover using the default `<ra2_dir>/vera20k/scripts` layout.
    pub fn discover(ra2_dir: &Path) -> Self {
        Self::discover_at(&ra2_dir.join(SCRIPT_DIR))
    }

    /// Discover at an explicit root (for hosts that override the default).
    pub fn discover_at(root: &Path) -> Self {
        let mut out = Self {
            root: root.to_path_buf(),
            ..Self::default()
        };
        if !root.is_dir() {
            out.diagnostics
                .push(format!("script directory not found: {}", root.display()));
            return out;
        }
        out.present = true;

        match std::fs::read_to_string(root.join(MANIFEST_NAME)) {
            Ok(text) => match serde_json::from_str::<ScriptManifest>(&text) {
                Ok(manifest) => out.manifest = Some(manifest),
                Err(error) => out.diagnostics.push(format!(
                    "invalid {MANIFEST_NAME}: {error}; no scripts will load"
                )),
            },
            Err(_) => out
                .diagnostics
                .push(format!("{MANIFEST_NAME} missing; no scripts will load")),
        }

        // Sorted for deterministic diagnostics across filesystems.
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                if entry.path().is_file() {
                    names.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
        names.sort();
        for file_name in names {
            let kind = match Path::new(&file_name)
                .extension()
                .and_then(|ext| ext.to_str())
            {
                Some(ext) if ext.eq_ignore_ascii_case(MODULE_EXT) => ScriptFileKind::Module,
                Some(ext) if ext.eq_ignore_ascii_case(SOURCE_EXT) => ScriptFileKind::Source,
                _ => ScriptFileKind::Unknown,
            };
            out.files.push(ScriptFile { file_name, kind });
        }

        if let Some(manifest) = &out.manifest {
            for entry in &manifest.scripts {
                let found = out.files.iter().any(|file| {
                    file.file_name == entry.module
                        && matches!(file.kind, ScriptFileKind::Module | ScriptFileKind::Source)
                });
                if !found {
                    out.diagnostics.push(format!(
                        "manifest script '{}' references missing module '{}'",
                        entry.name, entry.module
                    ));
                }
            }
        }
        out
    }

    /// Resolve a `[Unit] Script=` value to a file name through the manifest.
    pub fn module_for(&self, binding: &str) -> Option<&str> {
        self.manifest
            .as_ref()?
            .scripts
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(binding))
            .map(|entry| entry.module.as_str())
    }

    pub fn module_count(&self) -> usize {
        self.files
            .iter()
            .filter(|file| file.kind == ScriptFileKind::Module)
            .count()
    }

    /// Editable script sources the host can load without recompiling.
    pub fn source_count(&self) -> usize {
        self.files
            .iter()
            .filter(|file| file.kind == ScriptFileKind::Source)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "vera20k_script_discovery_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp script dir");
        root
    }

    #[test]
    fn missing_directory_is_advisory_not_fatal() {
        let root = temp_root().join("absent");
        let dir = ScriptDirectory::discover_at(&root);
        assert!(!dir.present);
        assert!(dir.files.is_empty());
        assert_eq!(dir.diagnostics.len(), 1);
    }

    #[test]
    fn manifest_resolves_bindings_and_classifies_files() {
        let root = temp_root();
        std::fs::write(
            root.join(MANIFEST_NAME),
            r#"{"scripts":[{"name":"ExampleShow","module":"example_show.rhai"}]}"#,
        )
        .unwrap();
        std::fs::write(root.join("example_show.rhai"), b"fn on_update(ctx, s) { s }").unwrap();
        std::fs::write(root.join("notes.txt"), b"ignored").unwrap();

        let dir = ScriptDirectory::discover_at(&root);
        assert!(dir.present);
        assert_eq!(dir.source_count(), 1);
        assert_eq!(dir.module_for("exampleshow"), Some("example_show.rhai"));
        assert_eq!(dir.module_for("missing"), None);
        assert!(dir
            .files
            .iter()
            .any(|f| f.kind == ScriptFileKind::Unknown));
        assert!(!dir
            .diagnostics
            .iter()
            .any(|d| d.contains("missing module")));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_module_and_bad_manifest_are_reported() {
        let root = temp_root();
        std::fs::write(
            root.join(MANIFEST_NAME),
            r#"{"scripts":[{"name":"X","module":"gone.wasm"}]}"#,
        )
        .unwrap();
        let dir = ScriptDirectory::discover_at(&root);
        assert!(dir
            .diagnostics
            .iter()
            .any(|d| d.contains("missing module 'gone.wasm'")));

        std::fs::write(root.join(MANIFEST_NAME), b"not json").unwrap();
        let bad = ScriptDirectory::discover_at(&root);
        assert!(bad.manifest.is_none());
        assert!(bad
            .diagnostics
            .iter()
            .any(|d| d.contains("invalid scripts.json")));

        std::fs::remove_dir_all(&root).ok();
    }
}
