//! Unit sprite atlas — pre-renders voxel models into a packed GPU texture.
//!
//! At map load time, all VXL-rendered entities are identified by (type_id, facing).
//! Each unique combination is rendered once via the software rasterizer, then all
//! resulting sprites are shelf-packed into lossless GPU texture pages. During the
//! render loop, unit SpriteInstances reference UV regions within one page while
//! the app layer preserves the original flat draw order across page changes.
//!
//! This retains the proven TileAtlas pre-render/cache approach while paging only
//! the texture storage and ordered draw submission needed for lossless capacity.
//! A voxel model that first appears mid-match has only its own sprites rendered,
//! appended to a growth page (`atlas_growth`); resident pages are not repacked.
//!
//! ## Dependency rules
//! - Part of render/ — depends on assets/ (VXL/HVA/Palette), render/batch (GPU upload),
//!   render/vxl_raster (software rendering).
//! - Reads from sim/ via EntityStore iteration (GameEntity fields).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Instant;

#[path = "unit_shadow_cache.rs"]
mod shadow_cache;

use crate::assets::asset_manager::AssetManager;
use crate::assets::hva_file::HvaFile;
use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::VxlFile;
use crate::map::voxel_pose::VxlSlopeBlend;
use crate::render::atlas_growth::{self, GrowthShelf, SPRITE_PADDING};
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::vxl_raster::{self, VxlRenderParams, VxlSprite};
use crate::rules::ruleset::RuleSet;

/// Edge of a growth page. One byte per texel makes it 64 MB, room for the
/// every-slope sprite set of a few new vehicle types.
const GROWTH_PAGE_SIZE: u32 = 8192;
/// Body/composite facing quantization step: 8 = 32 buckets (11.25° per bucket).
///
/// This is not an atlas-size compromise — it is the renderer's real resolution. The
/// original quantizes facing to 5 bits before building the voxel rotation matrix, so
/// only 32 distinct body orientations exist. Baking finer buckets would store up to 8
/// byte-identical copies of every sprite.
const UNIT_FACING_STEP: u8 = 8;
/// Number of pre-rendered facing directions for body/composite sprites.
///
/// `u16` for arithmetic headroom against the step; `bucket * step` stays below 256, so
/// the facing derived from a bucket is still a byte.
const UNIT_FACING_BUCKETS: u16 = crate::render::vxl_raster::VOXEL_FACING_STEPS as u16;
// Private atlas companion, never a native HVA frame. It preserves the existing
// geometry when a prepared native shadow's runtime caller is unsupported.
pub(crate) const LEGACY_SHADOW_FRAME: u32 = u32::MAX;
/// Turret/barrel facing quantization step: 8 = 32 buckets (11.25° per bucket).
///
/// Turret and barrel matrices go through the same 5-bit facing quantization as the
/// body, so turrets step through the same 32 orientations however smoothly the
/// simulation rotates them.
const TURRET_FACING_STEP: u8 = 8;
/// Number of pre-rendered facing directions for turret/barrel sprites.
const TURRET_FACING_BUCKETS: u16 = crate::render::vxl_raster::VOXEL_FACING_STEPS as u16;

// VxlLayer lives in rules::voxel_frame_catalog — re-exported here for convenience.
pub use crate::rules::voxel_frame_catalog::VxlLayer;

/// The locomotor Draw_Matrix arm a posed body is drawn through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PoseTilt {
    /// `FlyLocomotionClass`'s crashing arm (`0x004CF610`): roll and pitch in
    /// radians (`TechnoClass+0x328`, `+0x32C`).
    Fly([f32; 2]),
    /// `JumpjetLocomotionClass`'s `TiltCrashJumpjet=` arm (`0x0054DCC0`), with
    /// the same roll and pitch.
    Jumpjet([f32; 2]),
    /// `RocketLocomotionClass::Draw_Matrix` (`0x00663470`): CurrentPitch in
    /// radians above the horizon, never zero here.
    Rocket(f32),
    /// Shared Drive4AFF60/Ship69F670 dynamic matrix, retained per Unit part.
    Ground(vxl_raster::GroundPose),
}

/// Cache key: unique combination of object type, facing, layer, frame, and slope.
///
/// Note: house color is NOT in the key. Atlas tiles store house-neutral palette
/// indices (post-VPL, pre-house-remap); house remap happens at fragment-shader
/// time. Dropping the house dimension is the central memory win of the GPU
/// remap architecture — N players no longer multiply atlas size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnitSpriteKey {
    /// Object type ID from rules.ini (e.g., "HTNK").
    pub type_id: String,
    /// Selected gun model for Turret/Barrel (and a transient crash composite).
    /// Body and Shadow keys always use zero, so gun changes share the hull.
    pub turret_index: i32,
    /// Facing direction (0–255).
    pub facing: u8,
    /// Which VXL layer this entry represents.
    pub layer: VxlLayer,
    /// HVA animation frame index. 0 for most units; >0 for multi-frame animations.
    pub frame: u32,
    /// Terrain slope type (0–16). 0 = flat, 1-4 = edge ramps, 5-8 = corner
    /// ramps, 9-12 = corner tilt at NW/NE/SE/SW (alias of 5-8 in gamemd.exe),
    /// 13-16 = edge tilt at NW/NE/SE/SW. The consumer in app/presentation/instances/units.rs
    /// clamps any value ≥ 17 to 0 before constructing this key. Different
    /// slopes produce distinct pre-rendered sprites with tilted models.
    pub slope_type: u8,
    /// A barrel's pitch step, `d32 - 8` of the unit's barrel elevation
    /// (`TechnoClass+0x370`); 0 for every other layer.
    pub barrel_pitch: i8,
}

/// The pitch each type's barrel image was first drawn at.
///
/// `TechnoClass::DrawVoxel @ 0x00706640` keeps a unit's turret and barrel
/// images per type (`TechnoType+0x258`, `+0x280`), stored on a miss and blitted
/// on a hit, under the turret's key: its step, the hull's step when the type
/// has a `TurretOffset=`, the locomotor's key and the turret frame
/// (`0x0073B748..0x0073B79F`). The barrel elevation is not in it, and when both
/// images are cached `UnitClass::DrawVoxelBody` skips the turret and barrel
/// matrices (`0x0073BA12..0x0073BA47`). So a key's barrel keeps the elevation of
/// the first unit drawn under it: level for a unit drawn while its elevation
/// still rounds level (the first two frames after Unlimbo at the default
/// `FireAngle=`).
/// Building draw43DA80 submits the same type cache owners: facing and the
/// low byte of its selected HVA frame, without a hull facing or slope. Its
/// recoil arm bypasses this cache (tools/voxel_oracle/building_barrel.json).
///
/// Not modelled here:
/// - Every cached body, turret, barrel and shadow image shares one 4,000,000
///   byte pool (`0x00887460`, sized at `0x0040F270`). Running it out frees every
///   type's caches (`0x005F99E0`, called at `0x00706AE0` and from the shadow
///   draw at `0x00706E38`), and later draws cache the pitch of the moment. VERA
///   keeps first-drawn pitches until the scenario ends; how often retail games
///   run the pool out is not measured.
/// - `DisableVoxelCache=` (`Type+0xDBE`, tested at `0x00706806`) bypasses the
///   cache; retail sets it only on SHAD, PDPLANE and SPYP, none of which has a
///   barrel.
#[derive(Default)]
pub struct BarrelImagePitches {
    by_type: HashMap<String, HashMap<BarrelImageKey, i8>>,
}

/// The parts of a gun key a barrel image varies by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BarrelImageKey {
    turret_index: i32,
    turret_facing: u8,
    /// The hull's facing, keyed only for a type with a `TurretOffset=`.
    body_facing: Option<u8>,
    slope_type: u8,
    turret_frame: u32,
}

impl BarrelImagePitches {
    /// The pitch the barrel image cached with `turret`'s image holds: the
    /// first `drawn` pitch for that key.
    pub fn pitch(&mut self, turret: &UnitSpriteKey, body_facing: Option<u8>, drawn: i8) -> i8 {
        let key = BarrelImageKey {
            turret_index: turret.turret_index,
            turret_facing: turret.facing,
            body_facing,
            slope_type: turret.slope_type,
            turret_frame: turret.frame,
        };
        if let Some(pitches) = self.by_type.get_mut(turret.type_id.as_str()) {
            return *pitches.entry(key).or_insert(drawn);
        }
        self.by_type
            .entry(turret.type_id.clone())
            .or_default()
            .insert(key, drawn);
        drawn
    }

    /// Each scenario and loaded game starts empty, as a freshly started
    /// gamemd does; whether gamemd's type caches outlive a scenario within one
    /// process is not established.
    pub fn clear(&mut self) {
        self.by_type.clear();
    }
}

/// UV and offset data for one sprite within the unit atlas.
#[derive(Debug, Clone, Copy)]
pub struct UnitSpriteEntry {
    /// Top-left UV coordinate in the atlas (0.0..1.0).
    pub uv_origin: [f32; 2],
    /// UV width and height (0.0..1.0).
    pub uv_size: [f32; 2],
    /// Sprite dimensions in pixels.
    pub pixel_size: [f32; 2],
    /// X offset from the model's center to the sprite's top-left corner.
    /// Used to position the sprite so the unit appears centered on its cell.
    pub offset_x: f32,
    /// Y offset from the model's center to the sprite's top-left corner.
    pub offset_y: f32,
    /// Native per-part destination x/y/width/height relative to its draw
    /// anchor, independent of padded atlas storage. None means the asset
    /// could not provide trustworthy native bounds; do not split from padding.
    pub native_draw_bounds: Option<[i32; 4]>,
    /// Texture page containing this sprite.
    pub page: usize,
}

/// One texture page in the unit atlas.
pub struct UnitAtlasPage {
    /// Packed palette-index texture for this page.
    pub texture: BatchTexture,
}

/// Observation of resident palette-index textures, including unused page space.
/// This excludes CPU caches, pose/slope caches, other atlases and driver overhead.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct UnitAtlasStatistics {
    pub resident_sprite_count: usize,
    /// Last non-no-op build's rasterized sprites, including shadow sprites;
    /// this is not the number successfully admitted to pages.
    pub last_build_rasterized_sprite_count: u32,
    pub pages: Vec<UnitAtlasPageStatistics>,
    pub total_texel_payload_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct UnitAtlasPageStatistics {
    pub extent: [u32; 3],
    pub format: &'static str,
    pub dimension: &'static str,
    pub mip_level_count: u32,
    pub sample_count: u32,
    pub texel_payload_bytes: u64,
}

/// A paged GPU texture atlas containing pre-rendered unit voxel sprites.
///
/// Created once at map load and queried per-frame to build unit
/// SpriteInstances. A refresh appends the sprites of newly seen voxel models to
/// growth pages without touching the rest.
pub struct UnitAtlas {
    /// Lossless texture pages containing all unit sprites.
    pub pages: Vec<UnitAtlasPage>,
    /// Lookup: (type_id, facing, frame) → UV rectangle + offset data.
    entries: HashMap<UnitSpriteKey, UnitSpriteEntry>,
    /// HVA frame counts per (type_id, layer, turret_index). Missing entries have 1 frame.
    /// A draw takes its HVA frame as `Foot+0x538` modulo the model's count.
    pub frame_counts: BTreeMap<(String, VxlLayer, i32), u32>,
    /// Palette indices of every resident sprite; native shadow masks are
    /// composed from them.
    rendered_cache: Vec<CachedUnitSprite>,
    /// How many sprites the last build or refresh rendered.
    pub rendered: u32,
    /// First eligible composed-body mask for each supported shadow key.
    /// Pages are never repacked, so an uploaded mask stays where it was written.
    shadow_masks: std::cell::RefCell<HashMap<UnitSpriteKey, Vec<u8>>>,
    /// Voxel models whose sprites have been collected. Every key derives from
    /// a model, so coverage is checked per model, never per key.
    covered: UnitAtlasDemand,
    /// Page refreshes append to; created when the first refresh needs room.
    growth: Option<GrowthShelf>,
}

/// The voxel models a world can draw: vehicle types seeded as ground units
/// (every slope) or as aircraft (flat only), and building voxel turrets. A
/// model's sprite keys depend on nothing else, so this is what coverage and
/// refreshes are decided on.
#[derive(Debug, Default)]
pub struct UnitAtlasDemand {
    ground: BTreeSet<String>,
    air: BTreeSet<String>,
    turrets: BTreeSet<String>,
}

/// One voxel model an object draws with.
#[derive(Clone, Copy)]
enum VoxelModel<'a> {
    Ground(&'a str),
    Air(&'a str),
    Turret(&'a str),
}

/// The voxel model of every object in the world, keyed by its rules type.
/// Buildings keep that identity too: their native loader resolves separate
/// TurretAnim turret/barrel slots, and the draw consumes their own metadata.
fn world_voxel_models<'a>(
    entities: &'a crate::sim::entity_store::EntityStore,
    rules: Option<&'a RuleSet>,
    interner: Option<&'a crate::sim::intern::StringInterner>,
) -> impl Iterator<Item = VoxelModel<'a>> + 'a {
    use crate::map::entities::EntityCategory;
    entities.values().filter_map(move |entity| {
        let type_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
        if entity.is_voxel() {
            return Some(if entity.category() == EntityCategory::Aircraft {
                VoxelModel::Air(type_str)
            } else {
                VoxelModel::Ground(type_str)
            });
        }
        if entity.category() != EntityCategory::Structure {
            return None;
        }
        let obj = rules?.object(type_str)?;
        (obj.turret_anim_is_voxel && obj.turret_anim.is_some())
            .then_some(VoxelModel::Turret(type_str))
    })
}

/// Whether `atlas` — None while no unit atlas exists — already holds every
/// voxel model the world draws, so no refresh is needed. This runs on every
/// tick with a spawn, death or Limbo, so it allocates nothing and stops at
/// the first miss.
pub fn atlas_covers_world(
    atlas: Option<&UnitAtlas>,
    entities: &crate::sim::entity_store::EntityStore,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> bool {
    let mut models = world_voxel_models(entities, rules, interner);
    match atlas {
        Some(atlas) => models.all(|model| atlas.covered.contains(model)),
        None => models.next().is_none(),
    }
}

impl UnitAtlasDemand {
    /// Every voxel model the world's objects draw with.
    pub fn of_world(
        entities: &crate::sim::entity_store::EntityStore,
        rules: Option<&RuleSet>,
        interner: Option<&crate::sim::intern::StringInterner>,
    ) -> Self {
        let mut demand = Self::default();
        for model in world_voxel_models(entities, rules, interner) {
            if !demand.contains(model) {
                let (set, type_id) = match model {
                    VoxelModel::Ground(type_id) => (&mut demand.ground, type_id),
                    VoxelModel::Air(type_id) => (&mut demand.air, type_id),
                    VoxelModel::Turret(type_id) => (&mut demand.turrets, type_id),
                };
                set.insert(type_id.to_string());
            }
        }
        demand
    }

    fn contains(&self, model: VoxelModel<'_>) -> bool {
        match model {
            VoxelModel::Ground(type_id) => self.ground.contains(type_id),
            VoxelModel::Air(type_id) => self.air.contains(type_id),
            VoxelModel::Turret(type_id) => self.turrets.contains(type_id),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ground.is_empty() && self.air.is_empty() && self.turrets.is_empty()
    }

    fn len(&self) -> usize {
        self.ground.len() + self.air.len() + self.turrets.len()
    }

    /// The models in `self` that `covered` does not hold.
    fn without(&self, covered: &Self) -> Self {
        let missing = |mine: &BTreeSet<String>, theirs: &BTreeSet<String>| {
            mine.difference(theirs).cloned().collect()
        };
        Self {
            ground: missing(&self.ground, &covered.ground),
            air: missing(&self.air, &covered.air),
            turrets: missing(&self.turrets, &covered.turrets),
        }
    }

    fn extend(&mut self, other: Self) {
        self.ground.extend(other.ground);
        self.air.extend(other.air);
        self.turrets.extend(other.turrets);
    }
}

impl UnitAtlas {
    /// Look up the atlas entry for a given key.
    pub fn get(&self, key: &UnitSpriteKey) -> Option<&UnitSpriteEntry> {
        self.entries.get(key)
    }

    /// Number of unique sprites in the atlas.
    pub fn sprite_count(&self) -> usize {
        self.entries.len()
    }

    /// Derive capture evidence from the actual resources. No simulation state
    /// or per-frame counters are maintained for this observation.
    pub(crate) fn statistics(&self) -> anyhow::Result<UnitAtlasStatistics> {
        let mut pages = Vec::with_capacity(self.pages.len());
        let mut total_texel_payload_bytes = 0_u64;
        for page in &self.pages {
            // wgpu 27 exposes the original descriptor through TextureView's
            // texture handle. Do not estimate from sprite sizes or roster keys.
            let texture = page.texture.view.texture();
            let extent = texture.size();
            anyhow::ensure!(
                texture.format() == wgpu::TextureFormat::R8Uint
                    && texture.dimension() == wgpu::TextureDimension::D2
                    && texture.mip_level_count() == 1
                    && texture.sample_count() == 1
                    && extent.depth_or_array_layers == 1
                    && extent.width > 0
                    && extent.height > 0,
                "UnitAtlas payload accounting requires a single-layer/mip/sample D2 R8Uint texture: {:?}, {:?}, {:?}, mips={}, samples={}",
                extent,
                texture.format(),
                texture.dimension(),
                texture.mip_level_count(),
                texture.sample_count(),
            );
            let texel_payload_bytes = u64::from(extent.width) * u64::from(extent.height);
            total_texel_payload_bytes = total_texel_payload_bytes
                .checked_add(texel_payload_bytes)
                .ok_or_else(|| anyhow::anyhow!("UnitAtlas texture payload total overflow"))?;
            pages.push(UnitAtlasPageStatistics {
                extent: [extent.width, extent.height, extent.depth_or_array_layers],
                format: "R8Uint",
                dimension: "D2",
                mip_level_count: texture.mip_level_count(),
                sample_count: texture.sample_count(),
                texel_payload_bytes,
            });
        }
        Ok(UnitAtlasStatistics {
            resident_sprite_count: self.entries.len(),
            last_build_rasterized_sprite_count: self.rendered,
            pages,
            total_texel_payload_bytes,
        })
    }

    /// Number of texture pages in the atlas.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Get one atlas page.
    pub fn page(&self, page: usize) -> Option<&UnitAtlasPage> {
        self.pages.get(page)
    }

    /// Get the texture for one atlas page.
    pub fn page_texture(&self, page: usize) -> Option<&BatchTexture> {
        self.page(page).map(|atlas_page| &atlas_page.texture)
    }

    fn new(pages: Vec<UnitAtlasPage>, entries: HashMap<UnitSpriteKey, UnitSpriteEntry>) -> Self {
        Self {
            pages,
            entries,
            frame_counts: BTreeMap::new(),
            rendered_cache: Vec::new(),
            rendered: 0,
            shadow_masks: Default::default(),
            covered: UnitAtlasDemand::default(),
            growth: None,
        }
    }

    /// Place sprites rendered after the map-load pack on growth pages and
    /// upload the rows they fill, one write per page.
    fn append_sprites(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        sprites: Vec<CachedUnitSprite>,
    ) {
        let (sprites, malformed): (Vec<_>, Vec<_>) = sprites
            .into_iter()
            .partition(|sprite| sprite.pixels.len() == (sprite.width * sprite.height) as usize);
        for sprite in malformed {
            log::warn!(
                "{} VXL sprite has {} pixels for {}x{}",
                sprite.key.type_id,
                sprite.pixels.len(),
                sprite.width,
                sprite.height,
            );
        }
        let max_dim = device.limits().max_texture_dimension_2d;
        let pages = &mut self.pages;
        let (placed, unplaced) = atlas_growth::place_on_growth_pages(
            &mut self.growth,
            sprites,
            |sprite| [sprite.width, sprite.height],
            max_dim,
            |[width, height]| {
                let size = [width, height].map(|side| GROWTH_PAGE_SIZE.max(side).min(max_dim));
                let texture = batch.create_blank_unit_atlas_texture(device, size[0], size[1]);
                pages.push(UnitAtlasPage { texture });
                let page = pages.len() - 1;
                log::info!("Unit atlas growth page {page} ({}x{})", size[0], size[1]);
                (page, size)
            },
        );
        for group in placed {
            let texture = &self.pages[group.page].texture;
            let rects: Vec<_> = group
                .sprites
                .iter()
                .map(|(origin, sprite)| (*origin, [sprite.width, sprite.height]))
                .collect();
            atlas_growth::write_band(queue, texture.view.texture(), 1, &rects, |i| {
                group.sprites[i].1.pixels.as_slice()
            });
            let page_size = [texture.width, texture.height];
            for (origin, sprite) in group.sprites {
                let entry = unit_entry(&sprite, origin, group.page, page_size);
                self.entries.insert(sprite.key.clone(), entry);
                self.rendered_cache.push(sprite);
            }
        }
        for sprite in unplaced {
            log::warn!(
                "{} VXL sprite ({}x{}) exceeds the texture limit {max_dim}",
                sprite.key.type_id,
                sprite.width,
                sprite.height,
            );
        }
    }
}

/// UV rectangle and draw data of `sprite` placed at `origin` on a page.
fn unit_entry(
    sprite: &CachedUnitSprite,
    origin: [u32; 2],
    page: usize,
    page_size: [u32; 2],
) -> UnitSpriteEntry {
    let [page_width, page_height] = page_size.map(|v| v as f32);
    UnitSpriteEntry {
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
        native_draw_bounds: sprite.native_draw_bounds,
        page,
    }
}

/// One key's rendered sprite and, for a prepared native shadow, its companion
/// with the legacy shadow geometry (keyed at `LEGACY_SHADOW_FRAME`).
struct RenderedKey {
    sprite: CachedUnitSprite,
    legacy_shadow: Option<CachedUnitSprite>,
}

fn render_unit_key(
    model: &UnitModel,
    key: &UnitSpriteKey,
    vpl: Option<&VplFile>,
    pose: &mut Option<PoseParts>,
) -> Option<RenderedKey> {
    let (sprite, native_draw_bounds) = model.render(key, vpl, None, pose)?;
    let legacy_shadow = (key.layer == VxlLayer::Shadow && native_draw_bounds.is_some())
        .then(|| {
            let mut fallback_key = key.clone();
            fallback_key.frame = LEGACY_SHADOW_FRAME;
            let (fallback, _) = model.render(&fallback_key, vpl, None, pose)?;
            Some(CachedUnitSprite::from_rendered(RenderedSprite {
                key: fallback_key,
                sprite: fallback,
                native_draw_bounds: None,
            }))
        })
        .flatten();
    Some(RenderedKey {
        sprite: CachedUnitSprite::from_rendered(RenderedSprite {
            key: key.clone(),
            sprite,
            native_draw_bounds,
        }),
        legacy_shadow,
    })
}

/// Intermediate rendered sprite before atlas packing (temporary, during build).
struct RenderedSprite {
    key: UnitSpriteKey,
    sprite: VxlSprite,
    native_draw_bounds: Option<[i32; 4]>,
}

/// Cached rendered unit sprite — palette indices only, depth buffer stripped.
/// Depth is only used during VXL compositing (body+turret+barrel merge),
/// not after packing. One byte per pixel (palette index, post-VPL).
struct CachedUnitSprite {
    key: UnitSpriteKey,
    /// Palette-index pixels (1 byte each, width × height total).
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    offset_x: f32,
    offset_y: f32,
    native_draw_bounds: Option<[i32; 4]>,
}

impl CachedUnitSprite {
    fn from_rendered(rs: RenderedSprite) -> Self {
        Self {
            key: rs.key,
            pixels: rs.sprite.palette_indices,
            width: rs.sprite.width,
            height: rs.sprite.height,
            offset_x: rs.sprite.offset_x,
            offset_y: rs.sprite.offset_y,
            native_draw_bounds: rs.native_draw_bounds,
        }
    }
}

// F09: the variant/layer/HVA-count derivation moved to the GPU-independent
// sim-side catalog so construction (app + headless) and atlas seeding share
// one source. Re-exported so this module's tests and callers keep their view.
#[cfg(test)]
use crate::rules::voxel_frame_catalog::voxel_image_id;
use crate::rules::voxel_frame_catalog::{building_voxel_names, voxel_gun_basename};
pub(crate) use crate::rules::voxel_frame_catalog::{
    detect_hva_frame_count, seed_layers_for, unit_atlas_variants,
};

fn insert_unit_layer_keys(
    needed: &mut HashSet<UnitSpriteKey>,
    type_id: &str,
    layer: VxlLayer,
    turret_index: i32,
    num_frames: u32,
    is_ground_vehicle: bool,
    barrel_pitches: &[i8],
) {
    let (step, buckets) = facing_config_for_layer(layer);
    let slope_range = if is_ground_vehicle { 0..=16 } else { 0..=0 };
    let pitches: &[i8] = if layer == VxlLayer::Barrel {
        barrel_pitches
    } else {
        &[0]
    };
    for bucket in 0..buckets {
        let facing = (bucket * u16::from(step)) as u8;
        for frame in 0..num_frames {
            for slope_type in slope_range.clone() {
                for &barrel_pitch in pitches {
                    needed.insert(UnitSpriteKey {
                        type_id: type_id.to_string(),
                        turret_index,
                        facing,
                        layer,
                        frame,
                        slope_type,
                        barrel_pitch,
                    });
                }
            }
        }
    }
}

/// Every pitch step a unit's barrel is drawn at. Unlimbo is the only writer
/// of a unit's barrel elevation: it snaps it level and turns it toward the
/// `FireAngle=` target ([`crate::sim::game_entity::unlimbo_barrel_target`]),
/// so the steps are those along that turn's short arc, `d32 - 8` of each
/// heading.
pub(crate) fn unit_barrel_pitches(fire_angle: i32) -> Vec<i8> {
    use crate::sim::game_entity::{BARREL_LEVEL, unlimbo_barrel_target};
    let arc = i32::from(unlimbo_barrel_target(fire_angle).wrapping_sub(BARREL_LEVEL) as i16);
    let mut pitches = Vec::new();
    // One step spans 0x800; sampling every 0x100 visits each one.
    for turned in (0..=arc.abs()).step_by(0x100) {
        let heading = BARREL_LEVEL.wrapping_add((turned * arc.signum()) as u16);
        let pitch = vxl_raster::voxel_facing_step_u16(heading) as i8 - 8;
        if !pitches.contains(&pitch) {
            pitches.push(pitch);
        }
    }
    pitches
}

fn seed_unit_variant_keys(
    needed: &mut HashSet<UnitSpriteKey>,
    frame_counts: &mut BTreeMap<(String, VxlLayer, i32), u32>,
    variant: &str,
    is_ground_vehicle: bool,
    asset_manager: &AssetManager,
    rules: Option<&RuleSet>,
) {
    let layers = seed_layers_for(asset_manager, variant, rules);
    let building_has_turret =
        building_voxel_names(variant, rules).is_some() && layers.contains(&(VxlLayer::Turret, 0));
    let barrel_pitches = rules
        .and_then(|rules| rules.object(variant))
        .map_or_else(|| vec![0], |object| unit_barrel_pitches(object.fire_angle));
    for (layer, turret_index) in layers {
        let frame_key = (variant.to_string(), layer, turret_index);
        let num_frames = *frame_counts.entry(frame_key).or_insert_with(|| {
            detect_hva_frame_count(asset_manager, variant, layer, turret_index, rules)
        });
        insert_unit_layer_keys(
            needed,
            variant,
            layer,
            turret_index,
            // Building draw43DA80 holds C0 at frame zero when B8 exists;
            // a C0-only building animates that part through its own HVA.
            if building_has_turret && layer == VxlLayer::Barrel {
                1
            } else {
                num_frames
            },
            is_ground_vehicle,
            &barrel_pitches,
        );
    }
    // Ground vehicles and ships cast a voxel shadow (one frame, every facing
    // and slope). Aircraft use FlyLocomotion's own shadow matrix and point,
    // which are not modelled yet, so they get none (recorded residual).
    if is_ground_vehicle {
        insert_unit_layer_keys(needed, variant, VxlLayer::Shadow, 0, 1, true, &[0]);
    }
}

type UnitFrameCounts = BTreeMap<(String, VxlLayer, i32), u32>;

/// Every unit sprite key the voxel models in `demand` can draw, with their
/// HVA frame counts.
///
/// Ground vehicles get all 17 slope variants (0-16) pre-rendered so that no
/// atlas rebuild is needed when they drive onto any populated ramp.
fn needed_unit_keys(
    demand: &UnitAtlasDemand,
    asset_manager: &AssetManager,
    rules: Option<&RuleSet>,
) -> (HashSet<UnitSpriteKey>, UnitFrameCounts) {
    let mut needed: HashSet<UnitSpriteKey> = HashSet::new();
    let mut frame_counts: UnitFrameCounts = BTreeMap::new();
    for (types, is_ground_vehicle) in [(&demand.ground, true), (&demand.air, false)] {
        for type_str in types {
            for variant in unit_atlas_variants(type_str, rules) {
                seed_unit_variant_keys(
                    &mut needed,
                    &mut frame_counts,
                    &variant,
                    is_ground_vehicle,
                    asset_manager,
                    rules,
                );
            }
        }
    }

    // Building voxel guns share the part/frame/pitch owner with vehicles,
    // while their SHP hull never enters the voxel atlas or slope variants.
    for building_id in &demand.turrets {
        seed_unit_variant_keys(
            &mut needed,
            &mut frame_counts,
            building_id,
            false,
            asset_manager,
            rules,
        );
    }

    (needed, frame_counts)
}

/// Build or refresh the unit sprite atlas for every voxel model in the world.
///
/// Without an `existing` atlas every model's sprites are rendered and
/// shelf-packed into new pages. With one, only the models it has not collected
/// yet are rendered, and their sprites are appended to a growth page; the
/// resident pages are not touched and the rasterizer only runs for the new
/// models. Sprites come from the CPU replay of the native visibility writes
/// (`PreparedDraw::render_cpu`), each model's poses spread over the cores.
///
/// Returns `None` only when no prior atlas exists and no voxel sprite can be
/// produced.
pub fn build_unit_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    entities: &crate::sim::entity_store::EntityStore,
    asset_manager: &AssetManager,
    rules: Option<&RuleSet>,
    existing: Option<UnitAtlas>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> Option<UnitAtlas> {
    let started = Instant::now();
    // Step 1: the voxel models the atlas has not collected yet — every one at
    // map load — and the keys they draw with. For turret units, separate
    // Body/Turret/Barrel entries per facing; for the rest a single Composite
    // entry per facing. Multi-frame HVA units get entries for each frame.
    let demand = UnitAtlasDemand::of_world(entities, rules, interner);
    let missing = match &existing {
        Some(atlas) => demand.without(&atlas.covered),
        None => demand,
    };
    if missing.is_empty() {
        log::info!("No new voxel models — keeping the current unit atlas");
        return existing;
    }
    let (needed, frame_counts) = needed_unit_keys(&missing, asset_manager, rules);
    // A type seeded as an aircraft after its ground variant (or the reverse)
    // shares its flat keys with the resident ones.
    let mut new_keys: Vec<UnitSpriteKey> = needed
        .into_iter()
        .filter(|key| {
            existing
                .as_ref()
                .is_none_or(|atlas| !atlas.entries.contains_key(key))
        })
        .collect();
    // By model, then pose: each model is parsed once, and each pose's body,
    // turret and barrel are rasterized once for its three part keys.
    new_keys.sort_unstable_by(|a, b| {
        (
            &a.type_id,
            a.frame,
            a.facing,
            a.slope_type,
            a.turret_index,
            a.layer,
            a.barrel_pitch,
        )
            .cmp(&(
                &b.type_id,
                b.frame,
                b.facing,
                b.slope_type,
                b.turret_index,
                b.layer,
                b.barrel_pitch,
            ))
    });
    log::info!(
        "Unit atlas: {} new sprites to render for {} new voxel models",
        new_keys.len(),
        missing.len(),
    );

    // Step 2: Render the new sprites.
    let mut rendered: Vec<CachedUnitSprite> = Vec::with_capacity(new_keys.len());
    let mut failed: BTreeMap<&str, usize> = BTreeMap::new();
    if !new_keys.is_empty() {
        // Load VPL file for Blinn-Phong lighting lookup (optional).
        let vpl: Option<VplFile> =
            asset_manager
                .get_ref("VOXELS.VPL")
                .and_then(|data| match VplFile::from_bytes(data) {
                    Ok(v) => {
                        log::info!("Loaded VOXELS.VPL ({} lighting sections)", v.num_sections);
                        Some(v)
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to parse VOXELS.VPL: {} — using fallback N·L shading",
                            e
                        );
                        None
                    }
                });

        // One model at a time, its poses spread over the cores. Each worker
        // takes whole poses, so a pose's parts still share one render, and
        // results are merged back in key order.
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
        for type_keys in new_keys.chunk_by(|a, b| a.type_id == b.type_id) {
            let type_id = type_keys[0].type_id.as_str();
            let Some(model) = UnitModel::load(asset_manager, type_id, rules) else {
                *failed.entry(type_id).or_default() += type_keys.len();
                continue;
            };
            let poses: Vec<&[UnitSpriteKey]> = type_keys
                .chunk_by(|a, b| {
                    (a.frame, a.facing, a.slope_type) == (b.frame, b.facing, b.slope_type)
                })
                .collect();
            let (model, vpl) = (&model, vpl.as_ref());
            let results: Vec<Option<RenderedKey>> = std::thread::scope(|scope| {
                let jobs: Vec<_> = poses
                    .chunks(poses.len().div_ceil(workers))
                    .map(|chunk| {
                        scope.spawn(move || {
                            let mut pose = None;
                            chunk
                                .iter()
                                .flat_map(|keys| keys.iter())
                                .map(|key| render_unit_key(model, key, vpl, &mut pose))
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect();
                jobs.into_iter()
                    .flat_map(|job| job.join().expect("VXL render worker panicked"))
                    .collect()
            });
            for (key, result) in type_keys.iter().zip(results) {
                match result {
                    Some(RenderedKey {
                        sprite,
                        legacy_shadow,
                    }) => {
                        rendered.extend(legacy_shadow);
                        rendered.push(sprite);
                    }
                    None => *failed.entry(key.type_id.as_str()).or_default() += 1,
                }
            }
        }
        log::info!("VXL render: {} sprites", rendered.len());
        for (type_id, count) in failed {
            log::warn!("Failed to render {count} VXL sprites for {type_id}");
        }
    }

    // Step 3: shelf-pack into new pages at map load; afterwards append to a
    // growth page so resident sprites are never repacked or re-uploaded.
    let added = rendered.len();
    let mut atlas = match existing {
        Some(mut atlas) => {
            atlas.append_sprites(device, queue, batch, rendered);
            atlas
        }
        None if rendered.is_empty() => {
            log::warn!("No unit sprites rendered");
            return None;
        }
        None => match pack_sprites_on_device(device, queue, batch, &rendered) {
            Ok(mut atlas) => {
                atlas.rendered_cache = rendered;
                atlas
            }
            Err(err) => {
                log::error!("Unit atlas packing failed: {err}");
                return None;
            }
        },
    };
    atlas.frame_counts.extend(frame_counts);
    atlas.covered.extend(missing);
    atlas.rendered = u32::try_from(added).unwrap_or(u32::MAX);
    let page_dimensions = atlas
        .pages
        .iter()
        .map(|page| format!("{}x{}", page.texture.width, page.texture.height))
        .collect::<Vec<_>>()
        .join(", ");
    log::info!(
        "Unit atlas: {} sprites added in {:.1} ms; {} sprites across {} page(s): {}",
        added,
        started.elapsed().as_secs_f64() * 1000.0,
        atlas.sprite_count(),
        atlas.page_count(),
        page_dimensions,
    );
    Some(atlas)
}

// Physical admission has one GPU-independent owner shared with native
// IsVoxel6F9E10/RockingUpdate. This module only materializes that owner's
// parsed parts; it never infers loading from ART or maintains a second parser.
use crate::rules::voxel_asset_catalog::VoxelPart;

fn render_part(part: &VoxelPart, params: &VxlRenderParams, vpl: Option<&VplFile>) -> VxlSprite {
    vxl_raster::render_vxl(part.vxl(), part.hva(), params, vpl)
}

/// `Ok(None)` for an omitted part; `Err` when a present part has no
/// native draw bounds.
fn part_native_draw_bounds(
    part: Option<&VoxelPart>,
    params: &VxlRenderParams,
) -> Result<Option<[i32; 4]>, ()> {
    let Some(part) = part else {
        return Ok(None);
    };
    vxl_raster::native_vxl_draw_bounds(part.vxl(), part.hva(), params)
        .map(Some)
        .ok_or(())
}

/// A voxel model's files, parsed once and shared by every sprite drawn from
/// it. They are named by [`crate::rules::voxel_frame_catalog::voxel_image_id`].
pub(crate) struct UnitModel {
    type_id: String,
    /// Buildings have an SHP hull and only fill their native gun slots.
    body: Option<VoxelPart>,
    /// One pair per native turret index; the body is parsed and stored once.
    guns: Vec<GunParts>,
    /// Ordinary ground Drive units cast the prepared native shadow.
    drive_shadow: bool,
}

#[derive(Default)]
struct GunParts {
    turret: Option<VoxelPart>,
    barrel: Option<VoxelPart>,
}

/// One hull raster per pose, shared by every indexed gun. Only requested gun
/// pairs are rasterized; their parts share the result across layer keys.
pub(crate) struct PoseParts {
    pose: (u32, u8, u8, Option<VxlSlopeBlend>),
    body: Option<VxlSprite>,
    guns: BTreeMap<i32, RenderedGunParts>,
}

struct RenderedGunParts {
    turret: Option<VxlSprite>,
    barrel: Option<VxlSprite>,
}

impl UnitModel {
    pub(crate) fn load(
        asset_manager: &AssetManager,
        type_id: &str,
        rules: Option<&RuleSet>,
    ) -> Option<Self> {
        if let Some((turret, barrel)) = building_voxel_names(type_id, rules) {
            // BuildingType45FA90 independently admits B8/C0 on each VXL's
            // availability. In particular SAM lives in C0 alone; GTGCAN
            // has both GTGCANTUR and GTGCANBARL. The missing-HVA fallback
            // remains the loader's established malformed-asset tolerance.
            let guns = GunParts {
                turret: turret.and_then(|image| VoxelPart::load(asset_manager, &image, false)),
                barrel: VoxelPart::load(asset_manager, &barrel, false),
            };
            if guns.turret.is_none() && guns.barrel.is_none() {
                return None;
            }
            return Some(Self {
                type_id: type_id.to_string(),
                body: None,
                guns: vec![guns],
                drive_shadow: false,
            });
        }
        let (body, guns) =
            crate::rules::voxel_asset_catalog::load_main_voxel(asset_manager, type_id, rules)?;
        let guns = guns
            .into_iter()
            .map(|(turret, barrel)| GunParts { turret, barrel })
            .collect();
        Some(Self {
            type_id: type_id.to_string(),
            body: Some(body),
            guns,
            drive_shadow: rules.and_then(|r| r.object(type_id)).is_some_and(|o| {
                o.locomotor == crate::rules::locomotor_type::LocomotorKind::Drive
                    && !o.considered_aircraft
            }),
        })
    }

    /// Invalid mod indices have no gun parts. Native instead performs an
    /// unchecked array access (73B8D9..73B94F); no fallback to gun zero is
    /// claimed as parity. A valid model's hull remains drawable.
    fn gun_parts(&self, index: i32) -> (Option<&VoxelPart>, Option<&VoxelPart>) {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.guns.get(index))
            .map_or((None, None), |parts| {
                (parts.turret.as_ref(), parts.barrel.as_ref())
            })
    }

    /// Render one key's sprite with its native draw bounds. `pose` keeps the
    /// last body/turret/barrel render, so a model's Body, Turret and Barrel
    /// keys of one pose, rendered in a row, rasterize it once.
    pub(crate) fn render(
        &self,
        key: &UnitSpriteKey,
        vpl: Option<&VplFile>,
        slope_blend: Option<VxlSlopeBlend>,
        pose: &mut Option<PoseParts>,
    ) -> Option<(VxlSprite, Option<[i32; 4]>)> {
        let (turret, barrel) = self.gun_parts(key.turret_index);
        if (key.layer == VxlLayer::Turret && turret.is_none())
            || (key.layer == VxlLayer::Barrel && barrel.is_none())
        {
            return None;
        }
        let params: VxlRenderParams = VxlRenderParams {
            frame: if key.layer == VxlLayer::Shadow && key.frame == LEGACY_SHADOW_FRAME {
                0
            } else {
                key.frame
            },
            facing: key.facing, // already quantized by atlas key generation
            slope_type: key.slope_type,
            slope_blend,
            ..VxlRenderParams::default()
        };
        // Ordinary ground, single-section ShadowIndex/frame-zero geometry. Keep
        // aircraft scaling and unsupported callers on the existing path.
        if key.layer == VxlLayer::Shadow
            && key.frame == 0
            && self.drive_shadow
            && let Some(body) = &self.body
            && let Some(sprite) = vxl_raster::shadow::render(body.vxl(), body.hva(), &params)
        {
            let bounds = [
                sprite.offset_x as i32,
                sprite.offset_y as i32,
                sprite.width as i32,
                sprite.height as i32,
            ];
            return Some((sprite, Some(bounds)));
        }
        // A barrel key draws its part at its pitch; the pose's other parts,
        // and every other key, stay level.
        let part_params = VxlRenderParams {
            barrel_pitch: key.barrel_pitch,
            ..params.clone()
        };
        let native_draw_bounds = self.native_draw_bounds(&part_params, key.layer, key.turret_index);

        // House remap is no longer applied at bake time — the fragment shader
        // does it via per-instance DrawState::remap_row + house_ramp texture lookup.
        // The rasterizer outputs post-VPL palette indices directly.
        let sprite: VxlSprite = match key.layer {
            VxlLayer::Composite => composite_parts(
                self.body.as_ref().map(|body| (body.vxl(), body.hva())),
                turret,
                barrel,
                &params,
                vpl,
            ),
            VxlLayer::Shadow => {
                let body = self.body.as_ref()?;
                vxl_raster::render_legacy_vxl_shadow(body.vxl(), body.hva(), &params)
            }
            VxlLayer::Body | VxlLayer::Turret | VxlLayer::Barrel => {
                let key_pose = (params.frame, params.facing, params.slope_type, slope_blend);
                if pose.as_ref().is_none_or(|parts| parts.pose != key_pose) {
                    *pose = Some(PoseParts {
                        pose: key_pose,
                        body: self
                            .body
                            .as_ref()
                            .map(|body| render_part(body, &params, vpl)),
                        guns: BTreeMap::new(),
                    });
                }
                let parts = pose.as_mut().expect("the pose was just rendered");
                let guns = parts
                    .guns
                    .entry(key.turret_index)
                    .or_insert_with(|| RenderedGunParts {
                        turret: turret.map(|part| render_part(part, &params, vpl)),
                        barrel: barrel.map(|part| render_part(part, &params, vpl)),
                    });
                let pitched_barrel = barrel
                    .filter(|_| key.barrel_pitch != 0)
                    .map(|part| render_part(part, &part_params, vpl));
                let barrel = pitched_barrel.as_ref().or(guns.barrel.as_ref());
                let all_layers: Vec<&VxlSprite> = [parts.body.as_ref()]
                    .into_iter()
                    .chain([guns.turret.as_ref(), barrel])
                    .flatten()
                    .collect();

                let requested: &VxlSprite = match key.layer {
                    VxlLayer::Body => parts.body.as_ref()?,
                    VxlLayer::Turret => guns.turret.as_ref()?,
                    VxlLayer::Barrel => barrel?,
                    _ => unreachable!(),
                };
                if self.body.is_none() {
                    // Building43DA80 blits each gun directly; each part's
                    // own extent seeds its vertical Z gradient. A unit's
                    // off-screen composite instead shares the union below.
                    requested.clone()
                } else {
                    pad_layer_to_union_bounds(requested, &all_layers)
                }
            }
        };

        // Skip tiny/empty sprites (degenerate models).
        if sprite.width <= 1 && sprite.height <= 1 {
            log::trace!(
                "VXL {} produced empty sprite at facing {}",
                self.type_id,
                key.facing
            );
            return None;
        }

        Some((sprite, native_draw_bounds))
    }

    /// One posed body's composite, rendered afresh. The Fly and Jumpjet arms
    /// key their draw -1, so native never caches it. The Rocket arm keys a
    /// pitched draw -1 unless the pitch is the block's stored PitchFinal
    /// product (`0x00663547`), and its image depends only on the facing step
    /// and the pitch, so a fresh render is the cached one. The Jumpjet arm's
    /// half sizes come from this model's main voxel; one that never loaded
    /// leaves them at the constructor's zero (`0x00710C4E`).
    pub(crate) fn render_pose(
        &self,
        key: &UnitSpriteKey,
        vpl: Option<&VplFile>,
        tilt: PoseTilt,
    ) -> Option<(VxlSprite, Option<[i32; 4]>)> {
        let body = self.body.as_ref()?;
        if let PoseTilt::Ground(pose) = tilt {
            let matrices = vxl_raster::ground_unit_pose_matrices(
                key.slope_type,
                pose,
                crate::rules::voxel_asset_catalog::voxel_tilt_half_sizes(body.vxl())
                    .unwrap_or([0.; 2]),
                key.facing,
                key.barrel_pitch,
            );
            let (turret, barrel) = self.gun_parts(key.turret_index);
            let (part, matrix) = match key.layer {
                VxlLayer::Body => (body, matrices[0]),
                VxlLayer::Turret => (turret?, matrices[1]),
                VxlLayer::Barrel => (barrel?, matrices[2]),
                // Unit consumer retains independent native-ordered parts;
                // the existing un-tilted shadow has its own owner.
                VxlLayer::Composite | VxlLayer::Shadow => return None,
            };
            let params = VxlRenderParams {
                frame: key.frame,
                body_tilt: Some(vxl_raster::BodyTilt::Matrix(matrix)),
                ..VxlRenderParams::default()
            };
            return Some((
                render_part(part, &params, vpl),
                part_native_draw_bounds(Some(part), &params).ok().flatten(),
            ));
        }
        let body_tilt = match tilt {
            PoseTilt::Fly(angles) => vxl_raster::BodyTilt::Fly(angles),
            PoseTilt::Jumpjet(angles) => vxl_raster::BodyTilt::Jumpjet {
                angles,
                half_sizes: crate::rules::voxel_asset_catalog::voxel_tilt_half_sizes(body.vxl())
                    .unwrap_or([0.0; 2]),
            },
            PoseTilt::Rocket(pitch) => vxl_raster::BodyTilt::Rocket(pitch),
            PoseTilt::Ground(_) => unreachable!("ground parts returned above"),
        };
        let params = VxlRenderParams {
            frame: key.frame,
            facing: key.facing,
            slope_type: key.slope_type,
            body_tilt: Some(body_tilt),
            ..VxlRenderParams::default()
        };
        let (turret, barrel) = self.gun_parts(key.turret_index);
        let sprite = composite_parts(Some((body.vxl(), body.hva())), turret, barrel, &params, vpl);
        Some((
            sprite,
            self.native_draw_bounds(&params, VxlLayer::Composite, key.turret_index),
        ))
    }

    /// A `VoxelAnimClass` piece at its bounce matrix: the body, drawn without
    /// an HVA and lit diffusely, and its ground shadow stencil
    /// (`VoxelAnimClass::DrawIt @ 0x00749B70`). DrawIt draws only the type's
    /// `VoxelIndex=` section; this draws a single-section model at index zero
    /// and nothing otherwise. Every stock `[VoxelAnims]` image (`TIRE`,
    /// `PIECE`, `GASTANK`, `MTRS`, `MTRB`) is a single-section model, and no
    /// stock type authors `VoxelIndex=`.
    pub(crate) fn render_tumble(
        &self,
        section: i32,
        matrix: glam::Mat4,
        vpl: Option<&VplFile>,
    ) -> Option<(VxlSprite, Option<VxlSprite>)> {
        let body = self.body.as_ref()?;
        if section != 0 || body.vxl().limbs.len() != 1 {
            return None;
        }
        let (sprite, _) =
            self.render_matrix(matrix, 0, false, vxl_raster::VxlLighting::Diffuse, vpl)?;
        Some((
            sprite,
            vxl_raster::shadow::render_matrix(body.vxl(), matrix),
        ))
    }

    /// Shared direct-matrix body raster. Bullet46B0C0 submits every section
    /// with HVA frame modulo count and diffuse lighting; the VoxelAnim caller
    /// instead selects no HVA and separately renders its shadow.
    pub(crate) fn render_matrix(
        &self,
        matrix: glam::Mat4,
        frame: u32,
        use_hva: bool,
        lighting: vxl_raster::VxlLighting,
        vpl: Option<&VplFile>,
    ) -> Option<(VxlSprite, Option<[i32; 4]>)> {
        let body = self.body.as_ref()?;
        let hva = if use_hva { Some(body.hva()?) } else { None };
        let frame = if let Some(hva) = hva {
            frame.checked_rem(hva.frame_count)?
        } else {
            frame
        };
        let params = VxlRenderParams {
            body_tilt: Some(vxl_raster::BodyTilt::Matrix(matrix)),
            lighting,
            frame,
            ..VxlRenderParams::default()
        };
        let bounds = vxl_raster::native_vxl_draw_bounds(body.vxl(), hva, &params)?;
        Some((
            vxl_raster::render_vxl(body.vxl(), hva, &params, vpl),
            Some(bounds),
        ))
    }

    /// Metadata follows the requested VXL, not the union-sized texture canvas
    /// used to store each separate layer. Composite keys retain their actual
    /// body/turret/barrel bake order; live independently facing parts are united
    /// later by presentation at their actual anchors and draw order.
    fn native_draw_bounds(
        &self,
        params: &VxlRenderParams,
        layer: VxlLayer,
        turret_index: i32,
    ) -> Option<[i32; 4]> {
        let (turret, barrel) = self.gun_parts(turret_index);
        let part = |part| part_native_draw_bounds(part, params);
        match layer {
            VxlLayer::Shadow => None,
            VxlLayer::Body => part(self.body.as_ref()).ok().flatten(),
            VxlLayer::Turret => part(turret).ok().flatten(),
            VxlLayer::Barrel => part(barrel).ok().flatten(),
            VxlLayer::Composite => {
                let mut bounds = part(self.body.as_ref()).ok()?;
                let turret = part(turret).ok()?;
                let barrel = part(barrel).ok()?;
                for part in [turret, barrel].into_iter().flatten() {
                    vxl_raster::union_native_voxel_draw_bounds(&mut bounds, part);
                }
                bounds
            }
        }
    }
}

/// Body plus optional turret and barrel, depth-composited on the CPU.
///
/// Split out of the atlas bake path so headless callers can produce the same
/// composited sprite the game does. The bake path composites through the same
/// `composite_parts`, so the two cannot drift apart.
///
/// Pure CPU: no `GpuContext`, no atlas state, no wgpu. This works on an image,
/// not a type: it previews the base `{image}TUR` and `{image}BARL` files
/// when present. Runtime type/index admission belongs to UnitModel and the
/// shared voxel catalog; this image-only preview has no simulation state.
pub fn composite_unit_vxl_cpu(
    asset_manager: &AssetManager,
    body: &VxlFile,
    body_hva: Option<&HvaFile>,
    image: &str,
    params: &VxlRenderParams,
    vpl: Option<&VplFile>,
) -> VxlSprite {
    let part = |layer| VoxelPart::load(asset_manager, &voxel_gun_basename(image, layer, 0), false);
    let turret = part(VxlLayer::Turret);
    let barrel = part(VxlLayer::Barrel);
    composite_parts(
        Some((body, body_hva)),
        turret.as_ref(),
        barrel.as_ref(),
        params,
        vpl,
    )
}

fn composite_parts(
    body: Option<(&VxlFile, Option<&HvaFile>)>,
    turret: Option<&VoxelPart>,
    barrel: Option<&VoxelPart>,
    params: &VxlRenderParams,
    vpl: Option<&VplFile>,
) -> VxlSprite {
    let mut layers: Vec<VxlSprite> = body
        .map(|(body, hva)| vxl_raster::render_vxl(body, hva, params, vpl))
        .into_iter()
        .collect();
    layers.extend(turret.map(|part| render_part(part, params, vpl)));
    layers.extend(barrel.map(|part| render_part(part, params, vpl)));
    composite_vxl_layers(&layers)
}

/// Composite body/turret/barrel layers using depth-correct Z-buffer merging.
/// VERA-internal retained part composition; gamemd equivalence is UNCHECKED.
/// Native one-VXL visibility/crop proof does not establish relative part
/// matrices or final body/turret/barrel surface-blit ordering.
/// Each layer's per-pixel depth is compared against the shared depth buffer,
/// so turret voxels behind the body are correctly occluded (and vice versa).
/// Pixels are palette indices (1 byte each); byte 0 = transparent.
fn composite_vxl_layers(layers: &[VxlSprite]) -> VxlSprite {
    if layers.is_empty() {
        return VxlSprite {
            palette_indices: vec![0],
            depth: vec![f32::NEG_INFINITY],
            width: 1,
            height: 1,
            offset_x: 0.0,
            offset_y: 0.0,
        };
    }
    if layers.len() == 1 {
        return VxlSprite {
            palette_indices: layers[0].palette_indices.clone(),
            depth: layers[0].depth.clone(),
            width: layers[0].width,
            height: layers[0].height,
            offset_x: layers[0].offset_x,
            offset_y: layers[0].offset_y,
        };
    }

    // Offsets are already integer-truncated from the fixed-point rasterizer,
    // so we can safely cast to i32 for pixel-exact compositing.
    let min_x_i: i32 = layers.iter().map(|s| s.offset_x as i32).min().unwrap_or(0);
    let min_y_i: i32 = layers.iter().map(|s| s.offset_y as i32).min().unwrap_or(0);
    let max_x_i: i32 = layers
        .iter()
        .map(|s| s.offset_x as i32 + s.width as i32)
        .max()
        .unwrap_or(1);
    let max_y_i: i32 = layers
        .iter()
        .map(|s| s.offset_y as i32 + s.height as i32)
        .max()
        .unwrap_or(1);

    let width: u32 = (max_x_i - min_x_i).max(1) as u32;
    let height: u32 = (max_y_i - min_y_i).max(1) as u32;
    let pixel_count: usize = (width * height) as usize;
    let mut palette_indices: Vec<u8> = vec![0u8; pixel_count];
    let mut depth_buf: Vec<f32> = vec![f32::NEG_INFINITY; pixel_count];

    // Merge layers using shared depth buffer for correct occlusion.
    for layer in layers {
        let dx: i32 = layer.offset_x as i32 - min_x_i;
        let dy: i32 = layer.offset_y as i32 - min_y_i;
        for y in 0..layer.height as i32 {
            for x in 0..layer.width as i32 {
                let src_pix: usize = (y as u32 * layer.width + x as u32) as usize;
                let src_byte: u8 = layer.palette_indices[src_pix];
                if src_byte == 0 {
                    continue; // transparent source pixel
                }
                let px = dx + x;
                let py = dy + y;
                if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                    continue;
                }
                let dst_pix: usize = (py as u32 * width + px as u32) as usize;
                let src_depth: f32 = layer.depth[src_pix];
                // Only write pixel if it's closer (or equal) to the camera.
                if src_depth >= depth_buf[dst_pix] {
                    depth_buf[dst_pix] = src_depth;
                    palette_indices[dst_pix] = src_byte;
                }
            }
        }
    }

    VxlSprite {
        palette_indices,
        depth: depth_buf,
        width,
        height,
        offset_x: min_x_i as f32,
        offset_y: min_y_i as f32,
    }
}

/// Pad a single VXL layer sprite into a canvas sized to the union bounding box
/// of all layers. This ensures body/turret/barrel share the same offset origin
/// so they align when drawn at the same screen position.
fn pad_layer_to_union_bounds(layer: &VxlSprite, all_layers: &[&VxlSprite]) -> VxlSprite {
    // Compute union bounding box across all layers (integer, same as composite_vxl_layers).
    let min_x_i: i32 = all_layers
        .iter()
        .map(|s| s.offset_x as i32)
        .min()
        .unwrap_or(0);
    let min_y_i: i32 = all_layers
        .iter()
        .map(|s| s.offset_y as i32)
        .min()
        .unwrap_or(0);
    let max_x_i: i32 = all_layers
        .iter()
        .map(|s| s.offset_x as i32 + s.width as i32)
        .max()
        .unwrap_or(1);
    let max_y_i: i32 = all_layers
        .iter()
        .map(|s| s.offset_y as i32 + s.height as i32)
        .max()
        .unwrap_or(1);

    let width: u32 = (max_x_i - min_x_i).max(1) as u32;
    let height: u32 = (max_y_i - min_y_i).max(1) as u32;
    let pixel_count: usize = (width * height) as usize;
    let mut palette_indices: Vec<u8> = vec![0u8; pixel_count];
    let mut depth_buf: Vec<f32> = vec![f32::NEG_INFINITY; pixel_count];

    // Blit the requested layer into the union-sized canvas at its correct position.
    let dx: i32 = layer.offset_x as i32 - min_x_i;
    let dy: i32 = layer.offset_y as i32 - min_y_i;
    for y in 0..layer.height as i32 {
        for x in 0..layer.width as i32 {
            let src_pix: usize = (y as u32 * layer.width + x as u32) as usize;
            let src_byte: u8 = layer.palette_indices[src_pix];
            if src_byte == 0 {
                continue;
            }
            let px: i32 = dx + x;
            let py: i32 = dy + y;
            if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                continue;
            }
            let dst_pix: usize = (py as u32 * width + px as u32) as usize;
            palette_indices[dst_pix] = src_byte;
            depth_buf[dst_pix] = layer.depth[src_pix];
        }
    }

    VxlSprite {
        palette_indices,
        depth: depth_buf,
        width,
        height,
        offset_x: min_x_i as f32,
        offset_y: min_y_i as f32,
    }
}

/// Canonicalize body/composite facing to one of `UNIT_FACING_BUCKETS` buckets.
///
/// Rounds to the nearest of the renderer's 32 facing steps rather than truncating,
/// because that is what the voxel rotation matrix does. Truncating would bias every
/// unit's rendered heading by up to half a step against its simulated one.
pub fn canonical_unit_facing(facing: u8) -> u8 {
    vxl_raster::voxel_facing_step(facing) * UNIT_FACING_STEP
}

/// Canonicalize turret/barrel facing to one of `TURRET_FACING_BUCKETS` buckets.
/// Accepts 16-bit DirStruct, converts to 8-bit for sprite frame selection.
/// This is the single u16→u8 conversion point for turret rendering.
///
/// Quantizes straight off the 16-bit facing — the form the original uses — so the
/// rounding is not applied to an already-truncated byte.
pub fn canonical_turret_facing(facing_u16: u16) -> u8 {
    vxl_raster::voxel_facing_step_u16(facing_u16) * TURRET_FACING_STEP
}

/// Get the facing quantization step and bucket count for a given VxlLayer.
fn facing_config_for_layer(layer: VxlLayer) -> (u8, u16) {
    match layer {
        VxlLayer::Body | VxlLayer::Composite | VxlLayer::Shadow => {
            (UNIT_FACING_STEP, UNIT_FACING_BUCKETS)
        }
        VxlLayer::Turret | VxlLayer::Barrel => (TURRET_FACING_STEP, TURRET_FACING_BUCKETS),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnitAtlasPlacement {
    sprite_index: usize,
    page: usize,
    x: u32,
    y: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UnitAtlasPackPlan {
    page_width: u32,
    page_heights: Vec<u32>,
    placements: Vec<UnitAtlasPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UnitAtlasPackError {
    ZeroTextureLimit,
    SpriteExceedsTextureLimit {
        sprite_index: usize,
        width: u32,
        height: u32,
        limit: u32,
    },
    InvalidPixelCount {
        sprite_index: usize,
        expected: usize,
        actual: usize,
    },
}

impl std::fmt::Display for UnitAtlasPackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroTextureLimit => write!(formatter, "GPU reports a zero 2D texture limit"),
            Self::SpriteExceedsTextureLimit {
                sprite_index,
                width,
                height,
                limit,
            } => write!(
                formatter,
                "sprite {sprite_index} is {width}x{height}, exceeding GPU limit {limit}"
            ),
            Self::InvalidPixelCount {
                sprite_index,
                expected,
                actual,
            } => write!(
                formatter,
                "sprite {sprite_index} has {actual} pixels; expected {expected}"
            ),
        }
    }
}

/// Build a GPU-independent, lossless page-placement plan.
fn plan_sprite_pages(
    dimensions: &[(u32, u32)],
    max_texture_dim: u32,
) -> Result<UnitAtlasPackPlan, UnitAtlasPackError> {
    if max_texture_dim == 0 {
        return Err(UnitAtlasPackError::ZeroTextureLimit);
    }
    for (sprite_index, &(width, height)) in dimensions.iter().enumerate() {
        if width > max_texture_dim || height > max_texture_dim {
            return Err(UnitAtlasPackError::SpriteExceedsTextureLimit {
                sprite_index,
                width,
                height,
                limit: max_texture_dim,
            });
        }
    }

    let mut indices: Vec<usize> = (0..dimensions.len()).collect();
    indices.sort_by(|&a, &b| dimensions[b].1.cmp(&dimensions[a].1));

    let total_area: u64 = dimensions
        .iter()
        .map(|&(width, height)| {
            (width as u64 + SPRITE_PADDING as u64) * (height as u64 + SPRITE_PADDING as u64)
        })
        .sum();
    let estimated_side = (total_area as f64).sqrt().ceil() as u32;
    let widest_sprite = dimensions
        .iter()
        .map(|&(width, _)| width)
        .max()
        .unwrap_or(0);
    let minimum_width = 64.min(max_texture_dim);
    let mut page_width = estimated_side
        .max(minimum_width)
        .max(widest_sprite)
        .min(max_texture_dim);

    while simulate_shelf_height(&indices, dimensions, page_width) > max_texture_dim as u64
        && page_width < max_texture_dim
    {
        page_width = page_width.saturating_mul(2).min(max_texture_dim);
    }

    let mut placements = Vec::with_capacity(dimensions.len());
    let mut page_heights: Vec<u32> = Vec::new();
    let mut page = 0usize;
    let mut cursor_x = 0u32;
    let mut cursor_y = 0u32;
    let mut shelf_height = 0u32;

    for &sprite_index in &indices {
        let (width, height) = dimensions[sprite_index];
        if cursor_x + width > page_width {
            let next_y = cursor_y + shelf_height + SPRITE_PADDING;
            if next_y + height > max_texture_dim {
                page += 1;
                cursor_x = 0;
                cursor_y = 0;
                shelf_height = 0;
            } else {
                cursor_x = 0;
                cursor_y = next_y;
                shelf_height = 0;
            }
        }
        if cursor_y + height > max_texture_dim {
            page += 1;
            cursor_x = 0;
            cursor_y = 0;
            shelf_height = 0;
        }

        placements.push(UnitAtlasPlacement {
            sprite_index,
            page,
            x: cursor_x,
            y: cursor_y,
        });
        if page_heights.len() <= page {
            page_heights.resize(page + 1, 0);
        }
        page_heights[page] = page_heights[page].max(cursor_y + height);
        cursor_x += width + SPRITE_PADDING;
        shelf_height = shelf_height.max(height);
    }

    Ok(UnitAtlasPackPlan {
        page_width,
        page_heights,
        placements,
    })
}

fn simulate_shelf_height(indices: &[usize], dimensions: &[(u32, u32)], page_width: u32) -> u64 {
    let mut cursor_x = 0u64;
    let mut cursor_y = 0u64;
    let mut shelf_height = 0u64;
    for &sprite_index in indices {
        let (width, height) = dimensions[sprite_index];
        let width = width as u64;
        let height = height as u64;
        if cursor_x + width > page_width as u64 {
            cursor_y += shelf_height + SPRITE_PADDING as u64;
            cursor_x = 0;
            shelf_height = 0;
        }
        cursor_x += width + SPRITE_PADDING as u64;
        shelf_height = shelf_height.max(height);
    }
    cursor_y + shelf_height
}

/// Shelf-pack cached sprites into lossless GPU texture pages.
fn pack_sprites_on_device(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    sprites: &[CachedUnitSprite],
) -> Result<UnitAtlas, UnitAtlasPackError> {
    let max_texture_dim: u32 = device.limits().max_texture_dimension_2d;
    let plan = plan_cached_sprite_pages(sprites, max_texture_dim)?;
    if plan.page_heights.len() > 1 {
        log::info!(
            "Unit atlas split into {} pages (GPU texture limit {})",
            plan.page_heights.len(),
            max_texture_dim,
        );
    }

    let mut pages = Vec::with_capacity(plan.page_heights.len());
    let mut entries = HashMap::with_capacity(plan.placements.len());
    for (page_index, &page_height) in plan.page_heights.iter().enumerate() {
        let mut pixels = vec![0u8; (plan.page_width * page_height) as usize];
        for placement in plan
            .placements
            .iter()
            .filter(|placement| placement.page == page_index)
        {
            let rs = &sprites[placement.sprite_index];
            let expected_pixels = (rs.width * rs.height) as usize;
            if rs.pixels.len() != expected_pixels {
                return Err(UnitAtlasPackError::InvalidPixelCount {
                    sprite_index: placement.sprite_index,
                    expected: expected_pixels,
                    actual: rs.pixels.len(),
                });
            }
            for y in 0..rs.height {
                let src_start = (y * rs.width) as usize;
                let src_end = src_start + rs.width as usize;
                let dst_start = ((placement.y + y) * plan.page_width + placement.x) as usize;
                let dst_end = dst_start + rs.width as usize;
                pixels[dst_start..dst_end].copy_from_slice(&rs.pixels[src_start..src_end]);
            }
            entries.insert(
                rs.key.clone(),
                unit_entry(
                    rs,
                    [placement.x, placement.y],
                    page_index,
                    [plan.page_width, page_height],
                ),
            );
        }
        let texture = batch.create_unit_atlas_texture_on_device(
            device,
            queue,
            plan.page_width,
            page_height,
            &pixels,
        );
        pages.push(UnitAtlasPage { texture });
    }

    Ok(UnitAtlas::new(pages, entries))
}

fn plan_cached_sprite_pages(
    sprites: &[CachedUnitSprite],
    max_texture_dim: u32,
) -> Result<UnitAtlasPackPlan, UnitAtlasPackError> {
    let dimensions = sprites
        .iter()
        .map(|sprite| (sprite.width, sprite.height))
        .collect::<Vec<_>>();
    plan_sprite_pages(&dimensions, max_texture_dim)
}

// Tests extracted to unit_atlas_tests.rs to stay under 600 lines.
#[cfg(test)]
#[path = "unit_atlas_tests.rs"]
mod tests;

#[cfg(test)]
impl UnitAtlas {
    pub(crate) fn from_test_pages(pages: Vec<UnitAtlasPage>) -> Self {
        Self::new(pages, HashMap::new())
    }

    pub(crate) fn from_test_entries(entries: HashMap<UnitSpriteKey, UnitSpriteEntry>) -> Self {
        Self::new(Vec::new(), entries)
    }
}
