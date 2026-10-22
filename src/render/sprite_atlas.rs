//! SHP sprite atlas — pre-renders SHP sprites into packed GPU texture pages.
//!
//! At map load every SHP sprite the world can draw — infantry sequences,
//! building bodies with their make and overlay animations, world effects — is
//! rendered to palette indices and shelf-packed into as few pages as the GPU
//! texture limit allows. Each sprite resolves its indices through one of the
//! atlas's palette rows (`ShpPalettes`): its render palette, house ramp
//! included. When a new (type, house colour) appears mid-match, only its missing
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
use crate::render::atlas_growth::{self, GrowthShelf, SPRITE_PADDING};
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::rules::art_data::{self, ArtRegistry};
use crate::rules::effect_asset_catalog::available_effect_anim_frame_count;
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};
use crate::rules::projectile_type::ProjectileType;
use crate::rules::ruleset::RuleSet;

/// Edge of a growth page: 16 MiB of palette indices, room for the sprites of
/// about ten new (type, house colour) pairs.
const GROWTH_PAGE_SIZE: u32 = 4096;
const INFANTRY_FACING_STEP: u8 = crate::util::direction::FACING_UNITS_PER_DIRECTION;
const INFANTRY_FACING_BUCKETS: u8 = 8;
/// The shadow InfantryClass::DrawIt draws under an airborne infantry: the
/// `INFSHDW.SHP` literal at `0x00825524`, cached at `0x00A8F244`, frame 1
/// (`0x0051957C`).
const INFANTRY_AIR_SHADOW_SHP: &str = "INFSHDW";
const INFANTRY_AIR_SHADOW_FRAME: u16 = 1;

fn register_effect_anim_frames(
    needed: &mut HashSet<ShpSpriteKey>,
    frame_counts: &mut HashMap<String, u16>,
    anim_type: &str,
    raw_count: u16,
    scheduler_owned: bool,
    shadow: bool,
) -> u16 {
    let count = available_effect_anim_frame_count(raw_count, scheduler_owned, shadow);
    // DrawIt selects the shadow from the physical second half of the SHP.
    // Residency includes that companion without extending animation timing.
    for frame in 0..raw_count {
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
    if art
        .and_then(|registry| registry.anim_runtime_config(anim_type))
        .is_some_and(|config| !config.art_body_read)
    {
        // AnimType::ReadINI @ 0x00427D22 exits before the image loader for an
        // unread type. An orphan SHP must not create an atlas entry for it.
        return Vec::new();
    }
    let image_id = art.map_or_else(
        || anim_type.to_ascii_uppercase(),
        |registry| registry.resolve_anim_image_id(anim_type),
    );
    art_data::anim_shp_candidates(art, anim_type, &image_id, theater_ext, theater_name)
}

/// First existing file among `candidates`, with its bytes.
/// The files a building type's Buildup SHP loads from: its art `Buildup=`
/// through the theater lookup (`rules::buildup_asset_catalog`), none without
/// one.
fn buildup_shp_candidates(
    art: &ArtRegistry,
    rules: Option<&RuleSet>,
    type_id: &str,
    theater_name: &str,
) -> Vec<String> {
    let rules_image = rules
        .and_then(|rules| rules.object(type_id))
        .map_or(type_id, |object| object.image.as_str());
    art.resolve_metadata_entry(type_id, rules_image)
        .and_then(|entry| entry.buildup.as_deref())
        .map(|buildup| art_data::building_shp_names(buildup, theater_name).to_vec())
        .unwrap_or_default()
}

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

/// A building animation's usable and physical frame counts and their source
/// file, resolved as AnimTypeClass loads its image. Only the header is read.
fn scan_building_anim_frame_count(
    asset_manager: &AssetManager,
    art_reg: &ArtRegistry,
    anim_type: &str,
    loop_end: u16,
    theater_ext: &str,
    theater_name: &str,
) -> Option<(u16, u16, String)> {
    let anim_image: String = art_reg.resolve_anim_image_id(anim_type);
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
    Some((count, raw_count, file.to_string()))
}

/// Register every frame of one building animation (ActiveAnim, IdleAnim and
/// their damaged or garrisoned variants) for one house colour. `scanned`
/// holds the counts this build has read, so each animation's SHP header is
/// read once per build.
fn register_building_anim(
    needed: &mut HashSet<ShpSpriteKey>,
    scanned: &mut HashMap<String, (u16, u16)>,
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
    if !scanned.contains_key(&upper)
        && let Some((count, raw_count, file)) = scan_building_anim_frame_count(
            asset_manager,
            art,
            anim_type,
            loop_end,
            theater_ext,
            theater_name,
        )
    {
        log::debug!("Building anim {anim_type}: {count} frames from {file}");
        scanned.insert(upper.clone(), (count, raw_count));
        shp_files.insert(upper.clone(), file);
    }
    let (count, raw_count) = scanned.get(&upper).copied().unwrap_or((1, 1));
    let config = art.anim_runtime_config(anim_type);
    let resident_count = if config.is_some_and(|config| config.shadow) {
        raw_count
    } else {
        count
    };
    insert_building_anim_frame_keys(
        needed,
        anim_type,
        resident_count,
        house_color,
        attached_anim_palette_context(config),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum ShpPaletteContext {
    #[default]
    Legacy,
    GlobalAnim,
    SelectedScheme,
    Cell,
    /// BulletClass::Draw 46838D: global PALETTE.PAL Convert87F6C4.
    BulletDefault,
    /// Bullet AnimPalette selects ANIM.PAL without AnimType AltPalette logic.
    BulletAnim,
    /// Caller supplies BulletClass's retained ColorScheme identity (+114).
    BulletFirer,
    /// AnimClass `+0xD4` holding a house's scheme from this frozen palette
    /// name. The interner and atlas both have scenario lifetime.
    ExtraScheme(crate::sim::intern::InternedId),
}

/// One remapped AnimClass palette variant: the anim type, its remap's atlas
/// context ([`anim_remap_context`]) and the remap colour.
pub type AnimRemapKey = (String, ShpPaletteContext, HouseColorIndex);

impl ShpPaletteContext {
    fn is_projectile(self) -> bool {
        matches!(
            self,
            Self::BulletDefault | Self::BulletAnim | Self::BulletFirer
        )
    }
}

/// BulletDraw46838D..468409 selects AnimPalette before FirersPalette. The
/// latter requires the bullet's retained scheme, not its source's current house.
pub fn projectile_key(
    type_id: &str,
    projectile_type: &ProjectileType,
    frame: u16,
    house_color: HouseColorIndex,
) -> ShpSpriteKey {
    let palette_context = if projectile_type.anim_palette {
        ShpPaletteContext::BulletAnim
    } else if projectile_type.firers_palette {
        ShpPaletteContext::BulletFirer
    } else {
        ShpPaletteContext::BulletDefault
    };
    ShpSpriteKey {
        palette_context,
        type_id: type_id.to_ascii_uppercase(),
        facing: 0,
        frame,
        house_color: if palette_context == ShpPaletteContext::BulletFirer {
            house_color
        } else {
            HouseColorIndex(0)
        },
    }
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
    /// The `SpriteInstance::source_palette` that resolves this sprite's
    /// palette indices: its row in the atlas's palette rows, plus one.
    pub source_palette: u32,
}

/// A single page of the multi-page sprite atlas.
/// Each page is a separate GPU texture with its own bind group.
pub struct SpriteAtlasPage {
    /// The page's palette indices (`BatchRenderer::create_indexed_page`),
    /// bound beside the atlas's palette rows.
    pub texture: BatchTexture,
}

/// Initial palette capacity. Distinct names/house colours grow this texture
/// without changing row identities, bounded by the device's actual 2D limit.
const SHP_PALETTE_ROWS: u32 = 256;

/// The palettes an SHP atlas's index pages resolve through, so the atlas
/// stores each sprite pixel once, as its palette index. Each row holds the 256
/// RGBA colours of one render palette: a base palette, house-remapped or not
/// ([`render_shp_frame`]). Rows are keyed by content, so every sprite drawn
/// through the same colours shares a row whichever file or remap produced
/// them. Registered rows reach the GPU when pages are created or grown.
#[derive(Clone, Default)]
struct ShpPalettes {
    /// Row of each registered palette, by its RGBA bytes.
    rows: HashMap<Vec<u8>, u32>,
    /// Registered palettes in row order; the first `uploaded` are on the GPU.
    colors: Vec<Vec<u8>>,
    uploaded: usize,
    /// The row texture, created by the first upload.
    texture: Option<(wgpu::Texture, wgpu::TextureView)>,
}

impl ShpPalettes {
    /// Register a stable row identity. Physical texture limits are checked
    /// after the refresh has collected every row, before publishing it.
    fn row(&mut self, palette: &Palette) -> Option<u32> {
        let colors = palette.to_rgba_bytes();
        if let Some(&row) = self.rows.get(&colors) {
            return Some(row);
        }
        let row = u32::try_from(self.colors.len()).ok()?;
        self.rows.insert(colors.clone(), row);
        self.colors.push(colors);
        Some(row)
    }

    /// Upload the rows registered since the last upload, creating the row
    /// texture on first use, and return its view for page bind groups.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
        let required = u32::try_from(self.colors.len())
            .expect("palette rows fit u32")
            .max(1);
        let max_rows = device.limits().max_texture_dimension_2d;
        assert!(
            required <= max_rows,
            "atlas validated the palette texture limit"
        );
        if self
            .texture
            .as_ref()
            .is_none_or(|(texture, _)| texture.height() < required)
        {
            let height = required
                .next_power_of_two()
                .max(SHP_PALETTE_ROWS)
                .min(max_rows);
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("SHP atlas palette rows"),
                size: wgpu::Extent3d {
                    width: 256,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // sRGB like the RGBA pages it replaces: reads decode the
                // palette bytes to the same linear values.
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.texture = Some((texture, view));
            self.uploaded = 0;
        }
        let (texture, view) = self.texture.as_ref().expect("palette texture installed");
        for (row, colors) in self.colors.iter().enumerate().skip(self.uploaded) {
            atlas_growth::write_texels(queue, texture, [0, row as u32], [256, 1], 4, colors);
        }
        self.uploaded = self.colors.len();
        view.clone()
    }

    /// The RGBA the shaders resolve `sprite`'s indices to (`indexed_color`).
    #[cfg(test)]
    fn rgba(&self, sprite: &RenderedShpSprite) -> Vec<u8> {
        let colors = &self.colors[sprite.palette as usize];
        sprite
            .indices()
            .into_iter()
            .flat_map(|index| {
                let entry = &colors[usize::from(index) * 4..usize::from(index) * 4 + 4];
                [
                    entry[0],
                    entry[1],
                    entry[2],
                    if index == 0 { 0 } else { entry[3] },
                ]
            })
            .collect()
    }
}

/// World-effect and parachute animations, registered once per match by the
/// map-load build; refreshes reuse them instead of re-reading the rules.
#[derive(Default)]
struct EffectRegistry {
    /// Each registered name as the rules spell it and its resident frame count,
    /// including shadow companions, by upper-case name. Remaps expand through it.
    frames: HashMap<String, (String, u16)>,
    /// Registered names as the rules spell them; they take the effect palette.
    type_ids: HashSet<String>,
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
    /// Object type → physical frame count of its resolved object SHP. Kept
    /// separately from presentation counts, which may exclude shadow frames.
    object_shp_frame_counts: HashMap<String, u16>,
    /// Keys with nothing to draw: the SHP is missing or fails to decode, or
    /// the frame is empty. Remembered so a refresh never renders them again.
    unrenderable: HashSet<ShpSpriteKey>,
    /// House colours whose sprites have been collected, by object type.
    covered_objects: HashMap<String, HashSet<HouseColorIndex>>,
    /// AnimClass remaps whose sprites have been collected.
    covered_anim_remaps: HashSet<AnimRemapKey>,
    /// Whether the harvest overlay frames were rendered (or found missing).
    harvest_overlay_loaded: bool,
    /// The file each counted animation, make and parachute type's frames came
    /// from (upper-case type), so its frames render from that file.
    shp_files: HashMap<String, String>,
    /// ProjectileType image binding is separate from AnimType frame sources.
    projectile_files: HashMap<String, (String, u16)>,
    effects: EffectRegistry,
    /// Page refreshes append to; created when the first refresh needs room.
    growth: Option<GrowthShelf>,
    /// Palette-index textures of the growth pages, by page; each page's bind
    /// group holds only a view of it.
    growth_indices: HashMap<usize, wgpu::Texture>,
    /// The palettes every page's indices resolve through.
    palettes: ShpPalettes,
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
    for anim_name in rules.art().scheduler_anim_types() {
        push_effect_name(&mut effect_names, anim_name);
    }
    // Explosion animations from every warhead's AnimList=.
    for wh in rules.warheads_iter() {
        for anim_name in &wh.anim_list {
            push_effect_name(&mut effect_names, anim_name);
        }
    }
    // Weapon Anim= and OccupantAnim=. Bullet images have a separate binding
    // and palette owner; they are not AnimType registrations.
    for weapon in rules.weapons_iter() {
        for anim_name in &weapon.anim {
            push_effect_name(&mut effect_names, anim_name);
        }
        if let Some(ref anim_name) = weapon.occupant_anim {
            push_effect_name(&mut effect_names, anim_name);
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
    fn new(
        pages: Vec<SpriteAtlasPage>,
        entries: HashMap<ShpSpriteKey, ShpSpriteEntry>,
        palettes: ShpPalettes,
    ) -> Self {
        Self {
            pages,
            entries,
            make_frame_counts: HashMap::new(),
            active_anim_frame_counts: HashMap::new(),
            object_shp_frame_counts: HashMap::new(),
            unrenderable: HashSet::new(),
            covered_objects: HashMap::new(),
            covered_anim_remaps: HashSet::new(),
            harvest_overlay_loaded: false,
            shp_files: HashMap::new(),
            projectile_files: HashMap::new(),
            effects: EffectRegistry::default(),
            growth: None,
            growth_indices: HashMap::new(),
            palettes,
        }
    }

    /// Look up the atlas entry for a given sprite key.
    /// The returned entry includes a `page` field identifying which atlas page
    /// holds this sprite's texture data.
    pub fn get(&self, key: &ShpSpriteKey) -> Option<&ShpSpriteEntry> {
        self.entries.get(key)
    }

    /// [`INFANTRY_AIR_SHADOW_SHP`]'s frame 1, the shadow an airborne infantry
    /// casts.
    pub(crate) fn infantry_air_shadow(&self) -> Option<&ShpSpriteEntry> {
        self.get(&ShpSpriteKey {
            palette_context: ShpPaletteContext::GlobalAnim,
            type_id: INFANTRY_AIR_SHADOW_SHP.to_string(),
            facing: 0,
            frame: INFANTRY_AIR_SHADOW_FRAME,
            house_color: HouseColorIndex(0),
        })
    }

    /// Physical frame count from the object type's resolved SHP source.
    pub(crate) fn object_shp_frame_count(&self, type_id: &str) -> Option<u16> {
        self.object_shp_frame_counts
            .get(&type_id.to_ascii_uppercase())
            .copied()
    }

    /// Exact frame lookup; an absent frame is not substituted or wrapped.
    /// `scheme` is the colour a `FirersPalette=` bullet draws with: its
    /// retained Bullet+114 House's, else the local player's
    /// (`0x004683A1..0x004683D1`).
    pub fn projectile_sprite(
        &self,
        type_id: &str,
        projectile_type: &ProjectileType,
        frame: u16,
        scheme: HouseColorIndex,
    ) -> Option<&ShpSpriteEntry> {
        self.get(&projectile_key(type_id, projectile_type, frame, scheme))
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

    /// Whether the atlas has collected every [`object_pairs`] pair the world
    /// draws, every live AnimClass colour remap, and the harvest overlay once
    /// a harvester exists. This runs on every tick with a spawn, death or
    /// Limbo, so it allocates nothing per object and stops at the first miss.
    /// Keys found unrenderable count as collected: a sprite with nothing to
    /// draw never forces another refresh.
    fn covers_world(
        &self,
        entities: &crate::sim::entity_store::EntityStore,
        house_colors: &HouseColorMap,
        extra_building_types: &[&str],
        anim_remaps: &HashSet<AnimRemapKey>,
        interner: Option<&crate::sim::intern::StringInterner>,
    ) -> bool {
        object_pairs(entities, house_colors, extra_building_types, interner)
            .all(|(type_id, color)| self.covers_object(type_id, color))
            && anim_remaps.is_subset(&self.covered_anim_remaps)
            && (self.harvest_overlay_loaded || !entities.values().any(|e| e.miner.is_some()))
    }

    fn covers_object(&self, type_id: &str, color: HouseColorIndex) -> bool {
        self.covered_objects
            .get(type_id)
            .is_some_and(|colors| colors.contains(&color))
    }

    /// Place sprites rendered after the map-load pack on growth pages and
    /// upload the rows they fill, one write per page and texture. A sprite
    /// no page can hold is remembered as unrenderable. `palettes` is this
    /// atlas's palette registry with the rows the sprites registered.
    fn append_sprites(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        sprites: Vec<RenderedShpSprite<'_>>,
        palettes: ShpPalettes,
    ) {
        let max_dim = device.limits().max_texture_dimension_2d;
        self.palettes = palettes;
        let old_rows = self
            .palettes
            .texture
            .as_ref()
            .map(|(texture, _)| texture.height());
        let palette_rows = self.palettes.upload(device, queue);
        if old_rows
            != self
                .palettes
                .texture
                .as_ref()
                .map(|(texture, _)| texture.height())
        {
            // All resident pages keep their index planes/row IDs and bind the
            // replacement palette texture. Nothing needs repacking.
            for page in &mut self.pages {
                batch.rebind_indexed_palette(device, &mut page.texture, &palette_rows);
            }
        }
        let (pages, growth_indices) = (&mut self.pages, &mut self.growth_indices);
        let (placed, unplaced) = atlas_growth::place_on_growth_pages(
            &mut self.growth,
            sprites,
            |sprite| [sprite.width, sprite.height],
            max_dim,
            |[width, height]| {
                let size = [width, height].map(|side| GROWTH_PAGE_SIZE.max(side).min(max_dim));
                let (texture, source_indices) =
                    batch.create_indexed_page(device, size[0], size[1], &palette_rows);
                pages.push(SpriteAtlasPage { texture });
                let page = pages.len() - 1;
                growth_indices.insert(page, source_indices);
                log::info!("Sprite atlas growth page {page} ({}x{})", size[0], size[1]);
                (page, size)
            },
        );
        for group in placed {
            let texture = &self.pages[group.page].texture;
            write_sprite_band(
                queue,
                &self.growth_indices[&group.page],
                group
                    .sprites
                    .iter()
                    .map(|(origin, sprite)| (*origin, sprite)),
            );
            let page_size = [texture.width, texture.height];
            for (origin, sprite) in group.sprites {
                match atlas_entry(&sprite, origin, group.page, page_size) {
                    Some(entry) => {
                        self.entries.insert(sprite.key, entry);
                    }
                    None => {
                        log::warn!("Sprite atlas page {} exceeds the u8 page index", group.page);
                        self.unrenderable.insert(sprite.key);
                    }
                }
            }
        }
        for sprite in unplaced {
            log::warn!(
                "{} frame {} ({}x{}) exceeds the texture limit {max_dim}",
                sprite.key.type_id,
                sprite.key.frame,
                sprite.width,
                sprite.height,
            );
            self.unrenderable.insert(sprite.key);
        }
    }
}

/// UV rectangle and draw offsets of `sprite` placed at `origin` on a page;
/// None past the entry's u8 page index.
fn atlas_entry(
    sprite: &RenderedShpSprite,
    origin: [u32; 2],
    page: usize,
    page_size: [u32; 2],
) -> Option<ShpSpriteEntry> {
    let [page_width, page_height] = page_size.map(|v| v as f32);
    Some(ShpSpriteEntry {
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
        page: u8::try_from(page).ok()?,
        source_palette: sprite.palette + 1,
    })
}

/// Intermediate rendered sprite before atlas packing.
///
/// It names its frame instead of holding the pixels, so the sprites waiting
/// for the pack hold none: each band decodes its sprites' frames as it is
/// uploaded ([`RenderedShpSprite::indices`]).
struct RenderedShpSprite<'a> {
    key: ShpSpriteKey,
    /// The atlas palette row its indices resolve through ([`ShpPalettes`]).
    palette: u32,
    /// The SHP file the frame is stored in, already decoded whole once, and
    /// the frame.
    shp: &'a [u8],
    frame: usize,
    width: u32,
    height: u32,
    /// Offset from cell center to top-left of sprite.
    offset_x: f32,
    offset_y: f32,
    canvas_rect: [f32; 4],
    extended: bool,
}

impl RenderedShpSprite<'_> {
    /// Palette indices of the stored frame rectangle, without canvas padding.
    fn indices(&self) -> Vec<u8> {
        ShpFile::decode_frame(self.shp, self.frame)
            .expect("a rendered sprite's SHP decoded whole when it was rendered")
    }
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

/// Whether `atlas` — None while no sprite atlas exists — already holds
/// everything the world draws, so no refresh is needed.
pub fn atlas_covers_world(
    atlas: Option<&SpriteAtlas>,
    entities: &crate::sim::entity_store::EntityStore,
    house_colors: &HouseColorMap,
    extra_building_types: &[&str],
    anim_remaps: &HashSet<AnimRemapKey>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> bool {
    match atlas {
        Some(atlas) => atlas.covers_world(
            entities,
            house_colors,
            extra_building_types,
            anim_remaps,
            interner,
        ),
        None => {
            object_pairs(entities, house_colors, extra_building_types, interner)
                .next()
                .is_none()
                && anim_remaps.is_empty()
        }
    }
}

/// Every (type, house colour) pair the world's objects draw: each placed
/// non-voxel object in its owner's colour, and every building a unit can
/// deploy into in every house colour. Every sprite key an object needs
/// derives from one of these pairs, so the atlas records coverage per pair.
fn object_pairs<'a>(
    entities: &'a crate::sim::entity_store::EntityStore,
    house_colors: &'a HouseColorMap,
    extra_building_types: &'a [&'a str],
    interner: Option<&'a crate::sim::intern::StringInterner>,
) -> impl Iterator<Item = (&'a str, HouseColorIndex)> + 'a {
    let placed = entities
        .values()
        .filter(|entity| !entity.is_voxel())
        .map(move |entity| {
            let owner = interner.map_or("", |i| i.resolve(entity.owner()));
            let color = house_colors
                .get(owner)
                .copied()
                .unwrap_or(crate::rules::house_colors::NO_REMAP);
            (interner.map_or("", |i| i.resolve(entity.type_ref())), color)
        });
    let deploy_targets = extra_building_types
        .iter()
        .flat_map(move |&type_id| house_colors.values().map(move |&color| (type_id, color)));
    placed.chain(deploy_targets)
}

/// Live AnimClass palette variants that must exist in the atlas. Producers
/// install these after construction (notably OverlayClass CellAnim over a
/// Tiberium cell and Unit deploy animations), so the attached object's live
/// ownership cannot supply the color key.
pub fn collect_anim_remap_base_keys(
    sim: &crate::sim::world::Simulation,
    house_colors: &HouseColorMap,
) -> HashSet<AnimRemapKey> {
    anim_remap_base_keys(sim, house_colors).collect()
}

/// Anim construction and producer palette assignment can occur without an
/// entity spawn or ownership change. Check their coverage on committed ticks
/// so those variants reach the ordinary AnimClass draw before its first frame.
/// Only live Anims are visited; the first missing pair ends the check.
pub(crate) fn atlas_covers_anim_remaps(
    atlas: Option<&SpriteAtlas>,
    sim: &crate::sim::world::Simulation,
    house_colors: &HouseColorMap,
) -> bool {
    anim_remap_base_keys(sim, house_colors)
        .all(|key| atlas.is_some_and(|atlas| atlas.covered_anim_remaps.contains(&key)))
}

fn anim_remap_base_keys<'a>(
    sim: &'a crate::sim::world::Simulation,
    house_colors: &'a HouseColorMap,
) -> impl Iterator<Item = AnimRemapKey> + 'a {
    sim.anims().filter_map(move |(_, anim)| {
        let color = anim_remap_color(anim, sim.interner(), house_colors)?;
        Some((
            sim.interner().resolve(anim.type_id()).to_ascii_uppercase(),
            anim_remap_context(anim),
            color,
        ))
    })
}

/// Resolve the producer's fixed palette identity for both atlas coverage and
/// draw lookup. House colors have one owner, the immutable match color map;
/// an attached unit's later owner/disguise does not change this selection.
pub(crate) fn anim_remap_color(
    anim: &crate::sim::anim_class::AnimObject,
    interner: &crate::sim::intern::StringInterner,
    house_colors: &HouseColorMap,
) -> Option<HouseColorIndex> {
    use crate::sim::anim_class::AnimRemap;
    Some(match anim.remap()? {
        AnimRemap::ColorScheme(color) => color,
        AnimRemap::House(house) | AnimRemap::ExtraBuildingScheme { house, .. } => house_colors
            .get(interner.resolve(house))
            .copied()
            .unwrap_or(crate::rules::house_colors::NO_REMAP),
    })
}

/// The palette a remapped anim's scheme is built on: the unit palette, or
/// the extra building palette for a custom-palette producer's scheme.
pub(crate) fn anim_remap_context(anim: &crate::sim::anim_class::AnimObject) -> ShpPaletteContext {
    match anim.remap() {
        Some(crate::sim::anim_class::AnimRemap::ExtraBuildingScheme { palette, .. }) => {
            ShpPaletteContext::ExtraScheme(palette)
        }
        _ => ShpPaletteContext::SelectedScheme,
    }
}

/// Every frame of each remapped AnimClass type the effect registry holds, in
/// its ColorScheme. Other remapped types have no preloaded frames.
fn insert_anim_remap_frame_keys(
    needed: &mut HashSet<ShpSpriteKey>,
    effects: &EffectRegistry,
    anim_remaps: &HashSet<AnimRemapKey>,
) {
    for (anim_type, context, color) in anim_remaps {
        let Some((type_id, frame_count)) = effects.frames.get(&anim_type.to_ascii_uppercase())
        else {
            continue;
        };
        for frame in 0..*frame_count {
            needed.insert(ShpSpriteKey {
                palette_context: *context,
                type_id: type_id.clone(),
                facing: 0,
                frame,
                house_color: *color,
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
    /// BulletClass's ordinary global Convert, independent of the theater.
    Bullet,
    /// The selected successfully loaded extra building palette for the
    /// active theater (`RuleSet::load_extra_building_palette`).
    ExtraBuilding,
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

/// `extra_palette_type_ids` holds the custom-palette TechnoTypes, `None` when
/// the extra building palette did not load. TechnoClass::DrawSHP tests
/// TerrainPalette first (`0x00705EC7`), then draws from GetRemapColour
/// (`0x00705F81`), whose `+0xDF0` arm (`0x00705D82..0x00705D9A`) picks the
/// house's scheme from the type's generated palette.
fn sprite_palette_for_key(
    key: &ShpSpriteKey,
    art: Option<&ArtRegistry>,
    effect_type_ids: &HashSet<String>,
    cell_palette_type_ids: &HashSet<String>,
    extra_palette_type_ids: Option<&HashSet<String>>,
) -> SpritePaletteChoice {
    match key.palette_context {
        ShpPaletteContext::BulletDefault => SpritePaletteChoice::Bullet,
        ShpPaletteContext::BulletAnim => SpritePaletteChoice::Anim,
        ShpPaletteContext::BulletFirer => SpritePaletteChoice::Unit,
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
        ShpPaletteContext::ExtraScheme(_) if extra_palette_type_ids.is_some() => {
            SpritePaletteChoice::ExtraBuilding
        }
        // Production creates this context only for a successfully bound name.
        // An unavailable diagnostic resource keeps the ordinary house scheme.
        ShpPaletteContext::ExtraScheme(_) => SpritePaletteChoice::Unit,
        ShpPaletteContext::Legacy => {
            let upper = key.type_id.to_ascii_uppercase();
            if !cell_palette_type_ids.contains(&upper)
                && extra_palette_type_ids.is_some_and(|ids| ids.contains(&upper))
            {
                SpritePaletteChoice::ExtraBuilding
            } else {
                sprite_palette_choice(&key.type_id, art, effect_type_ids, cell_palette_type_ids)
            }
        }
    }
}

/// Every sprite key one object (type, house colour) can be drawn with: a
/// structure's body, source-bound raw Infantry records, or a generic layout.
/// Return body keys for paired shadows without scanning the
/// complete atlas key set for every newly admitted type and house colour.
fn insert_object_keys(
    needed: &mut HashSet<ShpSpriteKey>,
    type_str: &str,
    category: EntityCategory,
    color_idx: HouseColorIndex,
    rules: Option<&RuleSet>,
    infantry_frames: Option<&[u16]>,
) -> Vec<ShpSpriteKey> {
    let mut body_keys = Vec::new();
    let mut insert = |frame: u16| {
        let key = ShpSpriteKey {
            palette_context: ShpPaletteContext::Legacy,
            type_id: type_str.to_string(),
            facing: 0,
            frame,
            house_color: color_idx,
        };
        if matches!(
            category,
            EntityCategory::Structure | EntityCategory::Unit | EntityCategory::Infantry
        ) {
            body_keys.push(key.clone());
        }
        needed.insert(key);
    };
    match category {
        EntityCategory::Infantry => {
            // Infantry518D80 indexes the full 42-record signed bank by Doing.
            // The source-bound list was resolved once for this type, before
            // expanding its house colours. No generic SequenceKind projection
            // or invalid-frame standing fallback participates in this path.
            for &frame in infantry_frames.unwrap_or_default() {
                insert(frame);
            }
        }
        EntityCategory::Structure => {
            insert(0);
            // Completed Idle bodies use frame0 when healthy and frame1 when
            // damaged: GetCurrentFrame43EF90, executed in
            // tools/spatial_oracle/building_body_transition.json. The draw
            // consumer requests both, so loading only0 hides an ordinary
            // damaged GAPOWR before its fatal receiver.
            insert(1);
            // CanBeOccupied bodies additionally select frames2/3. Missing
            // entries retain the existing garrison-body fallback in the draw
            // consumer; this does not invent a frame for a shorter SHP.
            let can_be_occupied = rules
                .and_then(|r| r.object(type_str))
                .is_some_and(|obj| obj.can_be_occupied);
            if can_be_occupied {
                for frame in 2u16..=3 {
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
                    SequenceKind::Shovel,
                    SequenceKind::AirDeathStart,
                    SequenceKind::AirDeathFalling,
                    SequenceKind::AirDeathFinish,
                ];
                for kind in relevant_kinds {
                    if let Some(seq_def) = set.get(kind) {
                        for f_idx in 0..seq_def.facings {
                            for frame_offset in 0..seq_def.frame_count {
                                // Use facing=0 for all infantry keys — the absolute
                                // frame index already encodes the facing direction.
                                // House variants share these absolute frame keys.
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
    body_keys
}

/// Frames an InfantryType's signed records can select in this actual SHP.
///
/// Infantry518E08..1F and518F3B..88 retain signed Stage and DWORD composition.
/// Iterate the asset, never a potentially enormous/negative sequence count.
/// For each facing, invert the composed base with wrapping subtraction and
/// let the sole frame resolver admit the candidate Stage. Every signed native
/// remainder is smaller than i32::MAX, so this candidate is unique even when
/// composition wraps. All 8 native facing slots have a representative byte.
fn infantry_asset_frames(
    actions: &[crate::rules::infantry_sequence::InfantrySequenceEntry],
    frame_count: u16,
) -> Vec<u16> {
    let mut bases = Vec::with_capacity(actions.len() * usize::from(INFANTRY_FACING_BUCKETS));
    for action in actions {
        let facings = if action.facings > 0 {
            INFANTRY_FACING_BUCKETS
        } else {
            1
        };
        for slot in 0..facings {
            let facing = slot * INFANTRY_FACING_STEP;
            bases.push((
                action,
                facing,
                crate::sim::animation::resolve_shp_frame(action, facing, 0),
            ));
        }
    }
    (0..frame_count)
        .filter(|&frame| {
            let frame = i32::from(frame);
            bases.iter().any(|&(action, facing, base)| {
                crate::sim::animation::resolve_shp_frame(action, facing, frame.wrapping_sub(base))
                    == frame
            })
        })
        .collect()
}

/// The keys of `new_objects`, each drawn as the category of the placed objects
/// of its type; a deploy target not on the map yet is a structure. A selected
/// Infantry disguise type can be absent from the placed set; its rules class
/// still supplies the Infantry source and signed record bank.
///
/// Return decoded object sources for the render pass to consume, so each
/// Infantry type's frame membership and each SHP object's shadow frames are
/// computed from its resolved SHP once per refresh, independent of house colour.
fn insert_new_object_keys<'a>(
    needed: &mut HashSet<ShpSpriteKey>,
    new_objects: &HashSet<(String, HouseColorIndex)>,
    entities: &crate::sim::entity_store::EntityStore,
    interner: Option<&crate::sim::intern::StringInterner>,
    asset_manager: &'a AssetManager,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
) -> HashMap<String, Option<ShpSource<'a>>> {
    let categories: HashMap<&str, EntityCategory> = entities
        .values()
        .filter(|entity| !entity.is_voxel())
        .map(|entity| {
            (
                interner.map_or("", |i| i.resolve(entity.type_ref())),
                entity.category(),
            )
        })
        .collect();
    let mut sources = HashMap::new();
    let mut frame_sets: HashMap<&str, Vec<u16>> = HashMap::new();
    for (type_str, color_idx) in new_objects {
        let category = categories
            .get(type_str.as_str())
            .copied()
            .unwrap_or_else(|| {
                if rules
                    .and_then(|rules| rules.object(type_str))
                    .is_some_and(|object| {
                        object.category == crate::rules::object_type::ObjectCategory::Infantry
                    })
                {
                    EntityCategory::Infantry
                } else {
                    EntityCategory::Structure
                }
            });
        let infantry_frames = if category == EntityCategory::Infantry {
            let frames = frame_sets.entry(type_str.as_str()).or_insert_with(|| {
                let source = load_shp_source(
                    asset_manager,
                    type_str,
                    None,
                    theater_ext,
                    theater_name,
                    rules,
                    art,
                );
                let actions = rules
                    .and_then(|rules| rules.animation_sequence(type_str))
                    .and_then(|set| set.infantry_actions());
                let frames = match (source.as_ref(), actions) {
                    (Some(source), Some(actions)) => {
                        // ShpFile's parser reads the u16 header count. Keep
                        // admission unavailable if that invariant is absent.
                        u16::try_from(source.shp.frames.len())
                            .map(|count| infantry_asset_frames(actions, count))
                            .unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
                sources.insert(type_str.clone(), source);
                frames
            });
            Some(frames.as_slice())
        } else {
            None
        };
        let body_keys = insert_object_keys(
            needed,
            type_str,
            category,
            *color_idx,
            rules,
            infantry_frames,
        );

        if matches!(
            category,
            EntityCategory::Structure | EntityCategory::Unit | EntityCategory::Infantry
        ) {
            // Resolve the actual theater/rules-selected object source once and
            // retain it for rendering. Its physical frame count establishes
            // the embedded shadow half; Type.Image text alone does not.
            let source = sources.entry(type_str.clone()).or_insert_with(|| {
                load_shp_source(
                    asset_manager,
                    type_str,
                    None,
                    theater_ext,
                    theater_name,
                    rules,
                    art,
                )
            });
            if let Some(raw_count) = source
                .as_ref()
                .and_then(|source| u16::try_from(source.shp.frames.len()).ok())
            {
                let shadow_start = raw_count / 2;
                // Techno7064C2..7064CD adds half the physical SHP count
                // to the selected body frame. Infantry51958A calls this same
                // Techno draw; retain its companions too, including nonempty
                // mod masks (many retail infantry shadows are empty).
                needed.extend(
                    body_keys
                        .into_iter()
                        .filter(|key| key.frame < shadow_start)
                        .map(|body_key| ShpSpriteKey {
                            frame: body_key.frame + shadow_start,
                            ..body_key
                        }),
                );
            }
        }
    }
    sources
}

/// Groups keys by type, so each type's SHP is decoded whole once, in a stable order.
fn sprite_key_order(key: &ShpSpriteKey) -> (&str, ShpPaletteContext, u8, u16, u8) {
    (
        key.type_id.as_str(),
        key.palette_context,
        key.house_color.0,
        key.frame,
        key.facing,
    )
}

/// ObjectType LoadImage5F9070 uses BulletType's already resolved Image25,
/// exact ART metadata and one second-character G retry. It does not follow an
/// AnimType Image redirect or try both theater and SHP extensions.
fn projectile_shp_candidates(
    projectile: &ProjectileType,
    theater_ext: &str,
    theater_name: &str,
) -> Vec<String> {
    let Some(load) = &projectile.image_load else {
        return Vec::new();
    };
    let theater = load.theater;
    let upper = load.image.to_ascii_uppercase();
    let [substituted, generic] = crate::rules::art_data::theater_shp_names(&upper, theater_name);
    let first = if !theater && load.new_theater {
        substituted
    } else {
        upper
    };
    let ext = if theater { theater_ext } else { "SHP" };
    let mut candidates = vec![format!("{first}.{ext}")];
    let fallback = format!("{generic}.{ext}");
    if fallback != candidates[0] {
        candidates.push(fallback);
    }
    candidates
}

/// Every frame of every drawable BulletType; a `FirersPalette=` type in each
/// House colour of the match, the schemes its draw can select.
fn register_projectile_frames(
    needed: &mut HashSet<ShpSpriteKey>,
    files: &mut HashMap<String, (String, u16)>,
    assets: &AssetManager,
    rules: Option<&RuleSet>,
    house_colors: &HouseColorMap,
    theater_ext: &str,
    theater_name: &str,
) {
    let Some(rules) = rules else {
        return;
    };
    for weapon in rules.weapons_iter() {
        let Some(projectile) = weapon
            .projectile
            .as_deref()
            .and_then(|id| rules.projectile(id))
        else {
            continue;
        };
        if projectile.inviso || projectile.voxel {
            continue;
        }
        let id = projectile.id.to_ascii_uppercase();
        let (_, count) = files.entry(id.clone()).or_insert_with(|| {
            let candidates = projectile_shp_candidates(projectile, theater_ext, theater_name);
            find_shp(assets, &candidates)
                .and_then(|(file, bytes)| {
                    ShpFile::frame_count_from_bytes(bytes)
                        .ok()
                        .map(|count| (file.to_string(), count))
                })
                .unwrap_or_default()
        });
        for frame in 0..*count {
            if projectile.firers_palette && !projectile.anim_palette {
                for &color in house_colors.values() {
                    needed.insert(projectile_key(&id, projectile, frame, color));
                }
            } else {
                needed.insert(projectile_key(&id, projectile, frame, HouseColorIndex(0)));
            }
        }
    }
}

/// Build or refresh the SHP sprite atlas for every object in the ECS world.
///
/// Without an `existing` atlas every key the world can draw is rendered and
/// shelf-packed into new pages. With one, only the object (type, house
/// colour) pairs and AnimClass colour remaps it has not collected are
/// expanded to keys; those it neither holds nor has found unrenderable are
/// rendered and appended to a growth page. Resident index pages are not
/// repacked; their bindings follow shared palette-texture growth. The world
/// effects registered at map load are not read again.
///
/// 1. Collects the keys of new object pairs (bodies, infantry sequences,
///    building and make animations) and new remaps, plus at map load the
///    rules' world effects and the parachute.
/// 2. Renders the keys not yet resolved, decoding each type's SHP whole once.
/// 3. Packs them into new pages at map load, onto growth pages afterwards.
///    Each upload band decodes its sprites' frames again, so the build never
///    holds every sprite's pixels at once.
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
    anim_remap_keys: &HashSet<AnimRemapKey>,
    cell_drawer_type_ids: &HashSet<String>,
    cell_palette: Option<&Palette>,
    existing: Option<SpriteAtlas>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> Option<SpriteAtlas> {
    let started = Instant::now();
    let previous_atlas = existing;

    // Step 1: object keys, for the (type, house colour) pairs the atlas has
    // not collected — every pair at map load. Structures get facing 0 since
    // buildings don't rotate.
    let collected = |type_id: &str, color| {
        previous_atlas
            .as_ref()
            .is_some_and(|atlas| atlas.covers_object(type_id, color))
    };
    let new_objects: HashSet<(String, HouseColorIndex)> =
        object_pairs(entities, house_colors, extra_building_types, interner)
            .filter(|&(type_id, color)| !collected(type_id, color))
            .map(|(type_id, color)| (type_id.to_string(), color))
            .collect();
    let new_remaps: HashSet<AnimRemapKey> = anim_remap_keys
        .iter()
        .filter(|remap| {
            !previous_atlas
                .as_ref()
                .is_some_and(|atlas| atlas.covered_anim_remaps.contains(*remap))
        })
        .cloned()
        .collect();
    let mut needed: HashSet<ShpSpriteKey> = HashSet::new();
    let mut object_sources = insert_new_object_keys(
        &mut needed,
        &new_objects,
        entities,
        interner,
        asset_manager,
        theater_ext,
        theater_name,
        rules,
        art,
    );

    // Frame counts and source files registered so far: a refresh extends the
    // atlas's own and replaces them only once it has succeeded.
    let (mut active_anim_frame_counts, mut make_frame_counts, mut shp_files) = previous_atlas
        .as_ref()
        .map(|atlas| {
            (
                atlas.active_anim_frame_counts.clone(),
                atlas.make_frame_counts.clone(),
                atlas.shp_files.clone(),
            )
        })
        .unwrap_or_default();
    let mut object_shp_frame_counts = previous_atlas
        .as_ref()
        .map(|atlas| atlas.object_shp_frame_counts.clone())
        .unwrap_or_default();
    for (type_id, source) in &object_sources {
        if let Some(source) = source
            && let Ok(frame_count) = u16::try_from(source.shp.frames.len())
        {
            let key = type_id.to_ascii_uppercase();
            object_shp_frame_counts.insert(key.clone(), frame_count);
            shp_files.insert(key, source.found_name.clone());
        }
    }
    let mut projectile_files = previous_atlas
        .as_ref()
        .map(|atlas| atlas.projectile_files.clone())
        .unwrap_or_default();
    register_projectile_frames(
        &mut needed,
        &mut projectile_files,
        asset_manager,
        rules,
        house_colors,
        theater_ext,
        theater_name,
    );

    // Step 1b: Collect every declared building animation frame required by art
    // metadata. Runtime owns timing; atlas construction only ensures that a
    // live `BuildingAnimOverlays` frame is drawable.
    //
    // The keys follow this build's own count, never the atlas's: a name that
    // is also a world effect (e.g. GAPOWR_A) holds the effect count there,
    // shadow half included. Map load counts building animations before the
    // effects, so a building on the map at load gets the same frames as one
    // that appears later, and the effect keeps the presentation count.
    let mut building_anim_counts: HashMap<String, (u16, u16)> = HashMap::new();
    if let Some(art_reg) = art {
        for (type_id, house_color) in &new_objects {
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
                        &mut building_anim_counts,
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
    for (anim_type, (count, _)) in building_anim_counts {
        active_anim_frame_counts.entry(anim_type).or_insert(count);
    }

    // Step 1c: every frame of each building's make (build-up) SHP, a separate
    // file (e.g., GTCNSTMK.SHP) showing the building assembling.
    if let Some(art_reg) = art {
        for (type_id, color) in &new_objects {
            // Skip anim overlay types (they don't have make SHPs).
            if type_id.contains('_') {
                continue;
            }
            let make_key: String = format!("{}_MAKE", type_id);
            let count = match make_frame_counts.get(&make_key) {
                Some(&count) => count,
                None => {
                    let candidates = buildup_shp_candidates(art_reg, rules, type_id, theater_name);
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
                    object_shp_frame_counts.insert(make_key.to_ascii_uppercase(), raw_count);
                    make_frame_counts.insert(make_key.clone(), count);
                    count
                }
            };
            // The shadow half too: DrawSHP's shadow arm (`0x00705E00`) draws
            // frame + count/2 of the Buildup like any body image.
            let raw_count = object_shp_frame_counts
                .get(&make_key.to_ascii_uppercase())
                .copied()
                .unwrap_or(count);
            for frame in 0..raw_count {
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

    // Step 1c': a war factory's unload art (`ArtEntry::factory_door`), every
    // frame of each SHP, shadows included. `BuildingClass::DrawIt @
    // 0x0043D290` draws it through the building's own DrawSHP, like the bib.
    if let Some(art_reg) = art {
        for (type_id, color) in &new_objects {
            if type_id.contains('_') {
                continue;
            }
            let rules_image: String = rules
                .and_then(|r| r.object(type_id))
                .map(|o| o.image.clone())
                .unwrap_or_else(|| type_id.clone());
            let Some(entry) = art_reg.resolve_metadata_entry(type_id, &rules_image) else {
                continue;
            };
            let palette_context = if entry.terrain_palette {
                ShpPaletteContext::Cell
            } else {
                ShpPaletteContext::SelectedScheme
            };
            for name in entry.factory_door.names() {
                let upper = name.to_ascii_uppercase();
                let count = match object_shp_frame_counts.get(&upper) {
                    Some(&count) => count,
                    None => {
                        let candidates = art_data::theater_shp_names(&upper, theater_name)
                            .map(|name| format!("{name}.SHP"));
                        let Some((file, data)) = find_shp(asset_manager, &candidates) else {
                            continue;
                        };
                        let Ok(count) = ShpFile::frame_count_from_bytes(data) else {
                            continue;
                        };
                        object_shp_frame_counts.insert(upper.clone(), count);
                        shp_files.insert(upper.clone(), file.to_string());
                        count
                    }
                };
                for frame in 0..count {
                    needed.insert(ShpSpriteKey {
                        palette_context,
                        type_id: upper.clone(),
                        facing: 0,
                        frame,
                        house_color: *color,
                    });
                }
            }
        }
    }

    // Step 1d: Pre-load world effect SHPs (warp animations, explosions, etc.)
    // once, at map load. Names come from rules.ini [General]
    // WarpIn=/WarpOut=/WarpAway= — NOT hardcoded. These use anim.pal (effect
    // palette), not unit.pal — tracked in the effect registry so step 2 can
    // pick the correct palette. Only the SHP header is read here.
    let mut registered_effects = EffectRegistry::default();
    let effect_names = match previous_atlas {
        None => rules.map(collect_effect_names).unwrap_or_default(),
        Some(_) => Vec::new(),
    };
    for name in &effect_names {
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
                    // Keep body timing separate from shadow-frame residency.
                    let count = register_effect_anim_frames(
                        &mut needed,
                        &mut active_anim_frame_counts,
                        name,
                        raw_count,
                        scheduler_owned,
                        shadow,
                    );
                    registered_effects
                        .frames
                        .insert(name_upper.clone(), (name.clone(), raw_count));
                    registered_effects.type_ids.insert(name.clone());
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
    // This registers the no-remap GlobalAnim/colour-0 fallback. The live
    // AnimClass remap closure adds SelectedScheme variants for remapped canopies;
    // both use the unit palette here because PARACH sets `AltPalette=`.
    let parachute = rules
        .filter(|_| previous_atlas.is_none())
        .and_then(|r| r.general.parachute_shp.as_deref());
    if let Some(shp_name) = parachute {
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

    // Step 1f: the airborne infantry shadow, the literal `INFSHDW.SHP`
    // (`0x00825524`) InfantryClass::DrawIt loads on first use and draws at
    // frame 1 ([`INFANTRY_AIR_SHADOW_SHP`]).
    if previous_atlas.is_none() {
        let candidates = ["infshdw.shp".to_string(), "INFSHDW.SHP".to_string()];
        match find_shp(asset_manager, &candidates)
            .and_then(|(file, data)| Some((file, ShpFile::frame_count_from_bytes(data).ok()?)))
        {
            Some((file, frame_count)) if frame_count > INFANTRY_AIR_SHADOW_FRAME => {
                needed.insert(ShpSpriteKey {
                    palette_context: ShpPaletteContext::GlobalAnim,
                    type_id: INFANTRY_AIR_SHADOW_SHP.to_string(),
                    facing: 0,
                    frame: INFANTRY_AIR_SHADOW_FRAME,
                    house_color: HouseColorIndex(0),
                });
                shp_files.insert(INFANTRY_AIR_SHADOW_SHP.to_string(), file.to_string());
            }
            _ => log::warn!("INFSHDW.SHP frame 1 missing: airborne infantry draw no shadow"),
        }
    }

    let effects = previous_atlas
        .as_ref()
        .map_or(&registered_effects, |atlas| &atlas.effects);
    insert_anim_remap_frame_keys(&mut needed, effects, &new_remaps);

    let mut contextual = Vec::new();
    for key in &needed {
        if key.palette_context == ShpPaletteContext::Legacy
            && effects.type_ids.contains(&key.type_id)
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

    if needed.is_empty() && previous_atlas.is_none() {
        log::info!("No SHP sprite entities found");
        return None;
    }

    // Step 2: the keys to render — every one at map load, afterwards only those
    // the atlas neither holds nor has found unrenderable. Sorting groups each
    // type's keys so its SHP is decoded whole once.
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
    // etc.) are drawn with anim.pal, not unit.pal. Init_Game 0x0052BA60
    // converts ANIM.PAL and PALETTE.PAL like every palette load.
    let effect_palette: Option<Palette> = asset_manager
        .get_ref("anim.pal")
        .and_then(|d| Palette::from_bytes(d).ok());
    let bullet_palette = asset_manager
        .get_ref("palette.pal")
        .and_then(|data| Palette::from_bytes(data).ok());
    if effect_palette.is_none() && !effects.type_ids.is_empty() {
        log::warn!("anim.pal not found — world effect SHPs will use unit.pal (wrong colors)");
    }

    // BuildingType TerrainPalette (004612B4, DrawSHP 00705EC7) has the same
    // palette owner as cell-drawer animations. Resolve the rules Image alias.
    // Refresh-local decoding cache; the existing atlas owns all resulting
    // rows and retires them with the scenario. No final-ART Palette reread.
    let mut extra_palettes: HashMap<&str, Option<Palette>> = HashMap::new();
    let mut extra_palette_names = HashMap::new();
    let mut cell_palette_type_ids = cell_drawer_type_ids.clone();
    let mut extra_palette_type_ids = HashSet::new();
    let new_types: HashSet<&str> = new_keys.iter().map(|key| key.type_id.as_str()).collect();
    for type_id in new_types {
        let base = type_id.strip_suffix("_MAKE").unwrap_or(type_id);
        let object = rules.and_then(|rules| rules.object(base));
        let image = object.map_or(base, |object| object.image.as_str());
        if art
            .and_then(|art| art.resolve_metadata_entry(base, image))
            .is_some_and(|entry| entry.terrain_palette)
        {
            cell_palette_type_ids.insert(type_id.to_ascii_uppercase());
        }
        if let Some(name) = object.and_then(|object| object.custom_palette_name()) {
            let palette = extra_palettes.entry(name).or_insert_with(|| {
                rules?
                    .load_extra_building_palette(asset_manager, theater_ext, Some(name))
                    .map(|(_, palette)| palette)
            });
            if palette.is_some() {
                extra_palette_type_ids.insert(type_id.to_ascii_uppercase());
                extra_palette_names.insert(type_id.to_ascii_uppercase(), name);
            }
        }
    }
    // DestroyAnim outlives its source. Its own retained group, rather than
    // any currently visible building or singleton, supplies the palette key.
    for key in &new_keys {
        if let ShpPaletteContext::ExtraScheme(palette) = key.palette_context
            && let Some(name) = interner.map(|interner| interner.resolve(palette))
        {
            extra_palettes.entry(name).or_insert_with(|| {
                rules?
                    .load_extra_building_palette(asset_manager, theater_ext, Some(name))
                    .map(|(_, palette)| palette)
            });
        }
    }
    let palette_choices: Vec<SpritePaletteChoice> = new_keys
        .iter()
        .map(|key| {
            let extra_available = match key.palette_context {
                ShpPaletteContext::ExtraScheme(palette) => interner
                    .and_then(|interner| extra_palettes.get(interner.resolve(palette)))
                    .is_some_and(Option::is_some),
                _ => !extra_palette_type_ids.is_empty(),
            };
            sprite_palette_for_key(
                key,
                art,
                &effects.type_ids,
                &cell_palette_type_ids,
                extra_available.then_some(&extra_palette_type_ids),
            )
        })
        .collect();
    if cell_palette.is_none() && palette_choices.contains(&SpritePaletteChoice::CellIso) {
        return abort_sprite_atlas_refresh(
            previous_atlas,
            "terrain-attached animations require the active theater ISO palette".to_string(),
        );
    }
    if bullet_palette.is_none() && palette_choices.contains(&SpritePaletteChoice::Bullet) {
        return abort_sprite_atlas_refresh(
            previous_atlas,
            "visible projectiles require PALETTE.PAL".to_string(),
        );
    }
    if effect_palette.is_none()
        && new_keys
            .iter()
            .any(|key| key.palette_context == ShpPaletteContext::BulletAnim)
    {
        return abort_sprite_atlas_refresh(
            previous_atlas,
            "AnimPalette projectiles require ANIM.PAL".to_string(),
        );
    }

    let mut rendered: Vec<RenderedShpSprite> = Vec::with_capacity(new_keys.len());
    let mut unrenderable: Vec<ShpSpriteKey> = Vec::new();
    // A refresh registers rows in a copy that is installed only with its
    // sprites, so an aborted refresh leaves the atlas as it was.
    let mut palettes = previous_atlas
        .as_ref()
        .map(|atlas| atlas.palettes.clone())
        .unwrap_or_default();
    let mut source: Option<(&str, bool, Option<ShpSource>)> = None;
    for (key, palette_choice) in new_keys.iter().zip(palette_choices) {
        let projectile = key.palette_context.is_projectile();
        if source.as_ref().is_none_or(|(type_id, was_projectile, _)| {
            *type_id != key.type_id || *was_projectile != projectile
        }) {
            let file = if projectile {
                projectile_files
                    .get(&key.type_id)
                    .map(|(file, _)| file.as_str())
            } else {
                shp_files
                    .get(&key.type_id.to_ascii_uppercase())
                    .map(String::as_str)
            };
            // Object key collection already decoded this exact source for
            // Infantry admission or Structure/Unit shadow-frame registration.
            // Reuse it instead of decoding the same file a second time.
            let mut loaded = (!projectile)
                .then(|| object_sources.remove(&key.type_id))
                .flatten()
                .unwrap_or_else(|| {
                    load_shp_source(
                        asset_manager,
                        &key.type_id,
                        file,
                        theater_ext,
                        theater_name,
                        rules,
                        art,
                    )
                });
            if projectile && let Some(loaded) = &mut loaded {
                // BulletDraw468090 has no AnimType/Techno XDrawOffset consumer.
                loaded.draw_offsets = (0, 0);
            }
            source = Some((&key.type_id, projectile, loaded));
        }
        let pal: &Palette = match palette_choice {
            SpritePaletteChoice::Anim => effect_palette.as_ref().unwrap_or(palette),
            SpritePaletteChoice::Unit => palette,
            SpritePaletteChoice::CellIso => {
                cell_palette.expect("cell-drawer palette was validated before atlas rendering")
            }
            SpritePaletteChoice::Bullet => {
                bullet_palette.as_ref().expect("bullet palette validated")
            }
            SpritePaletteChoice::ExtraBuilding => {
                let name = match key.palette_context {
                    ShpPaletteContext::ExtraScheme(palette) => interner
                        .expect("custom animation palette was resolved through the interner")
                        .resolve(palette),
                    _ => extra_palette_names[key.type_id.to_ascii_uppercase().as_str()],
                };
                extra_palettes[name]
                    .as_ref()
                    .expect("chosen only when loaded")
            }
        };
        // 0x00627590 builds each scheme with ColorScheme's constructor
        // (0x0068C710), which writes the colour's ramp over indices 16..31.
        let house_remap = match key.palette_context {
            ShpPaletteContext::Legacy => palette_choice != SpritePaletteChoice::CellIso,
            _ => matches!(
                palette_choice,
                SpritePaletteChoice::Unit | SpritePaletteChoice::ExtraBuilding
            ),
        };
        let sprite = source
            .as_ref()
            .and_then(|(_, _, loaded)| loaded.as_ref())
            .and_then(|loaded| {
                render_shp_frame(loaded, pal, house_remap, key, rules, &mut palettes)
            });
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
        let oregath_sprites: Vec<RenderedShpSprite> =
            render_harvest_overlay_frames(asset_manager, &mut palettes);
        if !oregath_sprites.is_empty() {
            log::info!(
                "Rendered {} oregath.shp harvest overlay frames",
                oregath_sprites.len()
            );
            rendered.extend(oregath_sprites);
        }
    }

    if palettes.colors.len() > device.limits().max_texture_dimension_2d as usize {
        return abort_sprite_atlas_refresh(
            previous_atlas,
            format!(
                "SHP atlas requires {} palette rows; device supports {}",
                palettes.colors.len(),
                device.limits().max_texture_dimension_2d,
            ),
        );
    }

    // Step 3: shelf-pack into new pages at map load; afterwards append to a
    // growth page so resident sprites are never repacked or re-uploaded.
    let added = rendered.len();
    let mut atlas = match previous_atlas {
        Some(mut atlas) => {
            atlas.append_sprites(device, queue, batch, rendered, palettes);
            atlas
        }
        None if rendered.is_empty() => {
            log::warn!("No SHP sprites rendered");
            return None;
        }
        None => {
            let mut atlas = pack_sprites(device, queue, batch, rendered, palettes);
            atlas.effects = registered_effects;
            atlas
        }
    };
    atlas.unrenderable.extend(unrenderable);
    atlas.make_frame_counts = make_frame_counts;
    atlas.active_anim_frame_counts = active_anim_frame_counts;
    atlas.object_shp_frame_counts = object_shp_frame_counts;
    atlas.shp_files = shp_files;
    atlas.projectile_files = projectile_files;
    for (type_id, color) in new_objects {
        atlas
            .covered_objects
            .entry(type_id)
            .or_default()
            .insert(color);
    }
    atlas.covered_anim_remaps.extend(new_remaps);
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
struct ShpSource<'a> {
    shp: ShpFile,
    /// The file `shp` was decoded from, which its sprites decode frames from.
    data: &'a [u8],
    found_name: String,
    /// DrawOffset from art.ini XDrawOffset/YDrawOffset for per-type fine-tuning.
    draw_offsets: (i32, i32),
}

impl<'a> ShpSource<'a> {
    /// Decode `data` whole, keeping the bytes its sprites decode frames from.
    fn decode(
        data: &'a [u8],
        found_name: String,
        draw_offsets: (i32, i32),
    ) -> Result<Self, crate::assets::error::AssetError> {
        Ok(Self {
            shp: ShpFile::from_bytes(data)?,
            data,
            found_name,
            draw_offsets,
        })
    }
}

/// Resolve and decode the SHP a type's keys are drawn from.
///
/// `file` is the file registration counted the type's frames in (animation,
/// make and parachute SHPs), so frames come from the file they were counted
/// in. Building bodies resolve through
/// [`art_data::building_body_shp_candidates`]; other bodies keep the object
/// naming path through rules/art `Image=`.
fn load_shp_source<'a>(
    asset_manager: &'a AssetManager,
    type_id: &str,
    file: Option<&str>,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
) -> Option<ShpSource<'a>> {
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
            let object = rules.and_then(|r| r.object(base_type_id));
            let rules_image: String = object
                .map(|o| o.image.clone())
                .unwrap_or_else(|| base_type_id.to_string());
            let image: String = art
                .map(|a| a.resolve_effective_image_id(base_type_id, &rules_image))
                .unwrap_or_else(|| rules_image.to_uppercase());
            if is_make {
                art.map(|art| buildup_shp_candidates(art, rules, base_type_id, theater_name))
                    .unwrap_or_default()
            } else if object.is_some_and(|object| {
                object.category == crate::rules::object_type::ObjectCategory::Building
            }) {
                art_data::building_body_shp_candidates(
                    art,
                    &rules_image,
                    &image,
                    theater_ext,
                    theater_name,
                )
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
    let draw_offsets = art.map(|a| a.draw_offsets(type_id)).unwrap_or((0, 0));
    let source = match ShpSource::decode(shp_data, found_name.to_string(), draw_offsets) {
        Ok(source) => source,
        Err(e) => {
            log::warn!("Failed to parse {}: {}", found_name, e);
            return None;
        }
    };
    let shp = &source.shp;

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
    Some(source)
}

/// Load and render a single SHP sprite, with the RGBA its indices resolve to.
#[cfg(test)]
fn render_shp_sprite<'a>(
    asset_manager: &'a AssetManager,
    palette: &Palette,
    house_remap: bool,
    key: &ShpSpriteKey,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
) -> Option<(RenderedShpSprite<'a>, Vec<u8>)> {
    let source = load_shp_source(
        asset_manager,
        &key.type_id,
        None,
        theater_ext,
        theater_name,
        rules,
        art,
    )?;
    let mut palettes = ShpPalettes::default();
    let sprite = render_shp_frame(&source, palette, house_remap, key, rules, &mut palettes)?;
    let rgba = palettes.rgba(&sprite);
    Some((sprite, rgba))
}

/// Render one key's frame of a decoded SHP: its palette indices, and the row
/// of `palettes` holding the palette they resolve through.
///
/// Selects the appropriate frame based on facing (8-direction for infantry).
/// An empty (0x0) frame has nothing to draw and returns None, as does a frame
/// whose palette the full rows cannot take.
fn render_shp_frame<'a>(
    source: &ShpSource<'a>,
    palette: &Palette,
    house_remap: bool,
    key: &ShpSpriteKey,
    rules: Option<&RuleSet>,
    palettes: &mut ShpPalettes,
) -> Option<RenderedShpSprite<'a>> {
    let shp = &source.shp;
    let found_name = source.found_name.as_str();

    // Source-bound Infantry preparation never registers an invalid frame.
    // Reject an out-of-asset key here too; this adapter's absent sprite does
    // not establish native invalid-index shape access, which the frame
    // selection receipts stop before. Other generic classes retain their
    // existing standing fallback.
    let infantry = rules
        .and_then(|rules| rules.object(&key.type_id))
        .is_some_and(|object| {
            object.category == crate::rules::object_type::ObjectCategory::Infantry
        });
    if (key.palette_context.is_projectile() || infantry) && key.frame as usize >= shp.frames.len() {
        return None;
    }
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

    let Some(palette_row) = palettes.row(render_pal) else {
        log::warn!(
            "{} frame {}: the sprite atlas palette rows are full",
            found_name,
            frame_idx
        );
        return None;
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
        palette: palette_row,
        shp: source.data,
        frame: frame_idx,
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
fn render_harvest_overlay_frames<'a>(
    asset_manager: &'a AssetManager,
    palettes: &mut ShpPalettes,
) -> Vec<RenderedShpSprite<'a>> {
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
    let shp_data: &'a [u8] = match asset_manager.get_ref("oregath.shp") {
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

    let Some(palette_row) = palettes.row(&palette) else {
        log::warn!("Sprite atlas palette rows are full — skipping harvest overlay");
        return Vec::new();
    };
    let hc: HouseColorIndex = HouseColorIndex::default();
    let mut sprites: Vec<RenderedShpSprite> = Vec::with_capacity(real_frames);

    let canvas_w: u32 = shp.width as u32;
    let canvas_h: u32 = shp.height as u32;

    for frame_idx in 0..real_frames {
        let frame = &shp.frames[frame_idx];
        if frame.frame_width == 0 || frame.frame_height == 0 {
            continue;
        }

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
            // The effect palette, with no house color remap.
            palette: palette_row,
            shp: shp_data,
            frame: frame_idx,
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

#[cfg(test)]
pub(crate) use tests::{native_palette_probe_page, native_probe_rgb, palette_probe_page};

/// Index bytes of rows at which a map-load upload band closes. A band holds
/// whole shelves, so it exceeds this by less than its last shelf.
const PACK_BAND_BYTES: u64 = 64 << 20;

/// How long a map-load upload band waits for the GPU to retire its staging copy.
const PACK_BAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Shelf-pack rendered SHP sprites into a multi-page GPU texture atlas.
///
/// Sprites are packed into pages of at most `max_texture_dim × max_texture_dim`
/// pixels each; a page is added when the shelves outgrow the GPU texture limit.
/// Each page holds only palette indices, resolved through `palettes`. It is
/// created blank and filled by [`upload_shelves`], so the pack never holds a
/// whole page of texels or its staging copy, nor the sprites' pixels beyond
/// the band being uploaded.
fn pack_sprites(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    sprites: Vec<RenderedShpSprite<'_>>,
    mut palettes: ShpPalettes,
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
        let trial_height: u32 = simulate_shelf_height(&indices, &sprites, atlas_width);
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
    let palette_rows = palettes.upload(device, queue);

    // Build each page's GPU texture.
    let mut pages: Vec<SpriteAtlasPage> = Vec::with_capacity(num_pages);
    let mut entries: HashMap<ShpSpriteKey, ShpSpriteEntry> =
        HashMap::with_capacity(placements.len());
    for page_idx in 0..num_pages as u8 {
        // Placements run shelf by shelf, top to bottom, as `upload_shelves` needs.
        let placed: Vec<(usize, [u32; 2])> = placements
            .iter()
            .filter(|p| p.page == page_idx)
            .map(|p| (p.idx, [p.px, p.py]))
            .collect();
        let page_height: u32 = placed
            .iter()
            .map(|&(idx, [_, y])| y + sprites[idx].height)
            .max()
            .unwrap_or(1);
        let (texture, source_indices) =
            batch.create_indexed_page(device, atlas_width, page_height, &palette_rows);
        for &(idx, origin) in &placed {
            let sprite = &sprites[idx];
            if let Some(entry) = atlas_entry(
                sprite,
                origin,
                usize::from(page_idx),
                [atlas_width, page_height],
            ) {
                entries.insert(sprite.key.clone(), entry);
            }
        }
        upload_shelves(
            device,
            queue,
            &source_indices,
            &sprites,
            &placed,
            PACK_BAND_BYTES,
        );
        pages.push(SpriteAtlasPage { texture });
    }

    // One palette index a texel.
    let texels: u64 = pages
        .iter()
        .map(|page| u64::from(page.texture.width) * u64::from(page.texture.height))
        .sum();
    let sizes: Vec<String> = pages
        .iter()
        .map(|page| format!("{}x{}", page.texture.width, page.texture.height))
        .collect();
    log::info!(
        "Sprite atlas packed into {} page(s) of at most {max_texture_dim} px: {} ({:.0} MiB), {} palette row(s)",
        pages.len(),
        sizes.join(", "),
        texels as f64 / (1024.0 * 1024.0),
        palettes.colors.len(),
    );
    SpriteAtlas::new(pages, entries, palettes)
}

/// Upload the sprites `placed` on a blank index page (sprite index and origin,
/// shelf by shelf from the top) in bands of whole shelves. A band takes shelves
/// until its rows reach `band_bytes`, so it holds at least one shelf. Bands
/// share no row, as [`atlas_growth::write_band`] requires. A band decodes its
/// sprites' frames as it is gathered, and its staging copy is retired before
/// the next band is gathered, so the upload holds one band of pixels at a time.
fn upload_shelves(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    indices: &wgpu::Texture,
    sprites: &[RenderedShpSprite<'_>],
    placed: &[(usize, [u32; 2])],
    band_bytes: u64,
) {
    debug_assert!(
        placed.windows(2).all(|pair| pair[0].1[1] <= pair[1].1[1]),
        "placements must run shelf by shelf from the top"
    );
    let row_bytes = u64::from(indices.width());
    let mut rest = placed;
    while let Some(&(_, [_, top])) = rest.first() {
        // Whole shelves (runs of one row origin) until the band's rows reach `band_bytes`.
        let (mut len, mut bottom) = (0, top);
        loop {
            let shelf_y = rest[len].1[1];
            while len < rest.len() && rest[len].1[1] == shelf_y {
                let (idx, [_, y]) = rest[len];
                bottom = bottom.max(y + sprites[idx].height);
                len += 1;
            }
            if len == rest.len() || u64::from(bottom - top) * row_bytes >= band_bytes {
                break;
            }
        }
        let (band, tail) = rest.split_at(len);
        write_sprite_band(
            queue,
            indices,
            band.iter().map(|&(idx, origin)| (origin, &sprites[idx])),
        );
        let submission_index = queue.submit(std::iter::empty());
        if let Err(error) = device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index),
            timeout: Some(PACK_BAND_TIMEOUT),
        }) {
            log::warn!("Sprite atlas upload could not wait for the GPU: {error}");
        }
        rest = tail;
    }
}

/// Upload sprites placed on rows of an index page that hold no other sprite,
/// in one [`atlas_growth::write_band`], decoding each frame as it is gathered.
fn write_sprite_band<'s, 'a: 's>(
    queue: &wgpu::Queue,
    indices: &wgpu::Texture,
    sprites: impl Iterator<Item = ([u32; 2], &'s RenderedShpSprite<'a>)>,
) {
    let sprites: Vec<_> = sprites.collect();
    let rects: Vec<_> = sprites
        .iter()
        .map(|&(origin, sprite)| (origin, [sprite.width, sprite.height]))
        .collect();
    atlas_growth::write_band(queue, indices, 1, &rects, |i| sprites[i].1.indices());
}

/// Simulate shelf-packing to determine total height without allocating buffers.
fn simulate_shelf_height(
    indices: &[usize],
    sprites: &[RenderedShpSprite<'_>],
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
#[path = "sprite_atlas_projectile_tests.rs"]
mod projectile_tests;

#[cfg(test)]
impl SpriteAtlas {
    pub(crate) fn from_test_pages(pages: Vec<SpriteAtlasPage>) -> Self {
        Self::new(pages, HashMap::new(), ShpPalettes::default())
    }

    /// Metadata-only atlas for production emitter regressions. GPU tests
    /// still provide actual pages through the existing device-backed owner.
    pub(crate) fn from_test_entries(entries: HashMap<ShpSpriteKey, ShpSpriteEntry>) -> Self {
        Self::new(Vec::new(), entries, ShpPalettes::default())
    }

    /// Record `type_id`'s physical SHP frame count, as an atlas build does.
    pub(crate) fn set_object_shp_frame_count_for_test(&mut self, type_id: &str, count: u16) {
        self.object_shp_frame_counts
            .insert(type_id.to_ascii_uppercase(), count);
    }

    /// A one-page atlas whose page holds `indices` resolved through
    /// `palette`, with the `source_palette` its instances carry.
    pub(crate) fn from_test_indexed_page(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        size: [u32; 2],
        indices: &[u8],
        palette: &Palette,
    ) -> (Self, u32) {
        let mut palettes = ShpPalettes::default();
        let row = palettes.row(palette).expect("an empty atlas has free rows");
        let palette_rows = palettes.upload(device, queue);
        let (texture, page) = batch.create_indexed_page(device, size[0], size[1], &palette_rows);
        atlas_growth::write_texels(queue, &page, [0, 0], size, 1, indices);
        let atlas = Self::new(vec![SpriteAtlasPage { texture }], HashMap::new(), palettes);
        (atlas, row + 1)
    }
}
