//! The committed schema is unchanged. Only explicit candidate export writes a file.
use std::io::Write;

use serde::{Deserialize, Serialize};

use super::*;

#[derive(Serialize, Deserialize, Default)]
pub(super) struct Manifest {
    pub schema: u32,
    /// Archive index order, not sorted IDs: matches the original corpus fold.
    pub archives: ArchiveInventory,
    pub format_counts: FormatCounts,
    #[serde(default)]
    pub bag_aud: usize,
    /// Rust decoder regression ratchets; not native execution outputs.
    pub decode_rollups: BTreeMap<String, u64>,
    pub files: Vec<(String, String, u64)>,
}

#[cfg(test)]
pub(super) fn read_manifest() -> Result<Manifest, String> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/retail_goldens/manifest.json");
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if manifest.schema != 1 {
        return Err(format!(
            "unsupported corpus manifest schema {}",
            manifest.schema
        ));
    }
    Ok(manifest)
}

#[derive(Debug, Serialize)]
pub struct CandidateReport {
    pub destination: String,
    pub all_mixes: bool,
    pub archives: usize,
    pub sniffed_files: usize,
    pub bag_entries: usize,
    pub warning: &'static str,
}

/// Collect the entire candidate before opening a new destination. Never updates
/// the checked-in reference, merges partial fields, or overwrites an existing file.
/// The caller must mount all disk MIXes using
/// `root::open_manager(root, true, AresSetting::Auto, &ArchiveNames::default())`.
pub fn export_candidate(am: &AssetManager, destination: &Path) -> Result<CandidateReport, String> {
    reject_ambient_write()?;
    let manifest = collect_manifest(am)?;
    write_candidate(&manifest, destination)?;
    Ok(CandidateReport {
        destination: destination.display().to_string(),
        all_mixes: true,
        archives: manifest.archives.len(),
        sniffed_files: manifest.format_counts.values().sum(),
        bag_entries: manifest.bag_aud,
        warning: "Candidate only: review corpus drift and decoder changes before replacing the reference. Decoder digests are Rust-versus-prior-Rust ratchets, not native parity evidence.",
    })
}

fn write_candidate(manifest: &Manifest, destination: &Path) -> Result<(), String> {
    let bytes =
        serde_json::to_vec_pretty(manifest).map_err(|e| format!("serialize candidate: {e}"))?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| format!("create new candidate {}: {e}", destination.display()))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| {
            format!(
                "write candidate {}: {e}; discard this incomplete candidate",
                destination.display()
            )
        })
}

#[cfg(all(test, tests_asset_tools))]
mod tests {
    use super::*;

    fn candidate_path(tag: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vera20k-corpus-{tag}-{}-{stamp}.json",
            std::process::id()
        ))
    }

    #[test]
    fn candidate_export_never_replaces_existing_bytes() {
        let path = candidate_path("preserve");
        std::fs::write(&path, b"existing reference").unwrap();
        assert!(write_candidate(&Manifest::default(), &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing reference");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn candidate_export_creates_one_complete_schema_document() {
        let path = candidate_path("create");
        let manifest = Manifest {
            schema: 1,
            bag_aud: 3438,
            ..Manifest::default()
        };
        write_candidate(&manifest, &path).unwrap();
        let saved: Manifest = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.bag_aud, 3438);
        assert_eq!(saved.schema, 1);
        assert!(write_candidate(&manifest, &path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
