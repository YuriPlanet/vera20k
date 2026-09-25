//! SHP sprite atlas — pre-renders SHP sprites into packed GPU texture pages.
//!
//! At map load every SHP sprite the world can draw — infantry sequences,
//! building bodies with their make and overlay animations, world effects — is
//! rendered to RGBA and shelf-packed into as few pages as the GPU texture limit
//! allows. When a new (type, house colour) appears mid-match, only its missing
//! sprites are rendered and appended to a growth page (`atlas_growth`); the
//! pages already resident are never repacked or re-uploaded. Keys with nothing
//! to draw are remembered, so no refresh retries them.
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
use std::time::Instant;

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::render::atlas_growth::{self, SPRITE_PADDING, ShelfCursor};
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::rules::art_data::{self, ArtRegistry};
use crate::rules::effect_asset_catalog::available_effect_anim_frame_count;
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};
use crate::rules::ruleset::RuleSet;

/// Edge of a growth page: RGBA plus indices is 80 MB, room for the sprites
/// of about ten new (type, house colour) pairs.
const GROWTH_PAGE_SIZE: u32 = 4096;
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

/// First existing file among `candidates`, with its bytes.
fn find_shp<'a, 'n>(
    asset_manager: &'a AssetManager,
    candidates: &'n [String],
) -> Option<(&'n str, &'a [u8])> {
    candidates.iter().find_map(|name| {
        asset_manager
            .get_ref(name)
            .map(|data| (name.as_str(), data))
    })
}

/// A building animation's usable frame count and the file it was counted in,
/// resolved as AnimTypeClass loads its image. Only the SHP header is read.
fn scan_building_anim_frame_count(
    asset_manager: &AssetManager,
    art_reg: &ArtRegistry,
    anim_type: &str,
    loop_end: u16,
    theater_ext: &str,
    theater_name: &str,
) -> Option<(u16, String)> {
    let anim_image: String = art_reg.resolve_effective_image_id(anim_type, anim_type);
    let candidates: Vec<String> = art_data::anim_shp_candidates(
        Some(art_reg),
        anim_type,
        &anim_image,
        theater_ext,
        theater_name,
    );
    let (file, data) = find_shp(asset_manager, &candidates)?;
    let raw_count = ShpFile::frame_count_from_bytes(data).ok()?;
    let real: u16 = raw_count / 2;
    let required_count = loop_end.max(1);
    let count = if real > 0 && real >= required_count {
        real
    } else if raw_count >= required_count {
        required_count
    } else {
        raw_count
    };
    Some((count, file.to_string()))
}

/// Register every frame of one building animation (ActiveAnim, IdleAnim and
/// their damaged or garrisoned variants) for one house colour, counting the
/// frames once per build.
#[allow(clippy::too_many_arguments)]
fn register_building_anim(
    needed: &mut HashSet<ShpSpriteKey>,
    frame_counts: &mut HashMap<String, u16>,
    shp_files: &mut HashMap<String, String>,
    asset_manager: &AssetManager,
    art: &ArtRegistry,
    anim_type: &str,
    loop_end: u16,
    house_color: HouseColorIndex,
    theater_ext: &str,
    theater_name: &str,
) {
    let upper = anim_type.to_ascii_uppercase();
    if !frame_counts.contains_key(&upper)
        && let Some((count, file)) = scan_building_anim_frame_count(
            asset_manager,
            art,
            anim_type,
            loop_end,
            theater_ext,
            theater_name,
        )
    {
        log::debug!("Building anim {anim_type}: {count} frames from {file}");
        frame_counts.insert(upper.clone(), count);
        shp_files.insert(upper.clone(), file);
    }
    let count = frame_counts.get(&upper).copied().unwrap_or(1);
    insert_building_anim_frame_keys(
        needed,
        anim_type,
        count,
        house_color,
        attached_anim_palette_context(art.anim_runtime_config(anim_type)),
    );
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

/// The page refreshes append to, and the palette-index texture that pairs
/// with its RGBA texture (the bind group holds only a view of it).
struct GrowthPage {
    page: usize,
    source_indices: wgpu::Texture,
    shelf: ShelfCursor,
}

/// A multi-page GPU texture atlas containing pre-rendered SHP sprites.
///
/// When the total sprite area exceeds the GPU texture limit, sprites are
/// split across multiple pages. Each page is an independent GPU texture
/// with its own bind group. The entry lookup returns a `page` index that
/// identifies which page's texture to bind when drawing that sprite.
///
/// Created once at map load; a refresh appends the sprites of newly seen
/// (type, house colour) pairs to growth pages without touching the rest.
pub struct SpriteAtlas {
    /// Atlas pages — each is a separate GPU texture. The map-load pack fills
    /// as few as the texture limit allows; refreshes append growth pages.
    pub pages: Vec<SpriteAtlasPage>,
    /// Lookup: (type_id, facing, frame, house_color) → UV rectangle + offset + page.
    entries: HashMap<ShpSpriteKey, ShpSpriteEntry>,
    /// Building type → number of make (build-up) animation frames.
    /// Key is the base type_id (e.g., "GACNST"), not the "_MAKE" suffixed key.
    pub make_frame_counts: HashMap<String, u16>,
    /// Building/world-animation type → available non-shadow frame count.
    /// Building consumers use this only to ensure their live animation frame is
    /// resident; the AnimClass and projectile-image presentation
    /// (`app/presentation/instances/overlays.rs`) reads the same map.
    pub active_anim_frame_counts: HashMap<String, u16>,
    /// Keys with nothing to draw: the SHP is missing or fails to decode, or
    /// the frame is empty. Remembered so a refresh never renders them again.
    unrenderable: HashSet<ShpSpriteKey>,
    /// Object (type, house colour) pairs whose sprites have been collected.
    covered_objects: HashSet<(String, HouseColorIndex)>,
    /// AnimClass (type, ColorScheme) remaps whose sprites have been collected.
    covered_anim_remaps: HashSet<(String, HouseColorIndex)>,
    /// Whether the harvest overlay frames were rendered (or found missing).
    harvest_overlay_loaded: bool,
    /// Page refreshes append to; created when the first refresh needs room.
    growth: Option<GrowthPage>,
}

fn push_effect_name(effect_names: &mut Vec<String>, name: &str) {
    if !effect_names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        effect_names.push(name.to_string());
    }
}

/// Every world-effect SHP the atlas preloads under the effect palette
/// (`anim.pal`), in first-mention order. Names come from the loaded rules,
/// never from a hand-written list.
fn collect_effect_names(rules: &RuleSet) -> Vec<String> {
    let mut effect_names: Vec<String> = Vec::new();
    push_effect_name(&mut effect_names, &rules.general.warp_in.name);
    push_effect_name(&mut effect_names, &rules.general.warp_out.name);
    push_effect_name(&mut effect_names, &rules.general.warp_away.name);
    // [General] Wake= (WAKE1): an AnimClass spawned behind ships.
    // Stock art sets no AltPalette= and its Theater= line is commented
    // out, so it is an anim.pal draw like every other AnimType; left
    // out of this set it fell through to unit.pal and drew green.
    push_effect_name(&mut effect_names, &rules.general.wake.name);
    // Damage fire types (FIRE01, FIRE02, FIRE03 by default).
    for fire_ref in &rules.general.damage_fire_types {
        push_effect_name(&mut effect_names, &fire_ref.name);
    }
    for anim_name in rules.art_registry.scheduler_anim_types() {
        push_effect_name(&mut effect_names, anim_name);
    }
    // Explosion animations from every warhead's AnimList=.
    for wh in rules.warheads_iter() {
        for anim_name in &wh.anim_list {
            push_effect_name(&mut effect_names, anim_name);
        }
    }
    // Weapon Anim=, OccupantAnim= and visible projectile images.
    for weapon in rules.weapons_iter() {
        for anim_name in &weapon.anim {
            push_effect_name(&mut effect_names, anim_name);
        }
        if let Some(ref anim_name) = weapon.occupant_anim {
            push_effect_name(&mut effect_names, anim_name);
        }
        if let Some(projectile) = weapon
            .projectile
            .as_deref()
            .and_then(|id| rules.projectile(id))
            && !projectile.inviso
            && let Some(image) = projectile.image.as_deref()
        {
            push_effect_name(&mut effect_names, image);
        }
    }
    // Particle SHPs: ParticleType.Image= goes through the ObjectTypeClass
    // Image= path -> anim.pal palette. Register every distinct name.
    for pt in rules.particle_types_iter() {
        if let Some(image) = pt.image.as_deref() {
            push_effect_name(&mut effect_names, image);
        }
    }
    effect_names
}

impl SpriteAtlas {
    fn new(pages: Vec<SpriteAtlasPage>, entries: HashMap<ShpSpriteKey, ShpSpriteEntry>) -> Self {
        Self {
            pages,
            entries,
            make_frame_counts: HashMap::new(),
            active_anim_frame_counts: HashMap::new(),
            unrenderable: HashSet::new(),
            covered_objects: HashSet::new(),
            covered_anim_remaps: HashSet::new(),
            harvest_overlay_loaded: false,
            growth: None,
        }
    }

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

    /// Whether every object (type, house colour) and AnimClass colour remap
    /// the world can draw has been collected, so no refresh is needed. Keys
    /// found unrenderable count as collected: a sprite with nothing to draw
    /// never forces another refresh.
    pub fn covers(
        &self,
        objects: &HashSet<(String, HouseColorIndex)>,
        anim_remaps: &HashSet<(String, HouseColorIndex)>,
    ) -> bool {
        objects.is_subset(&self.covered_objects) && anim_remaps.is_subset(&self.covered_anim_remaps)
    }

    /// Place sprites rendered after the map-load pack on growth pages and
    /// upload the rows they fill, one write per page and texture. A sprite
    /// larger than any page can hold is remembered as unrenderable.
    fn append_sprites(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        mut sprites: Vec<RenderedShpSprite>,
    ) {
        // Tallest first keeps each shelf tight.
        sprites.sort_by_key(|sprite| std::cmp::Reverse(sprite.height));
        let max_dim = device.limits().max_texture_dimension_2d;
        // A fresh shelf keeps resident sprites out of the rows uploaded below.
        if let Some(growth) = self.growth.as_mut() {
            growth.shelf.start_new_shelf();
        }
        let mut band: Vec<([u32; 2], RenderedShpSprite)> = Vec::new();
        for sprite in sprites {
            let [width, height] = [sprite.width, sprite.height];
            let placed = self
                .growth
                .as_mut()
                .and_then(|growth| growth.shelf.place(width, height));
            let origin = match placed {
                Some(origin) => origin,
                None => {
                    self.upload_band(queue, std::mem::take(&mut band));
                    let growth = self.add_growth_page(device, batch, [width, height], max_dim);
                    match growth.shelf.place(width, height) {
                        Some(origin) => origin,
                        None => {
                            log::warn!(
                                "{} frame {} ({width}x{height}) exceeds the texture limit {max_dim}",
                                sprite.key.type_id,
                                sprite.key.frame,
                            );
                            self.unrenderable.insert(sprite.key);
                            continue;
                        }
                    }
                }
            };
            band.push((origin, sprite));
        }
        self.upload_band(queue, band);
    }

    /// Upload sprites placed on the current growth page and record them.
    fn upload_band(&mut self, queue: &wgpu::Queue, band: Vec<([u32; 2], RenderedShpSprite)>) {
        let Some(growth) = self.growth.as_ref() else {
            return;
        };
        let texture = &self.pages[growth.page].texture;
        let rects = |texels: fn(&RenderedShpSprite) -> &[u8]| {
            band.iter()
                .map(|(origin, sprite)| (*origin, [sprite.width, sprite.height], texels(sprite)))
                .collect::<Vec<_>>()
        };
        atlas_growth::write_band(queue, texture.view.texture(), 4, &rects(|s| &s.rgba));
        atlas_growth::write_band(queue, &growth.source_indices, 1, &rects(|s| &s.indices));
        let page = growth.page;
        let page_size = [texture.width, texture.height];
        for (origin, sprite) in band {
            let entry = atlas_entry(&sprite, origin, page, page_size);
            self.entries.insert(sprite.key, entry);
        }
    }

    fn add_growth_page(
        &mut self,
        device: &wgpu::Device,
        batch: &BatchRenderer,
        sprite: [u32; 2],
        max_dim: u32,
    ) -> &mut GrowthPage {
        let width = GROWTH_PAGE_SIZE.max(sprite[0]).min(max_dim);
        let height = GROWTH_PAGE_SIZE.max(sprite[1]).min(max_dim);
        let (texture, source_indices) =
            batch.create_blank_texture_with_indices(device, width, height);
        self.pages.push(SpriteAtlasPage { texture });
        log::info!(
            "Sprite atlas growth page {} ({width}x{height})",
            self.pages.len() - 1
        );
        self.growth.insert(GrowthPage {
            page: self.pages.len() - 1,
            source_indices,
            shelf: ShelfCursor::new(width, height),
        })
    }
}

/// UV rectangle and draw offsets of `sprite` placed at `origin` on a page.
fn atlas_entry(
    sprite: &RenderedShpSprite,
    origin: [u32; 2],
    page: usize,
    page_size: [u32; 2],
) -> ShpSpriteEntry {
    let [page_width, page_height] = page_size.map(|v| v as f32);
    ShpSpriteEntry {
        uv_origin: [
            origin[0] as f32 / page_width,
            origin[1] as f32 / page_height,
        ],
        uv_size: [
            sprite.width as f32 / page_width,
            sprite.height as f32 / page_height,
        ],
        pixel_size: [sprite.width as f32, sprite.height as f32],
        offset_x: sprite.offset_x,
        offset_y: sprite.offset_y,
        canvas_rect: sprite.canvas_rect,
        extended: sprite.extended,
        page: u8::try_from(page).expect("sprite atlas page index fits in u8"),
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

/// End a refresh without changing the atlas the renderer is already using.
///
/// A refresh mutates the prior atlas only after every fallible step has
/// passed, so returning it here leaves it exactly as it was. Initial
/// construction deliberately has no fallback and keeps the existing fail-fast
/// contract for required cell-drawer assets.
#[cold]
fn abort_sprite_atlas_refresh(
    previous_atlas: Option<SpriteAtlas>,
    failure: String,
) -> Option<SpriteAtlas> {
    let Some(previous) = previous_atlas else {
        panic!("{failure}");
    };
    log::warn!("{failure}; keeping the previous valid sprite atlas");
    Some(previous)
}

/// Collect the base set of (type_id, house_color) pairs from the ECS world.
///
/// Every sprite key an object can need derives from one of these pairs, so a
/// refresh is needed only when the world holds a pair the atlas has not
/// collected yet (see [`SpriteAtlas::covers`]).
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

/// Every sprite key one object (type, house colour) can be drawn with: a
/// structure's body frames, or an infantry-style sequence set.
fn insert_object_keys(
    needed: &mut HashSet<ShpSpriteKey>,
    type_str: &str,
    category: EntityCategory,
    color_idx: HouseColorIndex,
    rules: Option<&RuleSet>,
) {
    let mut insert = |frame: u16| {
        needed.insert(ShpSpriteKey {
            palette_context: ShpPaletteContext::Legacy,
            type_id: type_str.to_string(),
            facing: 0,
            frame,
            house_color: color_idx,
        });
    };
    match category {
        EntityCategory::Structure => {
            insert(0);
            // CanBeOccupied buildings need frames 0..3 for the occupancy +
            // damage-tier frame swap (see building_frame_index in
            // app/presentation/instances/shp.rs). SHPs with fewer frames silently skip
            // missing entries; the renderer falls back to frame 0.
            let can_be_occupied = rules
                .and_then(|r| r.object(type_str))
                .is_some_and(|obj| obj.can_be_occupied);
            if can_be_occupied {
                for frame in 1u16..=3 {
                    insert(frame);
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
                                // Use facing=0 for all infantry keys — the absolute
                                // frame index already encodes the facing direction.
                                // This avoids cache key mismatches for non-8-facing
                                // sequences (most RA2 infantry use 6 facings).
                                insert(
                                    seq_def.start_frame
                                        + f_idx as u16 * seq_def.facing_multiplier
                                        + frame_offset,
                                );
                            }
                        }
                    }
                }
            } else {
                // Fallback: hardcoded default layout (stand=0-7, walk=8-55).
                // Use facing=0 for all keys — frame index encodes direction.
                for bucket in 0..INFANTRY_FACING_BUCKETS {
                    insert(bucket as u16);
                    for walk_frame in 0..6u16 {
                        insert(8 + bucket as u16 * 6 + walk_frame);
                    }
                }
            }
        }
    }
}

/// Groups keys by type, so each type's SHP is decoded once, in a stable order.
fn sprite_key_order(key: &ShpSpriteKey) -> (&str, u8, u8, u16, u8) {
    (
        key.type_id.as_str(),
        key.palette_context as u8,
        key.house_color.0,
        key.frame,
        key.facing,
    )
}

/// Build or refresh the SHP sprite atlas for every object in the ECS world.
///
/// Without an `existing` atlas every needed key is rendered and shelf-packed
/// into new pages. With one, only the keys it neither holds nor has found
/// unrenderable are rendered, and they are appended to a growth page; the
/// resident pages are not touched.
///
/// 1. Collects every key the world's object (type, house colour) pairs, their
///    building and make animations, the rules' world effects and the live
///    AnimClass colour remaps can draw.
/// 2. Renders the keys not yet resolved, decoding each type's SHP once.
/// 3. Packs them into new pages at map load, onto growth pages afterwards.
///
/// Returns None if no sprite entities exist or all fail to load.
///
/// `theater_ext` is the file extension for theater-specific SHP files
/// (e.g., "tem" for TEMPERATE). Civilian buildings use `{TYPE_ID}.{ext}`
/// instead of `{TYPE_ID}.SHP`. A supplied prior atlas is returned unchanged
/// when a required cell-drawer sprite cannot be produced.
pub fn build_sprite_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
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
    let started = Instant::now();
    let previous_atlas = existing;

    // Step 1: object keys. Every one derives from the (type, house colour) of
    // a placed object or of a building a unit can deploy into; structures get
    // facing 0 since buildings don't rotate.
    let object_bases =
        collect_needed_base_keys(entities, house_colors, extra_building_types, interner);
    let categories: HashMap<&str, EntityCategory> = entities
        .values()
        .filter(|entity| !entity.is_voxel)
        .map(|entity| {
            (
                interner.map_or("", |i| i.resolve(entity.type_ref())),
                entity.category,
            )
        })
        .collect();
    let mut needed: HashSet<ShpSpriteKey> = HashSet::new();
    for (type_str, color_idx) in &object_bases {
        // A deploy target that is not on the map yet is a structure.
        let category = categories
            .get(type_str.as_str())
            .copied()
            .unwrap_or(EntityCategory::Structure);
        insert_object_keys(&mut needed, type_str, category, *color_idx, rules);
    }

    // The file each type's frames were counted in (upper-case type), so they
    // are rendered from that same file.
    let mut shp_files: HashMap<String, String> = HashMap::new();

    // Step 1b: Collect every declared building animation frame required by art
    // metadata. Runtime owns timing; atlas construction only ensures that a
    // live `BuildingAnimOverlays` frame is drawable.
    let mut active_anim_frame_counts: HashMap<String, u16> = HashMap::new();
    if let Some(art_reg) = art {
        for (type_id, house_color) in &object_bases {
            let rules_image: String = rules
                .and_then(|r| r.object(type_id))
                .map(|o| o.image.clone())
                .unwrap_or_else(|| type_id.clone());
            let Some(entry) = art_reg.resolve_metadata_entry(type_id, &rules_image) else {
                continue;
            };
            for anim in &entry.building_anims {
                // Original location: `RA2-GAME.EXE-IDB` canon,
                // `assets.unitShpFrameSelection.ra2yr.json` and
                // `assets.buildingDrawOffsets.ra2yr.json`: declared
                // Idle/Super/Special frames are chosen by the live object
                // animation state, not collapsed to atlas frame zero.
                let variants = [&anim.damaged_variant, &anim.garrisoned_variant];
                let anims = std::iter::once((anim.anim_type.as_str(), anim.loop_end)).chain(
                    variants
                        .into_iter()
                        .flatten()
                        .map(|variant| (variant.anim_type.as_str(), variant.loop_end)),
                );
                for (anim_type, loop_end) in anims {
                    register_building_anim(
                        &mut needed,
                        &mut active_anim_frame_counts,
                        &mut shp_files,
                        asset_manager,
                        art_reg,
                        anim_type,
                        loop_end,
                        *house_color,
                        theater_ext,
                        theater_name,
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
                    house_color: *house_color,
                });
            }
        }
    }

    // Step 1c: every frame of each building's make (build-up) SHP, a separate
    // file (e.g., GTCNSTMK.SHP) showing the building assembling.
    let mut make_frame_counts: HashMap<String, u16> = HashMap::new();
    if let Some(art_reg) = art {
        for (type_id, color) in &object_bases {
            // Skip anim overlay types (they don't have make SHPs).
            if type_id.contains('_') {
                continue;
            }
            let make_key: String = format!("{}_MAKE", type_id);
            let count = match make_frame_counts.get(&make_key) {
                Some(&count) => count,
                None => {
                    let rules_image: String = rules
                        .and_then(|r| r.object(type_id))
                        .map(|o| o.image.clone())
                        .unwrap_or_else(|| type_id.clone());
                    let image: String = art_reg.resolve_effective_image_id(type_id, &rules_image);
                    let candidates: Vec<String> = art_data::make_shp_candidates(
                        Some(art_reg),
                        &image,
                        theater_ext,
                        theater_name,
                    );
                    let Some((file, data)) = find_shp(asset_manager, &candidates) else {
                        continue;
                    };
                    let Ok(raw_count) = ShpFile::frame_count_from_bytes(data) else {
                        continue;
                    };
                    // RA2 make SHPs have shadow frames in the second half — only use the first half.
                    let count = if raw_count / 2 > 0 {
                        raw_count / 2
                    } else {
                        raw_count
                    };
                    log::debug!("Make SHP for {type_id}: {count} frames from {file}");
                    shp_files.insert(make_key.to_ascii_uppercase(), file.to_string());
                    make_frame_counts.insert(make_key.clone(), count);
                    count
                }
            };
            for frame in 0..count {
                needed.insert(ShpSpriteKey {
                    palette_context: ShpPaletteContext::Legacy,
                    type_id: make_key.clone(),
                    facing: 0,
                    frame,
                    house_color: *color,
                });
            }
        }
    }

    // Step 1d: Pre-load world effect SHPs (warp animations, explosions, etc.).
    // Names come from rules.ini [General] WarpIn=/WarpOut=/WarpAway= — NOT hardcoded.
    // These use anim.pal (effect palette), not unit.pal — tracked in effect_type_ids
    // so step 2 can pick the correct palette. Only the SHP header is read here.
    let mut effect_type_ids: HashSet<String> = HashSet::new();
    for name in &rules.map(collect_effect_names).unwrap_or_default() {
        // Use the same resolved Image= and Theater=/NewTheater= filename
        // authority as scheduler binding. Stock cell-drawer rows such as
        // WA01X and TUNTOP01 are theater SHPs (`.TEM` on Temperate maps).
        let candidates = effect_anim_shp_candidates(name, art, theater_ext, theater_name);
        let required_cell_drawer = cell_drawer_type_ids.contains(&name.to_ascii_uppercase());
        match find_shp(asset_manager, &candidates) {
            Some((file, data)) => match ShpFile::frame_count_from_bytes(data) {
                Ok(raw_count) => {
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
                    shp_files.insert(name_upper, file.to_string());
                    log::debug!("Effect anim SHP {}: {} frames from {}", name, count, file);
                }
                Err(error) if required_cell_drawer => {
                    return abort_sprite_atlas_refresh(
                        previous_atlas,
                        format!("bound cell-drawer animation [{name}] failed SHP decode: {error}"),
                    );
                }
                Err(_) => {}
            },
            None if required_cell_drawer => {
                return abort_sprite_atlas_refresh(
                    previous_atlas,
                    format!("bound cell-drawer animation [{name}] has no SHP asset"),
                );
            }
            None => {}
        }
    }

    // Step 1e: Pre-load the parachute SHP (`[General] Parachute=`).
    // The canopy's frames are keyed on the unit-palette context the parachute
    // pass draws with, so they are registered here as well as through the
    // AnimClass closure `[General] Parachute=` now belongs to. Its palette is not
    // decided by this registration: `sprite_palette_choice` reads the art type's
    // `AltPalette=` flag, which PARACH sets, and that selects the unit palette.
    if let Some(shp_name) = rules.and_then(|r| r.general.parachute_shp.as_deref()) {
        let candidates = [
            format!("{}.shp", shp_name.to_ascii_lowercase()),
            format!("{}.SHP", shp_name),
        ];
        match find_shp(asset_manager, &candidates) {
            Some((file, data)) => match ShpFile::frame_count_from_bytes(data) {
                Ok(frame_count) => {
                    for frame in 0..frame_count {
                        needed.insert(ShpSpriteKey {
                            palette_context: ShpPaletteContext::GlobalAnim,
                            type_id: shp_name.to_string(),
                            facing: 0,
                            frame,
                            house_color: HouseColorIndex(0),
                        });
                    }
                    active_anim_frame_counts.insert(shp_name.to_string(), frame_count);
                    shp_files.insert(shp_name.to_ascii_uppercase(), file.to_string());
                    log::debug!(
                        "Parachute SHP {}: {} frames loaded (unit palette per AltPalette=yes)",
                        shp_name,
                        frame_count
                    );
                }
                Err(_) => log::warn!(
                    "Parachute SHP {} found in MIX but failed to parse",
                    shp_name
                ),
            },
            None => log::warn!(
                "Parachute SHP {} not found in MIX archives — chute will not render",
                shp_name
            ),
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

    // Step 2: the keys to render — every one at map load, afterwards only those
    // the atlas neither holds nor has found unrenderable. Sorting groups each
    // type's keys so its SHP is decoded once.
    let resolved = |key: &ShpSpriteKey| {
        previous_atlas.as_ref().is_some_and(|atlas| {
            atlas.entries.contains_key(key) || atlas.unrenderable.contains(key)
        })
    };
    let mut new_keys: Vec<ShpSpriteKey> = needed
        .iter()
        .filter(|key| !resolved(key))
        .cloned()
        .collect();
    new_keys.sort_unstable_by(|a, b| sprite_key_order(a).cmp(&sprite_key_order(b)));
    log::info!(
        "Sprite atlas: {} already resolved, {} new to render, {} total needed",
        needed.len() - new_keys.len(),
        new_keys.len(),
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
    let new_types: HashSet<&str> = new_keys.iter().map(|key| key.type_id.as_str()).collect();
    for type_id in new_types {
        let base = type_id.strip_suffix("_MAKE").unwrap_or(type_id);
        let image = rules
            .and_then(|r| r.object(base))
            .map_or(base, |o| o.image.as_str());
        if art
            .and_then(|a| a.resolve_metadata_entry(base, image))
            .is_some_and(|a| a.terrain_palette)
        {
            cell_palette_type_ids.insert(type_id.to_ascii_uppercase());
        }
    }
    let palette_choices: Vec<SpritePaletteChoice> = new_keys
        .iter()
        .map(|key| sprite_palette_for_key(key, art, &effect_type_ids, &cell_palette_type_ids))
        .collect();
    if cell_palette.is_none() && palette_choices.contains(&SpritePaletteChoice::CellIso) {
        return abort_sprite_atlas_refresh(
            previous_atlas,
            "terrain-attached animations require the active theater ISO palette".to_string(),
        );
    }

    let mut rendered: Vec<RenderedShpSprite> = Vec::with_capacity(new_keys.len());
    let mut unrenderable: Vec<ShpSpriteKey> = Vec::new();
    let mut source: Option<(&str, Option<ShpSource>)> = None;
    for (key, palette_choice) in new_keys.iter().zip(palette_choices) {
        if source
            .as_ref()
            .is_none_or(|(type_id, _)| *type_id != key.type_id)
        {
            let file = shp_files
                .get(&key.type_id.to_ascii_uppercase())
                .map(String::as_str);
            let loaded = load_shp_source(
                asset_manager,
                &key.type_id,
                file,
                theater_ext,
                theater_name,
                rules,
                art,
            );
            source = Some((&key.type_id, loaded));
        }
        let pal: &Palette = match palette_choice {
            SpritePaletteChoice::Anim => effect_palette.as_ref().unwrap_or(palette),
            SpritePaletteChoice::Unit => palette,
            SpritePaletteChoice::CellIso => {
                cell_palette.expect("cell-drawer palette was validated before atlas rendering")
            }
        };
        let house_remap = match key.palette_context {
            ShpPaletteContext::Legacy => palette_choice != SpritePaletteChoice::CellIso,
            _ => palette_choice == SpritePaletteChoice::Unit,
        };
        let sprite = source
            .as_ref()
            .and_then(|(_, loaded)| loaded.as_ref())
            .and_then(|loaded| render_shp_frame(loaded, pal, house_remap, key, rules));
        match sprite {
            Some(sprite) => rendered.push(sprite),
            None if cell_drawer_type_ids.contains(&key.type_id.to_ascii_uppercase()) => {
                return abort_sprite_atlas_refresh(
                    previous_atlas,
                    format!(
                        "bound cell-drawer animation [{}] frame {} failed SHP rendering",
                        key.type_id, key.frame
                    ),
                );
            }
            None => unrenderable.push(key.clone()),
        }
    }

    // Step 2b: Render oregath.shp harvest overlay frames once harvesters exist.
    // Uses anim.pal (effect palette) — no house color remap.
    let load_harvest_overlay = !previous_atlas
        .as_ref()
        .is_some_and(|atlas| atlas.harvest_overlay_loaded)
        && entities.values().any(|e| e.miner.is_some());
    if load_harvest_overlay {
        let oregath_sprites: Vec<RenderedShpSprite> = render_harvest_overlay_frames(asset_manager);
        if !oregath_sprites.is_empty() {
            log::info!(
                "Rendered {} oregath.shp harvest overlay frames",
                oregath_sprites.len()
            );
            rendered.extend(oregath_sprites);
        }
    }

    // Step 3: shelf-pack into new pages at map load; afterwards append to a
    // growth page so resident sprites are never repacked or re-uploaded.
    let added = rendered.len();
    let mut atlas = match previous_atlas {
        Some(mut atlas) => {
            atlas.append_sprites(device, queue, batch, rendered);
            atlas
        }
        None if rendered.is_empty() => {
            log::warn!("No SHP sprites rendered");
            return None;
        }
        None => pack_sprites(device, queue, batch, &rendered),
    };
    atlas.unrenderable.extend(unrenderable);
    atlas.make_frame_counts.extend(make_frame_counts);
    atlas
        .active_anim_frame_counts
        .extend(active_anim_frame_counts);
    atlas.covered_objects.extend(object_bases);
    atlas
        .covered_anim_remaps
        .extend(anim_remap_keys.iter().cloned());
    atlas.harvest_overlay_loaded |= load_harvest_overlay;
    log::info!(
        "SHP sprite atlas: {} sprites added in {:.1} ms; {} sprites on {} pages, {} unrenderable keys",
        added,
        started.elapsed().as_secs_f64() * 1000.0,
        atlas.sprite_count(),
        atlas.page_count(),
        atlas.unrenderable.len(),
    );
    Some(atlas)
}

/// One type's decoded SHP and the draw offsets every key of it shares.
struct ShpSource {
    shp: ShpFile,
    found_name: String,
    /// DrawOffset from art.ini XDrawOffset/YDrawOffset for per-type fine-tuning.
    draw_offsets: (i32, i32),
}

/// Resolve and decode the SHP a type's keys are drawn from.
///
/// `file` is the file registration counted the type's frames in (animation,
/// make and parachute SHPs), so frames come from the file they were counted
/// in. Other types resolve through rules/art `Image=` and the object naming
/// (with NewTheater substitution), falling back to direct {TYPE_ID}.SHP when
/// art data is unavailable.
fn load_shp_source(
    asset_manager: &AssetManager,
    type_id: &str,
    file: Option<&str>,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
) -> Option<ShpSource> {
    let candidates: Vec<String> = match file {
        Some(file) => vec![file.to_string()],
        None => {
            // Check if this is a make (build-up) SHP — type_id ends with "_MAKE".
            let is_make: bool = type_id.ends_with("_MAKE");
            let base_type_id: &str = if is_make {
                &type_id[..type_id.len() - 5]
            } else {
                type_id
            };
            // Resolve image name: type_id → rules.ini Image= → art.ini Image= override.
            let rules_image: String = rules
                .and_then(|r| r.object(base_type_id))
                .map(|o| o.image.clone())
                .unwrap_or_else(|| base_type_id.to_string());
            let image: String = art
                .map(|a| a.resolve_effective_image_id(base_type_id, &rules_image))
                .unwrap_or_else(|| rules_image.to_uppercase());
            if is_make {
                art_data::make_shp_candidates(art, &image, theater_ext, theater_name)
            } else {
                art_data::object_shp_candidates(art, &image, theater_ext, theater_name)
            }
        }
    };

    let Some((found_name, shp_data)) = find_shp(asset_manager, &candidates) else {
        log::warn!("SHP not found for {}: tried {:?}", type_id, candidates);
        return None;
    };
    // Log when a non-first candidate was selected (generic 'G' fallback is normal).
    if candidates.len() > 2 && found_name != candidates[0] {
        log::debug!(
            "Theater fallback for {}: wanted '{}' but loaded '{}' (tried {:?})",
            type_id,
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
    Some(ShpSource {
        shp,
        found_name: found_name.to_string(),
        draw_offsets: art.map(|a| a.draw_offsets(type_id)).unwrap_or((0, 0)),
    })
}

/// Load and render a single SHP sprite to RGBA pixels.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
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
    let source = load_shp_source(
        asset_manager,
        &key.type_id,
        None,
        theater_ext,
        theater_name,
        rules,
        art,
    )?;
    render_shp_frame(&source, palette, house_remap, key, rules)
}

/// Render one key's frame of a decoded SHP to RGBA pixels.
///
/// Selects the appropriate frame based on facing (8-direction for infantry).
/// An empty (0x0) frame has nothing to draw and returns None.
fn render_shp_frame(
    source: &ShpSource,
    palette: &Palette,
    house_remap: bool,
    key: &ShpSpriteKey,
    rules: Option<&RuleSet>,
) -> Option<RenderedShpSprite> {
    let shp = &source.shp;
    let found_name = source.found_name.as_str();

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
        log::debug!(
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
    let (xdo, ydo) = source.draw_offsets;
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
    device: &wgpu::Device,
    queue: &wgpu::Queue,
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
    let max_texture_dim: u32 = device.limits().max_texture_dimension_2d;
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

        for p in placements.iter().filter(|p| p.page == page_idx) {
            let rs: &RenderedShpSprite = &sprites[p.idx];
            blit_sprite_pixels(
                rs,
                [p.px, p.py],
                atlas_width,
                &mut rgba,
                &mut source_indices,
            );
            entries.insert(
                rs.key.clone(),
                atlas_entry(
                    rs,
                    [p.px, p.py],
                    usize::from(page_idx),
                    [atlas_width, page_height],
                ),
            );
        }

        let texture: BatchTexture = batch.create_texture_on_device(
            device,
            queue,
            &rgba,
            atlas_width,
            page_height,
            Some(&source_indices),
        );
        pages.push(SpriteAtlasPage { texture });
    }

    SpriteAtlas::new(pages, entries)
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
        Self::new(pages, HashMap::new())
    }
}
