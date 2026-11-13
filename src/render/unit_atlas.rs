//! Unit sprite atlas — voxel pictures drawn on first use into packed GPU
//! texture pages.
//!
//! The original renders a voxel image the first time a draw asks for it and
//! keeps it: `TechnoClass::Draw @ 0x00706640` renders a cache miss
//! (`0x00706ED0`), stores the image in its type's cache (`0x006C89E0`), and
//! later hits blit the stored copy (`0x00707480`). Nothing is drawn ahead.
//! This atlas does the same with its pictures: map load and each new voxel
//! model mid-match only admit the model ([`admit_world_models`]), with its
//! parsed files, HVA frame counts and the keys it can draw. The first
//! [`UnitAtlas::get`] of a key rasterizes it on the software rasterizer and
//! shelf-places it on a page; [`UnitAtlas::upload`] writes it before the
//! frame draws. A picture never moves once placed. Unit SpriteInstances
//! reference UV regions within one page while the app layer preserves the
//! original flat draw order across page changes.
//!
//! ## Dependency rules
//! - Part of render/ — depends on assets/ (VXL/HVA/Palette), render/batch (GPU upload),
//!   render/vxl_raster (software rendering).
//! - Reads from sim/ via EntityStore iteration (GameEntity fields).

use crate::util::lookup_hash::LookupMap;
use crate::util::time::Instant;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[path = "unit_shadow_cache.rs"]
mod shadow_cache;
pub(crate) use shadow_cache::{UnitShadowPart, composite_mask_part};

use crate::assets::asset_manager::AssetManager;
use crate::assets::hva_file::HvaFile;
use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::VxlFile;
use crate::map::voxel_pose::VxlSlopeBlend;
use crate::render::atlas_growth::{self, GrowthShelf, ShelfCursor};
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::vxl_raster::{self, VxlRenderParams, VxlSprite};
use crate::rules::ruleset::RuleSet;

/// Edge of a page. One byte per texel makes it 16 MB, room for the pictures
/// a usual match draws; a page opens when the last one is full.
const PAGE_SIZE: u32 = 4096;
/// Body/composite facing quantization step: 8 = 32 buckets (11.25° per bucket).
///
/// This is not an atlas-size compromise — it is the renderer's real resolution. The
/// original quantizes facing to 5 bits before building the voxel rotation matrix, so
/// only 32 distinct body orientations exist. Baking finer buckets would store up to 8
/// byte-identical copies of every sprite.
const UNIT_FACING_STEP: u8 = 8;
/// Number of facing directions body/composite sprites are drawn at.
///
/// `u16` for arithmetic headroom against the step; `bucket * step` stays below 256, so
/// the facing derived from a bucket is still a byte.
const UNIT_FACING_BUCKETS: u16 = crate::render::vxl_raster::VOXEL_FACING_STEPS as u16;
// Private atlas companion, never a native HVA frame. It preserves the existing
// geometry when a prepared native shadow's runtime caller is unsupported.
pub(crate) const LEGACY_SHADOW_FRAME: u32 = u32::MAX;
/// Unit73C468 separates the grounded composed mask from unmasked flight.
/// This atlas discriminator never changes the HVA frame used by a shadow.
pub(crate) const GROUNDED_SHADOW_FRAME_FLAG: u32 = 0x8000_0000;
/// Turret/barrel facing quantization step: 8 = 32 buckets (11.25° per bucket).
///
/// Turret and barrel matrices go through the same 5-bit facing quantization as the
/// body, so turrets step through the same 32 orientations however smoothly the
/// simulation rotates them.
const TURRET_FACING_STEP: u8 = 8;
/// Number of facing directions turret/barrel sprites are drawn at.
const TURRET_FACING_BUCKETS: u16 = crate::render::vxl_raster::VOXEL_FACING_STEPS as u16;

// VxlLayer lives in rules::voxel_frame_catalog — re-exported here for convenience.
pub use crate::rules::voxel_frame_catalog::VxlLayer;

/// The locomotor Draw_Matrix arm a posed body is drawn through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PoseTilt {
    /// `FlyLocomotionClass`'s crashing arm (`0x004CF610`): roll and pitch in
    /// radians (`TechnoClass+0x328`, `+0x32C`).
    Fly([f32; 2]),
    /// `FlyLocomotionClass::Draw_Matrix @ 0x004CF610` normal-flight arm:
    /// optional type PitchAngle about Y and signed RollAngle about X, after
    /// SecondaryFacing yaw. This is distinct from the crashing arm's X-then-Y
    /// `TechnoClass+0x328/+0x32C` pose.
    FlyNormal { pitch: f32, roll: f32 },
    /// `JumpjetLocomotionClass`'s `TiltCrashJumpjet=` arm (`0x0054DCC0`), with
    /// the same roll and pitch.
    Jumpjet([f32; 2]),
    /// `RocketLocomotionClass::Draw_Matrix` (`0x00663470`): CurrentPitch in
    /// radians above the horizon, never zero here.
    Rocket(f32),
    /// Shared Drive4AFF60/Ship69F670 dynamic matrix, retained per Unit part.
    /// Hover513F40's rocking arm draws it at slope 0.
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
    /// Object type ID from rules.ini (e.g., "HTNK"). Every drawn object builds
    /// and clones keys each frame, and a name this short stays inline.
    pub type_id: smol_str::SmolStr,
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
    /// slopes produce distinct sprites with tilted models.
    pub slope_type: u8,
    /// A barrel's pitch step, `d32 - 8` of the unit's barrel elevation
    /// (`TechnoClass+0x370`); 0 for every other layer.
    pub barrel_pitch: i8,
}

/// UV and offset data for one sprite within the unit atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
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

impl UnitSpriteEntry {
    /// The width and height of the native render it stands for: its native
    /// draw bounds', else its pixel size.
    pub fn native_size(&self) -> [i32; 2] {
        self.native_draw_bounds.map_or(
            self.pixel_size.map(|size| size as i32),
            |[_, _, width, height]| [width, height],
        )
    }
}

/// One texture page in the unit atlas.
pub struct UnitAtlasPage {
    /// Packed palette-index texture for this page.
    pub texture: BatchTexture,
}

/// Observation of resident palette-index textures, including unused page space.
/// This excludes CPU caches, pose/slope caches, other atlases and driver overhead.
/// The default is the receipt of an atlas never built: no sprites and no pages.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct UnitAtlasStatistics {
    pub resident_sprite_count: usize,
    /// Sprites rasterized on first use since the atlas was made, including
    /// shadow sprites; this is not the number successfully admitted to pages.
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

/// A paged GPU texture atlas of unit voxel pictures, each drawn the first
/// time a draw asks for it.
///
/// Made at map load, or with the world's first voxel model, and queried
/// per-frame to build unit SpriteInstances. Draws hold it shared, so the
/// pictures they draw live behind a `RefCell` until [`Self::upload`].
pub struct UnitAtlas {
    /// Texture pages, created by [`Self::upload`] as first-use draws open them.
    pub pages: Vec<UnitAtlasPage>,
    /// HVA frame counts per (type_id, layer, turret_index). Missing entries have 1 frame.
    /// A draw takes its HVA frame as `Foot+0x538` modulo the model's count.
    pub frame_counts: BTreeMap<(smol_str::SmolStr, VxlLayer, i32), u32>,
    /// The pictures drawn so far, where they sit, and the keys still to draw.
    resident: RefCell<Resident>,
    /// First eligible composed-body mask for each supported shadow key, with
    /// the image cache's flush count when it was composed: a flush frees it
    /// natively, so an older mask is composed again. Pictures never move, so
    /// an uploaded mask stays where it was written.
    shadow_masks: RefCell<HashMap<UnitSpriteKey, (u32, Vec<u8>)>>,
    /// Voxel models admitted so far. Every key derives from a model, so
    /// coverage is checked per model, never per key.
    covered: UnitAtlasDemand,
    /// Each admitted model's parsed files, by the model id its keys carry.
    models: HashMap<smol_str::SmolStr, UnitModel>,
    /// `VOXELS.VPL`, read when the first model is admitted; None draws with
    /// the rasterizer's fallback N·L shading.
    vpl: Option<VplFile>,
    /// The device's 2D texture limit, which bounds pages and pictures.
    max_texture_dim: u32,
}

/// [`UnitAtlas::resident`]: the CPU side of every picture.
#[derive(Default)]
struct Resident {
    /// Lookup: (type_id, facing, frame) → UV rectangle + offset data. Every
    /// drawn unit piece looks its sprite up here.
    entries: LookupMap<UnitSpriteKey, UnitSpriteEntry>,
    /// Palette indices of every resident sprite; native shadow masks are
    /// composed from them.
    rendered_cache: Vec<CachedUnitSprite>,
    /// Each resident sprite's index in `rendered_cache`, by key and by atlas
    /// placement, so shadow composition ([`UnitAtlas::stamp_shadow_part`],
    /// [`UnitAtlas::masked_shadow`]) finds its source without walking every
    /// sprite. The cache is the truth: it only grows, through
    /// [`Resident::place`], which keeps this index.
    rendered_index: RenderedIndex,
    /// Each resident sprite's native pool size
    /// ([`crate::render::voxel_image_cache::sprite_pool_bytes`]).
    pool_bytes: HashMap<UnitSpriteKey, u32>,
    /// Keys an admitted model draws that no draw has asked for yet. A key
    /// leaves when it is drawn, or fails to draw, and never returns.
    undrawn: HashSet<UnitSpriteKey>,
    /// Each model's last rasterized pose, which its other parts of that pose
    /// share ([`UnitModel::render`]).
    poses: HashMap<smol_str::SmolStr, Option<PoseParts>>,
    /// The page first-use draws place on, and its free space.
    shelf: Option<GrowthShelf>,
    /// Every page's size, including those [`UnitAtlas::upload`] has not
    /// created yet.
    page_sizes: Vec<[u32; 2]>,
    /// Texels placed since the last upload, in the order they were placed.
    uploads: Vec<PendingTexels>,
    /// Sprites rasterized so far.
    rendered: u32,
}

/// One rectangle of texels waiting for [`UnitAtlas::upload`].
struct PendingTexels {
    page: usize,
    origin: [u32; 2],
    size: [u32; 2],
    texels: Vec<u8>,
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

/// The voxel models the world's objects draw with, keyed by rules type, each
/// once however many objects share it: an object's model follows from its
/// type, whether it is voxel, and its class.
/// Buildings keep that identity too: their native loader resolves separate
/// TurretAnim turret/barrel slots, and the draw consumes their own metadata.
fn world_voxel_models<'a>(
    entities: &'a crate::sim::entity_store::EntityStore,
    rules: Option<&'a RuleSet>,
    interner: Option<&'a crate::sim::intern::StringInterner>,
) -> impl Iterator<Item = VoxelModel<'a>> + 'a {
    use crate::map::entities::EntityCategory;
    let mut asked = crate::util::lookup_hash::LookupSet::default();
    entities.values().filter_map(move |entity| {
        let voxel = entity.is_voxel();
        let category = entity.category();
        if !voxel && category != EntityCategory::Structure {
            return None;
        }
        let aircraft = category == EntityCategory::Aircraft;
        if !asked.insert((entity.type_ref(), voxel, aircraft)) {
            return None;
        }
        let type_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
        if voxel {
            return Some(if aircraft {
                VoxelModel::Air(type_str)
            } else {
                VoxelModel::Ground(type_str)
            });
        }
        let obj = rules?.object(type_str)?;
        (obj.turret_anim_is_voxel && obj.turret_anim.is_some())
            .then_some(VoxelModel::Turret(type_str))
    })
}

/// Whether `atlas` — None while no unit atlas exists — has admitted every
/// voxel model the world draws, so no admission is needed. This runs on
/// every tick with a spawn, death or Limbo, so it asks once for each model
/// and stops at the first miss.
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
    /// The pool bytes `key`'s image takes when the original caches it,
    /// drawing it first if no draw has asked for it yet.
    pub(crate) fn pool_bytes(&self, key: &UnitSpriteKey) -> Option<u32> {
        self.get(key)?;
        self.resident.borrow().pool_bytes.get(key).copied()
    }

    /// The atlas entry for `key`, its picture drawn the first time any draw
    /// asks for it: `TechnoClass::Draw @ 0x00706640` renders a cache miss
    /// (`0x00706ED0`) and stores it (`0x006C89E0`) for later hits to blit
    /// (`0x00707480`). None for a key no admitted model draws, or whose
    /// picture fails to draw. A picture drawn here reaches its page at the
    /// next [`Self::upload`].
    pub fn get(&self, key: &UnitSpriteKey) -> Option<UnitSpriteEntry> {
        let resident = self.resident.borrow().entries.get(key).copied();
        if resident.is_some() {
            return resident;
        }
        self.draw_first_use(key)
    }

    /// Rasterize `key` on its first request and place it, with the legacy
    /// shadow companion that is drawn beside a shadow template; a request for
    /// the companion draws its template.
    fn draw_first_use(&self, key: &UnitSpriteKey) -> Option<UnitSpriteEntry> {
        let drawn = if key.layer == VxlLayer::Shadow && key.frame == LEGACY_SHADOW_FRAME {
            UnitSpriteKey {
                frame: 0,
                ..key.clone()
            }
        } else {
            key.clone()
        };
        let mut resident = self.resident.borrow_mut();
        if !resident.undrawn.remove(&drawn) {
            return None;
        }
        let model = self.models.get(&drawn.type_id)?;
        let pose = resident.poses.entry(drawn.type_id.clone()).or_default();
        let Some(RenderedKey {
            sprite,
            legacy_shadow,
        }) = render_unit_key(model, &drawn, self.vpl.as_ref(), pose)
        else {
            log::debug!("VXL {} produced no sprite for {drawn:?}", drawn.type_id);
            return None;
        };
        for sprite in legacy_shadow.into_iter().chain([sprite]) {
            resident.place(sprite, self.max_texture_dim);
        }
        resident.entries.get(key).copied()
    }

    /// Create the pages first-use draws opened and write the pictures drawn
    /// since the last upload. A frame calls this after building its objects
    /// and before drawing them; the queue writes land before its draws.
    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
    ) {
        let resident = self.resident.get_mut();
        for &[width, height] in resident
            .page_sizes
            .get(self.pages.len()..)
            .unwrap_or_default()
        {
            let texture = batch.create_blank_unit_atlas_texture(device, width, height);
            self.pages.push(UnitAtlasPage { texture });
        }
        for texels in resident.uploads.drain(..) {
            let Some(page) = self.pages.get(texels.page) else {
                continue;
            };
            atlas_growth::write_texels(
                queue,
                page.texture.view.texture(),
                texels.origin,
                texels.size,
                1,
                &texels.texels,
            );
        }
    }

    /// Number of pictures drawn so far.
    pub fn sprite_count(&self) -> usize {
        self.resident.borrow().entries.len()
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
        let resident = self.resident.borrow();
        Ok(UnitAtlasStatistics {
            resident_sprite_count: resident.entries.len(),
            last_build_rasterized_sprite_count: resident.rendered,
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

    /// An atlas with no model admitted and no picture drawn, whose pages and
    /// pictures stay within the device's `max_texture_dim`.
    fn new(max_texture_dim: u32) -> Self {
        Self {
            pages: Vec::new(),
            frame_counts: BTreeMap::new(),
            resident: RefCell::default(),
            shadow_masks: RefCell::default(),
            covered: UnitAtlasDemand::default(),
            models: HashMap::new(),
            vpl: None,
            max_texture_dim,
        }
    }
}

/// [`Resident::rendered_index`]: the first resident sprite of each key, and
/// of each placement its key's entry currently names.
#[derive(Default)]
struct RenderedIndex {
    by_key: HashMap<UnitSpriteKey, usize>,
    by_place: HashMap<SpritePlace, usize>,
}

/// An atlas rectangle: page, UV origin and UV size, compared bit for bit.
type SpritePlace = (usize, [u32; 2], [u32; 2]);

fn sprite_place(entry: &UnitSpriteEntry) -> SpritePlace {
    (
        entry.page,
        entry.uv_origin.map(f32::to_bits),
        entry.uv_size.map(f32::to_bits),
    )
}

impl Resident {
    /// Place a rasterized sprite on the open page, opening another when it
    /// is full, and stage its texels for the next upload. A sprite larger
    /// than the texture limit, or whose pixels do not fill it, is counted
    /// and left out.
    fn place(&mut self, sprite: CachedUnitSprite, max_dim: u32) {
        self.rendered = self.rendered.saturating_add(1);
        let [width, height] = [sprite.width, sprite.height];
        if sprite.pixels.len() != width as usize * height as usize {
            log::warn!(
                "{} VXL sprite has {} pixels for {width}x{height}",
                sprite.key.type_id,
                sprite.pixels.len(),
            );
            return;
        }
        let spot = self
            .shelf
            .as_mut()
            .and_then(|open| Some((open.page, open.shelf.place(width, height)?)));
        let (page, origin) = match spot {
            Some(spot) => spot,
            None => {
                let size = [width, height].map(|side| PAGE_SIZE.max(side).min(max_dim));
                let mut shelf = ShelfCursor::new(size[0], size[1]);
                let Some(origin) = shelf.place(width, height) else {
                    log::warn!(
                        "{} VXL sprite ({width}x{height}) exceeds the texture limit {max_dim}",
                        sprite.key.type_id,
                    );
                    return;
                };
                let page = self.page_sizes.len();
                log::info!("Unit atlas page {page} ({}x{})", size[0], size[1]);
                self.page_sizes.push(size);
                self.shelf = Some(GrowthShelf { page, shelf });
                (page, origin)
            }
        };
        let entry = unit_entry(&sprite, origin, page, self.page_sizes[page]);
        self.uploads.push(PendingTexels {
            page,
            origin,
            size: [width, height],
            texels: sprite.pixels.clone(),
        });
        if let Some(old) = self.entries.insert(sprite.key.clone(), entry) {
            self.rendered_index.by_place.remove(&sprite_place(&old));
        }
        self.pool_bytes
            .insert(sprite.key.clone(), sprite.pool_bytes);
        self.rendered_cache.push(sprite);
        self.index_rendered(self.rendered_cache.len() - 1);
    }

    /// Stage `texels` over the picture `entry` names, written after every
    /// picture placed before it.
    fn restage(&mut self, entry: &UnitSpriteEntry, texels: Vec<u8>) {
        let Some(&[page_width, page_height]) = self.page_sizes.get(entry.page) else {
            return;
        };
        self.uploads.push(PendingTexels {
            page: entry.page,
            origin: [
                (entry.uv_origin[0] * page_width as f32).round() as u32,
                (entry.uv_origin[1] * page_height as f32).round() as u32,
            ],
            size: entry.pixel_size.map(|side| side as u32),
            texels,
        });
    }

    /// Index `rendered_cache[index]` under its key and its key's placement,
    /// keeping the first sprite of each, as a walk in cache order finds it.
    fn index_rendered(&mut self, index: usize) {
        let key = &self.rendered_cache[index].key;
        let first = *self
            .rendered_index
            .by_key
            .entry(key.clone())
            .or_insert(index);
        if let Some(entry) = self.entries.get(key) {
            self.rendered_index
                .by_place
                .entry(sprite_place(entry))
                .or_insert(first);
        }
    }

    /// The first resident sprite with `key`.
    fn rendered_by_key(&self, key: &UnitSpriteKey) -> Option<&CachedUnitSprite> {
        let index = *self.rendered_index.by_key.get(key)?;
        Some(&self.rendered_cache[index])
    }

    /// The first resident sprite whose key's entry is placed at `part`'s
    /// page and UV rectangle.
    fn rendered_by_place(&self, part: &UnitSpriteEntry) -> Option<&CachedUnitSprite> {
        let index = *self.rendered_index.by_place.get(&sprite_place(part))?;
        Some(&self.rendered_cache[index])
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
    // One companion per facing and slope, beside the flight template that
    // the ground draw's key falls back to.
    let legacy_shadow = (key.layer == VxlLayer::Shadow
        && key.frame == 0
        && native_draw_bounds.is_some()
        && model.shadow.is_some_and(|shadow| shadow.ground))
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
    let mut cached = CachedUnitSprite::from_rendered(RenderedSprite {
        key: key.clone(),
        sprite,
        native_draw_bounds,
    });
    if key.layer == VxlLayer::Shadow
        && native_draw_bounds.is_some()
        && let Some(shadow) = model.shadow
        && let Some(body) = &model.body
    {
        cached.shadow_center = vxl_raster::shadow::center(
            body.vxl(),
            body.hva(),
            &VxlRenderParams {
                facing: key.facing,
                slope_type: key.slope_type,
                ..Default::default()
            },
            shadow.index,
            shadow.considered_aircraft,
        );
    }
    Some(RenderedKey {
        sprite: cached,
        legacy_shadow,
    })
}

/// A rendered sprite before it is placed on a page.
struct RenderedSprite {
    key: UnitSpriteKey,
    sprite: VxlSprite,
    native_draw_bounds: Option<[i32; 4]>,
}

/// Cached rendered unit sprite — palette indices only, depth buffer stripped.
/// Depth is only used during VXL compositing (body+turret+barrel merge),
/// not after placement. One byte per pixel (palette index, post-VPL).
struct CachedUnitSprite {
    key: UnitSpriteKey,
    /// Palette-index pixels (1 byte each, width × height total).
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    offset_x: f32,
    offset_y: f32,
    native_draw_bounds: Option<[i32; 4]>,
    ///756860 adds its mask displacement to this fractional center before
    ///truncating. Source: the same model/key/HVA geometry as these pixels.
    shadow_center: Option<[f32; 2]>,
    /// The pool bytes the original's image cache takes for these pixels
    /// ([`crate::render::voxel_image_cache::sprite_pool_bytes`]), counted
    /// once where the sprite is rasterized.
    pool_bytes: u32,
}

impl CachedUnitSprite {
    fn from_rendered(rs: RenderedSprite) -> Self {
        let pool_bytes = crate::render::voxel_image_cache::sprite_pool_bytes(
            &rs.sprite.palette_indices,
            [rs.sprite.width, rs.sprite.height],
            [rs.sprite.offset_x, rs.sprite.offset_y],
            rs.native_draw_bounds,
        );
        Self {
            key: rs.key,
            pixels: rs.sprite.palette_indices,
            width: rs.sprite.width,
            height: rs.sprite.height,
            offset_x: rs.sprite.offset_x,
            offset_y: rs.sprite.offset_y,
            native_draw_bounds: rs.native_draw_bounds,
            shadow_center: None,
            pool_bytes,
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
                        type_id: type_id.into(),
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
    frame_counts: &mut BTreeMap<(smol_str::SmolStr, VxlLayer, i32), u32>,
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
    let shadow = rules.and_then(|rules| ShadowModel::for_type(rules, variant));
    let mut shadow_frames = 1;
    for (layer, turret_index) in layers {
        let frame_key = (variant.into(), layer, turret_index);
        let num_frames = *frame_counts.entry(frame_key).or_insert_with(|| {
            detect_hva_frame_count(asset_manager, variant, layer, turret_index, rules)
        });
        if matches!(layer, VxlLayer::Body | VxlLayer::Composite) {
            shadow_frames = num_frames;
        }
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
    // Aircraft414728 includes the body frame remainder in its shadow cache
    // key, but707280 still samples ShadowIndex's HVA frame zero. Unit's key
    // (73B619, 73C21E..73C287) carries no body frame.
    if is_ground_vehicle || shadow.is_some_and(|shadow| shadow.aircraft || shadow.jumpjet) {
        let mut shadows = HashSet::new();
        insert_unit_layer_keys(
            &mut shadows,
            variant,
            VxlLayer::Shadow,
            0,
            if shadow.is_some_and(|s| s.aircraft) {
                shadow_frames
            } else {
                1
            },
            true,
            &[0],
        );
        // Unit73C468's grounded mask has its own entry: on every slope under
        // the Jumpjet suffix, and on level ground for Drive.
        let grounded: Vec<_> = shadows
            .iter()
            .filter(|key| {
                shadow.is_some_and(|shadow| {
                    !shadow.aircraft && (shadow.jumpjet || key.slope_type == 0)
                })
            })
            .map(|key| UnitSpriteKey {
                frame: GROUNDED_SHADOW_FRAME_FLAG,
                ..key.clone()
            })
            .collect();
        needed.extend(shadows);
        needed.extend(grounded);
    }
}

type UnitFrameCounts = BTreeMap<(smol_str::SmolStr, VxlLayer, i32), u32>;

/// Every unit sprite key the voxel models in `demand` can draw, with their
/// HVA frame counts.
///
/// Ground vehicles get keys on all 17 slopes (0-16), so a draw on any ramp
/// finds its picture.
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

/// Admit the world's voxel models that `atlas` lacks, making the atlas with
/// the first one: each model's parsed files, HVA frame counts and the keys it
/// draws. Nothing is drawn here; the first [`UnitAtlas::get`] of a key draws
/// its picture, as gamemd draws an image on its first cache miss. Map load
/// and every tick whose world may hold a new model call this; a covered
/// world costs one check per model.
pub(crate) fn admit_world_models(
    atlas: &mut Option<UnitAtlas>,
    entities: &crate::sim::entity_store::EntityStore,
    asset_manager: &AssetManager,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    max_texture_dim: u32,
) {
    if atlas_covers_world(atlas.as_ref(), entities, rules, interner) {
        return;
    }
    let started = Instant::now();
    let atlas = atlas.get_or_insert_with(|| UnitAtlas {
        vpl: load_vpl(asset_manager),
        ..UnitAtlas::new(max_texture_dim)
    });
    // For turret units, separate Body/Turret/Barrel keys per facing; for the
    // rest a single Composite key per facing. Multi-frame HVA units get keys
    // for each frame.
    let missing = UnitAtlasDemand::of_world(entities, rules, interner).without(&atlas.covered);
    let (needed, frame_counts) = needed_unit_keys(&missing, asset_manager, rules);
    let mut by_model: BTreeMap<smol_str::SmolStr, Vec<UnitSpriteKey>> = BTreeMap::new();
    for key in needed {
        by_model.entry(key.type_id.clone()).or_default().push(key);
    }
    let resident = atlas.resident.get_mut();
    let mut admitted = 0;
    for (type_id, keys) in by_model {
        if !atlas.models.contains_key(&type_id) {
            let Some(model) = UnitModel::load(asset_manager, &type_id, rules) else {
                log::warn!(
                    "No voxel model for {type_id}: its {} pictures stay undrawn",
                    keys.len()
                );
                continue;
            };
            atlas.models.insert(type_id, model);
        }
        // A type seeded as an aircraft after its ground variant (or the
        // reverse) shares its flat keys with the resident ones.
        for key in keys {
            if !resident.entries.contains_key(&key) && resident.undrawn.insert(key) {
                admitted += 1;
            }
        }
    }
    log::info!(
        "Unit atlas: {} new voxel models admitted in {:.1} ms; {admitted} pictures drawn on first use",
        missing.len(),
        started.elapsed().as_secs_f64() * 1000.0,
    );
    atlas.frame_counts.extend(frame_counts);
    atlas.covered.extend(missing);
}

/// `VOXELS.VPL`'s lighting sections; None, with a warning, draws with the
/// rasterizer's fallback N·L shading.
fn load_vpl(asset_manager: &AssetManager) -> Option<VplFile> {
    let data = asset_manager.get_ref("VOXELS.VPL")?;
    match VplFile::from_bytes(data) {
        Ok(vpl) => {
            log::info!("Loaded VOXELS.VPL ({} lighting sections)", vpl.num_sections);
            Some(vpl)
        }
        Err(err) => {
            log::warn!("Failed to parse VOXELS.VPL: {err} — using fallback N·L shading");
            None
        }
    }
}

// Physical admission has one GPU-independent owner shared with native
// IsVoxel6F9E10/RockingUpdate. This module only materializes that owner's
// parsed parts; it never infers loading from ART or maintains a second parser.
use crate::rules::voxel_asset_catalog::VoxelPart;

fn render_part(part: &VoxelPart, params: &VxlRenderParams, vpl: Option<&VplFile>) -> VxlSprite {
    vxl_raster::render_vxl(part.vxl(), part.hva(), params, vpl)
}

/// [`render_part`] on the shared `depth` map when depth-tested.
fn render_part_on(
    part: &VoxelPart,
    params: &VxlRenderParams,
    vpl: Option<&VplFile>,
    depth: Option<&mut vxl_raster::DepthMap>,
) -> VxlSprite {
    vxl_raster::render_vxl_on(part.vxl(), part.hva(), params, vpl, depth)
}

/// A part's files, as a composite layer.
fn part_layer(part: &VoxelPart) -> (&VxlFile, Option<&HvaFile>) {
    (part.vxl(), part.hva())
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
    shadow: Option<ShadowModel>,
    /// The type's `UseBuffer=`: 706ED0 sets 0xB43180 from
    /// `GetTechnoType()+0xDAC` for its body, turret and barrel draws alike,
    /// so its parts are always depth-tested. The shadow raster ignores it.
    use_buffer: bool,
}

#[derive(Clone, Copy)]
struct ShadowModel {
    index: i32,
    considered_aircraft: bool,
    aircraft: bool,
    /// Draws through Unit73C1D2's Jumpjet suffix: on every slope, with its
    /// grounded mask in a separate entry.
    jumpjet: bool,
    /// Also draws its ground shadow outside that suffix, so its legacy
    /// stencil stays resident beside a native one.
    ground: bool,
}

impl ShadowModel {
    fn for_type(rules: &RuleSet, type_id: &str) -> Option<Self> {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::rules::object_type::ObjectCategory;
        let object = rules.object(type_id)?;
        let aircraft = object.category == ObjectCategory::Aircraft
            && matches!(object.locomotor, LocomotorKind::Fly | LocomotorKind::Rocket);
        let vehicle = object.category == ObjectCategory::Vehicle;
        let own_jumpjet = vehicle && object.locomotor == LocomotorKind::Jumpjet;
        // An `IsLocomotor=` hit (BulletClass::DetonateAtCoord 0x004694CB,
        // `sim::locomotor_lift`) installs its warhead's `Locomotor=` on a
        // vehicle whose Size its shot exceeds (stock MagneticBeam's 5000
        // exceeds every one's); a Jumpjet there (stock LocomotorBeam) draws
        // the victim through that suffix, whose Shadow_Matrix is the base
        // 0x0055A7D0 at its cell's slope.
        let lifted = vehicle
            && !own_jumpjet
            && rules
                .warheads_iter()
                .any(|warhead| warhead.is_locomotor && warhead.locomotor == LocomotorKind::Jumpjet);
        (aircraft
            || own_jumpjet
            || lifted
            || (object.locomotor == LocomotorKind::Drive && !object.considered_aircraft))
            .then_some(Self {
                index: object.voxel_shadow_index(),
                considered_aircraft: object.considered_aircraft,
                aircraft,
                jumpjet: own_jumpjet || lifted,
                ground: vehicle && !own_jumpjet,
            })
    }
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
        // ReadINI reads it for TechnoTypes only; bullets and voxel anims
        // share this model but draw outside 706ED0.
        let use_buffer = rules.is_some_and(|rules| {
            rules.object(type_id).is_some() && rules.art().use_buffer(type_id)
        });
        // ObjectType5F8110 and BuildingType45FA90 have one physical loader.
        // The catalog retains optional old slots and copied generations;
        // rendering must not reinterpret them from current file names.
        let model =
            crate::rules::voxel_asset_catalog::load_voxel_model(asset_manager, type_id, rules)?;
        let body = model.main().cloned();
        let building = rules
            .and_then(|rules| rules.object(type_id))
            .is_some_and(|object| {
                object.category == crate::rules::object_type::ObjectCategory::Building
            });
        if !building && body.is_none()
            || building && model.auxiliary().is_none() && model.barrel().is_none()
        {
            return None;
        }
        let count = crate::rules::voxel_frame_catalog::indexed_turret_count(type_id, rules);
        let guns = if count > 0 {
            // The loader owns the bounded native slot array; never allocate
            // arbitrary space from an unchecked authored TurretCount.
            (0..(count as usize).min(model.indexed_count()))
                .map(|index| {
                    let (turret, barrel) = model.indexed_parts(index);
                    GunParts {
                        turret: turret.cloned(),
                        barrel: barrel.cloned(),
                    }
                })
                .collect()
        } else if building
            || crate::rules::voxel_frame_catalog::draws_turret_parts(type_id, rules, 0)
        {
            vec![GunParts {
                turret: model.auxiliary().cloned(),
                barrel: model.barrel().cloned(),
            }]
        } else {
            Vec::new()
        };
        Some(Self {
            type_id: type_id.into(),
            body,
            guns,
            shadow: rules.and_then(|rules| ShadowModel::for_type(rules, type_id)),
            use_buffer,
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
        self.render_styled(key, vpl, slope_blend, pose, self.use_buffer, None)
    }

    /// A stable key's sprite as a draw renders it while 0xB43180 is set
    /// ([`crate::render::voxel_image_cache`]): depth-tested whatever the
    /// type's `UseBuffer=`. Shadows ignore the flag.
    /// `depth` is the shared depth map the draw reads and writes; None draws
    /// on a clear one.
    pub(crate) fn render_depth_tested(
        &self,
        key: &UnitSpriteKey,
        vpl: Option<&VplFile>,
        depth: Option<&mut vxl_raster::DepthMap>,
    ) -> Option<(VxlSprite, Option<[i32; 4]>)> {
        self.render_styled(key, vpl, None, &mut None, true, depth)
    }

    /// `depth` rasterizes the requested part, or each part of a Composite,
    /// on the shared depth map; the parts that only size its padding draw
    /// clear.
    fn render_styled(
        &self,
        key: &UnitSpriteKey,
        vpl: Option<&VplFile>,
        slope_blend: Option<VxlSlopeBlend>,
        pose: &mut Option<PoseParts>,
        use_buffer: bool,
        depth: Option<&mut vxl_raster::DepthMap>,
    ) -> Option<(VxlSprite, Option<[i32; 4]>)> {
        let (turret, barrel) = self.gun_parts(key.turret_index);
        if (key.layer == VxlLayer::Turret && turret.is_none())
            || (key.layer == VxlLayer::Barrel && barrel.is_none())
        {
            return None;
        }
        let params: VxlRenderParams = VxlRenderParams {
            frame: if key.layer == VxlLayer::Shadow {
                0
            } else {
                key.frame
            },
            facing: key.facing, // already quantized by atlas key generation
            slope_type: key.slope_type,
            slope_blend,
            use_buffer,
            ..VxlRenderParams::default()
        };
        // Aircraft templates are unmasked: the original4148A0->4DB0D0->
        //706BD0->707280 caller supplies NULL for its body-mask surface.
        // Unit Jumpjet has a distinct grounded first-fill mask and fresh
        // positive low-flight masks; its high-flight template stays unmasked.
        // Ground Drive retains its first-fill composed-body mask in the
        // grounded entry a Track/Wheel type keys at 73C468, so a lifted
        // vehicle's flight template stays unmasked.
        if key.layer == VxlLayer::Shadow
            && key.frame != LEGACY_SHADOW_FRAME
            && let Some(shadow) = self.shadow
            && (shadow.aircraft
                || shadow.jumpjet
                || (params.slope_type == 0 && key.frame == GROUNDED_SHADOW_FRAME_FLAG))
            && let Some(body) = &self.body
        {
            let sprite = vxl_raster::shadow::render(
                body.vxl(),
                body.hva(),
                &params,
                shadow.index,
                shadow.considered_aircraft,
            );
            if (shadow.aircraft || (shadow.jumpjet && !shadow.ground)) && sprite.is_none() {
                return None;
            }
            if let Some(sprite) = sprite {
                let bounds = [
                    sprite.offset_x as i32,
                    sprite.offset_y as i32,
                    sprite.width as i32,
                    sprite.height as i32,
                ];
                return Some((sprite, Some(bounds)));
            }
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
                &[self.body.as_ref(), turret, barrel]
                    .into_iter()
                    .flatten()
                    .map(part_layer)
                    .collect::<Vec<_>>(),
                &params,
                vpl,
                depth,
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
                let barrel_part = barrel;
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
                let live = match depth {
                    Some(depth) => Some(match key.layer {
                        VxlLayer::Body => {
                            render_part_on(self.body.as_ref()?, &params, vpl, Some(depth))
                        }
                        VxlLayer::Turret => render_part_on(turret?, &params, vpl, Some(depth)),
                        _ => render_part_on(barrel_part?, &part_params, vpl, Some(depth)),
                    }),
                    None => None,
                };
                let requested = live.as_ref().unwrap_or(requested);
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

    /// One posed body's composite, rendered afresh. The Fly crash and Jumpjet
    /// arms key their draw -1, so native never caches it. The Fly normal arm
    /// keys its pitch and roll bits (`0x004CF610`) and the Rocket arm keys a
    /// pitched draw -1 unless the pitch is the block's PitchInitial or stored
    /// PitchFinal product (`0x00663470`); either image depends only on the
    /// facing step and the angles, so a fresh render in the stored style is
    /// the cached one. The Jumpjet arm's half sizes come from this model's
    /// main voxel; one that never loaded leaves them at the constructor's
    /// zero (`0x00710C4E`). `depth_tested` is 0xB43180 at the draw; a
    /// `UseBuffer=` type always sets it.
    pub(crate) fn render_pose(
        &self,
        key: &UnitSpriteKey,
        vpl: Option<&VplFile>,
        tilt: PoseTilt,
        depth_tested: bool,
        depth: Option<&mut vxl_raster::DepthMap>,
    ) -> Option<(VxlSprite, Option<[i32; 4]>)> {
        let (layers, params, bounds) = self.pose_draw(key, tilt, depth_tested)?;
        Some((composite_parts(&layers, &params, vpl, depth), bounds))
    }

    /// [`Self::render_pose`] on a clear depth map, as an image for the GPU
    /// raster where the pose draws one part; a composite of several stays
    /// on the CPU.
    pub(crate) fn pose_image(
        &self,
        key: &UnitSpriteKey,
        vpl: Option<&VplFile>,
        tilt: PoseTilt,
        depth_tested: bool,
    ) -> Option<(vxl_raster::VoxelImage<'_>, Option<[i32; 4]>)> {
        let (layers, params, bounds) = self.pose_draw(key, tilt, depth_tested)?;
        let image = match layers[..] {
            [(vxl, hva)] => vxl_raster::voxel_image(vxl, hva, &params, vpl),
            _ => vxl_raster::VoxelImage::Cpu(composite_parts(&layers, &params, vpl, None)),
        };
        Some((image, bounds))
    }

    /// What a posed draw rasterizes, its parameters and its native draw
    /// bounds: a ground part alone, or the body with its gun parts.
    #[allow(clippy::type_complexity)]
    fn pose_draw(
        &self,
        key: &UnitSpriteKey,
        tilt: PoseTilt,
        depth_tested: bool,
    ) -> Option<(
        Vec<(&VxlFile, Option<&HvaFile>)>,
        VxlRenderParams,
        Option<[i32; 4]>,
    )> {
        let use_buffer = self.use_buffer || depth_tested;
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
                use_buffer,
                ..VxlRenderParams::default()
            };
            let bounds = part_native_draw_bounds(Some(part), &params).ok().flatten();
            return Some((vec![part_layer(part)], params, bounds));
        }
        let body_tilt = match tilt {
            PoseTilt::Fly(angles) => vxl_raster::BodyTilt::Fly(angles),
            PoseTilt::FlyNormal { pitch, roll } => vxl_raster::BodyTilt::FlyNormal { pitch, roll },
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
            use_buffer,
            ..VxlRenderParams::default()
        };
        let (turret, barrel) = self.gun_parts(key.turret_index);
        let layers = [Some(body), turret, barrel]
            .into_iter()
            .flatten()
            .map(part_layer)
            .collect();
        let bounds = self.native_draw_bounds(&params, VxlLayer::Composite, key.turret_index);
        Some((layers, params, bounds))
    }

    /// A `VoxelAnimClass` piece at its bounce matrix: the body, drawn without
    /// an HVA and lit diffusely, and its ground shadow stencil
    /// (`VoxelAnimClass::DrawIt @ 0x00749B70`). DrawIt draws only the type's
    /// `VoxelIndex=` section for both passes (749CCF/749DCE), while diffuse
    /// lighting still reads physical section zero (749D9F/753CE8).
    /// Invalid indices omit the draw instead of native's unchecked access.
    /// `use_buffer` is 0xB43180 at the
    /// draw; the shadow ignores it.
    pub(crate) fn render_tumble(
        body: &VoxelPart,
        section: i32,
        matrix: glam::Mat4,
        vpl: Option<&VplFile>,
        use_buffer: bool,
        depth: Option<&mut vxl_raster::DepthMap>,
    ) -> Option<(VxlSprite, Option<VxlSprite>)> {
        let index = usize::try_from(section).ok()?;
        let (vxl, hva, params, _) = Self::matrix_draw(
            body,
            matrix,
            0,
            false,
            vxl_raster::VxlLighting::Diffuse,
            use_buffer,
            Some(index),
        )?;
        Some((
            vxl_raster::render_vxl_on(vxl, hva, &params, vpl, depth),
            vxl_raster::shadow::render_matrix(vxl, matrix, section, false),
        ))
    }

    /// [`Self::render_tumble`] on a clear depth map, as images for the GPU
    /// raster.
    pub(crate) fn tumble_images<'a>(
        body: &'a VoxelPart,
        section: i32,
        matrix: glam::Mat4,
        vpl: Option<&VplFile>,
        use_buffer: bool,
    ) -> Option<(
        vxl_raster::VoxelImage<'a>,
        Option<vxl_raster::VoxelImage<'a>>,
    )> {
        let index = usize::try_from(section).ok()?;
        let (vxl, hva, params, _) = Self::matrix_draw(
            body,
            matrix,
            0,
            false,
            vxl_raster::VxlLighting::Diffuse,
            use_buffer,
            Some(index),
        )?;
        Some((
            vxl_raster::voxel_image(vxl, hva, &params, vpl),
            vxl_raster::shadow::matrix_image(vxl, matrix, section, false),
        ))
    }

    /// Shared direct-matrix body raster. Bullet46B0C0 submits every section
    /// with HVA frame modulo count and diffuse lighting; the VoxelAnim caller
    /// instead selects no HVA and separately renders its shadow. Neither
    /// stores the image: `use_buffer` is 0xB43180 at the draw, and `depth`
    /// the shared depth map it reads and writes (None: a clear one).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_matrix(
        &self,
        matrix: glam::Mat4,
        frame: u32,
        use_hva: bool,
        lighting: vxl_raster::VxlLighting,
        vpl: Option<&VplFile>,
        use_buffer: bool,
        depth: Option<&mut vxl_raster::DepthMap>,
    ) -> Option<(VxlSprite, Option<[i32; 4]>)> {
        let (vxl, hva, params, bounds) = Self::matrix_draw(
            self.body.as_ref()?,
            matrix,
            frame,
            use_hva,
            lighting,
            use_buffer,
            None,
        )?;
        Some((
            vxl_raster::render_vxl_on(vxl, hva, &params, vpl, depth),
            Some(bounds),
        ))
    }

    /// [`Self::render_matrix`] on a clear depth map, as an image for the GPU
    /// raster.
    pub(crate) fn matrix_image(
        &self,
        matrix: glam::Mat4,
        frame: u32,
        use_hva: bool,
        lighting: vxl_raster::VxlLighting,
        vpl: Option<&VplFile>,
        use_buffer: bool,
    ) -> Option<(vxl_raster::VoxelImage<'_>, Option<[i32; 4]>)> {
        let (vxl, hva, params, bounds) = Self::matrix_draw(
            self.body.as_ref()?,
            matrix,
            frame,
            use_hva,
            lighting,
            use_buffer,
            None,
        )?;
        Some((
            vxl_raster::voxel_image(vxl, hva, &params, vpl),
            Some(bounds),
        ))
    }

    /// A direct-matrix body draw's files, parameters and native draw bounds.
    fn matrix_draw(
        body: &VoxelPart,
        matrix: glam::Mat4,
        frame: u32,
        use_hva: bool,
        lighting: vxl_raster::VxlLighting,
        use_buffer: bool,
        section: Option<usize>,
    ) -> Option<(&VxlFile, Option<&HvaFile>, VxlRenderParams, [i32; 4])> {
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
            section,
            use_buffer,
            ..VxlRenderParams::default()
        };
        let bounds = vxl_raster::native_vxl_draw_bounds(body.vxl(), hva, &params)?;
        Some((body.vxl(), hva, params, bounds))
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
                    crate::util::rect::union_native_draw_bounds(&mut bounds, part);
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
    let layers: Vec<_> = [Some((body, body_hva))]
        .into_iter()
        .chain([turret.as_ref(), barrel.as_ref()].map(|part| part.map(part_layer)))
        .flatten()
        .collect();
    composite_parts(&layers, params, vpl, None)
}

/// The parts, body first, rasterized and depth-composited; one part is its
/// own image. `depth` rasterizes each part in turn on the shared depth map.
fn composite_parts(
    parts: &[(&VxlFile, Option<&HvaFile>)],
    params: &VxlRenderParams,
    vpl: Option<&VplFile>,
    mut depth: Option<&mut vxl_raster::DepthMap>,
) -> VxlSprite {
    let mut layers: Vec<VxlSprite> = parts
        .iter()
        .map(|&(vxl, hva)| vxl_raster::render_vxl_on(vxl, hva, params, vpl, depth.as_deref_mut()))
        .collect();
    if layers.len() == 1 {
        return layers.pop().expect("one layer");
    }
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

// Tests extracted to unit_atlas_tests.rs to stay under 600 lines.
#[cfg(test)]
#[path = "unit_atlas_tests.rs"]
mod tests;

#[cfg(test)]
impl UnitAtlas {
    /// An atlas holding these pages, with no picture on them.
    pub(crate) fn from_test_pages(pages: Vec<UnitAtlasPage>) -> Self {
        let atlas = Self {
            pages,
            ..Self::new(8192)
        };
        atlas.resident.borrow_mut().page_sizes = atlas
            .pages
            .iter()
            .map(|page| [page.texture.width, page.texture.height])
            .collect();
        atlas
    }

    /// An atlas whose pictures sit at these entries, with no pages.
    pub(crate) fn from_test_entries(entries: HashMap<UnitSpriteKey, UnitSpriteEntry>) -> Self {
        let atlas = Self::new(8192);
        atlas.resident.borrow_mut().entries = entries.into_iter().collect();
        atlas
    }

    /// An atlas with these sprites placed as first-use draws place them,
    /// staged for the next [`Self::upload`].
    fn from_test_sprites(sprites: Vec<CachedUnitSprite>) -> Self {
        let atlas = Self::new(8192);
        for sprite in sprites {
            atlas.place_test_sprite(sprite);
        }
        atlas
    }

    /// Place `sprite` as a first-use draw does.
    fn place_test_sprite(&self, sprite: CachedUnitSprite) {
        self.resident
            .borrow_mut()
            .place(sprite, self.max_texture_dim);
    }

    /// Every key the admitted models draw, drawn yet or not.
    pub(crate) fn admitted_keys(&self) -> HashSet<UnitSpriteKey> {
        let resident = self.resident.borrow();
        resident
            .entries
            .keys()
            .chain(&resident.undrawn)
            .cloned()
            .collect()
    }
}
