//! SHP sprite atlas — pre-renders SHP sprites into a packed GPU texture.
//!
//! At map load time, all SpriteModel-tagged entities (infantry, buildings) are
//! identified by (type_id, facing). Each unique combination has its SHP frame
//! rendered to RGBA and packed into a single GPU texture atlas.
//!
//! Mirrors the unit_atlas.rs approach: one texture, one draw call per frame.
//!
//! ## Frame selection
//! - Buildings (Structure): always frame 0 (no rotation).
//! - Infantry: `(facing / 32) % num_facings` for 8-direction standing pose.
//! - Facing is collapsed to 0 for structures in the cache key (dedup).
//!
//! ## Dependency rules
//! - Part of render/ — depends on assets/ (SHP/Palette), render/batch (GPU upload).
//! - Reads from sim/ via EntityStore iteration (GameEntity fields).

use std::collections::{HashMap, HashSet};

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::rules::art_data::{self, ArtRegistry};
use crate::rules::effect_asset_catalog::available_effect_anim_frame_count;
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};
use crate::rules::ruleset::RuleSet;

/// Maximum atlas texture width for SHP sprites (pixels).

/// Padding between sprites in the atlas to prevent texture bleeding.
const SPRITE_PADDING: u32 = 1;
const INFANTRY_FACING_STEP: u8 = crate::util::direction::FACING_UNITS_PER_DIRECTION;
const INFANTRY_FACING_BUCKETS: u8 = 8;

fn register_effect_anim_frames(
    needed: &mut HashSet<ShpSpriteKey>,
    frame_counts: &mut HashMap<String, u16>,
    anim_type: &str,
    raw_count: u16,
    scheduler_owned: bool,
    shadow: bool,
) -> u16 {
    let count = available_effect_anim_frame_count(raw_count, scheduler_owned, shadow);
    for frame in 0..count {
        needed.insert(ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
            type_id: anim_type.to_string(),
            facing: 0,
            frame,
            house_color: HouseColorIndex(0),
        });
    }
    frame_counts.insert(anim_type.to_ascii_uppercase(), count);
    count
}

fn effect_anim_shp_candidates(
    anim_type: &str,
    art: Option<&ArtRegistry>,
    theater_ext: &str,
    theater_name: &str,
) -> Vec<String> {
    let image_id = art.map_or_else(
        || anim_type.to_ascii_uppercase(),
        |registry| registry.resolve_effective_image_id(anim_type, anim_type),
    );
    art_data::anim_shp_candidates(art, anim_type, &image_id, theater_ext, theater_name)
}

fn scan_building_anim_frame_count(
    asset_manager: &AssetManager,
    art_reg: &ArtRegistry,
    anim_type: &str,
    loop_end: u16,
    theater_ext: &str,
    theater_name: &str,
) -> Option<u16> {
    let anim_image: String = art_reg.resolve_effective_image_id(anim_type, anim_type);
    let candidates: Vec<String> = art_data::anim_shp_candidates(
        Some(art_reg),
        anim_type,
        &anim_image,
        theater_ext,
        theater_name,
    );
    let data = candidates.iter().find_map(|c| asset_manager.get_ref(c))?;
    let shp = ShpFile::from_bytes(data).ok()?;
    let raw_count = shp.frames.len() as u16;
    let real: u16 = raw_count / 2;
    let required_count = loop_end.max(1);
    Some(if real > 0 && real >= required_count {
        real
    } else if raw_count >= required_count {
        required_count
    } else {
        raw_count
    })
}

fn insert_building_anim_frame_keys(
    needed: &mut HashSet<ShpSpriteKey>,
    anim_type: &str,
    count: u16,
    house_color: HouseColorIndex,
    palette_context: ShpPaletteContext,
) {
    for frame in 0..count.max(1) {
        needed.insert(ShpSpriteKey {
            palette_context,
            type_id: anim_type.to_string(),
            facing: 0,
            frame,
            house_color: if palette_context == ShpPaletteContext::GlobalAnim {
                HouseColorIndex(0)
            } else {
                house_color
            },
        });
    }
}

/// Palette ownership comes from the draw producer, not an animation's name.
/// Global ANIM and explicit selected ColorScheme may coexist for identical
/// type/frame/house keys (AnimClass DrawIt 00423280..00423354).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShpPaletteContext {
    #[default]
    Legacy,
    GlobalAnim,
    SelectedScheme,
    Cell,
}

pub(crate) fn attached_anim_palette_context(
    config: Option<&art_data::AnimTypeRuntimeConfig>,
) -> ShpPaletteContext {
    if config.is_none_or(|c| c.should_use_cell_drawer) {
        ShpPaletteContext::SelectedScheme
    } else {
        ShpPaletteContext::GlobalAnim
    }
}

/// Cache key: unique combination of object type, facing, and house color.
/// For structures, facing is always 0 (buildings don't rotate).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShpSpriteKey {
    pub palette_context: ShpPaletteContext,
    /// Object type ID from rules.ini (e.g., "E1", "GAPOWR").
    pub type_id: String,
    /// Facing direction (0–255). Collapsed to 0 for structures.
    pub facing: u8,
    /// Absolute SHP frame index.
    pub frame: u16,
    /// House color index for palette remapping (different per player).
    pub house_color: HouseColorIndex,
}

/// UV and offset data for one sprite within the SHP atlas.
#[derive(Debug, Clone, Copy)]
pub struct ShpSpriteEntry {
    /// Top-left UV coordinate in the atlas page (0.0..1.0).
    pub uv_origin: [f32; 2],
    /// UV width and height (0.0..1.0).
    pub uv_size: [f32; 2],
    /// Sprite dimensions in pixels.
    pub pixel_size: [f32; 2],
    /// X offset from the cell center to the sprite's top-left corner.
    pub offset_x: f32,
    /// Y offset from the cell center to the sprite's top-left corner.
    pub offset_y: f32,
    /// Logical canvas [x, y, width, height] retained for picking and sorting.
    /// Drawing and native Z use the stored frame rectangle above instead.
    pub canvas_rect: [f32; 4],
    /// SHP format bit 1: only the extended walker consumes BUILDNGZ.
    pub extended: bool,
    /// Atlas page index (0-based). Each page is a separate GPU texture.
    pub page: u8,
}

/// A single page of the multi-page sprite atlas.
/// Each page is a separate GPU texture with its own bind group.
pub struct SpriteAtlasPage {
    /// The packed GPU texture for this page.
    pub texture: BatchTexture,
}

/// Per-building-type bounding box for selection brackets and click picking.
///
/// Computed by unioning all SHP frames (main sprite + animation overlays) of the
/// building type.
///
/// Coordinates are relative to `(cell_center_x, screen_y)` where
/// `cell_center_x = screen_x + TILE_WIDTH / 2`.
#[derive(Debug, Clone, Copy)]
pub struct BuildingBounds {
    /// Left edge offset from cell center X.
    pub min_x: f32,
    /// Top edge offset from screen_y.
    pub min_y: f32,
    /// Total width in pixels.
    pub width: f32,
    /// Total height in pixels.
    pub height: f32,
}

/// A multi-page GPU texture atlas containing pre-rendered SHP sprites.
///
/// When the total sprite area exceeds the GPU texture limit, sprites are
/// split across multiple pages. Each page is an independent GPU texture
/// with its own bind group. The entry lookup returns a `page` index that
/// identifies which page's texture to bind when drawing that sprite.
///
/// Created once at map load, rebuilt incrementally when new entity types appear.
pub struct SpriteAtlas {
    /// Atlas pages — each is a separate GPU texture.
    /// Most maps need only 1 page; large games with many building types may use 2+.
    pub pages: Vec<SpriteAtlasPage>,
    /// Lookup: (type_id, facing, frame, house_color) → UV rectangle + offset + page.
    entries: HashMap<ShpSpriteKey, ShpSpriteEntry>,
    /// Building type → number of make (build-up) animation frames.
    /// Key is the base type_id (e.g., "GACNST"), not the "_MAKE" suffixed key.
    pub make_frame_counts: HashMap<String, u16>,
    /// Building/world-animation type → available non-shadow frame count.
    /// Building consumers use this only to ensure their live animation frame is
    /// resident; world-effect systems also consume the same established map.
    pub active_anim_frame_counts: HashMap<String, u16>,
    /// Per-building-type bounding boxes for selection brackets and click picking.
    /// Computed by unioning all SHP frame rects for each building type.
    pub building_bounds: HashMap<String, BuildingBounds>,
    /// Cached rendered sprites for incremental rebuild. On subsequent rebuilds,
    /// only genuinely new sprite keys are rendered; cached sprites are reused
    /// and everything is repacked.
    rendered_cache: Vec<RenderedShpSprite>,
}

#[cfg(test)]
fn push_effect_name(effect_names: &mut Vec<String>, name: &str) {
    if !effect_names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        effect_names.push(name.to_string());
    }
}

#[cfg(test)]
fn collect_effect_names(rules: &RuleSet) -> Vec<String> {
    let mut effect_names: Vec<String> = Vec::new();
    push_effect_name(&mut effect_names, &rules.general.warp_in.name);
    push_effect_name(&mut effect_names, &rules.general.warp_out.name);
    push_effect_name(&mut effect_names, &rules.general.warp_away.name);
    for fire_ref in &rules.general.damage_fire_types {
        push_effect_name(&mut effect_names, &fire_ref.name);
    }
    for anim_name in rules.art_registry.scheduler_anim_types() {
        push_effect_name(&mut effect_names, anim_name);
    }
    for wh in rules.warheads_iter() {
        for anim_name in &wh.anim_list {
            push_effect_name(&mut effect_names, anim_name);
        }
    }
    for weapon in rules.weapons_iter() {
        for anim_name in &weapon.anim {
            push_effect_name(&mut effect_names, anim_name);
        }
        if let Some(ref anim_name) = weapon.occupant_anim {
            push_effect_name(&mut effect_names, anim_name);
        }
    }
    for pt in rules.particle_types_iter() {
        if let Some(image) = pt.image.as_deref() {
            push_effect_name(&mut effect_names, image);
        }
    }
    effect_names
}

impl SpriteAtlas {
    /// Look up the atlas entry for a given sprite key.
    /// The returned entry includes a `page` field identifying which atlas page
    /// holds this sprite's texture data.
    pub fn get(&self, key: &ShpSpriteKey) -> Option<&ShpSpriteEntry> {
        self.entries.get(key)
    }

    /// Number of unique sprites across all pages.
    pub fn sprite_count(&self) -> usize {
        self.entries.len()
    }

    /// Number of atlas pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Get a specific atlas page by index.
    pub fn page(&self, idx: usize) -> Option<&SpriteAtlasPage> {
        self.pages.get(idx)
    }

    /// Iterate over all atlas entries (key + sprite data).
    pub fn entries_iter(&self) -> impl Iterator<Item = (&ShpSpriteKey, &ShpSpriteEntry)> {
        self.entries.iter()
    }

    /// Check whether the atlas already contains all sprite keys needed by the
    /// current ECS world. Returns true if no rebuild is necessary.
    pub fn has_all_keys(&self, needed: &HashSet<ShpSpriteKey>) -> bool {
        needed.iter().all(|k| self.entries.contains_key(k))
    }
}

/// Intermediate rendered sprite before atlas packing.
struct RenderedShpSprite {
    key: ShpSpriteKey,
    /// RGBA pixels of the stored frame rectangle, without canvas padding.
    rgba: Vec<u8>,
    indices: Vec<u8>,
    width: u32,
    height: u32,
    /// Offset from cell center to top-left of sprite.
    offset_x: f32,
    offset_y: f32,
    canvas_rect: [f32; 4],
    extended: bool,
}

/// End an incremental rebuild without invalidating the atlas that was already
/// usable by the renderer.
///
/// Some rebuild failures happen after `rendered_cache` has been moved out of
/// the prior atlas and new sprites have been appended. Truncating to the exact
/// extraction length before putting the vector back makes that move atomic from
/// the caller's point of view. Initial construction deliberately has no fallback
/// and keeps the existing fail-fast contract for required cell-drawer assets.
#[cold]
fn abort_sprite_atlas_refresh(
    previous_atlas: Option<SpriteAtlas>,
    extracted_cache: Option<Vec<RenderedShpSprite>>,
    previous_cache_len: usize,
    failure: String,
) -> Option<SpriteAtlas> {
    let Some(mut previous) = previous_atlas else {
        panic!("{failure}");
    };

    if let Some(mut cache) = extracted_cache {
        cache.truncate(previous_cache_len);
        previous.rendered_cache = cache;
    }
    log::warn!("{failure}; keeping the previous valid sprite atlas");
    Some(previous)
}

/// Collect the base set of (type_id, house_color) pairs from the ECS world.
///
/// Returns the set of unique (type_id, color) combos that would trigger atlas entries.
/// Used by the incremental rebuild path: if every combo already has entries in the
/// existing atlas, we can skip the expensive full rebuild.
pub fn collect_needed_base_keys(
    entities: &crate::sim::entity_store::EntityStore,
    house_colors: &HouseColorMap,
    extra_building_types: &[&str],
    interner: Option<&crate::sim::intern::StringInterner>,
) -> HashSet<(String, HouseColorIndex)> {
    let mut base_keys: HashSet<(String, HouseColorIndex)> = HashSet::new();
    for entity in entities.values() {
        if entity.is_voxel {
            continue;
        }
        let owner_str = interner.map_or("", |i| i.resolve(entity.owner()));
        let type_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
        let color_idx: HouseColorIndex = house_colors
            .get(owner_str)
            .copied()
            .unwrap_or(crate::rules::house_colors::NO_REMAP);
        base_keys.insert((type_str.to_string(), color_idx));
    }
    // Include extra building types (deployable ConYards etc.).
    if !extra_building_types.is_empty() {
        let all_colors: Vec<HouseColorIndex> = house_colors.values().copied().collect();
        for &type_id in extra_building_types {
            for &color in &all_colors {
                base_keys.insert((type_id.to_string(), color));
            }
        }
    }
    base_keys
}

/// Live AnimClass palette variants that must exist in the atlas. Producers
/// install these after construction (notably OverlayClass CellAnim over a
/// Tiberium cell), so entity ownership cannot supply the color key.
pub fn collect_anim_remap_base_keys(
    sim: &crate::sim::world::Simulation,
) -> HashSet<(String, HouseColorIndex)> {
    sim.anims()
        .filter_map(|(_, anim)| {
            Some((
                sim.interner.resolve(anim.type_id).to_ascii_uppercase(),
                anim.remap_color?,
            ))
        })
        .collect()
}

fn insert_anim_remap_frame_keys(
    needed: &mut HashSet<ShpSpriteKey>,
    type_id: &str,
    frame_count: u16,
    anim_remap_keys: &HashSet<(String, HouseColorIndex)>,
) {
    for &(_, color) in anim_remap_keys
        .iter()
        .filter(|(anim_type, _)| anim_type.eq_ignore_ascii_case(type_id))
    {
        for frame in 0..frame_count {
            needed.insert(ShpSpriteKey {
                palette_context: crate::render::sprite_atlas::ShpPaletteContext::SelectedScheme,
                type_id: type_id.to_string(),
                facing: 0,
                frame,
                house_color: color,
            });
        }
    }
}

/// Coverage is specific to the producer's palette context. A legacy entity
/// entry cannot satisfy an explicit animation ColorScheme preload.
pub fn atlas_covers_base_keys(
    atlas: &SpriteAtlas,
    base_keys: &HashSet<(String, HouseColorIndex)>,
    palette_context: ShpPaletteContext,
) -> bool {
    base_keys.iter().all(|(type_id, color)| {
        atlas
            .get(&ShpSpriteKey {
                palette_context,
                type_id: type_id.clone(),
                facing: 0,
                frame: 0,
                house_color: *color,
            })
            .is_some()
    })
}

/// Which palette a sprite's frames are baked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpritePaletteChoice {
    /// The theater unit palette — the atlas's default and the target of
    /// `AltPalette=yes`.
    Unit,
    /// `anim.pal`, used for animation art that does not set `AltPalette=`.
    Anim,
    /// Active theater ISO palette, used by AnimClass +0x196 cell-drawer rows.
    CellIso,
}

/// Pick the palette a sprite key's frames are baked against.
///
/// gamemd decides this per animation type, not per name list: the animation draw
/// path reaches for the global ANIM.PAL conversion and swaps in the first colour
/// scheme's converted unit palette when the type sets `AltPalette=yes`. So the art
/// flag is consulted first and the world-effect name set only decides the types
/// that leave the flag alone. 41 stock sections carry the flag — the Battle
/// Bunker, the `CABUNK0x` bunkers, the Weather Storm clouds, the squid grapple and
/// the parachute among them — and before this they were baked against `anim.pal`
/// or the unit palette by whether their name happened to land in that set.
///
/// Types with no art entry keep the previous behaviour, so non-animation art
/// (units, infantry, structures) is unaffected.
pub(crate) fn sprite_palette_choice(
    type_id: &str,
    art: Option<&ArtRegistry>,
    effect_type_ids: &HashSet<String>,
    cell_drawer_type_ids: &HashSet<String>,
) -> SpritePaletteChoice {
    if cell_drawer_type_ids.contains(&type_id.to_ascii_uppercase()) {
        return SpritePaletteChoice::CellIso;
    }
    let alt_palette = art
        .and_then(|registry| registry.anim_runtime_config(type_id))
        .map(art_data::anim_draw_palette)
        == Some(art_data::AnimDrawPalette::Unit);
    if alt_palette {
        return SpritePaletteChoice::Unit;
    }
    if effect_type_ids.contains(type_id) {
        SpritePaletteChoice::Anim
    } else {
        SpritePaletteChoice::Unit
    }
}

fn sprite_palette_for_key(
    key: &ShpSpriteKey,
    art: Option<&ArtRegistry>,
    effect_type_ids: &HashSet<String>,
    cell_palette_type_ids: &HashSet<String>,
) -> SpritePaletteChoice {
    match key.palette_context {
        ShpPaletteContext::SelectedScheme => SpritePaletteChoice::Unit,
        ShpPaletteContext::Cell => SpritePaletteChoice::CellIso,
        ShpPaletteContext::GlobalAnim => {
            if art
                .and_then(|a| a.anim_runtime_config(&key.type_id))
                .is_some_and(|c| c.alt_palette)
            {
                SpritePaletteChoice::Unit
            } else {
                SpritePaletteChoice::Anim
            }
        }
        ShpPaletteContext::Legacy => {
            sprite_palette_choice(&key.type_id, art, effect_type_ids, cell_palette_type_ids)
        }
    }
}

/// Build a SHP sprite atlas from all SpriteModel entities in the ECS world.
///
/// Uses incremental rendering: if `existing` is provided, its cached rendered
/// sprites are reused and only genuinely new keys are rendered. This avoids
/// expensive SHP loading and frame blitting for sprites already in the atlas.
///
/// 1. Queries the world for all (TypeRef, Facing, Category, SpriteModel) entities.
/// 2. Collects unique (type_id, facing) pairs (facing=0 for structures).
/// 3. Diffs against cached sprites — renders only new keys.
/// 4. Shelf-packs all sprites (cached + new) into a single atlas texture.
///
/// Returns None if no sprite entities exist or all fail to load.
///
/// `theater_ext` is the file extension for theater-specific SHP files
/// (e.g., "tem" for TEMPERATE). Civilian buildings use `{TYPE_ID}.{ext}`
/// instead of `{TYPE_ID}.SHP`. A supplied prior atlas is returned unchanged
/// when no replacement sprite can be produced.
pub fn build_sprite_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    entities: &crate::sim::entity_store::EntityStore,
    asset_manager: &AssetManager,
    palette: &Palette,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
    house_colors: &HouseColorMap,
    extra_building_types: &[&str],
    anim_remap_keys: &HashSet<(String, HouseColorIndex)>,
    cell_drawer_type_ids: &HashSet<String>,
    cell_palette: Option<&Palette>,
    existing: Option<SpriteAtlas>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> Option<SpriteAtlas> {
    let mut previous_atlas = existing;
    // Step 1: Collect unique (type_id, facing, frame, house_color) keys.
    // Structures get facing=0 since buildings don't rotate.
    let mut needed: HashSet<ShpSpriteKey> = HashSet::new();
    for entity in entities.values() {
        if entity.is_voxel {
            continue;
        }
        let owner_str = interner.map_or("", |i| i.resolve(entity.owner()));
        let type_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
        let color_idx: HouseColorIndex = house_colors
            .get(owner_str)
            .copied()
            .unwrap_or(crate::rules::house_colors::NO_REMAP);
        match entity.category {
            EntityCategory::Structure => {
                needed.insert(ShpSpriteKey {
                    palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                    type_id: type_str.to_string(),
                    facing: 0,
                    frame: 0,
                    house_color: color_idx,
                });
                // CanBeOccupied buildings need frames 0..3 for the occupancy +
                // damage-tier frame swap (see building_frame_index in
                // app/presentation/instances/shp.rs). SHPs with fewer frames silently skip
                // missing entries; the renderer falls back to frame 0.
                let can_be_occupied = rules
                    .and_then(|r| r.object(type_str))
                    .map(|obj| obj.can_be_occupied)
                    .unwrap_or(false);
                if can_be_occupied {
                    for frame in 1u16..=3 {
                        needed.insert(ShpSpriteKey {
                            palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                            type_id: type_str.to_string(),
                            facing: 0,
                            frame,
                            house_color: color_idx,
                        });
                    }
                }
            }
            _ => {
                // Presentation consumes the same immutable per-type catalog as
                // simulation. Image aliases, metadata fallback, voxel gating,
                // and frame timing are resolved once when RuleSet is bound.
                let seq_set = rules.and_then(|registry| registry.animation_sequence(type_str));

                if let Some(set) = seq_set {
                    // Data-driven: iterate Stand + Walk sequences (and others)
                    // to collect exactly the right frames.
                    use crate::sim::animation::SequenceKind;
                    let relevant_kinds: &[SequenceKind] = &[
                        SequenceKind::Stand,
                        SequenceKind::Walk,
                        SequenceKind::Attack,
                        SequenceKind::Idle1,
                        SequenceKind::Idle2,
                        SequenceKind::Die1,
                        SequenceKind::Die2,
                        SequenceKind::Die3,
                        SequenceKind::Die4,
                        SequenceKind::Die5,
                        SequenceKind::Prone,
                        SequenceKind::Crawl,
                        SequenceKind::FireProne,
                        SequenceKind::Down,
                        SequenceKind::Up,
                        SequenceKind::Cheer,
                        SequenceKind::Paradrop,
                        SequenceKind::Panic,
                        SequenceKind::Deploy,
                        SequenceKind::Undeploy,
                        SequenceKind::Deployed,
                        SequenceKind::DeployedFire,
                        SequenceKind::DeployedIdle,
                        SequenceKind::SecondaryFire,
                        SequenceKind::SecondaryProne,
                        SequenceKind::Swim,
                        SequenceKind::Fly,
                        SequenceKind::FireFly,
                        SequenceKind::Hover,
                        SequenceKind::Tread,
                        SequenceKind::WetAttack,
                        SequenceKind::WetIdle1,
                        SequenceKind::WetIdle2,
                    ];
                    for kind in relevant_kinds {
                        if let Some(seq_def) = set.get(kind) {
                            for f_idx in 0..seq_def.facings {
                                for frame_offset in 0..seq_def.frame_count {
                                    let frame: u16 = seq_def.start_frame
                                        + f_idx as u16 * seq_def.facing_multiplier
                                        + frame_offset;
                                    // Use facing=0 for all infantry keys — the absolute
                                    // frame index already encodes the facing direction.
                                    // This avoids cache key mismatches for non-8-facing
                                    // sequences (most RA2 infantry use 6 facings).
                                    needed.insert(ShpSpriteKey {
                                        palette_context:
                                            crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                                        type_id: type_str.to_string(),
                                        facing: 0,
                                        frame,
                                        house_color: color_idx,
                                    });
                                }
                            }
                        }
                    }
                } else {
                    // Fallback: hardcoded default layout (stand=0-7, walk=8-55).
                    // Use facing=0 for all keys — frame index encodes direction.
                    for bucket in 0..INFANTRY_FACING_BUCKETS {
                        needed.insert(ShpSpriteKey {
                            palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                            type_id: type_str.to_string(),
                            facing: 0,
                            frame: bucket as u16,
                            house_color: color_idx,
                        });
                        for walk_frame in 0..6u16 {
                            needed.insert(ShpSpriteKey {
                                palette_context:
                                    crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                                type_id: type_str.to_string(),
                                facing: 0,
                                frame: 8 + bucket as u16 * 6 + walk_frame,
                                house_color: color_idx,
                            });
                        }
                    }
                }
            }
        }
    }

    // Step 1a: Pre-load extra building types (e.g., ConYards for MCV deployment).
    // These may not exist on the map initially but can be spawned at runtime.
    if !extra_building_types.is_empty() {
        let all_colors: Vec<HouseColorIndex> = house_colors.values().copied().collect();
        for &type_id in extra_building_types {
            for &color in &all_colors {
                needed.insert(ShpSpriteKey {
                    palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                    type_id: type_id.to_string(),
                    facing: 0,
                    frame: 0,
                    house_color: color,
                });
            }
        }
    }

    // Step 1b: Collect every declared building animation frame required by art
    // metadata. Runtime owns timing; atlas construction only ensures that a
    // live `BuildingAnimOverlays` frame is drawable.
    let mut active_anim_frame_counts: HashMap<String, u16> = HashMap::new();
    if let Some(art_reg) = art {
        let building_keys: Vec<ShpSpriteKey> = needed
            .iter()
            .filter(|k| k.facing == 0) // structures only
            .cloned()
            .collect();
        for key in &building_keys {
            let rules_image: String = rules
                .and_then(|r| r.object(&key.type_id))
                .map(|o| o.image.clone())
                .unwrap_or_else(|| key.type_id.clone());
            let art_entry: Option<&crate::rules::art_data::ArtEntry> =
                art_reg.resolve_metadata_entry(&key.type_id, &rules_image);
            if let Some(entry) = art_entry {
                for anim in &entry.building_anims {
                    // Original location: `RA2-GAME.EXE-IDB` canon,
                    // `assets.unitShpFrameSelection.ra2yr.json` and
                    // `assets.buildingDrawOffsets.ra2yr.json`: declared
                    // Idle/Super/Special frames are chosen by the live object
                    // animation state, not collapsed to atlas frame zero.
                    let anim_upper: String = anim.anim_type.to_uppercase();
                    if !active_anim_frame_counts.contains_key(&anim_upper) {
                        if let Some(count) = scan_building_anim_frame_count(
                            asset_manager,
                            art_reg,
                            &anim.anim_type,
                            anim.loop_end,
                            theater_ext,
                            theater_name,
                        ) {
                            active_anim_frame_counts.insert(anim_upper.clone(), count);
                            log::info!(
                                "Building anim {} for {}: {} frames (kind={:?}, loop_end={})",
                                anim.anim_type,
                                key.type_id,
                                count,
                                anim.kind,
                                anim.loop_end,
                            );
                        }
                    }
                    let count = active_anim_frame_counts
                        .get(&anim_upper)
                        .copied()
                        .unwrap_or(1);
                    insert_building_anim_frame_keys(
                        &mut needed,
                        &anim.anim_type,
                        count,
                        key.house_color,
                        attached_anim_palette_context(art_reg.anim_runtime_config(&anim.anim_type)),
                    );

                    for variant in [&anim.damaged_variant, &anim.garrisoned_variant] {
                        let Some(variant) = variant.as_ref() else {
                            continue;
                        };
                        let variant_type = variant.anim_type.as_str();
                        let variant_upper: String = variant_type.to_uppercase();
                        if !active_anim_frame_counts.contains_key(&variant_upper) {
                            if let Some(count) = scan_building_anim_frame_count(
                                asset_manager,
                                art_reg,
                                variant_type,
                                variant.loop_end,
                                theater_ext,
                                theater_name,
                            ) {
                                active_anim_frame_counts.insert(variant_upper.clone(), count);
                                log::info!(
                                    "Building anim variant {} for {}: {} frames (loop_end={})",
                                    variant_type,
                                    key.type_id,
                                    count,
                                    variant.loop_end,
                                );
                            }
                        }
                        let count: u16 = active_anim_frame_counts
                            .get(&variant_upper)
                            .copied()
                            .unwrap_or(1);
                        insert_building_anim_frame_keys(
                            &mut needed,
                            variant_type,
                            count,
                            key.house_color,
                            attached_anim_palette_context(
                                art_reg.anim_runtime_config(variant_type),
                            ),
                        );
                    }
                }
                // BibShape: ground-level pad SHP (e.g., refinery dock GAREFNBB).
                // Use bib name directly as atlas key so render can look it up.
                if let Some(ref bib) = entry.bib_shape {
                    needed.insert(ShpSpriteKey {
                        palette_context: if entry.terrain_palette {
                            ShpPaletteContext::Cell
                        } else {
                            ShpPaletteContext::SelectedScheme
                        },
                        type_id: bib.to_uppercase(),
                        facing: 0,
                        frame: 0,
                        house_color: key.house_color,
                    });
                    log::info!("BibShape for {}: {}", key.type_id, bib);
                }
            }
        }
    }

    // Step 1c: Pre-scan for building "make" (build-up) SHPs and add all their frames.
    // The make SHP is a separate file (e.g., GTCNSTMK.SHP) with N frames showing
    // the building assembling. We need to load it briefly to count frames, then add
    // keys for each frame so they get rendered into the atlas.
    let mut make_frame_counts: HashMap<String, u16> = HashMap::new();
    if let Some(art_reg) = art {
        let building_type_ids: Vec<(String, HouseColorIndex)> = needed
            .iter()
            .filter(|k| k.facing == 0 && k.frame == 0)
            .map(|k| (k.type_id.clone(), k.house_color))
            .collect();
        for (type_id, color) in &building_type_ids {
            // Skip anim overlay types (they don't have make SHPs).
            if type_id.contains('_') {
                continue;
            }
            let make_key: String = format!("{}_MAKE", type_id);
            if make_frame_counts.contains_key(&make_key) {
                // Already scanned this type — just add keys for this color.
                let count: u16 = make_frame_counts[&make_key];
                for f in 0..count {
                    needed.insert(ShpSpriteKey {
                        palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                        type_id: make_key.clone(),
                        facing: 0,
                        frame: f,
                        house_color: *color,
                    });
                }
                continue;
            }
            let rules_image: String = rules
                .and_then(|r| r.object(type_id))
                .map(|o| o.image.clone())
                .unwrap_or_else(|| type_id.clone());
            let image: String = art_reg.resolve_effective_image_id(type_id, &rules_image);
            let candidates: Vec<String> =
                art_data::make_shp_candidates(Some(art_reg), &image, theater_ext, theater_name);
            let shp_data: Option<&[u8]> = candidates.iter().find_map(|c| asset_manager.get_ref(c));
            if let Some(data) = shp_data {
                if let Ok(shp) = ShpFile::from_bytes(data) {
                    // RA2 make SHPs have shadow frames in the second half — only use the first half.
                    let real_frames: u16 = (shp.frames.len() as u16) / 2;
                    let frame_count: u16 = if real_frames > 0 {
                        real_frames
                    } else {
                        shp.frames.len() as u16
                    };
                    log::info!(
                        "Make SHP for {}: {} frames ({})",
                        type_id,
                        frame_count,
                        candidates
                            .iter()
                            .find(|c| asset_manager.get_ref(c).is_some())
                            .unwrap_or(&String::new()),
                    );
                    make_frame_counts.insert(make_key.clone(), frame_count);
                    for f in 0..frame_count {
                        needed.insert(ShpSpriteKey {
                            palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                            type_id: make_key.clone(),
                            facing: 0,
                            frame: f,
                            house_color: *color,
                        });
                    }
                }
            }
        }
    }

    // Step 1d: Pre-load world effect SHPs (warp animations, explosions, etc.).
    // Names come from rules.ini [General] WarpIn=/WarpOut=/WarpAway= — NOT hardcoded.
    // These use anim.pal (effect palette), not unit.pal — tracked in effect_type_ids
    // so step 2 can pick the correct palette.
    let mut effect_type_ids: HashSet<String> = HashSet::new();
    {
        let mut effect_names: Vec<String> = Vec::new();
        if let Some(r) = rules {
            effect_names.push(r.general.warp_in.name.clone());
            effect_names.push(r.general.warp_out.name.clone());
            effect_names.push(r.general.warp_away.name.clone());
            // [General] Wake= (WAKE1): an AnimClass spawned behind ships.
            // Stock art sets no AltPalette= and its Theater= line is commented
            // out, so it is an anim.pal draw like every other AnimType; left
            // out of this set it fell through to unit.pal and drew green.
            effect_names.push(r.general.wake.name.clone());
            // Add damage fire types (FIRE01, FIRE02, FIRE03 by default).
            for fire_ref in &r.general.damage_fire_types {
                if !effect_names
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(&fire_ref.name))
                {
                    effect_names.push(fire_ref.name.clone());
                }
            }
            for anim_name in r.art_registry.scheduler_anim_types() {
                if !effect_names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(anim_name))
                {
                    effect_names.push(anim_name.clone());
                }
            }
            // Collect explosion animation names from all warhead AnimList= fields.
            for wh in r.warheads_iter() {
                for anim_name in &wh.anim_list {
                    if !effect_names
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case(anim_name))
                    {
                        effect_names.push(anim_name.clone());
                    }
                }
            }
            // Collect weapon Anim= and OccupantAnim names from weapons.
            for weapon in r.weapons_iter() {
                for anim_name in &weapon.anim {
                    if !effect_names
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case(anim_name))
                    {
                        effect_names.push(anim_name.clone());
                    }
                }
                if let Some(ref anim_name) = weapon.occupant_anim {
                    if !effect_names
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case(anim_name))
                    {
                        effect_names.push(anim_name.clone());
                    }
                }
                if let Some(projectile_id) = weapon.projectile.as_deref() {
                    if let Some(projectile) = r.projectile(projectile_id) {
                        if !projectile.inviso {
                            if let Some(image) = projectile.image.as_deref() {
                                if !effect_names.iter().any(|n| n.eq_ignore_ascii_case(image)) {
                                    effect_names.push(image.to_string());
                                }
                            }
                        }
                    }
                }
            }
            // Particle SHPs: ParticleType.Image= goes through the ObjectTypeClass
            // Image= path → anim.pal palette. Register every distinct name.
            for pt in r.particle_types_iter() {
                if let Some(image) = pt.image.as_deref() {
                    if !effect_names.iter().any(|n| n.eq_ignore_ascii_case(image)) {
                        effect_names.push(image.to_string());
                    }
                }
            }
        }
        for name in &effect_names {
            // Use the same resolved Image= and Theater=/NewTheater= filename
            // authority as scheduler binding. Stock cell-drawer rows such as
            // WA01X and TUNTOP01 are theater SHPs (`.TEM` on Temperate maps).
            let candidates = effect_anim_shp_candidates(name, art, theater_ext, theater_name);
            let required_cell_drawer = cell_drawer_type_ids.contains(&name.to_ascii_uppercase());
            match candidates.iter().find_map(|c| asset_manager.get_ref(c)) {
                Some(data) => match ShpFile::from_bytes(data) {
                    Ok(shp) => {
                        let raw_count = shp.frames.len() as u16;
                        let name_upper = name.to_ascii_uppercase();
                        let scheduler_owned = art.is_some_and(|registry| {
                            registry.scheduler_anim_types().contains(&name_upper)
                        });
                        let shadow = art
                            .and_then(|registry| registry.anim_runtime_config(name))
                            .is_some_and(|config| config.shadow);
                        // Keep atlas keys and the presentation-facing count in one
                        // registration so rendering cannot select an unloaded frame.
                        let count = register_effect_anim_frames(
                            &mut needed,
                            &mut active_anim_frame_counts,
                            name,
                            raw_count,
                            scheduler_owned,
                            shadow,
                        );
                        insert_anim_remap_frame_keys(&mut needed, name, count, anim_remap_keys);
                        effect_type_ids.insert(name.clone());
                        log::info!("Effect anim SHP {}: {} frames loaded", name, count);
                    }
                    Err(error) if required_cell_drawer => {
                        return abort_sprite_atlas_refresh(
                            previous_atlas,
                            None,
                            0,
                            format!(
                                "bound cell-drawer animation [{name}] failed SHP decode: {error}"
                            ),
                        );
                    }
                    Err(_) => {}
                },
                None if required_cell_drawer => {
                    return abort_sprite_atlas_refresh(
                        previous_atlas,
                        None,
                        0,
                        format!("bound cell-drawer animation [{name}] has no SHP asset"),
                    );
                }
                None => {}
            }
        }
    }

    // Step 1e: Pre-load the parachute SHP (`[General] Parachute=`).
    // The parachute is reached through `[General] Parachute=` rather than the
    // animation closure, so its frames need registering here. Its palette is not
    // decided by this registration: `sprite_palette_choice` reads the art type's
    // `AltPalette=` flag, which PARACH sets, and that selects the unit palette.
    if let Some(r) = rules {
        if let Some(pc) = r.general.parachute_render.as_ref() {
            let lower: String = pc.shp_name.to_ascii_lowercase();
            let candidates: Vec<String> =
                vec![format!("{}.shp", lower), format!("{}.SHP", pc.shp_name)];
            if let Some(data) = candidates.iter().find_map(|c| asset_manager.get_ref(c)) {
                if let Ok(shp) = ShpFile::from_bytes(data) {
                    let frame_count: u16 = shp.frames.len() as u16;
                    for f in 0..frame_count {
                        needed.insert(ShpSpriteKey {
                            palette_context: ShpPaletteContext::GlobalAnim,
                            type_id: pc.shp_name.clone(),
                            facing: 0,
                            frame: f,
                            house_color: HouseColorIndex(0),
                        });
                    }
                    active_anim_frame_counts.insert(pc.shp_name.clone(), frame_count);
                    log::info!(
                        "Parachute SHP {}: {} frames loaded (unit palette per AltPalette=yes)",
                        pc.shp_name,
                        frame_count
                    );
                } else {
                    log::warn!(
                        "Parachute SHP {} found in MIX but failed to parse",
                        pc.shp_name
                    );
                }
            } else {
                log::warn!(
                    "Parachute SHP {} not found in MIX archives — chute will not render",
                    pc.shp_name
                );
            }
        }
    }

    let mut contextual = Vec::new();
    for key in &needed {
        if key.palette_context == ShpPaletteContext::Legacy
            && effect_type_ids.contains(&key.type_id)
        {
            let mut global = key.clone();
            global.palette_context = ShpPaletteContext::GlobalAnim;
            global.house_color = HouseColorIndex(0);
            contextual.push(global.clone());
            if cell_drawer_type_ids.contains(&key.type_id.to_ascii_uppercase()) {
                global.palette_context = ShpPaletteContext::Cell;
                contextual.push(global);
            }
        }
    }
    needed.extend(contextual);

    if needed.is_empty() {
        log::info!("No SHP sprite entities found — keeping the current sprite atlas");
        return previous_atlas;
    }

    // Step 1e: Extract cached rendered sprites from existing atlas, diff against needed.
    let previous_cache_len = previous_atlas
        .as_ref()
        .map_or(0, |atlas| atlas.rendered_cache.len());
    let mut cached: Vec<RenderedShpSprite> = previous_atlas
        .as_mut()
        .map(|atlas| std::mem::take(&mut atlas.rendered_cache))
        .unwrap_or_default();
    let cached_keys: HashSet<ShpSpriteKey> = cached.iter().map(|s| s.key.clone()).collect();
    let new_count: usize = needed.iter().filter(|k| !cached_keys.contains(k)).count();

    log::info!(
        "Sprite atlas: {} cached, {} new to render, {} total needed",
        cached.len(),
        new_count,
        needed.len(),
    );

    // Load anim.pal for world effect SHPs. Anim types (explosions, warp flashes,
    // etc.) are drawn with anim.pal, not unit.pal.
    let effect_palette: Option<Palette> = asset_manager
        .get_ref("anim.pal")
        .and_then(|d| Palette::from_bytes(d).ok());
    if effect_palette.is_none() && !effect_type_ids.is_empty() {
        log::warn!("anim.pal not found — world effect SHPs will use unit.pal (wrong colors)");
    }

    // BuildingType TerrainPalette (004612B4, DrawSHP 00705EC7) has the same
    // palette owner as cell-drawer animations. Resolve the rules Image alias.
    let mut cell_palette_type_ids = cell_drawer_type_ids.clone();
    for key in &needed {
        let base = key.type_id.strip_suffix("_MAKE").unwrap_or(&key.type_id);
        let image = rules
            .and_then(|r| r.object(base))
            .map_or(base, |o| o.image.as_str());
        if art
            .and_then(|a| a.resolve_metadata_entry(base, image))
            .is_some_and(|a| a.terrain_palette)
        {
            cell_palette_type_ids.insert(key.type_id.to_ascii_uppercase());
        }
    }
    if (!cell_palette_type_ids.is_empty()
        || needed
            .iter()
            .any(|k| k.palette_context == ShpPaletteContext::Cell))
        && cell_palette.is_none()
    {
        return abort_sprite_atlas_refresh(
            previous_atlas,
            Some(cached),
            previous_cache_len,
            "terrain-attached animations require the active theater ISO palette".to_string(),
        );
    }

    // Step 2: Render only new sprites (skip cached ones).
    for key in &needed {
        if cached_keys.contains(key) {
            continue;
        }
        let palette_choice =
            sprite_palette_for_key(key, art, &effect_type_ids, &cell_palette_type_ids);
        let pal: &Palette = match palette_choice {
            SpritePaletteChoice::Anim => effect_palette.as_ref().unwrap_or(palette),
            SpritePaletteChoice::Unit => palette,
            SpritePaletteChoice::CellIso => {
                cell_palette.expect("cell-drawer palette was validated before atlas rendering")
            }
        };
        match render_shp_sprite(
            asset_manager,
            pal,
            match key.palette_context {
                ShpPaletteContext::Legacy => palette_choice != SpritePaletteChoice::CellIso,
                _ => palette_choice == SpritePaletteChoice::Unit,
            },
            key,
            theater_ext,
            theater_name,
            rules,
            art,
        ) {
            Some(sprite) => cached.push(sprite),
            None if cell_drawer_type_ids.contains(&key.type_id.to_ascii_uppercase()) => {
                return abort_sprite_atlas_refresh(
                    previous_atlas,
                    Some(cached),
                    previous_cache_len,
                    format!(
                        "bound cell-drawer animation [{}] frame {} failed SHP rendering",
                        key.type_id, key.frame
                    ),
                );
            }
            None => log::debug!(
                "No SHP for {} (facing {}, frame {})",
                key.type_id,
                key.facing,
                key.frame
            ),
        }
    }

    // Step 2b: Render oregath.shp harvest overlay frames (if any harvesters exist).
    // Uses anim.pal (effect palette) — no house color remap. Skip if already cached.
    let has_harvesters: bool = entities.values().any(|e| e.miner.is_some());
    let oregath_cached: bool = cached_keys.contains(&ShpSpriteKey {
        palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
        type_id: "OREGATH".to_string(),
        facing: 0,
        frame: 0,
        house_color: HouseColorIndex::default(),
    });
    if has_harvesters && !oregath_cached {
        let oregath_sprites: Vec<RenderedShpSprite> = render_harvest_overlay_frames(asset_manager);
        if !oregath_sprites.is_empty() {
            log::info!(
                "Rendered {} oregath.shp harvest overlay frames",
                oregath_sprites.len()
            );
            cached.extend(oregath_sprites);
        }
    }

    if cached.is_empty() {
        log::warn!("No SHP sprites rendered");
        if previous_atlas.is_some() {
            return abort_sprite_atlas_refresh(
                previous_atlas,
                Some(cached),
                previous_cache_len,
                "sprite atlas refresh produced no rendered sprites".to_string(),
            );
        }
        return None;
    }

    log::info!(
        "Packing {} SHP sprites into atlas ({} reused from cache)",
        cached.len(),
        cached.len().saturating_sub(new_count),
    );

    // Step 3: Shelf-pack all sprites (cached + newly rendered) into atlas.
    let mut atlas: SpriteAtlas = pack_sprites(gpu, batch, &cached);
    atlas.make_frame_counts = make_frame_counts;
    atlas.active_anim_frame_counts = active_anim_frame_counts;
    atlas.building_bounds = compute_building_bounds(&atlas, entities, art, rules, interner);
    atlas.rendered_cache = cached;
    log::info!(
        "SHP sprite atlas built: {} sprites, {} pages, {} make anims, {} active anims, {} building bounds",
        atlas.sprite_count(),
        atlas.page_count(),
        atlas.make_frame_counts.len(),
        atlas.active_anim_frame_counts.len(),
        atlas.building_bounds.len(),
    );
    Some(atlas)
}

/// Compute per-building-type bounding boxes by unioning all SHP frame rects.
///
/// For each structure type in the entity store, unions the main sprite rect with
/// any building animation overlay rects (ActiveAnim, IdleAnim, etc. from art.ini).
/// This matches the original engine's `BuildingTypeClass::CalculateBoundingBox`.
fn compute_building_bounds(
    atlas: &SpriteAtlas,
    entities: &crate::sim::entity_store::EntityStore,
    art: Option<&ArtRegistry>,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> HashMap<String, BuildingBounds> {
    let mut bounds: HashMap<String, BuildingBounds> = HashMap::new();

    // Collect unique building type_ids — include both existing structures and
    // any buildings reachable via DeploysInto (e.g. MCV → ConYard) so they are
    // clickable even if they didn't exist when the atlas was first built.
    let mut building_types: HashSet<String> = entities
        .values()
        .filter(|e| e.category == EntityCategory::Structure && !e.is_voxel)
        .map(|e| interner.map_or("".to_string(), |i| i.resolve(e.type_ref()).to_string()))
        .collect();
    if let Some(r) = rules {
        let deploy_targets: Vec<String> = entities
            .values()
            .filter_map(|e| {
                let t = interner.map_or("", |i| i.resolve(e.type_ref()));
                r.object(t).and_then(|o| o.deploys_into.clone())
            })
            .collect();
        building_types.extend(deploy_targets);
    }

    for type_id in &building_types {
        // Find any atlas entry for the main building sprite (frame 0, any house color).
        let main_entry = atlas
            .entries_iter()
            .find(|(k, _)| k.type_id == *type_id && k.frame == 0)
            .map(|(_, v)| *v);
        let Some(main) = main_entry else { continue };

        // Initialize bbox from main sprite.
        let mut min_x: f32 = main.canvas_rect[0];
        let mut min_y: f32 = main.canvas_rect[1];
        let mut max_x: f32 = main.canvas_rect[0] + main.canvas_rect[2];
        let mut max_y: f32 = main.canvas_rect[1] + main.canvas_rect[3];

        // Union all other frames of the main sprite (animation frames).
        for (k, v) in atlas.entries_iter() {
            if k.type_id == *type_id {
                min_x = min_x.min(v.canvas_rect[0]);
                min_y = min_y.min(v.canvas_rect[1]);
                max_x = max_x.max(v.canvas_rect[0] + v.canvas_rect[2]);
                max_y = max_y.max(v.canvas_rect[1] + v.canvas_rect[3]);
            }
        }

        // Union with building animation overlay sprites (ActiveAnim, IdleAnim, etc.).
        if let Some(art_reg) = art {
            let rules_image: String = rules
                .and_then(|r| r.object(type_id))
                .map(|o| o.image.clone())
                .unwrap_or_else(|| type_id.clone());
            if let Some(art_entry) = art_reg.resolve_metadata_entry(type_id, &rules_image) {
                for anim in &art_entry.building_anims {
                    // Anim overlays are offset by (anim.x, anim.y) pixels from building origin.
                    for anim_type in [
                        Some(anim.anim_type.as_str()),
                        anim.damaged_variant.as_ref().map(|v| v.anim_type.as_str()),
                        anim.garrisoned_variant
                            .as_ref()
                            .map(|v| v.anim_type.as_str()),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        for (k, v) in atlas.entries_iter() {
                            if k.type_id == anim_type {
                                let ax: f32 = anim.x as f32 + v.canvas_rect[0];
                                let ay: f32 = anim.y as f32 + v.canvas_rect[1];
                                min_x = min_x.min(ax);
                                min_y = min_y.min(ay);
                                max_x = max_x.max(ax + v.canvas_rect[2]);
                                max_y = max_y.max(ay + v.canvas_rect[3]);
                            }
                        }
                    }
                }
            }
        }

        bounds.insert(
            type_id.clone(),
            BuildingBounds {
                min_x,
                min_y,
                width: max_x - min_x,
                height: max_y - min_y,
            },
        );
    }

    bounds
}

/// Load and render a single SHP sprite to RGBA pixels.
///
/// Uses ArtRegistry to resolve the correct filename (with NewTheater substitution).
/// Falls back to direct {TYPE_ID}.SHP if art data is unavailable.
/// Selects the appropriate frame based on facing (8-direction for infantry).
fn render_shp_sprite(
    asset_manager: &AssetManager,
    palette: &Palette,
    house_remap: bool,
    key: &ShpSpriteKey,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
) -> Option<RenderedShpSprite> {
    // Check if this is a make (build-up) SHP — type_id ends with "_MAKE".
    let is_make: bool = key.type_id.ends_with("_MAKE");
    let base_type_id: &str = if is_make {
        &key.type_id[..key.type_id.len() - 5]
    } else {
        &key.type_id
    };

    // Resolve image name: type_id → rules.ini Image= → art.ini Image= override.
    let rules_image: String = rules
        .and_then(|r| r.object(base_type_id))
        .map(|o| o.image.clone())
        .unwrap_or_else(|| base_type_id.to_string());
    let image: String = art
        .map(|a| a.resolve_effective_image_id(base_type_id, &rules_image))
        .unwrap_or_else(|| rules_image.to_uppercase());

    // Build filename candidates — use make candidates for _MAKE types.
    let candidates: Vec<String> = if is_make {
        art_data::make_shp_candidates(art, &image, theater_ext, theater_name)
    } else {
        art_data::object_shp_candidates(art, &image, theater_ext, theater_name)
    };

    // Try each candidate in order until one is found.
    let mut lookup_result: Option<(&[u8], String)> = None;
    for name in &candidates {
        if let Some(data) = asset_manager.get_ref(name) {
            lookup_result = Some((data, name.clone()));
            break;
        }
    }
    let (shp_data, found_name) = match lookup_result {
        Some(pair) => pair,
        None => {
            log::warn!("SHP not found for {}: tried {:?}", key.type_id, candidates);
            return None;
        }
    };
    // Log when a non-first candidate was selected (generic 'G' fallback is normal).
    if candidates.len() > 2 && found_name != candidates[0] {
        log::debug!(
            "Theater fallback for {}: wanted '{}' but loaded '{}' (tried {:?})",
            key.type_id,
            candidates[0],
            found_name,
            candidates,
        );
    }
    log::trace!("Loaded SHP: {} ({} bytes)", found_name, shp_data.len());
    let shp: ShpFile = match ShpFile::from_bytes(shp_data) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Failed to parse {}: {}", found_name, e);
            return None;
        }
    };

    if shp.frames.is_empty() {
        log::warn!("{} has no frames", found_name);
        return None;
    }

    log::debug!(
        "{}: {}x{}, {} frames",
        found_name,
        shp.width,
        shp.height,
        shp.frames.len()
    );

    // Frame selection: use facing to pick among the first 8 frames (standing pose).
    // If SHP has fewer frames, use frame 0.
    let frame_idx: usize = if (key.frame as usize) < shp.frames.len() {
        key.frame as usize
    } else if shp.frames.len() >= 8 {
        (INFANTRY_FACING_BUCKETS as usize
            - (canonical_infantry_facing(key.facing) as usize / INFANTRY_FACING_STEP as usize))
            % INFANTRY_FACING_BUCKETS as usize
    } else {
        0
    };

    let frame = &shp.frames[frame_idx];
    if frame.frame_width == 0 || frame.frame_height == 0 {
        log::warn!(
            "{} frame {} is empty ({}x{})",
            found_name,
            frame_idx,
            frame.frame_width,
            frame.frame_height
        );
        return None;
    }

    // Apply house color remapping: replace palette indices 16–31 with the house's
    // `[Colors]` team-color band. NO_REMAP owners (civilian/neutral/special) keep
    // the raw theater palette — matching the GPU voxel path (which samples row 0,
    // the unremapped [16,32) range) and the documented NO_REMAP intent.
    let remapped_pal: Palette;
    let render_pal: &Palette =
        if !house_remap || key.house_color == crate::rules::house_colors::NO_REMAP {
            palette
        } else {
            let default_ramps = HouseColorRamps::default();
            let ramps = rules
                .map(|r| &r.house_color_ramps)
                .unwrap_or(&default_ramps);
            remapped_pal = palette.with_house_colors(ramps.ramp(key.house_color));
            &remapped_pal
        };

    let frame_rgba: Vec<u8> = match shp.frame_to_rgba(frame_idx, render_pal) {
        Ok(rgba) => rgba,
        Err(e) => {
            log::warn!(
                "Failed to convert {} frame {}: {}",
                found_name,
                frame_idx,
                e
            );
            return None;
        }
    };

    // CC_Draw_Shape @ 0x004AED70 centers the SHP canvas, then adds the
    // stored frame origin and passes its width/height to the blitter. Padding
    // to the canvas preserved color placement but seeded Z from the wrong rect.
    let full_w: u32 = shp.width as u32;
    let full_h: u32 = shp.height as u32;
    let fw: u32 = frame.frame_width as u32;
    let fh: u32 = frame.frame_height as u32;
    let fx: u32 = frame.frame_x as u32;
    let fy: u32 = frame.frame_y as u32;

    log::debug!(
        "SHP {} facing={}: {}x{} frame {}/{}, sub {}x{} at ({},{})",
        found_name,
        key.facing,
        full_w,
        full_h,
        frame_idx,
        shp.frames.len(),
        fw,
        fh,
        fx,
        fy,
    );

    // Center the logical canvas, then place its stored frame.
    // DrawOffset from art.ini XDrawOffset/YDrawOffset for per-type fine-tuning.
    // Uses integer division: -ShapeWidth/2 (truncated), to avoid sub-pixel drift
    // on odd-dimension SHPs.
    let (xdo, ydo) = art.map(|a| a.draw_offsets(&key.type_id)).unwrap_or((0, 0));
    let offset_x: f32 = -((full_w / 2) as f32) + xdo as f32;
    let offset_y: f32 = -((full_h / 2) as f32) + ydo as f32;

    Some(RenderedShpSprite {
        key: key.clone(),
        rgba: frame_rgba,
        indices: frame.pixels.clone(),
        width: fw,
        height: fh,
        offset_x: offset_x + fx as f32,
        offset_y: offset_y + fy as f32,
        canvas_rect: [offset_x, offset_y, full_w as f32, full_h as f32],
        extended: frame.format & 2 != 0,
    })
}

/// Canonicalize RA2 facing byte to one of the 8 infantry-facing buckets.
pub fn canonical_infantry_facing(facing: u8) -> u8 {
    (facing / INFANTRY_FACING_STEP) * INFANTRY_FACING_STEP
}

/// Render oregath.shp harvest overlay frames using the effect palette (anim.pal).
///
/// OREGATH uses anim.pal (the same palette used for explosion/effect SHPs),
/// with no house color remap.
///
/// Returns all 120 SHP frames (15 animation frames x 8 facings) as rendered sprites.
/// Each frame is keyed by OREGATH, facing zero and its SHP frame index.
/// At render time, the correct frame is: `facing_index * 15 + anim_frame`.
fn render_harvest_overlay_frames(asset_manager: &AssetManager) -> Vec<RenderedShpSprite> {
    // Load effect palette (anim.pal).
    let pal_data: &[u8] = match asset_manager.get_ref("anim.pal") {
        Some(d) => d,
        None => {
            log::warn!("anim.pal not found — skipping harvest overlay");
            return Vec::new();
        }
    };
    let palette: Palette = match Palette::from_bytes(pal_data) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to parse anim.pal: {} — skipping harvest overlay", e);
            return Vec::new();
        }
    };

    // Load oregath.shp.
    let shp_data: &[u8] = match asset_manager.get_ref("oregath.shp") {
        Some(d) => d,
        None => {
            log::warn!("oregath.shp not found — skipping harvest overlay");
            return Vec::new();
        }
    };
    let shp: ShpFile = match ShpFile::from_bytes(shp_data) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "Failed to parse oregath.shp: {} — skipping harvest overlay",
                e
            );
            return Vec::new();
        }
    };

    // RA2 SHP files often have shadow frames in the second half.
    // Use only the first half (real frames).
    let total_frames: usize = shp.frames.len();
    let real_frames: usize = if total_frames > 120 {
        total_frames / 2
    } else {
        total_frames
    };
    log::info!(
        "oregath.shp: {}x{}, {} total frames, {} real frames",
        shp.width,
        shp.height,
        total_frames,
        real_frames,
    );

    let hc: HouseColorIndex = HouseColorIndex::default();
    let mut sprites: Vec<RenderedShpSprite> = Vec::with_capacity(real_frames);

    let canvas_w: u32 = shp.width as u32;
    let canvas_h: u32 = shp.height as u32;

    for frame_idx in 0..real_frames {
        let frame = &shp.frames[frame_idx];
        if frame.frame_width == 0 || frame.frame_height == 0 {
            continue;
        }
        // Render with effect palette — no house color remap.
        let frame_rgba: Vec<u8> = match shp.frame_to_rgba(frame_idx, &palette) {
            Ok(rgba) => rgba,
            Err(_) => continue,
        };

        // Use per-frame dimensions instead of the full SHP canvas.
        // oregath.shp's canvas encompasses all 8 facings, so it's much larger
        // than any single frame. Using the canvas size makes the overlay huge.
        let fw: u32 = frame.frame_width as u32;
        let fh: u32 = frame.frame_height as u32;
        let fx: u32 = frame.frame_x as u32;
        let fy: u32 = frame.frame_y as u32;

        // Offset: frame position within canvas, relative to canvas center.
        // This positions the sub-frame correctly relative to the unit center.
        let offset_x: f32 = fx as f32 - (canvas_w as f32) / 2.0;
        let offset_y: f32 = fy as f32 - (canvas_h as f32) / 2.0;

        sprites.push(RenderedShpSprite {
            key: ShpSpriteKey {
                palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                type_id: "OREGATH".to_string(),
                facing: 0,
                frame: frame_idx as u16,
                house_color: hc,
            },
            rgba: frame_rgba,
            indices: frame.pixels.clone(),
            width: fw,
            height: fh,
            offset_x,
            offset_y,
            canvas_rect: [offset_x, offset_y, fw as f32, fh as f32],
            extended: frame.format & 2 != 0,
        });
    }

    sprites
}

/// Shelf-pack rendered SHP sprites into a multi-page GPU texture atlas.
///
/// Sprites are packed into pages of at most `max_texture_dim × max_texture_dim`
/// pixels each. Most maps fit in a single page; pages are added only when the
/// total sprite area exceeds what one GPU texture can hold.
fn blit_sprite_pixels(
    sprite: &RenderedShpSprite,
    position: [u32; 2],
    page_width: u32,
    rgba: &mut [u8],
    indices: &mut [u8],
) {
    for y in 0..sprite.height {
        let src = (y * sprite.width) as usize;
        let dst = ((position[1] + y) * page_width + position[0]) as usize;
        let width = sprite.width as usize;
        rgba[dst * 4..(dst + width) * 4].copy_from_slice(&sprite.rgba[src * 4..(src + width) * 4]);
        indices[dst..dst + width].copy_from_slice(&sprite.indices[src..src + width]);
    }
}

#[cfg(test)]
pub(crate) use tests::native_palette_probe_page;

fn pack_sprites(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    sprites: &[RenderedShpSprite],
) -> SpriteAtlas {
    // Sort by height descending for shelf packing efficiency.
    let mut indices: Vec<usize> = (0..sprites.len()).collect();
    indices.sort_by(|&a, &b| sprites[b].height.cmp(&sprites[a].height));

    // Estimate atlas width from total pixel area.
    let total_area: u64 = sprites
        .iter()
        .map(|s| {
            (s.width as u64 + SPRITE_PADDING as u64) * (s.height as u64 + SPRITE_PADDING as u64)
        })
        .sum();
    let estimated_side: u32 = (total_area as f64).sqrt().ceil() as u32;
    let max_texture_dim: u32 = gpu.device.limits().max_texture_dimension_2d;
    let mut atlas_width: u32 = estimated_side.clamp(64, max_texture_dim);

    // Try widening atlas to fit everything in a single page.
    loop {
        let trial_height: u32 = simulate_shelf_height(&indices, sprites, atlas_width);
        if trial_height <= max_texture_dim || atlas_width >= max_texture_dim {
            break;
        }
        atlas_width = (atlas_width.saturating_mul(2)).min(max_texture_dim);
    }

    // Shelf-pack with page splitting when height exceeds GPU limit.
    struct Placement {
        idx: usize,
        page: u8,
        px: u32,
        py: u32,
    }
    let mut placements: Vec<Placement> = Vec::with_capacity(sprites.len());
    let mut cursor_x: u32 = 0;
    let mut cursor_y: u32 = 0;
    let mut shelf_height: u32 = 0;
    let mut current_page: u8 = 0;

    for &idx in &indices {
        let w: u32 = sprites[idx].width;
        let h: u32 = sprites[idx].height;
        if cursor_x + w > atlas_width {
            let new_y: u32 = cursor_y + shelf_height + SPRITE_PADDING;
            if new_y + h > max_texture_dim {
                // Current page is full — start a new page.
                current_page += 1;
                cursor_x = 0;
                cursor_y = 0;
                shelf_height = 0;
            } else {
                cursor_y = new_y;
                cursor_x = 0;
                shelf_height = 0;
            }
        }
        placements.push(Placement {
            idx,
            page: current_page,
            px: cursor_x,
            py: cursor_y,
        });
        cursor_x += w + SPRITE_PADDING;
        shelf_height = shelf_height.max(h);
    }

    let num_pages: usize = current_page as usize + 1;
    if num_pages > 1 {
        log::info!(
            "Sprite atlas split into {} pages (GPU texture limit {})",
            num_pages,
            max_texture_dim,
        );
    }

    // Build each page's GPU texture.
    let mut pages: Vec<SpriteAtlasPage> = Vec::with_capacity(num_pages);
    let mut entries: HashMap<ShpSpriteKey, ShpSpriteEntry> =
        HashMap::with_capacity(placements.len());
    for page_idx in 0..num_pages as u8 {
        let page_height: u32 = placements
            .iter()
            .filter(|p| p.page == page_idx)
            .map(|p| p.py + sprites[p.idx].height)
            .max()
            .unwrap_or(1);

        let mut rgba: Vec<u8> = vec![0u8; (atlas_width * page_height * 4) as usize];
        let mut source_indices = vec![0u8; (atlas_width * page_height) as usize];
        let aw: f32 = atlas_width as f32;
        let ah: f32 = page_height as f32;

        for p in placements.iter().filter(|p| p.page == page_idx) {
            let rs: &RenderedShpSprite = &sprites[p.idx];
            let w: u32 = rs.width;
            let h: u32 = rs.height;

            blit_sprite_pixels(
                rs,
                [p.px, p.py],
                atlas_width,
                &mut rgba,
                &mut source_indices,
            );

            entries.insert(
                rs.key.clone(),
                ShpSpriteEntry {
                    uv_origin: [p.px as f32 / aw, p.py as f32 / ah],
                    uv_size: [w as f32 / aw, h as f32 / ah],
                    pixel_size: [w as f32, h as f32],
                    offset_x: rs.offset_x,
                    offset_y: rs.offset_y,
                    canvas_rect: rs.canvas_rect,
                    extended: rs.extended,
                    page: page_idx,
                },
            );
        }

        let texture: BatchTexture = batch.create_texture_with_indices(
            gpu,
            &rgba,
            atlas_width,
            page_height,
            Some(&source_indices),
        );
        pages.push(SpriteAtlasPage { texture });
    }

    SpriteAtlas {
        pages,
        entries,
        make_frame_counts: HashMap::new(),
        active_anim_frame_counts: HashMap::new(),
        building_bounds: HashMap::new(),
        rendered_cache: Vec::new(), // caller sets this after packing
    }
}

/// Simulate shelf-packing to determine total height without allocating buffers.
fn simulate_shelf_height(
    indices: &[usize],
    sprites: &[RenderedShpSprite],
    atlas_width: u32,
) -> u32 {
    let mut cursor_x: u32 = 0;
    let mut cursor_y: u32 = 0;
    let mut shelf_height: u32 = 0;
    for &idx in indices {
        let w: u32 = sprites[idx].width;
        let h: u32 = sprites[idx].height;
        if cursor_x + w > atlas_width {
            cursor_y += shelf_height + SPRITE_PADDING;
            cursor_x = 0;
            shelf_height = 0;
        }
        cursor_x += w + SPRITE_PADDING;
        shelf_height = shelf_height.max(h);
    }
    cursor_y + shelf_height
}

#[cfg(test)]
#[path = "sprite_atlas_tests.rs"]
mod tests;

#[cfg(test)]
impl SpriteAtlas {
    pub(crate) fn from_test_pages(pages: Vec<SpriteAtlasPage>) -> Self {
        Self {
            pages,
            entries: HashMap::new(),
            make_frame_counts: HashMap::new(),
            active_anim_frame_counts: HashMap::new(),
            building_bounds: HashMap::new(),
            rendered_cache: Vec::new(),
        }
    }
}
