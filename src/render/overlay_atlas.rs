//! Overlay sprite atlas — loads overlay/terrain SHP sprites into a packed GPU texture.
//!
//! At map load time, all overlay entries (from [OverlayPack]) and terrain objects
//! (from [Terrain]) are collected. Each unique (name, frame) combination has its
//! SHP frame rendered to RGBA and shelf-packed into a single GPU texture atlas.
//!
//! Follows the same pattern as sprite_atlas.rs and unit_atlas.rs.
//!
//! ## Dependency rules
//! - Part of render/ — depends on assets/ (SHP/Palette), render/batch (GPU upload).
//! - Reads overlay data from map/ (OverlayEntry, TerrainObject).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::map::overlay::{OverlayEntry, TerrainObject};
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::render::overlay_assets::resolve_overlay_name_for_render;
use crate::rules::art_data::{self, ArtRegistry};
use crate::rules::crate_rules::CrateRules;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::{
    OverlayTypeFlags, OverlayTypeRegistry, is_bridge_overlay_index, is_high_bridge_index,
    native_mark_overlay_data,
};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_object_type::TerrainObjectType;
use crate::rules::tiberium_type::TiberiumTypeRegistry;

#[path = "overlay_zshape.rs"]
mod zshape;
pub(crate) use zshape::OverlayShapeRequest;
use zshape::OverlayZShapes;

/// Padding between sprites in the atlas to prevent texture bleeding.
const SPRITE_PADDING: u32 = 1;

/// Connectivity frames per wall damage stage.
///
/// A wall cell's render frame is the raw overlay-data byte
/// `damage_level << 4 | connectivity_bitmask`, so every damage stage owns its
/// own block of 16 neighbour-connection frames.
const WALL_CONNECTIVITY_FRAMES: u32 = 16;

/// Upper bound on any preloaded overlay frame range.
///
/// The cell's overlay-data byte is a `u8`, so no frame above 255 is
/// addressable no matter how many damage stages art.ini declares.
const MAX_OVERLAY_FRAME_COUNT: u32 = 256;

/// Body frame drawn for `Crate=yes` overlays.
///
/// The native overlay-body draw takes a dedicated crate branch that forces the
/// frame to 0 instead of reading the cell's overlay-data byte.
pub const CRATE_BODY_FRAME: u8 = 0;

/// Count of body frames a wall overlay type can request at runtime.
///
/// `DamageLevels=` (art.ini) is the number of damage stages the sim can step
/// through, and each stage spans a full 16-frame connectivity block. Preloading
/// only the first block is what makes a freshly scratched wall miss the atlas
/// and fall back to frame 0 — a pristine, fully disconnected post.
fn wall_body_frame_count(damage_levels: u16) -> u32 {
    u32::from(damage_levels.max(1))
        .saturating_mul(WALL_CONNECTIVITY_FRAMES)
        .min(MAX_OVERLAY_FRAME_COUNT)
}

/// Leading SHP frames that are body art rather than shadow stencils.
///
/// Wall and bridge SHPs mirror every body frame with a 1-bit shadow stencil in
/// the second half of the file (GAWALL.SHP: 48 body + 48 shadow). Resolving a
/// frame out of the shadow half would blit a flat silhouette as the wall body.
///
/// The wall arm is VERA-internal with the gamemd equivalent UNCHECKED: the
/// native overlay draw indexes the cell's overlay byte straight into the shape
/// with no such cap. It cannot fire on stock content — every wall type's
/// `16 * DamageLevels` reachable range lands inside its SHP's body half — so it
/// is a guard against modded art/rules disagreeing, not a behaviour the engine
/// relies on.
fn body_frame_count(flags: &OverlayTypeFlags, total_frames: usize) -> usize {
    if flags.bridge_deck || flags.wall {
        total_frames / 2
    } else {
        total_frames
    }
}

/// SHP frame an overlay cell draws, or `None` when the cell draws nothing.
///
/// The cell's overlay-data byte IS the shape frame. Overlay draw:
/// `CellClass__DrawOverlay_Body @ 0x0047F6A0` reads `Cell+0x11E` and hands it to
/// the shape blitter unchanged on every non-Tiberium branch — no clamp, no
/// search for a populated frame. Nothing upstream normalises that byte either:
/// map load `ReadMapOverlayPacks @ 0x005FD2E0` pass 2 writes the decoded
/// `[OverlayDataPack]` value straight into `Cell+0x11E` for every in-bounds
/// cell, after pass 1's overlay construction. A zero-size frame therefore blits
/// nothing, and that silence is load-bearing:
/// low-bridge SHPs carry art only in frame 1, and a low bridge's flanking cells
/// (overlay data 0 and 2) are authored to draw nothing while the middle cell's
/// single wide sprite covers the row. Substituting the nearest populated frame
/// here painted a full deck onto all three columns.
///
/// The out-of-range arm is VERA-internal with the gamemd equivalent UNCHECKED:
/// it pairs with `body_frame_count`'s shadow-half cap, and collapsing to frame 0
/// keeps a wall visible when modded art declares fewer body frames than
/// `DamageLevels` can reach.
fn resolve_body_frame(
    requested: u8,
    max_normal_frame: usize,
    frame_sizes: &[(u16, u16)],
) -> Option<usize> {
    let requested_idx: usize = requested as usize;
    let frame_idx: usize = if requested_idx < max_normal_frame {
        requested_idx
    } else {
        0
    };
    let (width, height) = frame_sizes.get(frame_idx).copied()?;
    (width > 0 && height > 0).then_some(frame_idx)
}

/// Namespace prefix for smudge atlas keys.
///
/// Smudges share the OverlayAtlas (single texture, single bind group) but are
/// keyed under this prefix so a SmudgeType named `CRATER01` cannot collide
/// with an overlay named `CRATER01` (modded content is the realistic
/// concern). All smudge insertions and lookups MUST go through `smudge_key()`
/// so the prefix can never drift between sides.
pub const SMUDGE_KEY_PREFIX: &str = "__smudge::";

/// Source identity for Terrain DrawIt's body frame, apart from overlay keys:
/// a TerrainType draws its own image even when an OverlayType shares its name.
pub fn terrain_body_key(name: &str, frame: i32) -> OverlaySpriteKey {
    OverlaySpriteKey {
        name: format!("__terrain::{}", name.to_uppercase()),
        frame,
    }
}

/// Separate source identity for Terrain DrawIt's paired second-half stencil.
pub fn terrain_shadow_key(name: &str, frame: i32) -> OverlaySpriteKey {
    OverlaySpriteKey {
        name: format!("__terrain_shadow::{}", name.to_uppercase()),
        frame,
    }
}

/// Separate source identity for a destroyed building's rubble frame, keyed by
/// the SHP file its type's image resolved to
/// ([`crate::rules::building_rubble::RubbleArt`]).
pub fn rubble_key(shp: &str, frame: u16) -> OverlaySpriteKey {
    OverlaySpriteKey {
        name: rubble_image_name(shp),
        frame: i32::from(frame as u8),
    }
}

/// Shared image namespace for rubble body and Cell shadow atlases.
pub(crate) fn rubble_image_name(shp: &str) -> String {
    format!("__rubble::{}", shp.to_uppercase())
}

/// Build the canonical OverlayAtlas key for a smudge SHP.
///
/// Frame is always 0 — the per-cell draw of every multi-cell smudge footprint
/// uses frame 0; the cell offset within W×H is a screen-position shift, not a
/// frame index.
pub fn smudge_key(name: &str) -> OverlaySpriteKey {
    OverlaySpriteKey {
        name: format!("{}{}", SMUDGE_KEY_PREFIX, name.to_uppercase()),
        frame: 0,
    }
}

/// Cache key: unique combination of overlay name and frame index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OverlaySpriteKey {
    /// Overlay or terrain object name (e.g., "GEM01", "INTREE01").
    pub name: String,
    /// Terrain DrawIt71C208 retains the signed Stage dword. Ordinary overlay
    /// callers still supply their unchanged raw data byte (47F6A0).
    pub frame: i32,
}

/// Atlas keys reachable when a map-pack low bridge mutates its live overlay
/// identity. The data byte is preserved by every damage/repair writer, so only
/// data values actually seeded by low-bridge cells (plus frame zero fallback)
/// are reachable; every registered low-bridge identity can be selected later.
fn runtime_low_bridge_sprite_keys(
    overlays: &[OverlayEntry],
    overlay_registry: &OverlayTypeRegistry,
) -> HashSet<OverlaySpriteKey> {
    let mut frames: HashSet<u8> = overlays
        .iter()
        .filter(|entry| {
            is_bridge_overlay_index(entry.overlay_id) && !is_high_bridge_index(entry.overlay_id)
        })
        .map(|entry| entry.frame)
        .collect();
    if frames.is_empty() {
        return HashSet::new();
    }
    frames.insert(0);

    let mut keys = HashSet::new();
    for overlay_id in 0u8..=u8::MAX {
        if !is_bridge_overlay_index(overlay_id) || is_high_bridge_index(overlay_id) {
            continue;
        }
        let Some(name) = resolve_overlay_name_for_render(overlay_registry, overlay_id) else {
            continue;
        };
        for &frame in &frames {
            keys.insert(OverlaySpriteKey {
                name: name.clone(),
                frame: i32::from(frame),
            });
        }
    }
    keys
}

/// Atlas keys reachable through the native flat/sloped resource selector.
///
/// Active YR `CellClass__DrawOverlay_Body @ 0x0047F6A0` may draw any of the
/// TiberiumClass's12 primary and8 extra images (no extras for Image2) while
/// retaining the Cell data byte. Preload the same range owner the display
/// selector uses, including images absent from the current map pack.
fn runtime_tiberium_sprite_keys(
    overlay_registry: &OverlayTypeRegistry,
    tiberium_types: &TiberiumTypeRegistry,
) -> HashSet<OverlaySpriteKey> {
    let mut keys = HashSet::new();
    for ty in tiberium_types.types() {
        let variant_ids = overlay_registry.tiberium_display_variant_ids(ty);
        for overlay_id in variant_ids {
            let Some(name) = overlay_registry.name(overlay_id).map(str::to_owned) else {
                continue;
            };
            for frame in 0..ty.max_density {
                keys.insert(OverlaySpriteKey {
                    name: name.clone(),
                    frame: i32::from(frame),
                });
            }
        }
    }
    keys
}

/// Sprite keys reachable when scenario-start placement selects one of the
/// three resolved `[CrateRules]` identities. The selected type need not itself
/// declare `Crate=yes`: native allocates the name first, then Mark uses that
/// type's ordinary data rule and the draw path follows its actual flags.
fn runtime_crate_sprite_keys(
    overlay_registry: &OverlayTypeRegistry,
    crate_rules: &CrateRules,
) -> HashSet<OverlaySpriteKey> {
    let selected_ids: HashSet<u8> = [
        crate_rules.wood_crate_img.as_deref(),
        crate_rules.crate_img.as_deref(),
        crate_rules.water_crate_img.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter_map(|name| overlay_registry.id_for_name(name))
    .collect();
    let mut keys = HashSet::new();
    for overlay_id in 0u8..=u8::MAX {
        let Some(flags) = overlay_registry.flags(overlay_id) else {
            continue;
        };
        if flags.crate_type
            && crate::map::bridge_facts::high_bridge_stamp_for_overlay(overlay_id).is_none()
            && let Some(name) = resolve_overlay_name_for_render(overlay_registry, overlay_id)
        {
            keys.insert(OverlaySpriteKey {
                name,
                frame: i32::from(CRATE_BODY_FRAME),
            });
        }
    }

    for selected_id in selected_ids {
        let Some(flags) = overlay_registry.flags(selected_id) else {
            continue;
        };
        // High-bridge bodies and their shadow halves are drawn exclusively by
        // BridgeAtlas. Rooting 256 ordinary OverlayAtlas frames here cannot
        // make that production draw path reachable.
        if crate::map::bridge_facts::high_bridge_stamp_for_overlay(selected_id).is_some() {
            continue;
        }
        let mut reachable: Vec<(u8, std::ops::RangeInclusive<u8>)> = Vec::new();
        match selected_id {
            // Active-YR low endpoint Mark replaces the trigger with one fixed
            // identity and may fill four body variants, all with states 0/1/2.
            0x7A => {
                reachable.push((0x5C, 0..=2));
                for id in 0x4A..=0x4D {
                    reachable.push((id, 0..=2));
                }
            }
            0x7B => {
                reachable.push((0x5E, 0..=2));
                for id in 0x4A..=0x4D {
                    reachable.push((id, 0..=2));
                }
            }
            0x7C => {
                reachable.push((0x60, 0..=2));
                for id in 0x53..=0x56 {
                    reachable.push((id, 0..=2));
                }
            }
            0x7D => {
                reachable.push((0x62, 0..=2));
                for id in 0x53..=0x56 {
                    reachable.push((id, 0..=2));
                }
            }
            0xE9 => {
                reachable.push((0xDF, 0..=2));
                for id in 0xCD..=0xD0 {
                    reachable.push((id, 0..=2));
                }
            }
            0xEA => {
                reachable.push((0xE1, 0..=2));
                for id in 0xCD..=0xD0 {
                    reachable.push((id, 0..=2));
                }
            }
            0xEB => {
                reachable.push((0xE3, 0..=2));
                for id in 0xD6..=0xD9 {
                    reachable.push((id, 0..=2));
                }
            }
            0xEC => {
                reachable.push((0xE5, 0..=2));
                for id in 0xD6..=0xD9 {
                    reachable.push((id, 0..=2));
                }
            }
            _ if flags.wall => {
                // The general wall preload below owns the complete frame set.
            }
            _ if flags.crate_type => {
                // Crate=yes stores 0xFF but the draw path selects the crate
                // body frame; the general crate preload above owns that key.
            }
            _ if flags.land == crate::rules::terrain_rules::LandType::Railroad => {
                reachable.push((selected_id, 0..=0));
            }
            // Mark's Land-5 arm (`0x005FD0D9`) germinates a resource type's
            // density from its neighbours (`0x004818E0`).
            _ if flags.land == crate::rules::terrain_rules::LandType::Tiberium
                && flags.tiberium =>
            {
                reachable.push((selected_id, 0..=11));
            }
            _ => {
                reachable.push((
                    selected_id,
                    native_mark_overlay_data(flags)..=native_mark_overlay_data(flags),
                ));
            }
        }
        for (overlay_id, frames) in reachable {
            let Some(name) = resolve_overlay_name_for_render(overlay_registry, overlay_id) else {
                continue;
            };
            for frame in frames {
                keys.insert(OverlaySpriteKey {
                    name: name.clone(),
                    frame: i32::from(frame),
                });
            }
        }
    }
    keys
}

/// UV and offset data for one overlay sprite within the atlas.
#[derive(Debug, Clone, Copy)]
pub struct OverlaySpriteEntry {
    /// Top-left UV coordinate in the atlas (0.0..1.0).
    pub uv_origin: [f32; 2],
    /// UV width and height (0.0..1.0).
    pub uv_size: [f32; 2],
    /// Sprite dimensions in pixels.
    pub pixel_size: [f32; 2],
    /// X offset from the cell center to the sprite's top-left corner.
    pub offset_x: f32,
    /// Y offset from the cell center to the sprite's top-left corner.
    pub offset_y: f32,
    /// Ordinary overlay crop/centering to CC_Draw_Shape's point. Bridge and
    /// terrain proxies do not consume optional overlay shape metadata.
    pub draw_point_offset: [i32; 2],
    /// Only the extended ordinary SHP walker consumes optional depth bytes.
    pub extended: bool,
}

/// A GPU texture atlas containing pre-rendered overlay sprites.
pub struct OverlayAtlas {
    /// The packed GPU texture.
    pub texture: BatchTexture,
    /// Lookup: (name, frame) → UV rectangle + offset data.
    entries: HashMap<OverlaySpriteKey, OverlaySpriteEntry>,
    zshapes: Option<OverlayZShapes>,
}

impl OverlayAtlas {
    pub(crate) fn apply_zshape(
        &self,
        request: OverlayShapeRequest,
        sprite: &OverlaySpriteEntry,
        camera: [f32; 2],
        viewport: [f32; 2],
        instance: &mut crate::render::batch::SpriteInstance,
    ) -> bool {
        self.zshapes.as_ref().is_none_or(|shapes| {
            shapes.apply(
                request,
                sprite.draw_point_offset,
                sprite.extended,
                camera,
                viewport,
                instance,
            )
        })
    }

    pub(crate) fn zshape_bind_group(&self) -> Option<&wgpu::BindGroup> {
        self.zshapes.as_ref().map(|shapes| &shapes.bind_group)
    }
    /// Look up the atlas entry for a given (name, frame) pair.
    pub fn get(&self, key: &OverlaySpriteKey) -> Option<&OverlaySpriteEntry> {
        self.entries.get(key)
    }

    /// Terrain body frame `frame` and its second-half stencil from one SHP.
    /// Either is `None` when its frame is missing or empty and draws nothing.
    pub fn terrain_pieces(
        &self,
        name: &str,
        frame: i32,
    ) -> (Option<&OverlaySpriteEntry>, Option<&OverlaySpriteEntry>) {
        (
            self.get(&terrain_body_key(name, frame)),
            self.get(&terrain_shadow_key(name, frame)),
        )
    }

    /// Number of unique sprites in the atlas.
    pub fn sprite_count(&self) -> usize {
        self.entries.len()
    }
}

/// Intermediate rendered sprite before atlas packing.
struct RenderedOverlay {
    key: OverlaySpriteKey,
    rgba: Vec<u8>,
    source_indices: Vec<u8>,
    width: u32,
    height: u32,
    offset_x: f32,
    offset_y: f32,
    draw_point_offset: [i32; 2],
    extended: bool,
}

/// Build an overlay sprite atlas from map overlay/terrain data.
///
/// Loads SHP sprites for each unique overlay type and packs into a GPU texture.
/// Returns None if no overlays can be rendered.
pub fn build_overlay_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    overlays: &[OverlayEntry],
    terrain_objects: &[TerrainObject],
    extra_terrain_types: &[String],
    asset_manager: &AssetManager,
    theater_palette: &Palette,
    unit_palette: &Palette,
    tiberium_palette: &Palette,
    theater_ext: &str,
    theater_name: &str,
    overlay_registry: &OverlayTypeRegistry,
    rules: &RuleSet,
    rules_ini: &IniFile,
    smudge_types: Option<&crate::rules::smudge_type::SmudgeTypeRegistry>,
    rubble_arts: &[crate::rules::building_rubble::RubbleArt],
) -> Option<OverlayAtlas> {
    build_overlay_atlas_on_device(
        &gpu.device,
        &gpu.queue,
        batch,
        overlays,
        terrain_objects,
        extra_terrain_types,
        asset_manager,
        theater_palette,
        unit_palette,
        tiberium_palette,
        theater_ext,
        theater_name,
        overlay_registry,
        rules,
        rules_ini,
        smudge_types,
        rubble_arts,
    )
}

/// Device-only entry to the same retail atlas owner; no surface is needed to
/// resolve and upload overlay art for a bounded offscreen production witness.
pub(crate) fn build_overlay_atlas_on_device(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    overlays: &[OverlayEntry],
    terrain_objects: &[TerrainObject],
    extra_terrain_types: &[String],
    asset_manager: &AssetManager,
    theater_palette: &Palette,
    unit_palette: &Palette,
    tiberium_palette: &Palette,
    theater_ext: &str,
    theater_name: &str,
    overlay_registry: &OverlayTypeRegistry,
    rules: &RuleSet,
    rules_ini: &IniFile,
    smudge_types: Option<&crate::rules::smudge_type::SmudgeTypeRegistry>,
    rubble_arts: &[crate::rules::building_rubble::RubbleArt],
) -> Option<OverlayAtlas> {
    let tiberium_types = &rules.tiberium_types;
    let crate_rules = &rules.crate_rules;
    let art_registry = rules.art();
    // Collect unique (name, frame) pairs from overlays.
    let mut needed: HashSet<OverlaySpriteKey> = HashSet::new();

    for entry in overlays {
        if let Some(mapped_name) =
            resolve_overlay_name_for_render(overlay_registry, entry.overlay_id)
        {
            needed.insert(OverlaySpriteKey {
                name: mapped_name.clone(),
                frame: i32::from(entry.frame),
            });
            // Also include frame 0 as fallback.
            needed.insert(OverlaySpriteKey {
                name: mapped_name,
                frame: 0,
            });
        }
    }

    let low_bridge_keys = runtime_low_bridge_sprite_keys(overlays, overlay_registry);
    if !low_bridge_keys.is_empty() {
        log::info!(
            "Pre-loading {} live low-bridge overlay variant(s)",
            low_bridge_keys.len()
        );
        needed.extend(low_bridge_keys);
    }

    let tiberium_keys = runtime_tiberium_sprite_keys(overlay_registry, tiberium_types);
    if !tiberium_keys.is_empty() {
        log::info!(
            "Pre-loading {} tiberium display frame(s)",
            tiberium_keys.len()
        );
        needed.extend(tiberium_keys);
    }

    // Overlay types the sim can create or mutate after this atlas is built are
    // not discoverable from the map's [OverlayPack], so preload every frame
    // they can reach:
    //
    // - Walls: player-built segments use any connectivity bitmask, and combat
    //   damage steps the high nibble, so the reachable range is
    //   `DamageLevels * 16`, not just the first connectivity block.
    // - Crates: scenario-start and goodie crates are placed into the live
    //   overlay grid well after map load, so no crate ever appears in the map
    //   pack. The native crate branch always draws frame 0.
    let mut wall_names_loaded: HashSet<String> = HashSet::new();
    let mut wall_frames_loaded: u32 = 0;
    for overlay_id in 0u8..=u8::MAX {
        let Some(flags) = overlay_registry.flags(overlay_id) else {
            continue;
        };
        if !flags.wall {
            continue;
        }
        let Some(mapped_name) = resolve_overlay_name_for_render(overlay_registry, overlay_id)
        else {
            continue;
        };
        if wall_names_loaded.insert(mapped_name.clone()) {
            let frame_count: u32 = wall_body_frame_count(flags.damage_levels);
            wall_frames_loaded += frame_count;
            for frame in 0..frame_count {
                needed.insert(OverlaySpriteKey {
                    name: mapped_name.clone(),
                    frame: frame as i32,
                });
            }
        }
    }
    let crate_keys = runtime_crate_sprite_keys(overlay_registry, crate_rules);
    let crate_names_loaded: HashSet<String> =
        crate_keys.iter().map(|key| key.name.clone()).collect();
    needed.extend(crate_keys);
    if !wall_names_loaded.is_empty() {
        log::info!(
            "Pre-loaded {} damage/connectivity frames for {} wall type(s): {:?}",
            wall_frames_loaded,
            wall_names_loaded.len(),
            wall_names_loaded,
        );
    }
    if !crate_names_loaded.is_empty() {
        log::info!(
            "Pre-loaded crate overlay body frame for {:?}",
            crate_names_loaded,
        );
    }

    // Mirage defaults are registered TerrainTypes even when no map Terrain
    // object uses them. Type/image loading must precede the first disguise.
    let mut terrain_names: Vec<&str> = Vec::new();
    for name in terrain_objects
        .iter()
        .map(|object| object.name.as_str())
        .chain(extra_terrain_types.iter().map(String::as_str))
    {
        if !terrain_names.contains(&name) {
            terrain_names.push(name);
        }
    }

    if needed.is_empty() && terrain_names.is_empty() {
        log::info!("No overlay/terrain sprites needed — skipping overlay atlas");
        return None;
    }

    log::info!(
        "Building overlay atlas for {} unique (name, frame) pairs",
        needed.len()
    );

    // Render each unique sprite.
    let mut rendered: Vec<RenderedOverlay> = Vec::with_capacity(needed.len());
    let mut load_fail_count: u32 = 0;

    for key in &needed {
        let flags: OverlayTypeFlags = overlay_registry
            .flags_by_name(&key.name)
            .cloned()
            .unwrap_or_default();
        // Palette selection:
        // - Tiberium overlays → tiberium palette (e.g., temperat.pal), NO remap
        // - Walls/veins/veinhole → unit palette
        // - Everything else → theater palette
        let palette: &Palette = if flags.tiberium {
            tiberium_palette
        } else if flags.wall || flags.is_veins || flags.is_veinhole_monster {
            unit_palette
        } else {
            theater_palette
        };
        match render_overlay_sprite(
            asset_manager,
            palette,
            key,
            theater_ext,
            theater_name,
            rules_ini,
            art_registry,
            &flags,
        ) {
            Some(sprite) => rendered.push(sprite),
            None => {
                load_fail_count += 1;
                // Only log at debug level — some overlay types (e.g., CYCL)
                // are unused RA1 remnants with no backing SHP file.
                let image_id: String = art_registry.resolve_overlay_image_id(&key.name, rules_ini);
                let candidates: Vec<String> = art_data::overlay_shp_candidates(
                    Some(art_registry),
                    &key.name,
                    &image_id,
                    theater_ext,
                    theater_name,
                );
                log::debug!(
                    "Overlay sprite not found: name={} frame={} (tried: {:?})",
                    key.name,
                    key.frame,
                    candidates,
                );
            }
        }
    }

    log::info!(
        "Overlay sprites: {} rendered, {} failed to load (of {} needed)",
        rendered.len(),
        load_fail_count,
        needed.len()
    );

    // --- Terrain objects ---
    // SpawnsTiberium trees use the unit palette; their draw point carries
    // Terrain DrawIt's 16-pixel lift.
    let overlay_sprite_count = rendered.len();
    for &name in &terrain_names {
        let Some(terrain_type) = rules.terrain_object_type_case_insensitive(name) else {
            continue;
        };
        let Some(shape_filename) = rules.terrain_shape_filename(name) else {
            continue;
        };
        let palette = if terrain_type.spawns_tiberium {
            unit_palette
        } else {
            theater_palette
        };
        rendered.extend(render_terrain_sprites(
            asset_manager,
            palette,
            terrain_type,
            shape_filename,
        ));
    }
    log::info!(
        "Terrain sprites: {} for {} type(s)",
        rendered.len() - overlay_sprite_count,
        terrain_names.len()
    );

    // --- Smudge SHPs ---
    // Smudges share this atlas (single texture / single bind group) but are
    // keyed under SMUDGE_KEY_PREFIX to keep the namespace collision-free with
    // overlays. Rendered with the iso theater palette to match the world pass
    // shading (smudges draw between terrain and overlays in the same pass).
    let mut smudge_rendered_count: u32 = 0;
    let mut smudge_failed_count: u32 = 0;
    if let Some(smudge_reg) = smudge_types {
        for (_id, def) in smudge_reg.iter_with_id() {
            let file_basename: &str = def.image_name.as_deref().unwrap_or(&def.name);
            match render_smudge_sprite(
                asset_manager,
                theater_palette,
                &def.name,
                file_basename,
                theater_ext,
            ) {
                Some(sprite) => {
                    rendered.push(sprite);
                    smudge_rendered_count += 1;
                }
                None => {
                    smudge_failed_count += 1;
                    let candidates: Vec<String> = smudge_shp_candidates(file_basename, theater_ext);
                    log::debug!(
                        "Smudge sprite not found: name={} (tried: {:?})",
                        def.name,
                        candidates,
                    );
                }
            }
        }
        log::info!(
            "Smudge sprites: {} rendered, {} failed (of {} types)",
            smudge_rendered_count,
            smudge_failed_count,
            smudge_reg.len(),
        );
    }

    // --- Building rubble ---
    // `CellClass::DrawOverlay_Body @ 0x0047F6A0`'s IsRubble arm blits the
    // destroyed type's own image frame through the cell's ordinary Convert, so
    // it shares the ordinary overlay palette and has no type Y bias.
    let mut rubble_loaded: HashSet<OverlaySpriteKey> = HashSet::new();
    for art in rubble_arts {
        let key = rubble_key(&art.shp, art.body_frame);
        if rubble_loaded.contains(&key) {
            continue;
        }
        let Some(shp) = asset_manager
            .get_ref(&art.shp)
            .and_then(|data| ShpFile::from_bytes(data).ok())
        else {
            continue;
        };
        let Some(sprite) = render_decoded_overlay_frame(
            &shp,
            theater_palette,
            key.clone(),
            usize::from(art.body_frame),
            0.0,
        )
        .filter(|sprite| sprite.width > 0 && sprite.height > 0) else {
            continue;
        };
        rendered.push(sprite);
        rubble_loaded.insert(key);
    }
    if !rubble_loaded.is_empty() {
        log::info!("Rubble sprites: {} rendered", rubble_loaded.len());
    }

    if rendered.is_empty() {
        return None;
    }

    let mut atlas = pack_overlay_sprites(device, queue, batch, &rendered);
    atlas.zshapes = OverlayZShapes::load(device, queue, batch, asset_manager, theater_ext);
    Some(atlas)
}

/// Load and render a single overlay SHP sprite to RGBA pixels.
///
/// The first candidate file that loads is the image, even when every frame is
/// empty: `CellClass::DrawOverlay_Body` (0x47F6A0) draws the shape
/// `OverlayTypeClass::GetImage` (0x5FEDE0) reads from the type's own image
/// name. No other file stands in, so retail's empty LOBRDG27/LOBRDB27 rows
/// draw nothing.
fn render_overlay_sprite(
    asset_manager: &AssetManager,
    palette: &Palette,
    key: &OverlaySpriteKey,
    theater_ext: &str,
    theater_name: &str,
    rules_ini: &IniFile,
    art_registry: &ArtRegistry,
    flags: &OverlayTypeFlags,
) -> Option<RenderedOverlay> {
    let image_id: String = art_registry.resolve_overlay_image_id(&key.name, rules_ini);
    let mut candidates: Vec<String> = art_data::overlay_shp_candidates(
        Some(art_registry),
        &key.name,
        &image_id,
        theater_ext,
        theater_name,
    );
    // Debug override: force all tiberium overlays to render using one chosen image
    // (e.g. TIB01/TIB02/TIB03) to quickly validate sprite selection issues.
    if flags.tiberium
        && let Some(forced_name) = forced_tiberium_image_name()
    {
        candidates.insert(
            0,
            format!("{}.{}", forced_name.to_ascii_lowercase(), theater_ext),
        );
        candidates.insert(1, format!("{}.shp", forced_name.to_ascii_lowercase()));
        candidates.insert(2, format!("{}.{}", forced_name, theater_ext));
        candidates.insert(3, format!("{}.shp", forced_name));
    }

    // Tiberium overlays now use the dedicated tiberium palette (e.g., temperat.pal)
    // which already has correct ore colors at all indices — no remap needed.
    // Walls use the unit palette with default remap range colors.

    let mut found_name: String = String::new();
    let mut shp_opt: Option<ShpFile> = None;
    for name in &candidates {
        let Some(data) = asset_manager.get_ref(name) else {
            continue;
        };
        let Ok(shp) = ShpFile::from_bytes(data) else {
            continue;
        };
        found_name = name.clone();
        shp_opt = Some(shp);
        break;
    }
    let shp: ShpFile = shp_opt?;
    log::trace!("Overlay sprite {} uses {}", key.name, found_name);

    if shp.frames.is_empty() {
        return None;
    }

    // Bridge and wall SHPs contain shadow frames in the second half (bridge:
    // frames 18-35 of 36; GAWALL: frames 48-95 of 96). Cap to the normal
    // (non-shadow) range so we never render a shadow stencil as the body.
    let max_normal_frame: usize = body_frame_count(flags, shp.frames.len());

    // Select frame:
    // High bridge overlays (BRIDGE1/2, BRIDGEB1/2) share one SHP file
    // (bridge.tem / bridgb.tem). The OverlayDataPack frame value already
    // encodes the direction: frames 0-8 = EW, frames 9-17 = NS.
    // No additional offset is needed — the map data handles it.
    let frame_sizes: Vec<(u16, u16)> = shp
        .frames
        .iter()
        .map(|fr| (fr.frame_width, fr.frame_height))
        .collect();
    let frame_idx: usize = resolve_body_frame(
        u8::try_from(key.frame).ok()?,
        max_normal_frame,
        &frame_sizes,
    )?;
    render_decoded_overlay_frame(&shp, palette, key.clone(), frame_idx, flags.y_draw_offset())
}

/// Body and shadow sprites of one TerrainType for every frame Terrain DrawIt
/// (0x71C1B0) can pick: each stage of an IsAnimated type, else frame 0 and the
/// frame 1 it draws below 2 health. Frame F's shadow is frame F + count/2.
///
/// Both draw whatever their SHP format: CC_Draw_Shape (0x4AED70) sends an
/// uncompressed frame through Bit_Blit_ClipRects (0x4373B0) to the same Z
/// read/write and darken blitters as the RLE path. One unported difference:
/// that walker seeds the body's bottom-seeded gradient from the clipped
/// rectangle, so an uncompressed body cut by the screen's bottom edge gets a
/// slightly different Z. A missing or empty frame draws nothing, as
/// SHPStruct::GetFrameBounds (0x69E7E0) gives CC_Draw_Shape an empty rectangle.
/// The rules asset owner supplies the resident shape selected by original
/// TerrainTypeClass::ReadINI (0x71DEA0) and InitTheater (0x71DCA0).
fn render_terrain_sprites(
    asset_manager: &AssetManager,
    palette: &Palette,
    terrain_type: &TerrainObjectType,
    shape_filename: &str,
) -> Vec<RenderedOverlay> {
    let Some(shp) = asset_manager
        .get_ref(shape_filename)
        .and_then(|bytes| ShpFile::from_bytes(bytes).ok())
    else {
        log::debug!("Terrain image not found: {shape_filename}");
        return Vec::new();
    };
    let name = &terrain_type.name;
    let half = shp.frames.len() / 2;
    let frames = if terrain_type.is_animated {
        0..half.max(1)
    } else {
        0..2
    };
    let mut sprites = Vec::new();
    for frame in frames {
        // Terrain DrawIt71C208/71C344 passes the full Stage and Stage+half
        // to CC_Draw_Shape. The SHP count, not an OverlayData byte, bounds
        // this animated-spawner preload domain (AI71C87A).
        let stage = frame as i32;
        for (key, index) in [
            (terrain_body_key(name, stage), frame),
            (terrain_shadow_key(name, stage), frame + half),
        ] {
            if shp
                .frames
                .get(index)
                .is_some_and(|source| source.frame_width > 0 && source.frame_height > 0)
            {
                sprites.extend(render_decoded_overlay_frame(&shp, palette, key, index, 0.0));
            }
        }
    }
    sprites
}

/// Retain the literal palette index/stencil next to decoded RGB. No palette
/// color, including black, substitutes for source-zero transparency.
fn render_decoded_overlay_frame(
    shp: &ShpFile,
    palette: &Palette,
    key: OverlaySpriteKey,
    frame_idx: usize,
    y_offset: f32,
) -> Option<RenderedOverlay> {
    let frame = shp.frames.get(frame_idx)?;
    let (offset_x, offset_y) = stored_frame_offset(
        shp.width,
        shp.height,
        frame.frame_x,
        frame.frame_y,
        y_offset,
    );
    Some(RenderedOverlay {
        key,
        rgba: shp.frame_to_rgba(frame_idx, palette).ok()?,
        source_indices: frame.pixels.clone(),
        width: u32::from(frame.frame_width),
        height: u32::from(frame.frame_height),
        offset_x,
        offset_y,
        draw_point_offset: [
            i32::from(shp.width / 2) - i32::from(frame.frame_x),
            i32::from(shp.height / 2) - i32::from(frame.frame_y),
        ],
        extended: frame.format & 2 != 0,
    })
}

/// CC_Draw_Shape @ 0x004AED70 centers by the integer canvas half, then
/// adds the stored frame X/Y before passing its width/height to the walker.
/// Keep that cropped rectangle: padding a 43-row GAWALL frame to its 70-row
/// canvas changes the upright Z seed even though its color pixels stay put.
pub(crate) fn stored_frame_offset(
    canvas_width: u16,
    canvas_height: u16,
    frame_x: u16,
    frame_y: u16,
    y_offset: f32,
) -> (f32, f32) {
    (
        f32::from(frame_x) - f32::from(canvas_width / 2),
        f32::from(frame_y) - f32::from(canvas_height / 2) + y_offset,
    )
}

/// Build the candidate SHP filename list for a SmudgeType.
///
/// Theater-extension first, then `.shp` fallback. Lowercase variants too —
/// asset_manager treats names case-sensitively in some code paths, and SHP
/// files in retail mix archives are lowercase.
fn smudge_shp_candidates(name: &str, theater_ext: &str) -> Vec<String> {
    let upper = name.to_string();
    let lower = name.to_ascii_lowercase();
    vec![
        format!("{}.{}", lower, theater_ext),
        format!("{}.shp", lower),
        format!("{}.{}", upper, theater_ext),
        format!("{}.shp", upper),
    ]
}

fn forced_tiberium_image_name() -> Option<&'static str> {
    static FORCED: OnceLock<Option<String>> = OnceLock::new();
    FORCED
        .get_or_init(|| {
            std::env::var("RA2_FORCE_TIB_IMAGE")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
        .as_deref()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OverlayRadarColorSource {
    /// Cell47C060 bypasses the type image for Tiberium's CellAnim branch.
    Cell,
    /// Preview641140 calls OverlayTypeGetRadarColor5FED00 directly.
    OverlayType,
}

/// Read exact SHP-frame radar metadata for every runtime-addressable overlay.
///
/// `CellClass::GetRadarColor @ 0x0047C060` selects frame 1 for the two low-
/// bridge ranges and the cell data byte otherwise. `OverlayTypeClass::GetRadarColor
/// @ 0x005FED00` / `GetTiberiumRadarColor @ 0x0069E860` read bytes 0x0C..0x0E
/// from that frame header; rendered-pixel averages and palettes are not part of
/// this path. Tiberium is deliberately different: `OverlayTypeClass::ReadINI
/// @ 0x005FE770` proves `OverlayType+0x29C` is the separately resolved
/// `CellAnim=` AnimType, so `CellClass::GetRadarColor` never reads the ore/gem
/// overlay's own SHP here. Runtime writers can select a frame absent from the
/// initial map, so the client-local table retains every parsed source frame.
pub fn compute_overlay_radar_colors(
    asset_manager: &AssetManager,
    overlay_registry: &OverlayTypeRegistry,
    source: OverlayRadarColorSource,
    overlay_names: &BTreeMap<u8, String>,
    theater_ext: &str,
    theater_name: &str,
    rules_ini: &IniFile,
    art_registry: &ArtRegistry,
) -> HashMap<(u8, u8), [u8; 3]> {
    // Cache: overlay_id -> loaded ShpFile
    let mut shp_cache: HashMap<u8, ShpFile> = HashMap::new();
    for (&overlay_id, name) in overlay_names {
        let load = |candidates: Vec<String>| {
            candidates.iter().find_map(|candidate| {
                asset_manager
                    .get_ref(candidate)
                    .and_then(|data| ShpFile::from_bytes(data).ok())
            })
        };
        let flags = overlay_registry.flags(overlay_id);
        let is_tiberium = flags.is_some_and(|flags| flags.tiberium);
        let cell_anim = || {
            flags
                .and_then(|flags| flags.cell_anim.as_deref())
                .and_then(|anim| {
                    let image_id = art_registry.resolve_anim_image_id(anim);
                    load(art_data::anim_shp_candidates(
                        Some(art_registry),
                        anim,
                        &image_id,
                        theater_ext,
                        theater_name,
                    ))
                })
        };
        let shp = if source == OverlayRadarColorSource::Cell && is_tiberium {
            cell_anim()
        } else {
            let image_id = art_registry.resolve_overlay_image_id(name, rules_ini);
            load(art_data::overlay_shp_candidates(
                Some(art_registry),
                name,
                &image_id,
                theater_ext,
                theater_name,
            ))
            .or_else(cell_anim)
        };
        if let Some(shp) = shp {
            shp_cache.insert(overlay_id, shp);
        }
    }

    let mut result: HashMap<(u8, u8), [u8; 3]> = HashMap::new();
    for (&overlay_id, shp) in &shp_cache {
        for (frame, header) in shp.frames.iter().take(usize::from(u8::MAX) + 1).enumerate() {
            if let Ok(frame) = u8::try_from(frame) {
                // Black is data, not an absence sentinel (e.g. retail bridge.tem).
                let rgb = if source == OverlayRadarColorSource::Cell
                    && overlay_registry
                        .flags(overlay_id)
                        .is_some_and(|flags| flags.tiberium)
                {
                    header.radar_color
                } else {
                    crate::rules::overlay_types::overlay_radar_channel_order(
                        overlay_id,
                        header.radar_color,
                    )
                };
                result.insert((overlay_id, frame), rgb);
            }
        }
    }

    log::info!(
        "Loaded overlay SHP-header radar colors: {} entries from {} overlay IDs",
        result.len(),
        overlay_names.len(),
    );

    result
}

/// Load and render a single SmudgeType SHP frame 0 to RGBA pixels.
///
/// Smudges always render with the iso theater palette and use a
/// `(-full_w/2, -full_h/2)` anchor centered on the footprint-origin cell.
/// Multi-cell SmudgeTypes have a single composite SHP frame whose internal
/// `frame_x`/`frame_y` already places the visual correctly relative to the
/// canvas center.
fn render_smudge_sprite(
    asset_manager: &AssetManager,
    palette: &Palette,
    key_name: &str,
    file_basename: &str,
    theater_ext: &str,
) -> Option<RenderedOverlay> {
    let candidates: Vec<String> = smudge_shp_candidates(file_basename, theater_ext);

    let mut found_name: String = String::new();
    let mut shp_opt: Option<ShpFile> = None;
    for candidate in &candidates {
        let Some(data) = asset_manager.get_ref(candidate) else {
            continue;
        };
        let Ok(shp) = ShpFile::from_bytes(data) else {
            continue;
        };
        let has_drawable = shp
            .frames
            .iter()
            .any(|fr| fr.frame_width > 0 && fr.frame_height > 0);
        if !has_drawable {
            continue;
        }
        found_name = candidate.clone();
        shp_opt = Some(shp);
        break;
    }
    let shp: ShpFile = shp_opt?;
    log::trace!("Smudge sprite {} uses {}", key_name, found_name);

    if shp.frames.is_empty() {
        return None;
    }
    let frame = &shp.frames[0];
    if frame.frame_width == 0 || frame.frame_height == 0 {
        return None;
    }

    let frame_rgba: Vec<u8> = match shp.frame_to_rgba(0, palette) {
        Ok(rgba) => rgba,
        Err(_) => return None,
    };

    let full_w: u32 = shp.width as u32;
    let full_h: u32 = shp.height as u32;
    let mut full_rgba: Vec<u8> = vec![0u8; (full_w * full_h * 4) as usize];
    let mut full_indices = vec![0u8; (full_w * full_h) as usize];

    let fw: u32 = frame.frame_width as u32;
    let fh: u32 = frame.frame_height as u32;
    let fx: u32 = frame.frame_x as u32;
    let fy: u32 = frame.frame_y as u32;

    for y in 0..fh {
        let dst_y: u32 = fy + y;
        if dst_y >= full_h {
            break;
        }
        let src_off: usize = (y * fw * 4) as usize;
        let copy_w: u32 = fw.min(full_w.saturating_sub(fx));
        let dst_off: usize = ((dst_y * full_w + fx) * 4) as usize;
        let bytes: usize = (copy_w * 4) as usize;
        if src_off + bytes <= frame_rgba.len() && dst_off + bytes <= full_rgba.len() {
            full_rgba[dst_off..dst_off + bytes]
                .copy_from_slice(&frame_rgba[src_off..src_off + bytes]);
            let source = (y * fw) as usize;
            let destination = (dst_y * full_w + fx) as usize;
            full_indices[destination..destination + copy_w as usize]
                .copy_from_slice(&frame.pixels[source..source + copy_w as usize]);
        }
    }

    let offset_x: f32 = -(full_w as f32) / 2.0;
    let offset_y: f32 = -(full_h as f32) / 2.0;

    Some(RenderedOverlay {
        key: smudge_key(key_name),
        rgba: full_rgba,
        source_indices: full_indices,
        width: full_w,
        height: full_h,
        offset_x,
        offset_y,
        draw_point_offset: [0; 2],
        extended: false,
    })
}

#[cfg(test)]
#[path = "overlay_atlas_tibtre_tests.rs"]
mod tibtre_tests;

#[cfg(test)]
#[path = "overlay_atlas_mirage_tests.rs"]
mod mirage_tests;

/// Shelf-pack rendered overlay sprites into a GPU texture atlas.
fn pack_overlay_sprites(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    sprites: &[RenderedOverlay],
) -> OverlayAtlas {
    let PackedOverlaySprites {
        rgba,
        source_indices,
        entries,
        width: atlas_width,
        height: atlas_height,
    } = pack_overlay_pixels(sprites, device.limits().max_texture_dimension_2d);
    log::info!(
        "Overlay atlas: {}x{} px ({:.1} MB), {} sprites",
        atlas_width,
        atlas_height,
        (atlas_width as u64 * atlas_height as u64 * 5) as f64 / (1024.0 * 1024.0),
        entries.len()
    );

    let texture: BatchTexture = batch.create_texture_on_device(
        device,
        queue,
        &rgba,
        atlas_width,
        atlas_height,
        Some(&source_indices),
    );
    OverlayAtlas {
        texture,
        entries,
        zshapes: None,
    }
}

struct PackedOverlaySprites {
    rgba: Vec<u8>,
    source_indices: Vec<u8>,
    entries: HashMap<OverlaySpriteKey, OverlaySpriteEntry>,
    width: u32,
    height: u32,
}

/// One placement/copy owner for both atlas planes, used by the real GPU upload.
fn pack_overlay_pixels(sprites: &[RenderedOverlay], max_texture_dim: u32) -> PackedOverlaySprites {
    // Sort by height descending for shelf packing efficiency.
    let mut indices: Vec<usize> = (0..sprites.len()).collect();
    indices.sort_by(|&a, &b| sprites[b].height.cmp(&sprites[a].height));

    let total_area: u64 = sprites
        .iter()
        .map(|s| {
            (s.width as u64 + SPRITE_PADDING as u64) * (s.height as u64 + SPRITE_PADDING as u64)
        })
        .sum();
    let estimated_side: u32 = (total_area as f64).sqrt().ceil() as u32;
    let mut atlas_width: u32 = estimated_side.clamp(64, max_texture_dim);

    // Shelf-pack with retry: widen atlas if height exceeds GPU texture limit.
    let placements: Vec<(usize, u32, u32)>;
    let atlas_height: u32;
    loop {
        let mut trial: Vec<(usize, u32, u32)> = Vec::with_capacity(sprites.len());
        let mut cx: u32 = 0;
        let mut cy: u32 = 0;
        let mut shelf_h: u32 = 0;
        for &idx in &indices {
            let w: u32 = sprites[idx].width;
            let h: u32 = sprites[idx].height;
            if cx + w > atlas_width {
                cy += shelf_h + SPRITE_PADDING;
                cx = 0;
                shelf_h = 0;
            }
            trial.push((idx, cx, cy));
            cx += w + SPRITE_PADDING;
            shelf_h = shelf_h.max(h);
        }
        let trial_height: u32 = trial
            .iter()
            .map(|&(idx, _, py)| py + sprites[idx].height)
            .max()
            .unwrap_or(1);
        if trial_height <= max_texture_dim {
            placements = trial;
            atlas_height = trial_height;
            break;
        }
        if atlas_width >= max_texture_dim {
            log::warn!(
                "Overlay atlas height {} exceeds GPU limit {} at max width",
                trial_height,
                max_texture_dim
            );
            placements = trial;
            atlas_height = trial_height.min(max_texture_dim);
            break;
        }
        atlas_width = (atlas_width.saturating_mul(2)).min(max_texture_dim);
    }

    let mut rgba: Vec<u8> = vec![0u8; (atlas_width * atlas_height * 4) as usize];
    let mut source_indices = vec![0u8; (atlas_width * atlas_height) as usize];
    let mut entries: HashMap<OverlaySpriteKey, OverlaySpriteEntry> =
        HashMap::with_capacity(placements.len());
    let aw: f32 = atlas_width as f32;
    let ah: f32 = atlas_height as f32;

    for &(idx, px, py) in &placements {
        let spr: &RenderedOverlay = &sprites[idx];
        let w: u32 = spr.width;
        let h: u32 = spr.height;

        for y in 0..h {
            let src_start: usize = (y * w * 4) as usize;
            let src_end: usize = src_start + (w * 4) as usize;
            let dst_start: usize = (((py + y) * atlas_width + px) * 4) as usize;
            let dst_end: usize = dst_start + (w * 4) as usize;
            if src_end <= spr.rgba.len() && dst_end <= rgba.len() {
                rgba[dst_start..dst_end].copy_from_slice(&spr.rgba[src_start..src_end]);
                source_indices[dst_start / 4..dst_end / 4]
                    .copy_from_slice(&spr.source_indices[src_start / 4..src_end / 4]);
            }
        }

        entries.insert(
            spr.key.clone(),
            OverlaySpriteEntry {
                uv_origin: [px as f32 / aw, py as f32 / ah],
                uv_size: [w as f32 / aw, h as f32 / ah],
                pixel_size: [w as f32, h as f32],
                offset_x: spr.offset_x,
                offset_y: spr.offset_y,
                draw_point_offset: spr.draw_point_offset,
                extended: spr.extended,
            },
        );
    }

    PackedOverlaySprites {
        rgba,
        source_indices,
        entries,
        width: atlas_width,
        height: atlas_height,
    }
}

#[cfg(test)]
impl OverlayAtlas {
    pub(crate) fn from_test_texture(texture: BatchTexture) -> Self {
        Self {
            texture,
            entries: HashMap::new(),
            zshapes: None,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn animated_terrain_binds_stage_256_body_and_paired_shadow() {
        use super::*;
        // TerrainAI71C87A resets a spawner at raw count/2, so514 is the
        // first shape whose Stage256 reaches DrawIt71C208/71C344.
        let directory = crate::map::source::test_support::TestDirectory::new("terrain-stage-256");
        let mut bytes = vec![0u8; 8 + 514 * 24];
        for (offset, value) in [(2, 1u16), (4, 1), (6, 514)] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        // Empty earlier frames keep this a single boundary witness. Palette
        // indices are authored input bytes, not native arithmetic goldens.
        for (frame, index) in [(256, 7u8), (513, 1)] {
            let header = 8 + frame * 24;
            bytes[header + 4..header + 6].copy_from_slice(&1u16.to_le_bytes());
            bytes[header + 6..header + 8].copy_from_slice(&1u16.to_le_bytes());
            let offset = bytes.len() as u32;
            bytes[header + 20..header + 24].copy_from_slice(&offset.to_le_bytes());
            bytes.push(index);
        }
        directory.write("TIBTRE01.TEM", &bytes);
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let palette = Palette::from_bytes(&[0; 768]).unwrap();
        let ini = IniFile::from_str(
            "[TerrainTypes]\n0=TIBTRE01\n[TIBTRE01]\nIsAnimated=yes\nSpawnsTiberium=yes\n",
        );
        let art = IniFile::from_str("[TIBTRE01]\nTheater=yes\n");
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        rules.bind_terrain_assets(&assets, "TEM", TerrainAssetLoadPhase::Fresh);
        let sprites = render_terrain_sprites(
            &assets,
            &palette,
            rules
                .terrain_object_type_case_insensitive("TIBTRE01")
                .unwrap(),
            rules.terrain_shape_filename("TIBTRE01").unwrap(),
        );
        assert_eq!(sprites.len(), 2, "both pieces of the last pre-reset stage");
        let packed = pack_overlay_pixels(&sprites, 64);
        for (prefix, index) in [("__terrain::", 7), ("__terrain_shadow::", 1)] {
            let (key, entry) = packed
                .entries
                .iter()
                .find(|(key, _)| key.name.starts_with(prefix))
                .unwrap();
            assert_eq!(key.frame, 256);
            let x = (entry.uv_origin[0] * packed.width as f32).round() as usize;
            let y = (entry.uv_origin[1] * packed.height as f32).round() as usize;
            assert_eq!(packed.source_indices[y * packed.width as usize + x], index);
        }
    }

    #[test]
    fn static_extended_pair_preserves_source_indices_through_decode_and_atlas_copy() {
        use super::*;
        // Original TREE01 stored canvas/frame offsets; synthetic one-row RLE
        // payloads isolate index ownership, including nonzero black colors.
        let mut bytes = vec![0u8; 56];
        for (offset, value) in [(2, 155u16), (4, 155), (6, 2)] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (frame, x, y, payload) in [
            (0usize, 61u16, 1u16, vec![1u8, 0, 1, 240, 255]),
            (1, 79, 43, vec![0, 1, 1, 0, 1, 254]),
        ] {
            let h = 8 + frame * 24;
            for (off, value) in [(0, x), (2, y), (4, 4), (6, 1)] {
                bytes[h + off..h + off + 2].copy_from_slice(&value.to_le_bytes());
            }
            bytes[h + 8] = 3;
            let offset = bytes.len() as u32;
            bytes[h + 20..h + 24].copy_from_slice(&offset.to_le_bytes());
            bytes.extend_from_slice(&((payload.len() + 2) as u16).to_le_bytes());
            bytes.extend_from_slice(&payload);
        }
        let shp = ShpFile::from_bytes(&bytes).unwrap();
        let palette = Palette::from_bytes(&[0u8; 768]).unwrap();
        let key = terrain_body_key("TREE01", 0);
        let body = render_decoded_overlay_frame(&shp, &palette, key.clone(), 0, 0.0).unwrap();
        let shadow_key = terrain_shadow_key("TREE01", 0);
        let shadow =
            render_decoded_overlay_frame(&shp, &palette, shadow_key.clone(), 1, 0.0).unwrap();
        assert_eq!((body.offset_x, body.offset_y), (-16.0, -76.0));
        assert_eq!((shadow.offset_x, shadow.offset_y), (2.0, -34.0));
        assert_eq!(body.source_indices, [1, 0, 240, 255]);
        assert_eq!(shadow.source_indices, [0, 1, 0, 254]);
        assert_eq!(body.rgba[..8], [0, 0, 0, 255, 0, 0, 0, 0]);
        let packed = pack_overlay_pixels(&[body, shadow], 64);
        for (key, expected) in [(key, [1u8, 0, 240, 255]), (shadow_key, [0, 1, 0, 254])] {
            let entry = &packed.entries[&key];
            let x = (entry.uv_origin[0] * packed.width as f32).round() as usize;
            let y = (entry.uv_origin[1] * packed.height as f32).round() as usize;
            let start = y * packed.width as usize + x;
            assert_eq!(packed.source_indices[start..start + 4], expected);
            for (j, index) in expected.into_iter().enumerate() {
                assert_eq!(
                    packed.rgba[(start + j) * 4 + 3],
                    if index == 0 { 0 } else { 255 }
                );
            }
            assert_eq!(
                packed.source_indices[start + 4],
                0,
                "atlas padding must not become a stencil pixel"
            );
        }
    }

    use std::fmt::Write as _;

    use super::{
        MAX_OVERLAY_FRAME_COUNT, OverlaySpriteKey, OverlayTypeFlags, body_frame_count,
        resolve_body_frame, runtime_crate_sprite_keys, runtime_low_bridge_sprite_keys,
        runtime_tiberium_sprite_keys, stored_frame_offset, wall_body_frame_count,
    };
    use crate::map::overlay::OverlayEntry;
    use crate::rules::crate_rules::CrateRules;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase;
    use crate::rules::tiberium_type::TiberiumTypeRegistry;

    /// `CellClass::DrawOverlay_Body` (0x47F6A0) draws the shape that
    /// `OverlayTypeClass::GetImage` (0x5FEDE0) loads from the type's own image
    /// name, at the cell's frame byte. Retail's destroyed north-south low
    /// bridge rows, `LOBRDG27` (wood) and `LOBRDB27` (concrete), are all-empty
    /// SHPs, so they draw nothing; the previous piece's art never stands in.
    #[test]
    fn retail_destroyed_low_bridge_rows_draw_their_own_empty_art() {
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let retail = crate::rules::retail_ini_fixture::retail_battle_rules()
            .expect("retail Battle rules beside the archives");
        let theater = crate::map::theater::load_theater(&mut assets, "SNOW").unwrap();
        let registry =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let render = |name: &str| {
            super::render_overlay_sprite(
                &assets,
                &theater.iso_palette,
                &OverlaySpriteKey {
                    name: name.to_owned(),
                    frame: 1,
                },
                theater.extension,
                "SNOW",
                &retail.processed_rules,
                retail.rules.art(),
                registry.flags_by_name(name).unwrap(),
            )
        };
        for (standing, destroyed) in [("LOBRDG26", "LOBRDG27"), ("LOBRDB26", "LOBRDB27")] {
            assert!(render(standing).is_some(), "{standing} draws frame 1");
            assert!(render(destroyed).is_none(), "{destroyed} draws nothing");
        }
    }

    /// Terrain DrawIt (0x71C1B0) draws body frame F and shadow frame
    /// F + count/2 whatever their SHP format, and an empty frame draws
    /// nothing. Retail SIGN03 has an uncompressed body; TREE19.SNO has an
    /// empty shadow frame. Frame 1 is the body DrawIt picks below 2 health.
    #[test]
    fn retail_terrain_draws_uncompressed_bodies_and_skips_empty_frames() {
        use super::{terrain_body_key, terrain_shadow_key};
        use std::collections::HashSet;
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules()
            .expect("retail Battle rules beside the archives");
        let theater = crate::map::theater::load_theater(&mut assets, "SNOW").unwrap();
        retail
            .rules
            .bind_terrain_assets(&assets, theater.extension, TerrainAssetLoadPhase::Fresh);
        let keys = |name: &str| {
            super::render_terrain_sprites(
                &assets,
                &theater.iso_palette,
                retail
                    .rules
                    .terrain_object_type_case_insensitive(name)
                    .unwrap(),
                retail.rules.terrain_shape_filename(name).unwrap(),
            )
            .into_iter()
            .map(|sprite| sprite.key)
            .collect::<HashSet<_>>()
        };
        assert_eq!(
            keys("SIGN03"),
            HashSet::from([
                terrain_body_key("SIGN03", 0),
                terrain_shadow_key("SIGN03", 0),
                terrain_body_key("SIGN03", 1),
            ])
        );
        assert_eq!(
            keys("TREE19"),
            HashSet::from([terrain_body_key("TREE19", 0)])
        );
    }

    /// Original TerrainTypeClass::InitTheater (0x71DCA0) loads the stored
    /// type ID, while TerrainTypeClass::ReadINI (0x71DEA0) keeps Image for
    /// its separate ART/read-time uses. Atlas pixels follow the resident SHP.
    #[test]
    fn retail_theater_spawner_atlas_ignores_rules_image_redirect() {
        use super::*;
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let patch = IniFile::from_str("[TIBTRE01]\nImage=TREE01\n");
        let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "Hills.mmx",
            &patch,
        )
        .expect("retail scenario reader with the authored Image override");
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        retail
            .rules
            .bind_terrain_assets(&assets, theater.extension, TerrainAssetLoadPhase::Fresh);
        let source = ShpFile::from_bytes(assets.get_ref("TIBTRE01.TEM").unwrap()).unwrap();
        let sprites = render_terrain_sprites(
            &assets,
            &theater.unit_palette,
            retail
                .rules
                .terrain_object_type_case_insensitive("TIBTRE01")
                .unwrap(),
            retail.rules.terrain_shape_filename("TIBTRE01").unwrap(),
        );
        let packed = pack_overlay_pixels(&sprites, 4096);
        for (key, frame) in [
            (terrain_body_key("TIBTRE01", 0), &source.frames[0]),
            (
                terrain_shadow_key("TIBTRE01", 0),
                &source.frames[source.frames.len() / 2],
            ),
        ] {
            let entry = &packed.entries[&key];
            assert_eq!(
                entry.pixel_size,
                [f32::from(frame.frame_width), f32::from(frame.frame_height)]
            );
            let x = (entry.uv_origin[0] * packed.width as f32).round() as usize;
            let y = (entry.uv_origin[1] * packed.height as f32).round() as usize;
            let width = usize::from(frame.frame_width);
            for row in 0..usize::from(frame.frame_height) {
                let start = (y + row) * packed.width as usize + x;
                assert_eq!(
                    packed.source_indices[start..start + width],
                    frame.pixels[row * width..(row + 1) * width],
                    "{key:?}: physical own-ID body/stencil source"
                );
            }
        }
    }

    #[test]
    fn wall_stored_frame_origin_preserves_authored_pixels_without_canvas_padding() {
        // Retail GAWALL.SHP, ra2.mix -> snow.mix: canvas 78x70;
        // frame 0 rect (17,17,42,43), frame 5 rect (12,23,50,40).
        // Its draw helper contributes -12 separately from canvas centering.
        assert_eq!(stored_frame_offset(78, 70, 17, 17, -12.0), (-22.0, -30.0));
        assert_eq!(stored_frame_offset(78, 70, 12, 23, -12.0), (-27.0, -24.0));
        // CC_Draw_Shape uses integer halves, including odd authored canvases.
        assert_eq!(stored_frame_offset(79, 71, 17, 17, -12.0), (-22.0, -30.0));
    }

    /// Frame table of a stock low-bridge SHP: 6 frames, art only in frame 1.
    ///
    /// Machine-derived from `LOBRDB10.des`, `LOBRDB13.des`, `LOBRDB23.des`,
    /// `LOBRDB25.des`, `LOBRDG10.des`, `LOBRDG23.tem` and `LOBRDB10.tem` — every
    /// low-bridge piece sampled across the desert and temperate theaters has the
    /// same shape.
    const LOW_BRIDGE_FRAME_SIZES: [(u16, u16); 6] =
        [(0, 0), (120, 70), (0, 0), (0, 0), (0, 0), (0, 0)];

    #[test]
    fn startup_crate_preload_follows_resolved_identity_and_native_mark_data() {
        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=FLAGGED\n1=PLAIN\n2=LANDBOX\n\
             [FLAGGED]\nCrate=yes\n[PLAIN]\n[LANDBOX]\nLand=Tiberium\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let rules = CrateRules {
            wood_crate_img: Some("PLAIN".to_owned()),
            crate_img: Some("FLAGGED".to_owned()),
            water_crate_img: Some("LANDBOX".to_owned()),
            ..CrateRules::default()
        };

        let keys = runtime_crate_sprite_keys(&registry, &rules);
        assert!(keys.contains(&OverlaySpriteKey {
            name: "FLAGGED".to_owned(),
            frame: 0,
        }));
        assert!(keys.contains(&OverlaySpriteKey {
            name: "PLAIN".to_owned(),
            frame: 0,
        }));
        // `Land=Tiberium` stores one (`0x005FD0D9..0x005FD0E5`); a type with
        // no TiberiumClass keeps it.
        assert!(keys.contains(&OverlaySpriteKey {
            name: "LANDBOX".to_owned(),
            frame: 1,
        }));
        assert_eq!(keys.len(), 3);

        // A resource type on the Land-5 arm takes its germinated density
        // (`0x004818E0`); on Road it keeps the ordinary zero.
        let resource_ini = IniFile::from_str(
            "[OverlayTypes]\n0=TIBBOX\n1=ROADTIB\n\
             [TIBBOX]\nLand=Tiberium\nTiberium=yes\n[ROADTIB]\nLand=Road\nTiberium=yes\n",
        );
        let resource_registry = OverlayTypeRegistry::from_ini(&resource_ini, None);
        let resource_rules = CrateRules {
            wood_crate_img: Some("TIBBOX".to_owned()),
            crate_img: Some("ROADTIB".to_owned()),
            water_crate_img: Some("ROADTIB".to_owned()),
            ..CrateRules::default()
        };
        let resource_keys = runtime_crate_sprite_keys(&resource_registry, &resource_rules);
        for frame in 0..=11 {
            assert!(resource_keys.contains(&OverlaySpriteKey {
                name: "TIBBOX".to_owned(),
                frame,
            }));
        }
        assert!(resource_keys.contains(&OverlaySpriteKey {
            name: "ROADTIB".to_owned(),
            frame: 0,
        }));
        assert_eq!(resource_keys.len(), 13);
    }

    #[test]
    fn startup_crate_preload_includes_special_mark_outputs() {
        let mut low_ini = String::from("[OverlayTypes]\n");
        for id in 0..=0x7Au8 {
            let name = if id == 0x7A {
                "LOWTRIGGER".to_owned()
            } else {
                format!("OV{id:03}")
            };
            writeln!(&mut low_ini, "{id}={name}").unwrap();
        }
        let low_registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(&low_ini), None);
        let low_rules = CrateRules {
            wood_crate_img: Some("LOWTRIGGER".to_owned()),
            crate_img: Some("LOWTRIGGER".to_owned()),
            water_crate_img: Some("LOWTRIGGER".to_owned()),
            ..CrateRules::default()
        };
        let low_keys = runtime_crate_sprite_keys(&low_registry, &low_rules);
        for id in [0x5C, 0x4A, 0x4B, 0x4C, 0x4D] {
            let name = low_registry.name(id).unwrap().to_owned();
            for frame in 0..=2 {
                assert!(low_keys.contains(&OverlaySpriteKey {
                    name: name.clone(),
                    frame,
                }));
            }
        }

        let mut high_ini = String::from("[OverlayTypes]\n");
        for id in 0..=0x18u8 {
            let name = if id == 0x18 {
                "HIGHANCHOR".to_owned()
            } else {
                format!("OV{id:03}")
            };
            writeln!(&mut high_ini, "{id}={name}").unwrap();
        }
        high_ini.push_str("[HIGHANCHOR]\nCrate=yes\n");
        let high_registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(&high_ini), None);
        let high_rules = CrateRules {
            wood_crate_img: Some("HIGHANCHOR".to_owned()),
            crate_img: Some("HIGHANCHOR".to_owned()),
            water_crate_img: Some("HIGHANCHOR".to_owned()),
            ..CrateRules::default()
        };
        let high_keys = runtime_crate_sprite_keys(&high_registry, &high_rules);
        assert!(
            high_keys.is_empty(),
            "high startup identities are rooted in BridgeAtlas, never OverlayAtlas"
        );
    }

    #[test]
    fn low_bridge_flank_columns_draw_nothing() {
        // A low bridge is three cells wide and the map pack stores the column
        // index in the cell's overlay-data byte: 0 = left flank, 1 = middle,
        // 2 = right flank (verified on `xlostlake.map`, 155 low-bridge cells
        // splitting 53/50/52 with the value tracking `x` across each row). Only
        // the middle column carries art; its single 120px-wide sprite covers the
        // whole row. Resolving the flanks to the populated frame drew the deck
        // three times, offset by a cell each way.
        let max_normal_frame: usize = LOW_BRIDGE_FRAME_SIZES.len() / 2;
        assert_eq!(
            resolve_body_frame(0, max_normal_frame, &LOW_BRIDGE_FRAME_SIZES),
            None,
            "left flank must draw nothing"
        );
        assert_eq!(
            resolve_body_frame(1, max_normal_frame, &LOW_BRIDGE_FRAME_SIZES),
            Some(1),
            "middle column carries the deck sprite"
        );
        assert_eq!(
            resolve_body_frame(2, max_normal_frame, &LOW_BRIDGE_FRAME_SIZES),
            None,
            "right flank must draw nothing"
        );
    }

    #[test]
    fn out_of_range_frames_still_collapse_to_zero() {
        // VERA-internal guard (gamemd equivalent UNCHECKED): a wall whose art
        // declares fewer body frames than DamageLevels can reach stays visible
        // as a pristine post rather than vanishing.
        let wall: Vec<(u16, u16)> = vec![(30, 30); 16];
        assert_eq!(resolve_body_frame(0x2F, wall.len(), &wall), Some(0));
    }

    #[test]
    fn wall_preload_spans_every_reachable_damage_stage() {
        // [GAWALL] DamageLevels=3 (artmd.ini): the sim writes damage 0..2 into
        // the high nibble, so a fully damaged, fully connected segment asks for
        // frame 0x2F. Preloading only the first connectivity block (0..16) is
        // what makes a scratched wall fall back to the pristine isolated post.
        assert_eq!(wall_body_frame_count(3), 48);
        assert!(
            wall_body_frame_count(3) > 0x2F,
            "GAWALL damage stage 2 + all four neighbours must be preloaded"
        );
        // [GASAND]/[CAFNCB]/[CAFNCW]/[CAKRMW]/[CAFNCP] DamageLevels=2.
        assert_eq!(wall_body_frame_count(2), 32);
        assert!(wall_body_frame_count(2) > 0x1F);
        // An absent DamageLevels= still needs the whole connectivity block.
        assert_eq!(wall_body_frame_count(1), 16);
        assert_eq!(wall_body_frame_count(0), 16);
        // The overlay-data byte is a u8 — nothing past 255 is addressable.
        assert_eq!(wall_body_frame_count(u16::MAX), MAX_OVERLAY_FRAME_COUNT);
    }

    #[test]
    fn wall_body_range_excludes_the_shadow_half() {
        // GAWALL.SHP is 96 frames: 48 body (3 damage x 16 connectivity) plus
        // 48 one-bit shadow stencils. A wall must never resolve a stencil as
        // its body art.
        let wall = OverlayTypeFlags {
            wall: true,
            ..OverlayTypeFlags::default()
        };
        assert_eq!(body_frame_count(&wall, 96), 48);
        assert_eq!(
            u32::try_from(body_frame_count(&wall, 96)).unwrap(),
            wall_body_frame_count(3),
            "GAWALL's body half and its DamageLevels-derived range must agree"
        );

        let bridge = OverlayTypeFlags {
            bridge_deck: true,
            ..OverlayTypeFlags::default()
        };
        assert_eq!(body_frame_count(&bridge, 36), 18);

        // Ordinary overlays (ore, gems, tracks) keep every frame addressable.
        let generic = OverlayTypeFlags::default();
        assert_eq!(body_frame_count(&generic, 96), 96);
    }

    #[test]
    fn gsi_04_13_overlay_atlas_preloads_live_low_bridge_identities_and_data() {
        let mut text = String::from("[OverlayTypes]\n");
        for overlay_id in 0u16..=238 {
            let name = match overlay_id {
                24 => "BRIDGE1".to_string(),
                25 => "BRIDGE2".to_string(),
                74..=101 => format!("LOBRDG{:02}", overlay_id - 73),
                122..=125 => format!("LOBRDGE{}", overlay_id - 121),
                205..=232 => format!("LOBRDB{:02}", overlay_id - 204),
                233..=236 => format!("LOBRDGB{}", overlay_id - 232),
                237 => "BRIDGEB1".to_string(),
                238 => "BRIDGEB2".to_string(),
                _ => format!("DUMMY{overlay_id}"),
            };
            text.push_str(&format!("{overlay_id}={name}\n"));
        }
        let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(&text), None);
        let overlays = [OverlayEntry {
            rx: 10,
            ry: 11,
            overlay_id: 0x4A,
            frame: 7,
        }];

        let keys = runtime_low_bridge_sprite_keys(&overlays, &registry);

        for (name, frame) in [
            ("LOBRDG07", 7),
            ("LOBRDG27", 7),
            ("LOBRDG27", 0),
            ("LOBRDB27", 7),
        ] {
            assert!(
                keys.contains(&OverlaySpriteKey {
                    name: name.to_string(),
                    frame,
                }),
                "missing runtime atlas key {name}:{frame}"
            );
        }
        assert!(
            !keys.iter().any(|key| key.name == "BRIDGE1"),
            "high bridges remain in the dedicated bridge atlas"
        );
    }

    #[test]
    fn gsi_13_05_overlay_atlas_preloads_every_parsed_flat_resource_density_key() {
        let mut text = String::from(
            "[Tiberiums]\n0=Riparius\n1=Cruentus\n\
             [Riparius]\nImage=1\n\
             [Cruentus]\nImage=2\n\
             [OverlayTypes]\n",
        );
        let mut resource_names = Vec::new();
        for overlay_id in 0..=113 {
            let name = match overlay_id {
                27..=38 => format!("GEM{:02}", overlay_id - 26),
                102..=113 => format!("TIB{:02}", overlay_id - 101),
                _ => format!("FILL{overlay_id:03}"),
            };
            text.push_str(&format!("{overlay_id}={name}\n"));
            if matches!(overlay_id, 27..=38 | 102..=113) {
                resource_names.push(name);
            }
        }
        for name in resource_names {
            text.push_str(&format!("[{name}]\nTiberium=yes\n"));
        }
        let ini = IniFile::from_str(&text);
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);
        let tiberiums = TiberiumTypeRegistry::from_ini(&ini);

        let keys = runtime_tiberium_sprite_keys(&overlays, &tiberiums);

        assert_eq!(keys.len(), 288, "2 types * 12 images * 12 densities");
        for (name, frame) in [("TIB05", 8), ("GEM01", 11)] {
            assert!(keys.contains(&OverlaySpriteKey {
                name: name.to_string(),
                frame,
            }));
        }
    }
}
