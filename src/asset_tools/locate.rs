//! Asset lookup with an explicit fallback into catalogued archives.
//!
//! `AssetManager::resolve_ref` is the production lookup and is what the game
//! would find. It cannot reach the catalogued nested archives — their bytes are
//! parsed and in memory, but they are absent from the lookup index. That is not
//! an edge case for a browser: the sidebar chrome lives there, so `POWERP.SHP`
//! and its neighbours resolve to nothing through the production path.
//!
//! A browsing tool that stopped there would be useless for the assets most
//! often under investigation, and one that silently merged the two would
//! misreport what the game loads. So the fallback happens, and says so.
//!
//! ## Dependency rules
//! - Depends on `assets/` only.

use crate::assets::asset_manager::AssetManager;
use crate::assets::mix_hash::mix_hash;

pub struct Resolved<'a> {
    pub bytes: &'a [u8],
    pub source_archive: String,
    pub entry_id: i32,
    /// True when production lookup missed and the bytes came from a catalogued
    /// archive instead. The game would NOT find this asset by this name.
    pub via_catalog: bool,
}

impl Resolved<'_> {
    /// Warning text for a catalogue hit, for reports that carry warnings.
    pub fn catalog_warning(&self) -> Option<String> {
        self.via_catalog.then(|| {
            format!(
                "found in \"{}\", a catalogued archive outside the name-lookup index — the \
                 engine's own lookup would not resolve this name",
                self.source_archive
            )
        })
    }
}

/// Resolve by production lookup first, then by sweeping catalogued archives.
pub fn locate<'a>(asset_manager: &'a AssetManager, name: &str) -> Option<Resolved<'a>> {
    if let Some(resolved) = asset_manager.resolve_ref(name) {
        return Some(Resolved {
            bytes: resolved.bytes,
            source_archive: resolved.source_archive.to_string(),
            entry_id: resolved.entry_id,
            via_catalog: false,
        });
    }

    // Find the holding archive by name first: the visitor hands out borrows tied
    // to its own call, so the bytes are re-fetched through `archive()` after.
    let entry_id = mix_hash(name);
    let mut holder: Option<String> = None;
    asset_manager.visit_archives(|archive_name, archive| {
        if holder.is_none() && archive.get_by_id(entry_id).is_some() {
            holder = Some(archive_name.to_string());
        }
    });

    let source_archive = holder?;
    let bytes = asset_manager
        .archive(&source_archive)?
        .get_by_id(entry_id)?;
    Some(Resolved {
        bytes,
        source_archive,
        entry_id,
        via_catalog: true,
    })
}
