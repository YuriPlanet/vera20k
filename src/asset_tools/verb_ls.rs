//! `asset ls <ARCHIVE>` — a paged listing of one archive's entries.
//!
//! Paging is not optional. `ra2.mix -> local.mix` alone carries 607 entries, and
//! an unbounded dump is both useless to read and expensive to produce, since
//! naming a row means sniffing its bytes.
//!
//! ## Dependency rules
//! - Depends on `assets/` and sibling `asset_tools` modules only.

use crate::asset_tools::identify::identify;
use crate::asset_tools::names::NameDict;
use crate::asset_tools::report::{ArchiveRow, ErrorReport, LsReport, LsRow};
use crate::assets::asset_manager::AssetManager;

/// Default page size. Chosen to stay readable in one response rather than to
/// match any archive's size.
pub const DEFAULT_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Hash,
    /// Archive index order — how the entries actually appear on disk.
    Index,
}

pub struct LsOptions {
    /// Case-insensitive substring filter on the resolved name.
    pub filter: Option<String>,
    /// Keep only rows whose sniffed format tag matches exactly.
    pub format: Option<String>,
    pub sort: SortKey,
    pub limit: usize,
    pub offset: usize,
}

impl Default for LsOptions {
    fn default() -> Self {
        Self {
            filter: None,
            format: None,
            sort: SortKey::Index,
            limit: DEFAULT_LIMIT,
            offset: 0,
        }
    }
}

pub fn run(
    asset_manager: &AssetManager,
    dict: &NameDict,
    archive_name: &str,
    opts: &LsOptions,
) -> Result<LsReport, ErrorReport> {
    let Some(archive) = asset_manager.archive(archive_name) else {
        let known = asset_manager.loaded_archive_names();
        let suggestion = known
            .iter()
            .find(|name| {
                name.to_ascii_lowercase()
                    .contains(&archive_name.to_ascii_lowercase())
            })
            .cloned();
        return Err(ErrorReport {
            error: format!("archive not loaded: {archive_name}"),
            hint: Some(match suggestion {
                Some(name) => {
                    format!("did you mean \"{name}\"? run `asset archives` for the full list")
                }
                None => format!(
                    "run `asset archives` to list the {} mounted archives",
                    known.len()
                ),
            }),
        });
    };

    let entry_count = archive.entry_count();
    let mut rows: Vec<LsRow> = Vec::with_capacity(entry_count);
    let mut identified_count = 0usize;

    for (index, entry) in archive.entries().iter().enumerate() {
        let (name, identified) = dict.resolve(entry.id);
        if identified {
            identified_count += 1;
        }
        let id = match archive.get_by_id(entry.id) {
            Some(bytes) => identify(bytes),
            None => crate::asset_tools::identify::Identified {
                format: "unreadable",
                detail: "entry could not be read from the archive".to_string(),
            },
        };
        rows.push(LsRow {
            index,
            entry_id: format!("{:#010X}", entry.id as u32),
            size: entry.size,
            name,
            identified,
            format: id.format.to_string(),
            detail: id.detail,
        });
    }

    if let Some(filter) = &opts.filter {
        let needle = filter.to_ascii_lowercase();
        rows.retain(|row| row.name.to_ascii_lowercase().contains(&needle));
    }
    if let Some(format) = &opts.format {
        let wanted = format.to_ascii_lowercase();
        rows.retain(|row| row.format == wanted);
    }

    match opts.sort {
        SortKey::Index => {}
        SortKey::Name => rows.sort_by(|a, b| {
            // Identified names first — unidentified rows are hex placeholders
            // and sorting them among real names is noise.
            (!a.identified).cmp(&!b.identified).then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
        }),
        SortKey::Size => rows.sort_by_key(|row| std::cmp::Reverse(row.size)),
        SortKey::Hash => rows.sort_by(|a, b| a.entry_id.cmp(&b.entry_id)),
    }

    let matched = rows.len();
    let page: Vec<LsRow> = rows
        .into_iter()
        .skip(opts.offset)
        .take(opts.limit)
        .collect();

    Ok(LsReport {
        archive: archive_name.to_string(),
        entry_count,
        matched,
        total_bytes: archive.byte_len(),
        identified_count,
        shown: page.len(),
        offset: opts.offset,
        limit: opts.limit,
        name_db: dict.db(),
        entries: page,
    })
}

/// `asset archives` — every mounted archive, with reachability marked.
pub fn archives(asset_manager: &AssetManager) -> Vec<ArchiveRow> {
    let reachable: Vec<String> = asset_manager.registered_archive_names();
    let mut rows = Vec::new();
    asset_manager.visit_archives(|name, archive| {
        rows.push(ArchiveRow {
            name: name.to_string(),
            entry_count: archive.entry_count(),
            total_bytes: archive.byte_len(),
            name_lookup_reachable: reachable.iter().any(|r| r == name),
        });
    });
    rows
}
