//! VERA: the art files in the game folder whose values the simulation reads,
//! compared with a retail install's, for the online agreement
//! (`ProcessAssets::multiplayer_rules_digest`) and Quick Match, which pairs
//! players without it and names changed game files instead.
//!
//! A match binds values from each player's own art files: SHP frame counts
//! and heights for the animation, effect, build-up, rubble and terrain
//! catalogs, crater and scorch frame sizes, building palettes, each voxel's
//! load state and size, whether each overlay's and light glow's SHP loads
//! (`init_helpers::bind_rules_art_metadata`, `bind_rules_catalog_assets`,
//! `bind_rules_alpha_images` and `effect_asset_catalog::resolved_overlay_shp_ids`;
//! `RuleSet::simulation_config_hash` folds the catalogs, with sound
//! references this agreement leaves out). A loose file or an extra archive
//! (`expandmd99.mix`, `ecache01.mix`) that changes one of those values plays
//! another match, so this finds them: it binds them once per theater on a
//! fresh manager, records the files they read ([`record_lookups`]), and
//! compares each one the game folder supplies with the copy a retail install
//! reads ([`RetailReads`]), by the values its reader takes only. A
//! replacement with other pixels and the same values changes nothing.
//!
//! Residuals: the names come from the process's Rules, Art and AI as
//! started, so an image only a mode INI or a map names (its trigger
//! animations, its overlays' cell animations) and a theater's tile
//! animations are not read; a game-folder file named ahead of the retail one
//! it hides (a theater-lettered name) is named even when its values match; a
//! replaced shipped archive is not caught, as in
//! `retail_sources::files_changing_shipped_inis`; the game folder is read
//! when the check first runs, so a file changed while VERA runs is compared
//! as it is then, not as the running game read it.

use std::collections::BTreeSet;

use crate::assets::asset_manager::{AssetManager, record_lookups};
use crate::map::theater::RetailReads;
use crate::rules::process_owner::NativeRulesProcessOwner;
use crate::rules::retail_sources::{game_folder_file, shipped_source};

/// The art files the game folder changes ([`find`]).
#[derive(Debug, Default)]
pub(crate) struct ChangedArt {
    /// The loose files and outermost archives that change them, once each.
    pub(crate) files: Vec<String>,
    /// Each changed file's name and the values the simulation reads from
    /// it; `None` when none changes.
    pub(crate) digest: Option<String>,
}

/// Bind the art that `rules`' Rules, ART and AI name for every theater on
/// the manager `open` gives, which this owns: it mounts each theater's
/// archives and empties its own `LoadFileFromMIX` cache first, so each
/// theater reads as a newly started game's would, the process's manager and
/// its history are untouched, and every player's game reads the same names
/// whichever theater it last played.
pub(crate) fn find(
    rules: &NativeRulesProcessOwner,
    open: impl FnOnce() -> anyhow::Result<AssetManager>,
) -> Result<ChangedArt, String> {
    use crate::app::loading::init_helpers;
    let processed = rules
        .startup_compatibility_projection()
        .map_err(|error| error.to_string())?;
    let mut projection = crate::rules::ruleset::RuleSet::from_processed_rules(&processed)
        .map_err(|error| error.to_string())?;
    let mut assets = open().map_err(|error| error.to_string())?;
    // Loading binds AI script animations with the map's (`0x006EF610`).
    let script_roots = crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(
        &assets,
        &crate::rules::ini_parser::IniFile::empty(),
        true,
    )
    .map(|registry| registry.script_anim_class_roots(&projection))
    .unwrap_or_else(|error| {
        log::warn!("Art check: the AI scripts did not load: {error}");
        Vec::new()
    });
    let overlays = projection.overlay_registry().clone();
    let mut found = ChangedArt::default();
    // Each changed name, its reader and the values read, in name order.
    let mut changed = BTreeSet::new();
    for (theater, extension) in crate::map::theater::theater_names_and_extensions() {
        let archives = crate::map::theater::theater_mix_archives(theater).unwrap_or_default();
        if let Err(error) = assets.activate_theater_archives(theater, archives) {
            log::warn!("Art check: theater {theater}'s archives did not mount: {error}");
        }
        assets.forget_loaded_files();
        let (_, sized) = record_lookups(|| {
            init_helpers::bind_rules_art_metadata(
                &mut projection,
                rules.fixed_art(),
                &assets,
                extension,
                theater,
            )
        });
        let roots = crate::map::resolved_terrain::scheduler_anim_roots(&projection, &overlays, &[]);
        let (_, catalogs) = record_lookups(|| {
            init_helpers::bind_rules_catalog_assets(
                &mut projection,
                &roots,
                &script_roots,
                &assets,
                extension,
                theater,
                crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
            )
        });
        let (_, glows) = record_lookups(|| {
            init_helpers::bind_rules_alpha_images(&mut projection, &overlays, &assets)
        });
        let (_, placed) = record_lookups(|| {
            crate::rules::effect_asset_catalog::resolved_overlay_shp_ids(
                &overlays, &assets, extension, theater,
            )
        });
        let retail = RetailReads::new(&assets);
        let reads = [
            (sized, Reader::FrameSize),
            (catalogs, Reader::Catalog),
            (glows, Reader::Glow),
            (placed, Reader::Overlay),
        ];
        for (names, reader) in reads {
            for name in names {
                let Some(used) = assets.resolve_ref(&name) else {
                    continue;
                };
                if shipped_source(used.source_archive) {
                    continue;
                }
                let values = read_values(&name, Some(used.bytes), reader);
                let original = retail.read(&name, &[]).map(|(_, bytes)| bytes);
                if values == read_values(&name, original, reader) {
                    continue;
                }
                let file = game_folder_file(used.source_archive);
                if !found.files.contains(&file) {
                    found.files.push(file);
                }
                changed.insert((name, reader, values));
            }
        }
    }
    if !changed.is_empty() {
        let mut hash = crate::util::sha256::Sha256::new();
        hash.update(b"VERA20k art values v1\0");
        for (name, reader, values) in &changed {
            hash.update(name.as_bytes());
            hash.update(&[0, *reader as u8]);
            hash.update(&(values.len() as u64).to_le_bytes());
            hash.update(values);
        }
        found.digest = Some(crate::util::sha256::digest_hex(hash.finalize()));
    }
    log::info!(
        "Game-folder art that changes what the original game's matches read: {}",
        if found.files.is_empty() {
            "none".to_owned()
        } else {
            found.files.join(", ")
        }
    );
    Ok(found)
}

/// Which binder read a file, for the values [`read_values`] takes from it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Reader {
    /// The catalogs and animation bindings (`bind_rules_catalog_assets`).
    Catalog,
    /// `ArtRegistry::populate_anim_frame_dims`, which also takes an SHP's
    /// middle frame size.
    FrameSize,
    /// `AlphaImageCatalog::bind`: whether the SHP's canvas and first frame
    /// read.
    Glow,
    /// `resolved_overlay_shp_ids`: whether the SHP parses, which decides
    /// which overlays a map places.
    Overlay,
}

/// The values `reader` reads from the file `name`, as bytes; an absent file
/// (`None`) and one that does not parse have their own.
fn read_values(name: &str, bytes: Option<&[u8]>, reader: Reader) -> Vec<u8> {
    use crate::assets::shp_file::ShpFile;
    let Some(bytes) = bytes else {
        return vec![0];
    };
    let mut values = vec![1];
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    if extension.eq_ignore_ascii_case("VXL") {
        // VoxelAssetCatalog::bind: whether it loads, and the tilt half sizes
        // GetFLH reads; the drawn voxels are not read.
        match crate::assets::vxl_file::VxlFile::from_bytes(bytes) {
            Ok(vxl) => {
                values.push(1);
                let sizes = crate::rules::voxel_asset_catalog::voxel_tilt_half_sizes(&vxl);
                let sizes = sizes.map(|sizes| sizes.map(f32::to_le_bytes).concat());
                push_optional(&mut values, sizes);
            }
            Err(_) => values.push(0),
        }
    } else if extension.eq_ignore_ascii_case("HVA") {
        // VoxelAssetCatalog::bind: whether it loads.
        values.push(u8::from(
            crate::assets::hva_file::HvaFile::from_bytes(bytes).is_ok(),
        ));
    } else if extension.eq_ignore_ascii_case("PAL") {
        // RuleSet::load_extra_building_palette: whether it loads.
        values.push(u8::from(
            crate::assets::pal_file::Palette::from_bytes(bytes).is_ok(),
        ));
    } else if reader == Reader::Glow {
        // AlphaImage::from_shp: whether its canvas and first frame read; the
        // glow's size and pixels only draw.
        values.push(u8::from(
            ShpFile::canvas_size_from_bytes(bytes).is_ok()
                && ShpFile::frame_bounds_from_bytes(bytes, 0).is_ok(),
        ));
    } else if reader == Reader::Overlay {
        // effect_asset_catalog::load_overlay_shape: whether it parses.
        values.push(u8::from(ShpFile::from_bytes(bytes).is_ok()));
    } else {
        // An SHP. The effect and terrain catalogs read its checked frame
        // count (`ShpFile::frame_count_from_bytes`); the animation, build-up
        // and rubble ones the raw header words at 4 (height) and 6 (frame
        // count). The terrain catalog's frame-0 radar color draws the radar
        // only.
        let count = ShpFile::frame_count_from_bytes(bytes).ok();
        push_optional(&mut values, count.map(u16::to_le_bytes));
        for offset in [4, 6] {
            push_optional(&mut values, bytes.get(offset..offset + 2));
        }
        if reader == Reader::FrameSize {
            let size = ShpFile::from_bytes(bytes).ok().and_then(|shp| {
                let frame = shp.frames.get(shp.frames.len() / 2)?;
                Some(
                    [frame.frame_width, frame.frame_height]
                        .map(u16::to_le_bytes)
                        .concat(),
                )
            });
            push_optional(&mut values, size);
        }
    }
    values
}

fn push_optional(values: &mut Vec<u8>, value: Option<impl AsRef<[u8]>>) {
    match value {
        Some(value) => {
            values.push(1);
            values.extend_from_slice(value.as_ref());
        }
        None => values.push(0),
    }
}
