//! `asset find <NAME>` — where an asset lives, and what shadows it.
//!
//! The non-obvious half of this verb is `catalog_only`. A number of nested
//! archives are parsed and held in memory but are not in the name-lookup index,
//! so `get`/`resolve_ref` cannot reach their contents even though the bytes are
//! right there. Reporting those hits separately is what stops a caller
//! concluding that a present asset is missing.
//!
//! ## Dependency rules
//! - Depends on `assets/` and sibling `asset_tools` modules only.

use std::collections::HashSet;

use crate::asset_tools::identify::identify;
use crate::asset_tools::names::NameDict;
use crate::asset_tools::report::{ErrorReport, FindReport, Located};
use crate::assets::asset_manager::AssetManager;
use crate::assets::mix_hash::{mix_hash, westwood_hash};

/// Loose files on disk report their source with this prefix.
const LOOSE_PREFIX: &str = "loose:";

pub struct FindOptions {
    /// Search every mounted archive, not just the winner.
    pub all: bool,
}

impl Default for FindOptions {
    fn default() -> Self {
        Self { all: true }
    }
}

pub fn run(
    asset_manager: &AssetManager,
    dict: &NameDict,
    name: &str,
    opts: &FindOptions,
) -> Result<FindReport, ErrorReport> {
    let crc = mix_hash(name);
    let westwood = westwood_hash(name);

    let winner = asset_manager.resolve_ref(name).map(|resolved| {
        let id = identify(resolved.bytes);
        Located {
            archive: resolved.source_archive.to_string(),
            entry_id: format!("{:#010X}", resolved.entry_id as u32),
            size: resolved.bytes.len() as u32,
            format: id.format.to_string(),
            detail: id.detail,
            note: None,
        }
    });

    let loose_override = winner
        .as_ref()
        .is_some_and(|w| w.archive.starts_with(LOOSE_PREFIX));

    let mut shadowed_by: Vec<Located> = Vec::new();
    let mut catalog_only: Vec<Located> = Vec::new();

    if opts.all {
        let reachable: HashSet<String> = asset_manager
            .registered_archive_names()
            .into_iter()
            .collect();
        let winner_archive = winner.as_ref().map(|w| w.archive.clone());

        asset_manager.visit_archives(|archive_name, archive| {
            // Probe both hash kinds: RA2 archives index by the CRC-derived hash,
            // but a stray legacy archive would index by the older one.
            for (entry_id, hash_kind) in [(crc, "crc32"), (westwood, "westwood")] {
                let Some(bytes) = archive.get_by_id(entry_id) else {
                    continue;
                };
                if winner_archive.as_deref() == Some(archive_name) {
                    continue;
                }
                let id = identify(bytes);
                let is_reachable = reachable.contains(archive_name);
                let located = Located {
                    archive: archive_name.to_string(),
                    entry_id: format!("{:#010X}", entry_id as u32),
                    size: bytes.len() as u32,
                    format: id.format.to_string(),
                    detail: id.detail,
                    note: if is_reachable {
                        (hash_kind == "westwood")
                            .then(|| "matched the legacy Westwood hash".to_string())
                    } else {
                        Some(
                            "catalogued nested archive — these bytes exist but name lookup \
                             cannot reach them"
                                .to_string(),
                        )
                    },
                };
                if is_reachable {
                    shadowed_by.push(located);
                } else {
                    catalog_only.push(located);
                }
            }
        });
    }

    let found = winner.is_some() || !catalog_only.is_empty();

    Ok(FindReport {
        name: name.to_string(),
        found,
        hash_crc32: format!("{:#010X}", crc as u32),
        hash_westwood: format!("{:#010X}", westwood as u32),
        winner,
        loose_override,
        shadowed_by,
        catalog_only,
        name_db: dict.db(),
    })
}
