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
//!
//! ## Dependency rules
//! - Part of render/ — depends on assets/ (VXL/HVA/Palette), render/batch (GPU upload),
//!   render/vxl_raster (software rendering).
//! - Reads from sim/ via EntityStore iteration (GameEntity fields).

use std::collections::{BTreeMap, HashMap, HashSet};

#[path = "unit_shadow_cache.rs"]
mod shadow_cache;

use crate::assets::asset_manager::AssetManager;
use crate::assets::hva_file::HvaFile;
use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::VxlFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::render::vxl_compute::VxlComputeRenderer;
use crate::render::vxl_raster::{self, VxlRenderParams, VxlSlopeBlend, VxlSprite};
use crate::rules::art_data::{self, ArtRegistry};
use crate::rules::ruleset::RuleSet;

/// Maximum atlas texture width for unit sprites (pixels).

/// Padding between sprites in the atlas to prevent texture bleeding.
const SPRITE_PADDING: u32 = 1;
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

// VxlLayer lives in sim::components — re-exported here for convenience.
pub use crate::sim::components::VxlLayer;

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

/// A paged GPU texture atlas containing pre-rendered unit voxel sprites.
///
/// Created once at map load. Queried per-frame to build unit SpriteInstances.
pub struct UnitAtlas {
    /// Lossless texture pages containing all unit sprites.
    pub pages: Vec<UnitAtlasPage>,
    /// Lookup: (type_id, facing, frame) → UV rectangle + offset data.
    entries: HashMap<UnitSpriteKey, UnitSpriteEntry>,
    /// HVA frame counts per (type_id, layer). Missing entries have 1 frame.
    /// Used at spawn time to initialize VoxelAnimation components.
    pub frame_counts: BTreeMap<(String, VxlLayer), u32>,
    /// Cached rendered sprites for incremental rebuild. On subsequent rebuilds,
    /// only genuinely new sprite keys are rendered; cached sprites are reused
    /// and everything is repacked.
    rendered_cache: Vec<CachedUnitSprite>,
    /// How many sprites were rendered via GPU compute in the last build.
    pub gpu_rendered: u32,
    /// How many sprites were rendered via CPU rasterizer in the last build.
    pub cpu_rendered: u32,
    /// First eligible composed-body mask for each supported shadow key.
    /// Atlas repacking retains these pixels; it does not regenerate the mask.
    shadow_masks: std::cell::RefCell<HashMap<UnitSpriteKey, Vec<u8>>>,
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

    /// Check whether the atlas already contains all sprite keys needed by the
    /// current ECS world. Returns true if no rebuild is necessary.
    pub fn has_all_keys(&self, needed: &HashSet<UnitSpriteKey>) -> bool {
        needed.iter().all(|k| self.entries.contains_key(k))
    }
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
pub(crate) use crate::sim::voxel_frame_catalog::{
    UnitAtlasVariant, detect_hva_frame_count, seed_layers_for, unit_atlas_variants,
};

fn insert_unit_layer_keys(
    needed: &mut HashSet<UnitSpriteKey>,
    type_id: &str,
    layer: VxlLayer,
    num_frames: u32,
    is_ground_vehicle: bool,
) {
    let (step, buckets) = facing_config_for_layer(layer);
    let slope_range = if is_ground_vehicle { 0..=16 } else { 0..=0 };
    for bucket in 0..buckets {
        let facing = (bucket * u16::from(step)) as u8;
        for frame in 0..num_frames {
            for slope_type in slope_range.clone() {
                needed.insert(UnitSpriteKey {
                    type_id: type_id.to_string(),
                    facing,
                    layer,
                    frame,
                    slope_type,
                });
            }
        }
    }
}

fn seed_unit_variant_keys(
    needed: &mut HashSet<UnitSpriteKey>,
    frame_counts: &mut BTreeMap<(String, VxlLayer), u32>,
    variant: &UnitAtlasVariant,
    is_ground_vehicle: bool,
    asset_manager: &AssetManager,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
) {
    let layers = seed_layers_for(
        asset_manager,
        &variant.type_id,
        variant.has_turret,
        rules,
        art,
    );
    for &layer in layers {
        let frame_key = (variant.type_id.clone(), layer);
        let num_frames = *frame_counts.entry(frame_key).or_insert_with(|| {
            detect_hva_frame_count(asset_manager, &variant.type_id, layer, rules, art)
        });
        insert_unit_layer_keys(
            needed,
            &variant.type_id,
            layer,
            num_frames,
            is_ground_vehicle,
        );
    }
    // Ground vehicles and ships cast a voxel shadow (one frame, every facing
    // and slope). Aircraft use FlyLocomotion's own shadow matrix and point,
    // which are not modelled yet, so they get none (recorded residual).
    if is_ground_vehicle {
        insert_unit_layer_keys(needed, &variant.type_id, VxlLayer::Shadow, 1, true);
    }
}

/// Collect the set of unit sprite keys needed by the current ECS world.
///
/// Used by the incremental rebuild path to diff against the existing atlas.
/// Ground vehicles get all 17 slope variants (0-16) pre-rendered so that no
/// atlas rebuild is needed when they drive onto any populated ramp.
pub fn collect_needed_unit_keys(
    entities: &crate::sim::entity_store::EntityStore,
    asset_manager: &AssetManager,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> HashSet<UnitSpriteKey> {
    use crate::map::entities::EntityCategory;
    let mut needed: HashSet<UnitSpriteKey> = HashSet::new();
    let mut frame_counts: BTreeMap<(String, VxlLayer), u32> = BTreeMap::new();
    for entity in entities.values() {
        if !entity.is_voxel {
            continue;
        }
        let type_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
        let is_ground_vehicle: bool = entity.category != EntityCategory::Aircraft;
        for variant in unit_atlas_variants(type_str, rules) {
            seed_unit_variant_keys(
                &mut needed,
                &mut frame_counts,
                &variant,
                is_ground_vehicle,
                asset_manager,
                rules,
                art,
            );
        }
    }

    // Step 1b: Building turret VXLs — non-voxel buildings with TurretAnimIsVoxel=true.
    // Buildings don't tilt on slopes, so slope_type is always 0.
    {
        for entity in entities.values() {
            if entity.is_voxel || entity.category != EntityCategory::Structure {
                continue;
            }
            let btype_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
            let obj = match rules.and_then(|r| r.object(btype_str)) {
                Some(o) => o,
                None => continue,
            };
            if !obj.turret_anim_is_voxel {
                continue;
            }
            let turret_id = match &obj.turret_anim {
                Some(id) => id,
                None => continue,
            };
            for bucket in 0..TURRET_FACING_BUCKETS {
                let facing: u8 = (bucket * u16::from(TURRET_FACING_STEP)) as u8;
                needed.insert(UnitSpriteKey {
                    type_id: turret_id.clone(),
                    facing,
                    layer: VxlLayer::Composite,
                    frame: 0,
                    slope_type: 0,
                });
            }
        }
    }

    needed
}

/// Build a unit sprite atlas from all VoxelModel entities in the ECS world.
///
/// Uses incremental rendering: if `existing` is provided, its cached rendered
/// sprites are reused and only genuinely new keys are rendered. This avoids
/// the expensive VXL software rasterization for sprites already in the atlas.
///
/// 1. Queries the world for all (TypeRef, Facing, VoxelModel) entities.
/// 2. Collects unique (type_id, facing) pairs.
/// 3. Diffs against cached sprites — renders only new keys.
/// 4. Shelf-packs all sprites (cached + new) into a single atlas texture.
///
/// Returns `None` only when no prior atlas exists and no voxel sprite can be
/// produced. A supplied prior atlas is returned unchanged on ordinary failure.
pub fn build_unit_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    entities: &crate::sim::entity_store::EntityStore,
    asset_manager: &AssetManager,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
    existing: Option<UnitAtlas>,
    mut compute: Option<&mut VxlComputeRenderer>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> Option<UnitAtlas> {
    use crate::map::entities::EntityCategory;
    let mut previous_atlas = existing;
    // Step 1: Collect unique (type_id, facing, house_color, layer, frame, slope_type) keys.
    // For turret units, insert separate Body/Turret/Barrel entries per facing.
    // For non-turret units, insert a single Composite entry per facing.
    // Multi-frame HVA units get entries for each frame.
    let mut needed: HashSet<UnitSpriteKey> = HashSet::new();
    let mut frame_counts: BTreeMap<(String, VxlLayer), u32> = BTreeMap::new();
    for entity in entities.values() {
        if !entity.is_voxel {
            continue;
        }
        let type_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
        let is_ground_vehicle: bool = entity.category != EntityCategory::Aircraft;
        for variant in unit_atlas_variants(type_str, rules) {
            seed_unit_variant_keys(
                &mut needed,
                &mut frame_counts,
                &variant,
                is_ground_vehicle,
                asset_manager,
                rules,
                art,
            );
        }
    }

    // Step 1b: Building turret VXLs — non-voxel buildings with TurretAnimIsVoxel=true.
    // These are separate VXL models (e.g., SAM.VXL for NASAM) drawn on top of SHP buildings.
    {
        for entity in entities.values() {
            if entity.is_voxel || entity.category != EntityCategory::Structure {
                continue;
            }
            let btype_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
            let obj = match rules.and_then(|r| r.object(btype_str)) {
                Some(o) => o,
                None => continue,
            };
            if !obj.turret_anim_is_voxel {
                continue;
            }
            let turret_id = match &obj.turret_anim {
                Some(id) => id,
                None => continue,
            };
            for bucket in 0..TURRET_FACING_BUCKETS {
                let facing: u8 = (bucket * u16::from(TURRET_FACING_STEP)) as u8;
                needed.insert(UnitSpriteKey {
                    type_id: turret_id.clone(),
                    facing,
                    layer: VxlLayer::Composite,
                    frame: 0,
                    slope_type: 0,
                });
            }
        }
    }

    if needed.is_empty() {
        log::info!("No voxel entities found — keeping the current unit atlas");
        return previous_atlas;
    }

    // Step 1.5: Extract cached sprites from existing atlas, diff against needed keys.
    let previous_cache_len = previous_atlas
        .as_ref()
        .map_or(0, |atlas| atlas.rendered_cache.len());
    let mut cached: Vec<CachedUnitSprite> = previous_atlas
        .as_mut()
        .map(|atlas| std::mem::take(&mut atlas.rendered_cache))
        .unwrap_or_default();
    let cached_keys: HashSet<UnitSpriteKey> = cached.iter().map(|s| s.key.clone()).collect();
    let new_keys: Vec<UnitSpriteKey> = needed
        .iter()
        .filter(|k| !cached_keys.contains(k))
        .cloned()
        .collect();

    log::info!(
        "Unit atlas: {} cached, {} new to render, {} total needed",
        cached.len(),
        new_keys.len(),
        needed.len(),
    );

    // Step 2: Render only new sprites (skip cached ones).
    let mut gpu_rendered: u32 = 0;
    let mut cpu_rendered: u32 = 0;
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

        for key in &new_keys {
            match render_unit_sprite(
                asset_manager,
                key,
                rules,
                art,
                vpl.as_ref(),
                compute.as_deref_mut(),
                gpu,
            ) {
                Some((sprite, used_gpu, native_draw_bounds)) => {
                    if key.layer == VxlLayer::Shadow && native_draw_bounds.is_some() {
                        let mut fallback_key = key.clone();
                        fallback_key.frame = LEGACY_SHADOW_FRAME;
                        if let Some((fallback, _, _)) = render_unit_sprite(
                            asset_manager,
                            &fallback_key,
                            rules,
                            art,
                            vpl.as_ref(),
                            None,
                            gpu,
                        ) {
                            cached.push(CachedUnitSprite::from_rendered(RenderedSprite {
                                key: fallback_key,
                                sprite: fallback,
                                native_draw_bounds: None,
                            }));
                            cpu_rendered += 1;
                        }
                    }
                    if used_gpu {
                        gpu_rendered += 1;
                    } else {
                        cpu_rendered += 1;
                    }
                    cached.push(CachedUnitSprite::from_rendered(RenderedSprite {
                        key: key.clone(),
                        sprite,
                        native_draw_bounds,
                    }));
                }
                None => {
                    log::warn!("Failed to render VXL for {}", key.type_id);
                }
            }
        }
        if gpu_rendered > 0 || cpu_rendered > 0 {
            log::info!(
                "VXL render: {} GPU compute, {} CPU rasterizer",
                gpu_rendered,
                cpu_rendered,
            );
        }
    }

    if cached.is_empty() {
        log::warn!("No unit sprites rendered");
        if let Some(mut previous) = previous_atlas {
            cached.truncate(previous_cache_len);
            previous.rendered_cache = cached;
            log::warn!("Keeping the previous valid unit atlas after render failure");
            return Some(previous);
        }
        return None;
    }

    // Step 3: Shelf-pack all sprites (cached + newly rendered) into atlas.
    let mut atlas: UnitAtlas = match pack_sprites(gpu, batch, &cached, frame_counts) {
        Ok(atlas) => atlas,
        Err(err) => {
            log::error!("Unit atlas packing failed: {err}");
            if let Some(mut previous) = previous_atlas {
                cached.truncate(previous_cache_len);
                previous.rendered_cache = cached;
                log::error!("Keeping the previous valid unit atlas after packing failure");
                return Some(previous);
            }
            return None;
        }
    };
    atlas.rendered_cache = cached;
    atlas.gpu_rendered = gpu_rendered;
    atlas.cpu_rendered = cpu_rendered;
    if let Some(previous) = previous_atlas.as_ref() {
        atlas.restore_shadow_masks(&gpu.queue, previous.shadow_masks.borrow().clone());
    }
    let page_dimensions = atlas
        .pages
        .iter()
        .map(|page| format!("{}x{}", page.texture.width, page.texture.height))
        .collect::<Vec<_>>()
        .join(", ");
    log::info!(
        "Unit atlas built: {} sprites across {} page(s): {}",
        atlas.sprite_count(),
        atlas.page_count(),
        page_dimensions,
    );
    Some(atlas)
}

/// Load and render a single VXL model to a 2D sprite.
///
/// Uses ArtRegistry to resolve the correct VXL/HVA filenames.
/// Falls back to direct {TYPE_ID}.VXL if art data is unavailable.
pub(crate) fn render_unit_sprite(
    asset_manager: &AssetManager,
    key: &UnitSpriteKey,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
    vpl: Option<&VplFile>,
    compute: Option<&mut VxlComputeRenderer>,
    gpu: &GpuContext,
) -> Option<(VxlSprite, bool, Option<[i32; 4]>)> {
    render_unit_sprite_with_slope_blend(asset_manager, key, rules, art, vpl, compute, gpu, None)
}

pub(crate) fn render_unit_sprite_with_slope_blend(
    asset_manager: &AssetManager,
    key: &UnitSpriteKey,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
    vpl: Option<&VplFile>,
    mut compute: Option<&mut VxlComputeRenderer>,
    gpu: &GpuContext,
    slope_blend: Option<VxlSlopeBlend>,
) -> Option<(VxlSprite, bool, Option<[i32; 4]>)> {
    // Resolve image name: type_id → rules.ini Image= → art.ini Image= override.
    let rules_image: String = rules
        .and_then(|r| r.object(&key.type_id))
        .map(|o| o.image.clone())
        .unwrap_or_else(|| key.type_id.clone());
    let image: String = art
        .map(|a| a.resolve_effective_image_id(&key.type_id, &rules_image))
        .unwrap_or_else(|| rules_image.to_uppercase());

    let (vxl_name, hva_name): (String, String) = art_data::voxel_asset_names(&image);

    let vxl_data = asset_manager.get_ref(&vxl_name)?;
    let vxl: VxlFile = match VxlFile::from_bytes(vxl_data) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("Failed to parse {}: {}", vxl_name, e);
            return None;
        }
    };

    // HVA is optional — some models don't have animation files.
    let hva: Option<HvaFile> =
        asset_manager
            .get_ref(&hva_name)
            .and_then(|data| match HvaFile::from_bytes(data) {
                Ok(h) => Some(h),
                Err(e) => {
                    log::trace!("No HVA for {} ({}), using default pose", key.type_id, e);
                    None
                }
            });

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
        && rules.and_then(|r| r.object(&key.type_id)).is_some_and(|o| {
            o.locomotor == crate::rules::locomotor_type::LocomotorKind::Drive
                && !o.considered_aircraft
        })
    {
        if let Some(sprite) = vxl_raster::shadow::render(&vxl, hva.as_ref(), &params) {
            let bounds = [
                sprite.offset_x as i32,
                sprite.offset_y as i32,
                sprite.width as i32,
                sprite.height as i32,
            ];
            return Some((sprite, false, Some(bounds)));
        }
    }
    let native_draw_bounds = native_unit_sprite_draw_bounds(
        asset_manager,
        &vxl,
        hva.as_ref(),
        &image,
        &params,
        key.layer,
    );

    // House remap is no longer applied at bake time — the fragment shader
    // does it via per-instance DrawState::remap_row + house_ramp texture lookup.
    // The rasterizer outputs post-VPL palette indices directly.

    // One native VXL draw has one center and section order. Parts keep their
    // independent native crops and existing CPU composition; merging their
    // sections into one GPU center has no established native equivalent.
    let has_parts = ["TUR", "BARL", "BARREL"].iter().any(|suffix| {
        asset_manager
            .get_ref(&format!("{image}{suffix}.VXL"))
            .is_some()
    });
    let mut used_gpu = false;
    let gpu_sprite = if key.layer == VxlLayer::Composite && !has_parts {
        compute.as_deref_mut().and_then(|renderer| {
            let draw = vxl_raster::prepare_native_draw(&vxl, hva.as_ref(), &params, vpl)?;
            renderer.render_native(&gpu.device, &gpu.queue, &draw)
        })
    } else {
        None
    };
    let sprite: VxlSprite = if let Some(sprite) = gpu_sprite {
        used_gpu = true;
        sprite
    } else {
        // CPU fallback path.
        match key.layer {
            VxlLayer::Composite => {
                composite_unit_vxl_cpu(asset_manager, &vxl, hva.as_ref(), &image, &params, vpl)
            }
            VxlLayer::Shadow => vxl_raster::render_legacy_vxl_shadow(&vxl, hva.as_ref(), &params),
            VxlLayer::Body | VxlLayer::Turret | VxlLayer::Barrel => {
                let body_sprite: VxlSprite =
                    vxl_raster::render_vxl(&vxl, hva.as_ref(), &params, vpl);
                let turret_sprite: Option<VxlSprite> =
                    render_optional_layer(asset_manager, &format!("{}TUR", image), &params, vpl);
                let barrel_sprite: Option<VxlSprite> =
                    render_optional_layer(asset_manager, &format!("{}BARL", image), &params, vpl)
                        .or_else(|| {
                            render_optional_layer(
                                asset_manager,
                                &format!("{}BARREL", image),
                                &params,
                                vpl,
                            )
                        });

                let all_layers: Vec<&VxlSprite> = [Some(&body_sprite)]
                    .into_iter()
                    .chain([turret_sprite.as_ref(), barrel_sprite.as_ref()])
                    .flatten()
                    .collect();

                let requested: Option<&VxlSprite> = match key.layer {
                    VxlLayer::Body => Some(&body_sprite),
                    VxlLayer::Turret => turret_sprite.as_ref(),
                    VxlLayer::Barrel => barrel_sprite.as_ref(),
                    _ => unreachable!(),
                };
                let requested: &VxlSprite = match requested {
                    Some(s) => s,
                    None => return None,
                };

                pad_layer_to_union_bounds(requested, &all_layers)
            }
        }
    };

    // Skip tiny/empty sprites (degenerate models).
    if sprite.width <= 1 && sprite.height <= 1 {
        log::trace!(
            "VXL {} produced empty sprite at facing {}",
            key.type_id,
            key.facing
        );
        return None;
    }

    Some((sprite, used_gpu, native_draw_bounds))
}

/// Metadata follows the requested VXL, not the union-sized texture canvas
/// used to store each separate layer. Composite keys retain their actual
/// body/turret/barrel bake order; live independently facing parts are united
/// later by presentation at their actual anchors and draw order.
fn native_unit_sprite_draw_bounds(
    assets: &AssetManager,
    body: &VxlFile,
    hva: Option<&HvaFile>,
    image: &str,
    params: &VxlRenderParams,
    layer: VxlLayer,
) -> Option<[i32; 4]> {
    let optional = |base: &str| -> Result<Option<[i32; 4]>, ()> {
        let Some(data) = assets.get_ref(&format!("{base}.VXL")) else {
            return Ok(None);
        };
        let Ok(vxl) = VxlFile::from_bytes(data) else {
            // render_optional_layer also omits an unparseable optional file.
            return Ok(None);
        };
        let hva = assets
            .get_ref(&format!("{base}.HVA"))
            .and_then(|data| HvaFile::from_bytes(data).ok());
        vxl_raster::native_vxl_draw_bounds(&vxl, hva.as_ref(), params)
            .map(Some)
            .ok_or(())
    };
    match layer {
        VxlLayer::Shadow => None,
        VxlLayer::Body => vxl_raster::native_vxl_draw_bounds(body, hva, params),
        VxlLayer::Turret => optional(&format!("{image}TUR")).ok().flatten(),
        VxlLayer::Barrel => optional(&format!("{image}BARL"))
            .ok()?
            .or_else(|| optional(&format!("{image}BARREL")).ok().flatten()),
        VxlLayer::Composite => {
            let mut bounds = Some(vxl_raster::native_vxl_draw_bounds(body, hva, params)?);
            let turret = optional(&format!("{image}TUR")).ok()?;
            let barrel = match optional(&format!("{image}BARL")).ok()? {
                Some(bounds) => Some(bounds),
                None => optional(&format!("{image}BARREL")).ok()?,
            };
            for part in [turret, barrel].into_iter().flatten() {
                vxl_raster::union_native_voxel_draw_bounds(&mut bounds, part);
            }
            bounds
        }
    }
}

/// Body plus optional turret and barrel, depth-composited on the CPU.
///
/// Split out of the atlas bake path so headless callers can produce the same
/// composited sprite the game does. The bake path's own CPU branch calls this,
/// so the two cannot drift apart.
///
/// Pure CPU: no `GpuContext`, no atlas state, no wgpu. The turret and barrel are
/// found by the conventional `TUR` / `BARL` / `BARREL` suffixes on the effective
/// image id; a model without them composites to just its body.
pub fn composite_unit_vxl_cpu(
    asset_manager: &AssetManager,
    body: &VxlFile,
    body_hva: Option<&HvaFile>,
    image: &str,
    params: &VxlRenderParams,
    vpl: Option<&VplFile>,
) -> VxlSprite {
    let body_sprite: VxlSprite = vxl_raster::render_vxl(body, body_hva, params, vpl);
    let mut layers: Vec<VxlSprite> = vec![body_sprite];

    if let Some(turret) = render_optional_layer(asset_manager, &format!("{image}TUR"), params, vpl)
    {
        layers.push(turret);
    }
    // BARL is the common spelling; a handful of models use BARREL.
    if let Some(barrel) = render_optional_layer(asset_manager, &format!("{image}BARL"), params, vpl)
        .or_else(|| render_optional_layer(asset_manager, &format!("{image}BARREL"), params, vpl))
    {
        layers.push(barrel);
    }

    composite_vxl_layers(&layers)
}

fn render_optional_layer(
    asset_manager: &AssetManager,
    layer_base: &str,
    params: &VxlRenderParams,
    vpl: Option<&VplFile>,
) -> Option<VxlSprite> {
    let vxl_name = format!("{}.VXL", layer_base);
    let vxl_data = asset_manager.get_ref(&vxl_name)?;
    let vxl = VxlFile::from_bytes(vxl_data).ok()?;
    let hva_name = format!("{}.HVA", layer_base);
    let hva = asset_manager
        .get_ref(&hva_name)
        .and_then(|data| HvaFile::from_bytes(data).ok());
    Some(vxl_raster::render_vxl(&vxl, hva.as_ref(), params, vpl))
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
fn pack_sprites(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    sprites: &[CachedUnitSprite],
    frame_counts: BTreeMap<(String, VxlLayer), u32>,
) -> Result<UnitAtlas, UnitAtlasPackError> {
    pack_sprites_on_device(&gpu.device, &gpu.queue, batch, sprites, frame_counts)
}

fn pack_sprites_on_device(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    sprites: &[CachedUnitSprite],
    frame_counts: BTreeMap<(String, VxlLayer), u32>,
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
        let page_width_f32 = plan.page_width as f32;
        let page_height_f32 = page_height as f32;
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
                UnitSpriteEntry {
                    uv_origin: [
                        placement.x as f32 / page_width_f32,
                        placement.y as f32 / page_height_f32,
                    ],
                    uv_size: [
                        rs.width as f32 / page_width_f32,
                        rs.height as f32 / page_height_f32,
                    ],
                    pixel_size: [rs.width as f32, rs.height as f32],
                    offset_x: rs.offset_x,
                    offset_y: rs.offset_y,
                    native_draw_bounds: rs.native_draw_bounds,
                    page: page_index,
                },
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

    Ok(UnitAtlas {
        pages,
        entries,
        frame_counts,
        rendered_cache: Vec::new(), // caller sets this after packing
        gpu_rendered: 0,            // caller sets after rendering
        cpu_rendered: 0,
        shadow_masks: Default::default(),
    })
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
        Self {
            pages,
            entries: HashMap::new(),
            frame_counts: BTreeMap::new(),
            rendered_cache: Vec::new(),
            gpu_rendered: 0,
            cpu_rendered: 0,
            shadow_masks: Default::default(),
        }
    }
}
