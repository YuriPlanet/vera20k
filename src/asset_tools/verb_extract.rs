//! `asset extract <NAME>` — one archive entry's bytes, verbatim, on disk.
//!
//! Everything else in this module reads an asset and reports *about* it. This
//! verb is the escape hatch for the cases where that is not enough: a hex
//! editor, a byte diff against a second install, an external decoder, or an INI
//! that wants grepping. The bytes are copied out untouched — no re-encode, no
//! normalisation, no trailing newline — so the file on disk is byte-identical
//! to the archive payload and a diff against it means something.
//!
//! It generalises `src/bin/extract-ini.rs`, which copies a selected set of YR
//! INIs into one output directory. That binary resolves through `get_with_source`,
//! which shares `lookup_asset_entry` with `resolve_ref`, so every name it can
//! reach this verb can reach too — plus the catalogued archives that name
//! lookup cannot see, via `locate`.
//!
//! **There is no checksum in the report, and that is deliberate.** The crate has
//! no hashing dependency, and inventing a weak one would be worse than none: the
//! archive entry ID already *is* the filename hash the MIX index stores, and it
//! is reported alongside the byte length. Those two plus the source archive are
//! the identity of what was written. Anything stronger belongs to whatever tool
//! reads the extracted file.
//!
//! ## Dependency rules
//! - Part of `asset_tools/`: depends on `assets/` (archive resolution) and the
//!   sibling `identify` / `locate` / `report` modules.
//! - Nothing from `sim/`, `render/`, `ui/`, `audio/`, or `net/`.

use std::path::{Path, PathBuf};

use crate::asset_tools::identify;
use crate::asset_tools::report::{ErrorReport, ExtractReport};
use crate::assets::asset_manager::AssetManager;

/// Output root when the caller does not name one. Relative and under `target/`,
/// which is gitignored, so nothing this verb writes can be committed by accident.
const DEFAULT_OUT_ROOT: &str = "target/asset";

/// Subdirectory under the output root that this verb owns. Keeping it separate
/// from `render/` means clearing one never touches the other.
const EXTRACT_SUBDIR: &str = "extract";

/// Filename used when the asset name sanitises to nothing. Must match
/// `verb_render`'s fallback: the two verbs place the same asset under the same
/// name, and a caller that found a render expects the extract beside it.
const FALLBACK_FILE_NAME: &str = "asset";

/// Options for one `asset extract` invocation.
#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// Output root. The file lands in `<out>/extract/<sanitised-name>`.
    pub out: PathBuf,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            out: PathBuf::from(DEFAULT_OUT_ROOT),
        }
    }
}

/// What the destination path held before this run wrote to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetState {
    New,
    Overwrite,
}

/// Write one asset's raw bytes to disk and report where they came from.
///
/// Errors are values, not panics: a missing asset, an unwritable root, or a
/// directory sitting on the destination path all yield an [`ErrorReport`] whose
/// hint names the verb or flag that resolves it.
pub fn run(
    asset_manager: &AssetManager,
    name: &str,
    opts: &ExtractOptions,
) -> Result<ExtractReport, ErrorReport> {
    let Some(resolved) = crate::asset_tools::locate::locate(asset_manager, name) else {
        return Err(ErrorReport {
            error: format!("asset not found: {name}"),
            hint: Some(format!(
                "run `asset find {name}` — it also reports catalogued archives that name lookup \
                 cannot reach"
            )),
        });
    };
    let source_archive = resolved.source_archive.clone();
    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(resolved.catalog_warning());
    let bytes = resolved.bytes;

    // Sniffed from the bytes, not from the extension: an entry named `.shp` that
    // is really a nested MIX is exactly the kind of thing someone extracts to
    // check, and the report should say so before they open it.
    let identified = identify::identify(bytes);

    let file_name = sanitise_name(name);
    if file_name != name {
        warnings.push(format!(
            "asset name {name} was sanitised to {file_name} for the filesystem"
        ));
    }
    let dir = extract_dir(&opts.out);
    let path = dir.join(&file_name);

    if prepare_target(&dir, &path, name)? == TargetState::Overwrite {
        warnings.push(format!("replaced an existing file at {}", path.display()));
    }
    if bytes.is_empty() {
        warnings.push(format!(
            "{name} is a zero-length archive entry; an empty file was written"
        ));
    }

    std::fs::write(&path, bytes).map_err(|err| ErrorReport {
        error: format!("could not write {}: {err}", path.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })?;

    Ok(ExtractReport {
        asset: name.to_string(),
        source_archive,
        // The MIX index key, which is the hash of the filename — reported in
        // place of a content checksum, see the module note.
        entry_id: format!("0x{:08X}", resolved.entry_id as u32),
        size: bytes.len(),
        format: identified.format.to_string(),
        detail: identified.detail,
        path: path.display().to_string(),
        warnings,
    })
}

/// Create the output directory and classify what is already at `path`.
///
/// Split out of [`run`] so the collision rule is exercisable without a mounted
/// retail install — it is the one branch here that can destroy someone's data
/// if it is wrong.
fn prepare_target(dir: &Path, path: &Path, name: &str) -> Result<TargetState, ErrorReport> {
    std::fs::create_dir_all(dir).map_err(|err| ErrorReport {
        error: format!("could not create {}: {err}", dir.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })?;

    // A directory at the destination is never something to clobber. It is
    // usually a previous `asset render` tree that shared the root, and the
    // degenerate names `.` and `..` land here as well. Refuse; delete nothing.
    if path.is_dir() {
        return Err(ErrorReport {
            error: format!(
                "{} already exists and is a directory, so the bytes were not written",
                path.display()
            ),
            hint: Some(format!(
                "extract somewhere else: `asset extract {name} --out <DIR>`"
            )),
        });
    }

    Ok(if path.exists() {
        TargetState::Overwrite
    } else {
        TargetState::New
    })
}

/// `<out>/extract/`, absolutised so the reported path does not depend on the
/// reader's working directory.
fn extract_dir(out: &Path) -> PathBuf {
    let root = std::path::absolute(out).unwrap_or_else(|_| out.to_path_buf());
    root.join(EXTRACT_SUBDIR)
}

/// Replace every character outside `[A-Za-z0-9._-]` so the name is safe as a
/// filename component on any platform.
///
/// Mirrors `verb_render::sanitise_name` character for character: the two verbs
/// must agree, or the same asset lands under two different names. It is copied
/// rather than called because that one is module-private; the test below pins
/// this copy to the same fixtures verb_render's own test asserts.
///
/// Flattening the path separators is also what keeps a name like `../../x` from
/// escaping the output root — the result is always a single path component.
fn sanitise_name(name: &str) -> String {
    let sanitised: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitised.is_empty() {
        FALLBACK_FILE_NAME.to_string()
    } else {
        sanitised
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vera20k-asset-extract-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_traversing_name_flattens_to_one_component() {
        assert_eq!(sanitise_name("../../etc/passwd"), ".._.._etc_passwd");
        let dir = extract_dir(Path::new(DEFAULT_OUT_ROOT));
        let path = dir.join(sanitise_name("../../etc/passwd"));
        assert!(path.ends_with(".._.._etc_passwd"), "{}", path.display());
        // One component below the extract directory, never above it.
        assert_eq!(path.parent(), Some(dir.as_path()), "{}", path.display());
    }

    #[test]
    fn extract_dir_is_absolute_and_ends_with_the_extract_subdir() {
        let dir = extract_dir(Path::new(DEFAULT_OUT_ROOT));
        assert!(dir.is_absolute(), "{}", dir.display());
        assert!(dir.ends_with(EXTRACT_SUBDIR), "{}", dir.display());
    }

    #[test]
    fn a_directory_at_the_destination_is_refused() {
        let root = temp_root("collision");
        let dir = extract_dir(&root);
        let path = dir.join("rulesmd.ini");
        std::fs::create_dir_all(&path).expect("temp collision directory");

        let err = prepare_target(&dir, &path, "rulesmd.ini")
            .expect_err("a directory on the destination path is an error");
        assert!(err.error.contains("is a directory"), "{}", err.error);
        let hint = err.hint.expect("the error names the flag that helps");
        assert!(hint.contains("--out"), "{hint}");

        // The refusal must not have deleted the directory.
        assert!(path.is_dir(), "{}", path.display());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prepare_target_creates_the_directory_and_reports_an_overwrite() {
        let root = temp_root("overwrite");
        let dir = extract_dir(&root);
        let path = dir.join("rulesmd.ini");

        assert_eq!(
            prepare_target(&dir, &path, "rulesmd.ini").expect("first call creates the directory"),
            TargetState::New
        );
        assert!(dir.is_dir(), "{}", dir.display());

        std::fs::write(&path, b"[General]\n").expect("write the first extract");
        assert_eq!(
            prepare_target(&dir, &path, "rulesmd.ini").expect("second call succeeds"),
            TargetState::Overwrite
        );
        // Classification alone must not touch the bytes; only `run` writes.
        assert_eq!(
            std::fs::read(&path).expect("read back"),
            b"[General]\n".to_vec()
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
