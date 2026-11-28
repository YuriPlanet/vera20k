//! SHP sprite atlas — pre-renders SHP sprites into packed GPU texture pages.
//!
//! At map load every SHP sprite the world can draw — infantry sequences,
//! building bodies with their make and overlay animations, world effects — is
//! rendered to palette indices and shelf-packed into as few pages as the GPU
//! texture limit allows. Each sprite resolves its indices through one of the
//! atlas's palette rows (`ShpPalettes`): its render palette, house ramp
//! included. Logical keys of the same immutable source/frame share its index
//! rectangle while keeping their own palette and draw metadata. When a new
//! (type, house colour) appears mid-match, missing physical frames append to a
//! growth page (`atlas_growth`); resident rectangles are reused without moving
//! or re-uploading them. Keys with nothing to draw are remembered, so no
//! refresh retries them.
//!
//! ## Frame selection
//! - Buildings (Structure): always frame 0 (no rotation).
//! - Infantry: `(facing / 32) % num_facings` for 8-direction standing pose.
//! - Facing is collapsed to 0 for structures in the cache key (dedup).
//!
//! ## Dependency rules
//! - Part of render/ — depends on assets/ (SHP/Palette), render/batch (GPU upload).
//! - Reads from sim/ via EntityStore iteration (GameEntity fields).

use crate::util::time::Instant;
use std::collections::{HashMap, HashSet};

use crate::assets::asset_manager::AssetManager;
use crate::assets::mix_archive::SharedBytes;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::render::atlas_growth::{self, GrowthShelf, SPRITE_PADDING};
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::picture_atlas::{PictureSource, PictureSources};
use crate::rules::art_data::{self, ArtRegistry};
use crate::rules::effect_asset_catalog::available_effect_anim_frame_count;
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};
use crate::rules::projectile_type::ProjectileType;
use crate::rules::ruleset::RuleSet;
use smol_str::StrExt;

#[path = "sprite_atlas_loading.rs"]
mod loading;
pub(crate) use loading::SpriteAtlasLoadingJob;

/// Edge of a growth page: 16 MiB of newly needed physical frame indices.
/// New palette/context aliases reuse their existing rectangles.
const GROWTH_PAGE_SIZE: u32 = 4096;
const INFANTRY_FACING_STEP: u8 = crate::util::direction::FACING_UNITS_PER_DIRECTION;
const INFANTRY_FACING_BUCKETS: u8 = 8;
/// The shadow InfantryClass::DrawIt draws under an airborne infantry: the
/// `INFSHDW.SHP` literal at `0x00825524`, cached at `0x00A8F244`, frame 1
/// (`0x0051957C`).
const INFANTRY_AIR_SHADOW_SHP: &str = "INFSHDW";
const INFANTRY_AIR_SHADOW_FRAME: u16 = 1;

/// The stock gather overlay `UnitClass::DrawIt @ 0x0073CEC0` draws: the
/// `FileSystem::OREGATH_SHP` literal, drawn with `FileSystem::ANIM_PAL`.
/// Custom gather animations use their separate AnimType draw context.
pub(crate) const HARVEST_OVERLAY_IMAGE: &str = "OREGATH";

fn register_effect_anim_frames(
    needed: &mut HashSet<ShpSpriteKey>,
    frame_counts: &mut HashMap<String, u16>,
    anim_type: &str,
    raw_count: u16,
    scheduler_owned: bool,
    shadow: bool,
    palette_context: ShpPaletteContext,
) -> u16 {
    let count = available_effect_anim_frame_count(raw_count, scheduler_owned, shadow);
    // DrawIt selects the shadow from the physical second half of the SHP.
    // Residency includes that companion without extending animation timing.
    for frame in 0..raw_count {
        needed.insert(ShpSpriteKey {
            palette_context,
            type_id: anim_type.into(),
            facing: 0,
            frame,
            house_color: HouseColorIndex(0),
        });
    }
    if palette_context != ShpPaletteContext::Particle {
        frame_counts.insert(anim_type.to_ascii_uppercase(), count);
    }
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
        |registry| registry.resolve_anim_image_id(anim_type),
    );
    art_data::effect_shp_candidates(
        crate::rules::art_data::EffectImageKind::Animation,
        art,
        anim_type,
        &image_id,
        theater_ext,
        theater_name,
    )
}

/// First existing file among `candidates`, with its bytes.
/// The files a building type's Buildup SHP loads from: its art `Buildup=`
/// through the theater lookup (`rules::buildup_asset_catalog`), none without
/// one.
pub(crate) fn buildup_shp_candidates(
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

/// Collect make-SHP frame requests for this atlas refresh. The BState-0 gate
/// branch in `BuildingClass::GetCurrentFrame @0x0043EF90` selects the frame;
/// `_MAKE` is the atlas key for the registered BuildingType's ART `Buildup=`.
fn collect_building_make_asset_requests(
    needed: &mut HashSet<ShpSpriteKey>,
    make_frame_counts: &mut HashMap<String, u16>,
    object_shp_frame_counts: &mut HashMap<String, u16>,
    shp_files: &mut ShpFiles,
    new_objects: &HashSet<(String, HouseColorIndex)>,
    asset_manager: &AssetManager,
    rules: Option<&RuleSet>,
    art: &ArtRegistry,
    theater_name: &str,
    mut pictures: Option<&mut PictureRoutes<'_>>,
) {
    for (type_id, color) in new_objects {
        // Only registered BuildingTypes have a `BuildingClass` make sequence;
        // many real building IDs contain `_` (for example GAGATE_A).
        if !rules
            .and_then(|rules| rules.object(type_id))
            .is_some_and(|object| {
                object.category == crate::rules::object_type::ObjectCategory::Building
            })
        {
            continue;
        }
        let make_key: String = format!("{}_MAKE", type_id);
        let count = match make_frame_counts.get(&make_key) {
            Some(&count) => count,
            None => {
                // VERA-only, experimental (docs/picture-model-art.md): beside
                // a declared Picture=, Buildup= names a picture whose frames
                // are counted from its header; the SHP is never opened.
                let picture = pictures.as_deref_mut().and_then(|routes| {
                    let source = routes.buildup(type_id)?;
                    Some(routes.frame_count(&source))
                });
                let (raw_count, file) = match picture {
                    Some(Some(raw_count)) => (raw_count, None),
                    Some(None) => continue,
                    None => {
                        let candidates = buildup_shp_candidates(art, rules, type_id, theater_name);
                        let Some((file, data)) = find_shp(asset_manager, &candidates) else {
                            continue;
                        };
                        let Ok(raw_count) = ShpFile::frame_count_from_bytes(&data) else {
                            continue;
                        };
                        (raw_count, Some(file.to_string()))
                    }
                };
                // RA2 make SHPs have shadow frames in the second half — only use the first half.
                let count = if raw_count / 2 > 0 {
                    raw_count / 2
                } else {
                    raw_count
                };
                if let Some(file) = file {
                    log::debug!("Make SHP for {type_id}: {count} frames from {file}");
                    shp_files.insert((ShpImageKind::Object, make_key.to_ascii_uppercase()), file);
                }
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
                type_id: make_key.as_str().into(),
                facing: 0,
                frame,
                house_color: *color,
            });
        }
    }
}

fn find_shp<'n>(
    asset_manager: &AssetManager,
    candidates: &'n [String],
) -> Option<(&'n str, SharedBytes)> {
    candidates.iter().find_map(|name| {
        asset_manager
            .get_shared(name)
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
    let candidates: Vec<String> = art_data::effect_shp_candidates(
        crate::rules::art_data::EffectImageKind::Animation,
        Some(art_reg),
        anim_type,
        &anim_image,
        theater_ext,
        theater_name,
    );
    let (file, data) = find_shp(asset_manager, &candidates)?;
    let raw_count = ShpFile::frame_count_from_bytes(&data).ok()?;
    let count = building_anim_frame_count(raw_count, loop_end);
    Some((count, raw_count, file.to_string()))
}

/// A building animation's usable frame count of `raw_count` physical frames:
/// the first half when it covers `loop_end`, else `loop_end` when the image
/// has that many, else every frame.
fn building_anim_frame_count(raw_count: u16, loop_end: u16) -> u16 {
    let real: u16 = raw_count / 2;
    let required_count = loop_end.max(1);
    if real > 0 && real >= required_count {
        real
    } else if raw_count >= required_count {
        required_count
    } else {
        raw_count
    }
}

/// Register every frame of one building animation (ActiveAnim, IdleAnim and
/// their damaged or garrisoned variants) for one house colour. `scanned`
/// holds the counts this build has read, so each animation's SHP header is
/// read once per build.
fn register_building_anim(
    needed: &mut HashSet<ShpSpriteKey>,
    scanned: &mut HashMap<String, (u16, u16)>,
    shp_files: &mut ShpFiles,
    asset_manager: &AssetManager,
    art: &ArtRegistry,
    anim_type: &str,
    loop_end: u16,
    house_color: HouseColorIndex,
    theater_ext: &str,
    theater_name: &str,
    pictures: Option<&mut PictureRoutes<'_>>,
) {
    let upper = anim_type.to_ascii_uppercase();
    if !scanned.contains_key(&upper) {
        // VERA-only, experimental (docs/picture-model-art.md): a declared
        // picture's frames are counted from its header; its SHP is never
        // opened.
        match pictures.and_then(|routes| routes.anim_frames(anim_type)) {
            Some(Some(raw_count)) => {
                let count = building_anim_frame_count(raw_count, loop_end);
                scanned.insert(upper.clone(), (count, raw_count));
            }
            Some(None) => {}
            None => {
                if let Some((count, raw_count, file)) = scan_building_anim_frame_count(
                    asset_manager,
                    art,
                    anim_type,
                    loop_end,
                    theater_ext,
                    theater_name,
                ) {
                    log::debug!("Building anim {anim_type}: {count} frames from {file}");
                    scanned.insert(upper.clone(), (count, raw_count));
                    shp_files.insert((ShpImageKind::Animation, upper.clone()), file);
                }
            }
        }
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
        anim_frame_context(config, attached_anim_palette_context(config)),
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
            type_id: anim_type.into(),
            facing: 0,
            frame,
            house_color: match palette_context {
                ShpPaletteContext::GlobalAnim | ShpPaletteContext::AnimPalette => {
                    HouseColorIndex(0)
                }
                _ => house_color,
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
    /// Phobos UnitClass_DrawIt_OreGath73D223 selects the AnimType's Shape and
    /// global ANIM Convert directly, without AnimType X/YDrawOffset. Its
    /// caller's placement therefore cannot share an ordinary AnimClass key.
    Harvest,
    /// ParticleRead644F6D owns a different ObjectType shape from a same-named
    /// AnimType. DrawIt62D015 selects the global ANIM Convert87F6C0.
    Particle,
    /// FoggedObject4D2176 selects the literal ANIM.PAL Convert without
    /// AnimClass's AltPalette override or a producer-specific palette, and
    /// so does AnimClass's Tiled column ([`tiled_anim_key`]).
    AnimPalette,
    SelectedScheme,
    Cell,
    /// AnimClass's selected scheme and cell drawer retain AnimType image
    /// identity independently of a same-named Techno's palette and Shape.
    AnimScheme,
    AnimCell,
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
        self.image_kind() == ShpImageKind::Projectile
    }

    fn image_kind(self) -> ShpImageKind {
        match self {
            Self::GlobalAnim
            | Self::Harvest
            | Self::AnimPalette
            | Self::AnimScheme
            | Self::AnimCell
            | Self::ExtraScheme(_) => ShpImageKind::Animation,
            Self::Particle => ShpImageKind::Particle,
            Self::BulletDefault | Self::BulletAnim | Self::BulletFirer => ShpImageKind::Projectile,
            Self::Legacy | Self::SelectedScheme | Self::Cell => ShpImageKind::Object,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ShpImageKind {
    Object,
    Animation,
    Particle,
    Projectile,
}

type ShpFiles = HashMap<(ShpImageKind, String), String>;

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
        type_id: type_id.to_ascii_uppercase_smolstr(),
        facing: 0,
        frame,
        house_color: if palette_context == ShpPaletteContext::BulletFirer {
            house_color
        } else {
            HouseColorIndex(0)
        },
    }
}

/// The key one tile of AnimClass DrawIt's Tiled column draws: the literal
/// ANIM.PAL Convert (`[0x0087F6C0]`, `0x004236F0`) and no remap
/// (`0x004236DC`), whatever Convert the instance selected.
pub(crate) fn tiled_anim_key(type_id: &str, frame: u16) -> ShpSpriteKey {
    ShpSpriteKey {
        palette_context: ShpPaletteContext::AnimPalette,
        type_id: type_id.into(),
        facing: 0,
        frame,
        house_color: HouseColorIndex(0),
    }
}

/// The palette an AnimType's frames are registered in: a Tiled type's body
/// draws only through [`tiled_anim_key`], any other through `selected`.
fn anim_frame_context(
    config: Option<&art_data::AnimTypeRuntimeConfig>,
    selected: ShpPaletteContext,
) -> ShpPaletteContext {
    if config.is_some_and(|config| config.tiled) {
        ShpPaletteContext::AnimPalette
    } else {
        selected
    }
}

pub(crate) fn attached_anim_palette_context(
    config: Option<&art_data::AnimTypeRuntimeConfig>,
) -> ShpPaletteContext {
    if config.is_none_or(|c| c.should_use_cell_drawer) {
        ShpPaletteContext::AnimScheme
    } else {
        ShpPaletteContext::GlobalAnim
    }
}

/// Frozen DrawAll4D2176 keeps the captured owner's ordinary ColorScheme
/// only for ShouldUseCellDrawer. The other arm ignores AltPalette.
pub(crate) fn frozen_anim_palette_context(
    config: Option<&art_data::AnimTypeRuntimeConfig>,
) -> ShpPaletteContext {
    if config.is_none_or(|config| config.should_use_cell_drawer) {
        ShpPaletteContext::AnimScheme
    } else {
        ShpPaletteContext::AnimPalette
    }
}

pub(crate) fn building_art_palette_context(art: &art_data::ArtEntry) -> ShpPaletteContext {
    if art.terrain_palette {
        ShpPaletteContext::Cell
    } else {
        ShpPaletteContext::SelectedScheme
    }
}

/// Cache key: unique combination of object type, facing, and house color.
/// For structures, facing is always 0 (buildings don't rotate).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShpSpriteKey {
    pub palette_context: ShpPaletteContext,
    /// Object type ID from rules.ini (e.g., "E1", "GAPOWR"). Every drawn
    /// object builds keys each frame, and a name this short stays inline.
    pub type_id: smol_str::SmolStr,
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
    /// Logical canvas [x, y, width, height] for non-Building depth and sorting.
    /// Building viewport admission uses the type bounds, not this selected canvas.
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

/// VERA-only, experimental: where a picture frame is on its page
/// ([`SpriteAtlas::append_pictures`]).
#[derive(Debug, Clone, Copy)]
struct PictureFramePlace {
    page: u8,
    uv_origin: [f32; 2],
    uv_size: [f32; 2],
    canvas: [u32; 2],
    origin: [u32; 2],
    size: [u32; 2],
}

/// Whether an instance's `source_palette` names a palette row of an indexed
/// page, not the RGBA source mode of a picture or model page
/// ([`crate::render::picture_atlas::PICTURE_HOUSE`]).
pub(crate) fn source_palette_indexed(source_palette: u32) -> bool {
    source_palette != 0 && source_palette & crate::render::picture_atlas::PICTURE_HOUSE == 0
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

    /// The registered rows without the GPU texture, for a refresh to register
    /// more in; [`SpriteAtlas::append_sprites`] installs it beside the
    /// atlas's texture.
    fn registry(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            colors: self.colors.clone(),
            uploaded: self.uploaded,
            texture: None,
        }
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
                // Unorm, unlike RGBA pages: `indexed_color` hands the palette
                // bytes straight to the shared palette conversion.
                format: wgpu::TextureFormat::Rgba8Unorm,
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
    /// The SHP file and frame each entry's indices decoded from, for a reader
    /// that needs them on the CPU ([`Self::frame_indices`]): a handle on the
    /// archive's bytes, not a copy. Kept and dropped with `entries`.
    frame_sources: HashMap<ShpSpriteKey, (SharedBytes, usize)>,
    /// Derived allocation index for these pages, not another asset/pixel cache.
    /// Source contents plus selected decoded frame identify immutable indices;
    /// logical entries retain their own palette, canvas and draw offsets.
    /// Updated after placement/upload and dropped with the atlas on map load.
    physical_frames: HashMap<ShpPhysicalFrame, ShpFramePlacement>,
    /// Building type → number of make (build-up) animation frames.
    /// Keyed by the make image's atlas name, the type id with `_MAKE`
    /// appended (e.g. "GACNST_MAKE").
    pub make_frame_counts: HashMap<String, u16>,
    /// Building/world-animation type → available non-shadow frame count.
    /// Building consumers use this only to ensure their live animation frame is
    /// resident; the AnimClass and projectile-image presentation
    /// (`app/presentation/instances/overlays.rs`) reads the same map.
    pub active_anim_frame_counts: HashMap<String, u16>,
    /// Object type → physical frame count of its resolved object SHP. Kept
    /// separately from presentation counts, which may exclude shadow frames.
    object_shp_frame_counts: HashMap<String, u16>,
    /// BuildingType render bounds (465570), in unzoomed surface pixels.
    /// Derived from immutable scenario image bindings, independent of palette,
    /// health/frame and camera. Growth publishes it with the atlas transaction;
    /// map/theater/rules reload drops the atlas and its bounds together.
    building_type_bounds: HashMap<String, [i32; 4]>,
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
    shp_files: ShpFiles,
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
    /// VERA-only, experimental: the RGBA pages among `pages` that bind no
    /// palette rows: the picture pages (`render::picture_atlas`, each bound
    /// once as an HD page, `render::picture_zoom`), the streamed pictures'
    /// HD pages (`render::picture_stream`) and the model page.
    picture_pages: HashSet<usize>,
    /// VERA-only, experimental (`docs/zoom-one-pass.md`): VERA's HD art
    /// pages of SHP frames ([`Self::bind_hd_art`]); `Some(None)` once a look
    /// found no pack. Their pages are among `pages`.
    hd: Option<Option<crate::render::hd_art::ShpHd>>,
    /// VERA-only, experimental: where each picture frame the pages hold is,
    /// shared by every key drawn from it; None for one that draws nothing or
    /// found no page. A refresh draws only frames missing here.
    picture_frames:
        HashMap<crate::render::picture_atlas::PictureFrameId, Option<PictureFramePlace>>,
    /// VERA-only, experimental (`docs/zoom-one-pass.md`): the picture
    /// frames of art above 1x at their own resolution, streamed onto HD
    /// pages among `pages` while the camera zooms in
    /// ([`Self::stream_pictures`]).
    picture_stream: crate::render::picture_stream::PictureStream,
    /// VERA-only, experimental: the RGBA page among `pages` that declared
    /// models are drawn on each frame (`render::model_draw`), once one is.
    model_page: Option<usize>,
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
            frame_sources: HashMap::new(),
            physical_frames: HashMap::new(),
            make_frame_counts: HashMap::new(),
            active_anim_frame_counts: HashMap::new(),
            object_shp_frame_counts: HashMap::new(),
            building_type_bounds: HashMap::new(),
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
            picture_pages: HashSet::new(),
            hd: None,
            picture_frames: HashMap::new(),
            picture_stream: Default::default(),
            model_page: None,
        }
    }

    /// VERA-only, experimental (`docs/picture-model-art.md`): add the new
    /// frames of `pictures` on picture pages of their own, each frame once at
    /// game pixels with its zoom-out mip level (`render::picture_zoom`), and
    /// its keys, placed as SHP frames, from those frames or the ones the
    /// pages hold. The pages read their owners' colours from the renderer's
    /// house ramps, which `pictures` brings. Frames of art above 1x also
    /// stream at their own resolution while the camera zooms in
    /// ([`Self::stream_pictures`]).
    pub(crate) fn append_pictures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        pictures: crate::render::picture_atlas::PreparedPictures,
    ) {
        let crate::render::picture_atlas::PreparedPictures {
            frames,
            empty,
            keys,
            ramps,
            hd_sources,
        } = pictures;
        let max_side = device.limits().max_texture_dimension_2d;
        for (file, source) in hd_sources {
            self.picture_stream.add_file(file, source, max_side);
        }
        for frame in empty {
            self.picture_frames.insert(frame, None);
        }
        if !ramps.is_empty() {
            batch.write_picture_ramps(queue, &ramps);
        }
        let new: Vec<_> = frames.iter().map(|(id, _)| id.clone()).collect();
        let mut pages =
            crate::render::picture_atlas::pack_picture_pages(frames, max_side).into_iter();
        while let Some(page) = pages.next() {
            let index = self.pages.len();
            let Ok(page_byte) = u8::try_from(index) else {
                log::warn!(
                    "Sprite atlas: no page left for {} picture frames",
                    page.frames.len()
                );
                // Every file left out, this page's and the later ones'.
                let files: std::collections::BTreeSet<&str> = std::iter::once(&page)
                    .chain(pages.as_slice())
                    .flat_map(|page| &page.frames)
                    .map(|(frame, _)| frame.id.file.as_str())
                    .collect();
                for file in files {
                    crate::rules::art_problems::report(
                        file,
                        "Picture=",
                        "the sprite atlas has no page left for it",
                        "it draws nothing",
                    );
                }
                break;
            };
            let texture =
                crate::render::picture_zoom::page_texture(device, queue, batch, &page.page);
            self.pages.push(SpriteAtlasPage { texture });
            self.picture_pages.insert(index);
            // UVs in game pixels over the page's extent in game pixels.
            let [width, height] = page.page.uv_extent();
            for (frame, [x, y]) in page.frames {
                let [w, h] = frame.size.map(|side| side as f32);
                self.picture_stream
                    .add_frame(&frame.id, frame.origin, frame.size);
                self.picture_frames.insert(
                    frame.id,
                    Some(PictureFramePlace {
                        page: page_byte,
                        uv_origin: [x as f32 / width, y as f32 / height],
                        uv_size: [w / width, h / height],
                        canvas: frame.canvas,
                        origin: frame.origin,
                        size: frame.size,
                    }),
                );
            }
            log::info!(
                "Sprite atlas picture page {index} ({}x{} texels, {} a game pixel)",
                page.page.size[0],
                page.page.size[1],
                page.page.scale
            );
        }
        // A new frame no page took (too large, or no page left) draws nothing.
        for id in new {
            self.picture_frames.entry(id).or_insert(None);
        }
        for key in keys {
            let Some(Some(place)) = self.picture_frames.get(&key.frame) else {
                continue;
            };
            let (offset, canvas_rect) = key.placement(place.canvas, place.origin);
            let entry = ShpSpriteEntry {
                uv_origin: place.uv_origin,
                uv_size: place.uv_size,
                pixel_size: place.size.map(|side| side as f32),
                offset_x: offset[0],
                offset_y: offset[1],
                canvas_rect,
                extended: key.extended,
                page: place.page,
                // The blitters' RGBA source mode (`sprite_source.wgsl`),
                // with the key's house colour when it takes one.
                source_palette: key.source_palette(),
            };
            self.picture_stream.add_key(&key.key, &key.frame, &entry);
            self.entries.insert(key.key.clone(), entry);
        }
    }

    /// The palette indices of `key`'s frame as its page holds them (the
    /// stored frame rectangle, `pixel_size` wide), decoded again from its
    /// file; None for a key this atlas did not render.
    pub(crate) fn frame_indices(&self, key: &ShpSpriteKey) -> Option<Vec<u8>> {
        let (shp, frame) = self.frame_sources.get(key)?;
        ShpFile::decode_frame(shp, *frame).ok()
    }

    /// Look up the atlas entry for a given sprite key.
    /// The returned entry includes a `page` field identifying which atlas page
    /// holds this sprite's texture data.
    ///
    /// VERA-only, experimental (`docs/zoom-one-pass.md`): while the camera
    /// zooms in, a picture frame of art above 1x on an HD page
    /// ([`Self::stream_pictures`]) is drawn from there, and one not there
    /// yet is asked for; while [`Self::bind_hd_art`] has VERA's HD art on, a
    /// frame the pack has art for is drawn from its HD page. Either entry
    /// names that page and the frame's place on it, everything else as the
    /// game-pixel entry.
    pub fn get(&self, key: &ShpSpriteKey) -> Option<&ShpSpriteEntry> {
        if let Some(entry) = self.picture_stream.entry(key) {
            return Some(entry);
        }
        if let Some(Some(hd)) = &self.hd
            && let Some(entry) = hd.entry(key)
        {
            return Some(entry);
        }
        self.entries.get(key)
    }

    /// VERA-only, experimental (`docs/zoom-one-pass.md`): whether this
    /// frame's SHP frames are drawn from VERA's HD art pack
    /// (`render::hd_art::ShpHd`): `on` while the battlefield draws zoomed in
    /// with HD art on. On, [`Self::get`] hands out the HD page entry of every
    /// frame the pack has art for, and the frames asked for since the last
    /// call that have not been looked at are read from the pack (a few a
    /// frame) and put on the HD pages. Off, every entry is the 1x one. Called
    /// once per frame before the world's instances are built and drawn
    /// (`render::hd_art::bind_hd_twins`).
    pub(crate) fn bind_hd_art(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        on: bool,
    ) {
        if !on {
            if let Some(Some(hd)) = &mut self.hd {
                hd.set_active(false);
            }
            return;
        }
        let Some(hd) = self
            .hd
            .get_or_insert_with(|| crate::render::hd_art::ShpHd::open(device))
        else {
            return;
        };
        let Some(palette_rows) = self.palettes.texture.as_ref().map(|(_, view)| view.clone())
        else {
            return;
        };
        hd.set_active(true);
        let (entries, sources) = (&self.entries, &self.frame_sources);
        hd.load(
            device,
            queue,
            batch,
            &mut self.pages,
            &palette_rows,
            |key| {
                let entry = *entries.get(key)?;
                let (shp, frame) = sources.get(key)?;
                let indices = ShpFile::decode_frame(shp, *frame).ok()?;
                Some((entry, indices))
            },
        );
    }

    /// VERA-only, experimental (`docs/zoom-one-pass.md`, "picture
    /// streaming"): the frame's picture streaming hook, once a frame before
    /// the world's instances are built (after `begin_model_frame`). With the
    /// world camera's zoom (`camera_zoom`) above 1, the picture frames
    /// [`Self::get`] handed out since the last call are decoded at their own
    /// resolution on worker threads, a bounded number of finished ones go
    /// onto HD pages (new pages among `pages`), and their keys' entries move
    /// there; the least recently drawn HD page is reused once the budget is
    /// full (`render::picture_stream`). At zoom 1 or below every picture
    /// key draws its game-pixel entry.
    pub(crate) fn stream_pictures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        camera_zoom: f32,
    ) {
        self.picture_stream.frame(
            device,
            queue,
            batch,
            &mut self.pages,
            &mut self.picture_pages,
            &self.entries,
            camera_zoom,
        );
    }

    /// [`INFANTRY_AIR_SHADOW_SHP`]'s frame 1, the shadow an airborne infantry
    /// casts.
    pub(crate) fn infantry_air_shadow(&self) -> Option<&ShpSpriteEntry> {
        self.get(&ShpSpriteKey {
            palette_context: ShpPaletteContext::GlobalAnim,
            type_id: INFANTRY_AIR_SHADOW_SHP.into(),
            facing: 0,
            frame: INFANTRY_AIR_SHADOW_FRAME,
            house_color: HouseColorIndex(0),
        })
    }

    /// Physical frame count from the object type's resolved SHP source.
    pub(crate) fn object_shp_frame_count(&self, type_id: &str) -> Option<u16> {
        // Draws ask this for each body and bib shadow: fold the key on the
        // stack.
        crate::util::native_string::with_ascii_uppercase(type_id, |key| {
            self.object_shp_frame_counts.get(key).copied()
        })
    }

    /// Native relative type bounds; `None` means this type has not been prepared.
    /// A known empty image/rectangle remains `Some`, including its origin.
    pub(crate) fn building_type_bounds(&self, type_id: &str) -> Option<[i32; 4]> {
        self.building_type_bounds.get(type_id).copied()
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

    /// Number of logical draw keys across all pages, including frame aliases.
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

    /// VERA-only, experimental (`docs/picture-model-art.md`): the page
    /// [`Self::install_model_page`] holds, if any.
    pub(crate) fn model_page(&self) -> Option<usize> {
        self.model_page
    }

    /// VERA-only, experimental (`docs/picture-model-art.md`): hold `texture`,
    /// a frame's render target of declared models (`render::model_draw`), as
    /// an RGBA page drawn by the SHP blitters' RGBA source mode; it replaces
    /// the one held before at the same index. None when no page index is left.
    pub(crate) fn install_model_page(&mut self, texture: BatchTexture) -> Option<usize> {
        if let Some(index) = self.model_page {
            self.pages[index].texture = texture;
            return Some(index);
        }
        let index = self.pages.len();
        if u8::try_from(index).is_err() {
            log::warn!("Sprite atlas: no page left for declared models");
            return None;
        }
        self.pages.push(SpriteAtlasPage { texture });
        self.picture_pages.insert(index);
        self.model_page = Some(index);
        Some(index)
    }

    /// Iterate over all atlas entries (key + sprite data).
    pub fn entries_iter(&self) -> impl Iterator<Item = (&ShpSpriteKey, &ShpSpriteEntry)> {
        self.entries.iter()
    }

    /// Whether the atlas has collected every [`object_pairs`] pair the world
    /// draws, every live AnimClass colour remap, and the harvest overlay once
    /// a harvester exists. It allocates nothing per object and stops at the
    /// first miss. Keys found unrenderable count as collected: a sprite with
    /// nothing to draw never forces another refresh.
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
            && self.covers_harvest_overlay(entities.values())
    }

    fn covers_object(&self, type_id: &str, color: HouseColorIndex) -> bool {
        self.covered_objects
            .get(type_id)
            .is_some_and(|colors| colors.contains(&color))
    }

    /// The harvest overlay is loaded, or none of `objects` harvests.
    fn covers_harvest_overlay<'a>(
        &self,
        mut objects: impl Iterator<Item = &'a crate::sim::game_entity::GameEntity>,
    ) -> bool {
        self.harvest_overlay_loaded || !objects.any(|entity| entity.miner.is_some())
    }

    /// Admit one logical key only after its physical rectangle was uploaded.
    /// Every alias reuses the rectangle while retaining its own draw metadata.
    fn admit_placed_sprite(&mut self, sprite: &RenderedShpSprite, placement: ShpFramePlacement) {
        match atlas_entry(
            sprite,
            placement.origin,
            placement.page,
            placement.page_size,
        ) {
            Some(entry) => {
                self.physical_frames
                    .insert(sprite.physical_frame(), placement);
                self.frame_sources
                    .insert(sprite.key.clone(), (sprite.shp.clone(), sprite.frame));
                self.entries.insert(sprite.key.clone(), entry);
            }
            None => {
                log::warn!(
                    "Sprite atlas page {} exceeds the u8 page index",
                    placement.page
                );
                self.unrenderable.insert(sprite.key.clone());
            }
        }
    }

    fn admit_aliases(&mut self, sprites: Vec<RenderedShpSprite>) {
        for sprite in sprites {
            if let Some(placement) = self.physical_frames.get(&sprite.physical_frame()).copied() {
                self.admit_placed_sprite(&sprite, placement);
            } else {
                // The representative did not fit a page or its u8 page index.
                self.unrenderable.insert(sprite.key);
            }
        }
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
        sprites: Vec<RenderedShpSprite>,
        palettes: ShpPalettes,
    ) {
        let (sprites, aliases) = separate_physical_frames(sprites, &self.physical_frames);
        let max_dim = device.limits().max_texture_dimension_2d;
        let mut palettes = palettes;
        palettes.texture = self.palettes.texture.take();
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
            let hd = self.hd.as_ref().and_then(Option::as_ref);
            for (index, page) in self.pages.iter_mut().enumerate() {
                if !self.picture_pages.contains(&index) && !hd.is_some_and(|hd| hd.is_page(index)) {
                    batch.rebind_indexed_palette(device, &mut page.texture, &palette_rows);
                }
            }
            if let Some(hd) = hd {
                hd.rebind(device, batch, &mut self.pages, &palette_rows);
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
                self.admit_placed_sprite(
                    &sprite,
                    ShpFramePlacement {
                        origin,
                        page: group.page,
                        page_size,
                    },
                );
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
        self.admit_aliases(aliases);
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

/// The same selected frame of the same immutable source has the same index
/// bytes: `indices()` only calls ShpFile::decode_frame. Palette/context and
/// per-type draw offsets never enter that decoder. Use content identity rather
/// than a borrowed pointer, so source remounts/allocations cannot alias by reuse.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ShpPhysicalFrame {
    source_digest: [u8; 32],
    frame: usize,
}

#[derive(Clone, Copy)]
struct ShpFramePlacement {
    origin: [u32; 2],
    page: usize,
    page_size: [u32; 2],
}

/// Keep one upload per newly needed physical frame. All other logical keys
/// wait until their resident or same-batch representative is admitted. Initial
/// packing and growth share this decision and the same admission owner.
fn separate_physical_frames(
    sprites: Vec<RenderedShpSprite>,
    resident: &HashMap<ShpPhysicalFrame, ShpFramePlacement>,
) -> (Vec<RenderedShpSprite>, Vec<RenderedShpSprite>) {
    let mut pending = HashSet::new();
    sprites.into_iter().partition(|sprite| {
        let frame = sprite.physical_frame();
        !resident.contains_key(&frame) && pending.insert(frame)
    })
}

/// Intermediate rendered sprite before atlas packing.
///
/// It names its frame instead of holding the pixels, so the sprites waiting
/// for the pack hold none: each band decodes its sprites' frames as it is
/// uploaded ([`RenderedShpSprite::indices`]). The shared physical file pins
/// the resolver's selected storage without borrowing the process AssetManager
/// or copying its bytes; CPU preparation can therefore outlive that lease.
struct RenderedShpSprite {
    key: ShpSpriteKey,
    /// The atlas palette row its indices resolve through ([`ShpPalettes`]).
    palette: u32,
    /// The SHP file the frame is stored in, already decoded whole once, and
    /// the frame.
    shp: SharedBytes,
    source_digest: [u8; 32],
    frame: usize,
    width: u32,
    height: u32,
    /// Offset from cell center to top-left of sprite.
    offset_x: f32,
    offset_y: f32,
    canvas_rect: [f32; 4],
    extended: bool,
}

impl RenderedShpSprite {
    fn physical_frame(&self) -> ShpPhysicalFrame {
        ShpPhysicalFrame {
            source_digest: self.source_digest,
            frame: self.frame,
        }
    }

    /// Palette indices of the stored frame rectangle, without canvas padding.
    fn indices(&self) -> Vec<u8> {
        ShpFile::decode_frame(&self.shp, self.frame)
            .expect("a rendered sprite's SHP decoded whole when it was rendered")
    }
}

/// End a refresh without changing the atlas the renderer is already using.
///
/// A refresh mutates the prior atlas only after every fallible step has
/// passed, so dropping it here leaves that atlas exactly as it was.
#[cold]
fn keep_previous_sprite_atlas(failure: &str) {
    log::warn!("{failure}; keeping the previous valid sprite atlas");
}

/// A map-load build that cannot draw what the map needs fails the load,
/// which returns the player to the shell with this message.
#[cold]
fn sprite_atlas_load_error(failure: String) -> anyhow::Error {
    anyhow::Error::msg(failure).context("could not prepare the map's sprites")
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

/// Every (type, house colour) pair the world's objects draw: each
/// [`entity_pairs`] pair of a stored object, and every building a unit can
/// deploy into in every house colour. Every sprite key an object needs
/// derives from one of these pairs, so the atlas records coverage per pair.
fn object_pairs<'a>(
    entities: &'a crate::sim::entity_store::EntityStore,
    house_colors: &'a HouseColorMap,
    extra_building_types: &'a [&'a str],
    interner: Option<&'a crate::sim::intern::StringInterner>,
) -> impl Iterator<Item = (&'a str, HouseColorIndex)> + 'a {
    let deploy_targets = extra_building_types
        .iter()
        .flat_map(move |&type_id| house_colors.values().map(move |&color| (type_id, color)));
    entity_pairs(entities.values(), house_colors, interner).chain(deploy_targets)
}

/// The (type, house colour) pairs `objects` draw: each non-voxel object in
/// its owner's colour, and each active Infantry disguise in its disguise
/// house's colour. Objects of one type and owner give one pair between them,
/// at the first.
fn entity_pairs<'a>(
    objects: impl Iterator<Item = &'a crate::sim::game_entity::GameEntity> + 'a,
    house_colors: &'a HouseColorMap,
    interner: Option<&'a crate::sim::intern::StringInterner>,
) -> impl Iterator<Item = (&'a str, HouseColorIndex)> + 'a {
    let mut asked = crate::util::lookup_hash::LookupSet::default();
    objects
        .filter(|entity| !entity.is_voxel())
        .flat_map(|entity| {
            // InfantryClass::DrawIt @ 0x00518DDF and TechnoClass::GetRemapColour
            // @ 0x00705D70 (branch 0x00705DAB) select its type and house. Keep both
            // images resident across the viewer-dependent cycle, even when
            // no real infantryman supplies that type/colour to the atlas.
            let disguise = if entity.category() == EntityCategory::Infantry {
                entity
                    .seldom
                    .disguise
                    .as_ref()
                    .filter(|disguise| disguise.is_disguised())
                    .and_then(|disguise| {
                        Some((
                            disguise.type_id()?,
                            disguise.house().unwrap_or(entity.owner()),
                        ))
                    })
            } else {
                None
            };
            [Some((entity.type_ref(), entity.owner())), disguise]
                .into_iter()
                .flatten()
        })
        .filter(move |pair| asked.insert(*pair))
        .map(move |(type_id, owner)| {
            let owner = interner.map_or("", |i| i.resolve(owner));
            let color = house_colors
                .get(owner)
                .copied()
                .unwrap_or(crate::rules::house_colors::NO_REMAP);
            (interner.map_or("", |i| i.resolve(type_id)), color)
        })
}

/// The world-derived inputs, beyond the live objects, that decide which SHP
/// sprites the entity atlas must hold. Map load and mid-match growth both
/// collect them here, so the two builds cannot cover different keys. Map load
/// alone adds the scenario-start crate CellAnim remaps
/// (`startup_crate_anim_remap_keys`), whose AnimObjects do not exist yet.
pub(crate) struct EntityAtlasCoverage<'r> {
    /// Deploy targets and Ares SHP water images, pre-loaded in every house
    /// colour.
    pub(crate) extra_types: Vec<&'r str>,
    /// Live AnimClass palette variants.
    pub(crate) anim_remap_keys: HashSet<AnimRemapKey>,
    /// Terrain tile animations, drawn from the theater ISO palette.
    pub(crate) cell_drawer_type_ids: HashSet<String>,
    /// FoggedObject frames that outlive their source.
    pub(crate) frozen_keys: HashSet<ShpSpriteKey>,
}

impl<'r> EntityAtlasCoverage<'r> {
    pub(crate) fn collect(
        sim: &crate::sim::world::Simulation,
        rules: Option<&'r RuleSet>,
        house_colors: &HouseColorMap,
    ) -> Self {
        let mut extra_types = deployable_building_types(rules);
        extra_types.extend(ares_shp_water_image_types(rules));
        // VERA: the types a module's `ImageOverride` draws objects as.
        if let Some(rules) = rules {
            for image in sim.ext_module_images() {
                if let Some(object) = rules.object(image)
                    && !extra_types
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(&object.id))
                {
                    extra_types.push(&object.id);
                }
            }
        }
        Self {
            extra_types,
            anim_remap_keys: collect_anim_remap_base_keys(sim, house_colors),
            cell_drawer_type_ids: sim
                .resolved_terrain()
                .into_iter()
                .flat_map(|terrain| terrain.tile_animations())
                .map(|anim| anim.anim_name.to_ascii_uppercase())
                .collect(),
            frozen_keys: rules.map_or_else(HashSet::new, |rules| {
                fogged_sprite_keys(
                    sim.fog().fogged_objects().values(),
                    sim.interner(),
                    rules,
                    house_colors,
                )
            }),
        }
    }
}

/// Building types reachable through `DeploysInto=` (e.g. AMCV→GACNST). Units
/// with DeploysInto can appear through production or scripted events without
/// being on the map yet, so the atlas pre-loads every deploy target in the
/// rules; no MCV/ConYard pair is hard-coded.
pub(crate) fn deployable_building_types(rules: Option<&RuleSet>) -> Vec<&str> {
    let Some(rules) = rules else {
        return Vec::new();
    };
    let mut result: Vec<&str> = Vec::new();
    for obj in rules.all_objects() {
        if let Some(target_id) = &obj.deploys_into
            && let Some(target_obj) = rules.object(target_id)
            && !result
                .iter()
                .any(|r| r.eq_ignore_ascii_case(&target_obj.id))
        {
            result.push(&target_obj.id);
        }
    }
    result
}

/// SHP VehicleTypes another type names as its `WaterImage=` in a game Ares
/// plays. `UnitClass_DrawSHP_WaterType` (hook `0x0073C5FC`) draws one the
/// moment its owner enters water, so the atlas loads it before any object of
/// that type exists; a voxel water image has no SHP to load.
pub(crate) fn ares_shp_water_image_types(rules: Option<&RuleSet>) -> Vec<&str> {
    let Some(rules) = rules.filter(|rules| rules.ares()) else {
        return Vec::new();
    };
    let mut result: Vec<&str> = Vec::new();
    for object in rules.all_objects() {
        if let Some(water) = object.water_image().and_then(|name| rules.object(name))
            && !rules
                .art()
                .resolve_metadata_entry(&water.id, &water.image)
                .is_some_and(|art| art.voxel)
            && !result.iter().any(|id| id.eq_ignore_ascii_case(&water.id))
        {
            result.push(&water.id);
        }
    }
    result
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

/// The sprite keys of these FoggedObject records' pictures. Immutable
/// FoggedObject4D1890 inputs also survive the last live source. Their
/// ordinary building/Anim palettes are separate atlas variants, with their
/// exact recorded frames rather than a reconstructed live animation.
fn fogged_sprite_keys<'a>(
    records: impl Iterator<Item = &'a crate::sim::vision::FoggedObjectFootprintRecord>,
    interner: &crate::sim::intern::StringInterner,
    rules: &RuleSet,
    house_colors: &HouseColorMap,
) -> HashSet<ShpSpriteKey> {
    use crate::sim::vision::FoggedDrawKind;
    let mut keys = HashSet::new();
    for frozen in records.filter_map(|record| record.drawing.as_ref()) {
        let color = house_colors
            .get(interner.resolve(frozen.owner))
            .copied()
            .unwrap_or(crate::rules::house_colors::NO_REMAP);
        for record in &frozen.records {
            let Ok(frame) = u16::try_from(record.frame) else {
                continue;
            };
            let name = interner.resolve(record.type_ref);
            let context = match record.kind {
                FoggedDrawKind::Building => {
                    let Some(object) = rules.object(name) else {
                        continue;
                    };
                    let art = rules.art().resolve_metadata_entry(name, &object.image);
                    let context = art.map_or(
                        ShpPaletteContext::SelectedScheme,
                        building_art_palette_context,
                    );
                    if let Some(bib) = art.and_then(|art| art.bib_shape.as_deref()) {
                        keys.insert(ShpSpriteKey {
                            palette_context: context,
                            type_id: bib.to_ascii_uppercase_smolstr(),
                            facing: 0,
                            frame,
                            house_color: color,
                        });
                    }
                    context
                }
                FoggedDrawKind::Animation => {
                    let config = rules.art().anim_runtime_config(name);
                    // A frame past the SHP (End=/LoopEnd= beyond its frames)
                    // draws nothing (`CC_Draw_Shape`, `0x0069E7F3`); the
                    // atlas would stand frame 0 in for it.
                    if config
                        .and_then(|config| config.raw_shp_frame_count)
                        .is_some_and(|count| record.frame >= count)
                    {
                        continue;
                    }
                    frozen_anim_palette_context(config)
                }
            };
            keys.insert(ShpSpriteKey {
                palette_context: context,
                type_id: name.into(),
                facing: 0,
                frame,
                house_color: if context == ShpPaletteContext::AnimPalette {
                    HouseColorIndex(0)
                } else {
                    color
                },
            });
        }
    }
    keys
}

/// A recorded frame or palette can first appear without any object spawn.
/// A source which could not supply that frame must not trigger rebuild loops.
pub(crate) fn atlas_covers_sprite_keys(
    atlas: Option<&SpriteAtlas>,
    keys: &HashSet<ShpSpriteKey>,
    rules: Option<&RuleSet>,
) -> bool {
    let Some(atlas) = atlas else {
        return keys.is_empty();
    };
    let resolved =
        |key: &ShpSpriteKey| atlas.entries.contains_key(key) || atlas.unrenderable.contains(key);
    keys.iter().all(|key| {
        resolved(key)
            && (!rules
                .and_then(|rules| rules.object(&key.type_id))
                .is_some_and(|object| {
                    object.category == crate::rules::object_type::ObjectCategory::Building
                })
                || atlas
                    .object_shp_frame_count(&key.type_id)
                    .and_then(|count| fogged_shadow_key(key, count))
                    .is_none_or(|shadow| resolved(&shadow)))
    })
}

fn fogged_shadow_key(body: &ShpSpriteKey, count: u16) -> Option<ShpSpriteKey> {
    Some(ShpSpriteKey {
        frame: body.frame.checked_add(count / 2)?,
        ..body.clone()
    })
}

/// Whether `atlas` holds what the objects whose look changed now draw: the
/// pairs and harvest overlay of `objects`, the colour remaps of `anims` and
/// the frames of the FoggedObject `records` added since the last check. A
/// look that changes without an entity spawn (a disguise, an in-place type
/// change, a producer's anim palette) reaches the ordinary draw this way
/// before its first frame. [`atlas_covers_world`] and
/// [`atlas_covers_sprite_keys`] ask the same of the whole world.
pub(crate) fn atlas_covers_changes<'a>(
    atlas: Option<&SpriteAtlas>,
    objects: &[&crate::sim::game_entity::GameEntity],
    anims: &[&crate::sim::anim_class::AnimObject],
    records: impl Iterator<Item = &'a crate::sim::vision::FoggedObjectFootprintRecord>,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&RuleSet>,
    house_colors: &HouseColorMap,
) -> bool {
    let mut pairs = entity_pairs(objects.iter().copied(), house_colors, Some(interner));
    let mut remaps = anim_remap_keys(anims.iter().copied(), interner, house_colors);
    let frozen = rules.map_or_else(HashSet::new, |rules| {
        fogged_sprite_keys(records, interner, rules, house_colors)
    });
    match atlas {
        Some(atlas) => {
            pairs.all(|(type_id, color)| atlas.covers_object(type_id, color))
                && atlas.covers_harvest_overlay(objects.iter().copied())
                && remaps.all(|key| atlas.covered_anim_remaps.contains(&key))
                && atlas_covers_sprite_keys(Some(atlas), &frozen, rules)
        }
        None => pairs.next().is_none() && remaps.next().is_none() && frozen.is_empty(),
    }
}

/// The atlas key of every live remapped Anim.
fn anim_remap_base_keys<'a>(
    sim: &'a crate::sim::world::Simulation,
    house_colors: &'a HouseColorMap,
) -> impl Iterator<Item = AnimRemapKey> + 'a {
    anim_remap_keys(
        sim.anims().map(|(_, anim)| anim),
        sim.interner(),
        house_colors,
    )
}

/// The atlas key of each remapped Anim of `anims`. Many Anims share a type,
/// palette and colour: each such triple gives one key, at its first Anim.
fn anim_remap_keys<'a>(
    anims: impl Iterator<Item = &'a crate::sim::anim_class::AnimObject> + 'a,
    interner: &'a crate::sim::intern::StringInterner,
    house_colors: &'a HouseColorMap,
) -> impl Iterator<Item = AnimRemapKey> + 'a {
    let mut asked = crate::util::lookup_hash::LookupSet::default();
    anims.filter_map(move |anim| {
        let color = anim_remap_color(anim, interner, house_colors)?;
        let context = anim_remap_context(anim);
        asked.insert((anim.type_id(), context, color)).then(|| {
            (
                interner.resolve(anim.type_id()).to_ascii_uppercase(),
                context,
                color,
            )
        })
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
    anim_remap_color_with(anim, |house| house_scheme(interner, house_colors, house))
}

/// [`anim_remap_color`], with each house's scheme from `scheme`: a draw that
/// meets many Anims of one house looks it up once.
pub(crate) fn anim_remap_color_with(
    anim: &crate::sim::anim_class::AnimObject,
    scheme: impl FnOnce(crate::sim::intern::InternedId) -> HouseColorIndex,
) -> Option<HouseColorIndex> {
    use crate::sim::anim_class::AnimRemap;
    Some(match anim.remap()? {
        AnimRemap::ColorScheme(color) => color,
        AnimRemap::House(house) | AnimRemap::ExtraBuildingScheme { house, .. } => scheme(house),
    })
}

/// House `house`'s colour scheme in `house_colors`, or none.
pub(crate) fn house_scheme(
    interner: &crate::sim::intern::StringInterner,
    house_colors: &HouseColorMap,
    house: crate::sim::intern::InternedId,
) -> HouseColorIndex {
    house_colors
        .get(interner.resolve(house))
        .copied()
        .unwrap_or(crate::rules::house_colors::NO_REMAP)
}

/// The palette a remapped anim's scheme is built on: the unit palette, or
/// the extra building palette for a custom-palette producer's scheme.
pub(crate) fn anim_remap_context(anim: &crate::sim::anim_class::AnimObject) -> ShpPaletteContext {
    match anim.remap() {
        Some(crate::sim::anim_class::AnimRemap::ExtraBuildingScheme { palette, .. }) => {
            ShpPaletteContext::ExtraScheme(palette)
        }
        _ => ShpPaletteContext::AnimScheme,
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
                type_id: type_id.as_str().into(),
                facing: 0,
                frame,
                house_color: *color,
            });
        }
    }
}

/// Which palette a sprite's frames are baked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// An Ares AnimType `CustomPalette=` file ([`AresCustomPalettes`]). Both
    /// hooks hand over its plain convert with no ColorScheme, so 16..31 keep
    /// the file's own colours.
    AresCustom,
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

/// The Ares AnimType `CustomPalette=` files that loaded, by atlas type id
/// (uppercase), as resolved names.
///
/// AnimClass_Draw_SetPalette (Ares.dll `1000ACE0`, hook `0x4232CE`, resumes
/// at `0x4232D4`) swaps an anim's ANIM.PAL convert for its own palette.
/// BulletClass_Draw_SetAnimPalette (`1001A7B0`, hook `0x468379`, resumes at
/// `0x4683D7`) gives a bullet the palette of the AnimType its ImageFile
/// names (GetConvert `10019360`, AnimType Find `1000D3E0`), skipping
/// PALETTE.PAL, ANIM.PAL and the firer's scheme.
#[derive(Default)]
struct AresCustomPalettes {
    anims: HashMap<String, String>,
    bullets: HashMap<String, String>,
}

impl AresCustomPalettes {
    /// The palette file the hook for `key`'s context would use, if any.
    fn file(&self, key: &ShpSpriteKey) -> Option<&str> {
        let by_type = match key.palette_context {
            ShpPaletteContext::GlobalAnim => &self.anims,
            ShpPaletteContext::BulletDefault
            | ShpPaletteContext::BulletAnim
            | ShpPaletteContext::BulletFirer => &self.bullets,
            _ => return None,
        };
        by_type
            .get(&key.type_id.to_ascii_uppercase())
            .map(String::as_str)
    }

    /// Load and register the palette `key`'s AnimType names, once per type.
    /// CustomPalette::LoadFromINI (`10076160`) turns the first `~~~` into the
    /// theater extension and opens the name as written, adding no `.pal`. A
    /// file that does not load keeps the stock palette.
    ///
    /// A bullet's Image must name an existing AnimType: Find `1000D3E0`
    /// searches only AnimTypeClass's array, and a miss keeps the stock arms.
    /// An `IsVeins=yes` anim takes the player's scheme at `0x42312A` and
    /// never reaches hook `0x4232CE`.
    fn register(
        &mut self,
        key: &ShpSpriteKey,
        rules: Option<&RuleSet>,
        art: Option<&ArtRegistry>,
        theater_ext: &str,
        load: &mut impl FnMut(&str) -> bool,
    ) {
        let (by_type, anim) = match key.palette_context {
            ShpPaletteContext::GlobalAnim => (&mut self.anims, Some(key.type_id.as_str())),
            ShpPaletteContext::BulletDefault
            | ShpPaletteContext::BulletAnim
            | ShpPaletteContext::BulletFirer => (
                &mut self.bullets,
                rules.and_then(|rules| {
                    let image = rules.projectile(key.type_id.as_str())?.image.as_deref()?;
                    rules
                        .anim_type_names
                        .contains(&image.to_ascii_uppercase())
                        .then_some(image)
                }),
            ),
            _ => return,
        };
        let upper = key.type_id.to_ascii_uppercase();
        if by_type.contains_key(&upper) {
            return;
        }
        let Some(name) = anim
            .and_then(|anim| art?.anim_runtime_config(anim))
            .filter(|config| !config.is_veins)
            .and_then(|config| config.custom_palette())
        else {
            return;
        };
        let file = name.replacen("~~~", theater_ext, 1);
        if load(&file) {
            by_type.insert(upper, file);
        }
    }
}

/// `extra_palette_type_ids` holds the custom-palette TechnoTypes, `None` when
/// the extra building palette did not load. TechnoClass::DrawSHP tests
/// TerrainPalette first (`0x00705EC7`), then draws from GetRemapColour
/// (`0x00705F81`), whose `+0xDF0` arm (`0x00705D82..0x00705D9A`) picks the
/// house's scheme from the type's generated palette.
///
/// `ares_palettes` holds the Ares `CustomPalette=` files that loaded. A bullet
/// takes its file in every context (hook `0x468379`). An anim takes it only
/// in GlobalAnim: a cell-drawn anim (+0x196, AnimCell, `0x4231FB`) and one
/// with its own scheme (+0xD4, AnimScheme, `0x4232AB`) skip hook `0x4232CE`,
/// and `AltPalette=` replaces the convert again after the hook resumes
/// (`0x4232D8`).
fn sprite_palette_for_key(
    key: &ShpSpriteKey,
    art: Option<&ArtRegistry>,
    effect_type_ids: &HashSet<String>,
    cell_palette_type_ids: &HashSet<String>,
    extra_palette_type_ids: Option<&HashSet<String>>,
    ares_palettes: &AresCustomPalettes,
) -> SpritePaletteChoice {
    match key.palette_context {
        ShpPaletteContext::BulletDefault
        | ShpPaletteContext::BulletAnim
        | ShpPaletteContext::BulletFirer
            if ares_palettes.file(key).is_some() =>
        {
            SpritePaletteChoice::AresCustom
        }
        ShpPaletteContext::BulletDefault => SpritePaletteChoice::Bullet,
        ShpPaletteContext::BulletAnim => SpritePaletteChoice::Anim,
        ShpPaletteContext::AnimPalette | ShpPaletteContext::Harvest => SpritePaletteChoice::Anim,
        ShpPaletteContext::BulletFirer => SpritePaletteChoice::Unit,
        ShpPaletteContext::SelectedScheme | ShpPaletteContext::AnimScheme => {
            SpritePaletteChoice::Unit
        }
        ShpPaletteContext::Cell | ShpPaletteContext::AnimCell => SpritePaletteChoice::CellIso,
        ShpPaletteContext::Particle => SpritePaletteChoice::Anim,
        ShpPaletteContext::GlobalAnim => {
            if art
                .and_then(|a| a.anim_runtime_config(&key.type_id))
                .is_some_and(|c| c.alt_palette)
            {
                SpritePaletteChoice::Unit
            } else if ares_palettes.file(key).is_some() {
                SpritePaletteChoice::AresCustom
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
            type_id: type_str.into(),
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
            let object = rules.and_then(|r| r.object(type_str));
            let can_be_occupied = object.is_some_and(|obj| obj.can_be_occupied);
            if can_be_occupied {
                for frame in 2u16..=3 {
                    insert(frame);
                }
            }
            // A Gate=yes body selects more than frames 0/1. GetCurrentFrame
            // 43EF90's completed gate arm (0x0043F0BD..0x0043F0F5) returns 0
            // above ConditionYellow and GateStages+1 at or below it, with no
            // undamaged fallback; DrawBody's Mission Open arm
            // (0x0043D546..0x0043D68F) draws ftol(progress*GateStages) clamped
            // to 0..GateStages-1, plus GateStages+1 when damaged. The body
            // therefore reaches exactly {0..GateStages-1} u {GateStages+1..=
            // 2*GateStages}, never GateStages itself. Without these keys a
            // damaged retail GAGATE_A requested frame 10, missed the atlas and
            // vanished (#3592); the type-level coverage check never re-admits
            // a type once frames 0/1 are present.
            //
            // No draw-side fallback accompanies this: ShapeFile::GetFrameData
            // (0x0069E740) returns NULL for an index past the SHP (0x0069E7B2
            // -> 0x0069E7D0) and DrawSHP draws nothing, which the consumer's
            // skip of an absent Structure key already mirrors. A mod gate with
            // authored damaged stages must show them. A gate SHP physically
            // shorter than 2*GateStages+1 instead falls to the packing-time
            // facing/frame-0 substitution further down in render_shp_frame, a
            // pre-existing exposure the CanBeOccupied frames 2/3 already
            // share. Retail G?GATE_A only looks undamaged at low health because
            // its frames 10..19 are byte-identical copies of 0..9.
            if let Some((rules, object)) = rules.zip(object).filter(|(_, object)| object.gate) {
                let stages = rules
                    .art()
                    .resolve_metadata_entry(type_str, &object.image)
                    .map_or(9, |entry| entry.building_gate_stages);
                if let Some(stages) = u16::try_from(stages)
                    .ok()
                    .filter(|&stages| stages > 0 && stages <= u16::MAX / 2)
                {
                    // Frames 0 and 1 are already requested above.
                    for frame in (2..stages).chain(stages + 1..=stages * 2) {
                        insert(frame);
                    }
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
            // A `Turret=yes` SHP Unit draws its turret from the same SHP:
            // thirty-two frames from `WalkFrames * 8`, or with Ares from
            // `WalkFrames * Facings` (`UnitClass_DrawSHP_FacingsB`). The draw
            // picks one from the turret heading, so all of them stay resident.
            if let Some(turret) = seq_set.and_then(|set| set.shp_vehicle_turret()) {
                let start = match rules.filter(|rules| rules.ares()) {
                    Some(rules) => rules
                        .object(type_str)
                        .and_then(|object| rules.unit_shp_read_state_of(object))
                        .and_then(|art| art.ares_turret_start_frame()),
                    None => Some(turret.start_frame),
                };
                if let Some(start) = start {
                    for slot in 0..crate::rules::art_data::SHP_TURRET_FACING_FRAMES {
                        insert(start.saturating_add(slot));
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
fn insert_new_object_keys(
    needed: &mut HashSet<ShpSpriteKey>,
    new_objects: &HashSet<(String, HouseColorIndex)>,
    entities: &crate::sim::entity_store::EntityStore,
    interner: Option<&crate::sim::intern::StringInterner>,
    asset_manager: &AssetManager,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
    mut pictures: Option<&mut PictureRoutes<'_>>,
) -> HashMap<(ShpImageKind, String), Option<ShpSource>> {
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
    let water_images = ares_shp_water_image_types(rules);
    for (type_str, color_idx) in new_objects {
        let category = categories
            .get(type_str.as_str())
            .copied()
            .unwrap_or_else(|| {
                use crate::rules::object_type::ObjectCategory;
                match rules
                    .and_then(|rules| rules.object(type_str))
                    .map(|object| object.category)
                {
                    Some(ObjectCategory::Infantry) => EntityCategory::Infantry,
                    Some(ObjectCategory::Vehicle) => EntityCategory::Unit,
                    _ => EntityCategory::Structure,
                }
            });
        let infantry_frames = if category == EntityCategory::Infantry {
            let frames = frame_sets.entry(type_str.as_str()).or_insert_with(|| {
                let actions = rules
                    .and_then(|rules| {
                        rules.animation_sequence_in_category(
                            crate::rules::object_type::ObjectCategory::Infantry,
                            type_str,
                        )
                    })
                    .and_then(|set| set.infantry_actions());
                // VERA-only, experimental (docs/picture-model-art.md): the
                // records select frames of a declared picture's frame count;
                // its SHP is never opened.
                if let Some(count) = pictures
                    .as_deref_mut()
                    .and_then(|routes| routes.object_frames(type_str))
                {
                    return count
                        .zip(actions)
                        .map(|(count, actions)| infantry_asset_frames(actions, count))
                        .unwrap_or_default();
                }
                let source = load_shp_source(
                    asset_manager,
                    type_str,
                    None,
                    theater_ext,
                    theater_name,
                    rules,
                    art,
                    ShpImageKind::Object,
                );
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
                sources.insert((ShpImageKind::Object, type_str.clone()), source);
                frames
            });
            Some(frames.as_slice())
        } else {
            None
        };
        let mut body_keys = insert_object_keys(
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
            // VERA-only, experimental (docs/picture-model-art.md): a declared
            // picture's frame count stands in, and its SHP is never opened.
            let picture_count = pictures
                .as_deref_mut()
                .and_then(|routes| routes.object_frames(type_str));
            let raw_count = match picture_count {
                Some(count) => count,
                None => sources
                    .entry((ShpImageKind::Object, type_str.clone()))
                    .or_insert_with(|| {
                        load_shp_source(
                            asset_manager,
                            type_str,
                            None,
                            theater_ext,
                            theater_name,
                            rules,
                            art,
                            ShpImageKind::Object,
                        )
                    })
                    .as_ref()
                    .and_then(|source| u16::try_from(source.shp.frames.len()).ok()),
            };
            if let Some(raw_count) = raw_count {
                // Unit73C6AD..73C6D3 draws a counting Unit's death image, which
                // no sequence holds. Past the SHP, GetFrameData (0x0069E740)
                // answers NULL and nothing draws.
                if let Some(death) = rules
                    .filter(|_| category == EntityCategory::Unit)
                    .and_then(|rules| rules.animation_sequence(type_str))
                    .and_then(|set| set.shp_vehicle_death())
                {
                    let images = death.images();
                    for frame in images.start.max(0)..images.end.min(i32::from(raw_count)) {
                        let key = ShpSpriteKey {
                            palette_context: ShpPaletteContext::Legacy,
                            type_id: type_str.as_str().into(),
                            facing: 0,
                            frame: frame as u16,
                            house_color: *color_idx,
                        };
                        needed.insert(key.clone());
                        body_keys.push(key);
                    }
                }
                let shadow_start = raw_count / 2;
                // An Ares water image is drawn with its owner's facing count
                // and turret block (`UnitClass_DrawSHP_ChangeType1`,
                // `FacingsA`/`FacingsB`), which its own blocks need not
                // match: keep its whole body half resident.
                if category == EntityCategory::Unit
                    && water_images
                        .iter()
                        .any(|water| water.eq_ignore_ascii_case(type_str))
                {
                    for frame in 0..shadow_start {
                        let key = ShpSpriteKey {
                            palette_context: ShpPaletteContext::Legacy,
                            type_id: type_str.as_str().into(),
                            facing: 0,
                            frame,
                            house_color: *color_idx,
                        };
                        needed.insert(key.clone());
                        body_keys.push(key);
                    }
                }
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
fn sprite_key_order(key: &ShpSpriteKey) -> (&str, ShpImageKind, ShpPaletteContext, u8, u16, u8) {
    (
        key.type_id.as_str(),
        key.palette_context.image_kind(),
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
    projectile
        .image_loads
        .iter()
        .flat_map(|load| load.shp_names(theater_ext, theater_name))
        .collect()
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
    mut pictures: Option<&mut PictureRoutes<'_>>,
) {
    let Some(rules) = rules else {
        return;
    };
    for weapon in rules.weapons_iter() {
        let Some(projectile) = rules.weapon_projectile(weapon) else {
            continue;
        };
        if projectile.inviso || projectile.voxel {
            continue;
        }
        let id = projectile.id.to_ascii_uppercase();
        let (_, count) = files.entry(id.clone()).or_insert_with(|| {
            // VERA-only, experimental (docs/picture-model-art.md): a declared
            // picture's frames, with no file; its SHP is never opened.
            if let Some(routes) = pictures.as_deref_mut()
                && let Some(source) = routes.projectile(projectile)
            {
                return (String::new(), routes.frame_count(&source).unwrap_or(0));
            }
            let candidates = projectile_shp_candidates(projectile, theater_ext, theater_name);
            find_shp(assets, &candidates)
                .and_then(|(file, bytes)| {
                    ShpFile::frame_count_from_bytes(&bytes)
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

/// VERA-only, experimental (docs/picture-model-art.md): a rules object's body
/// picture, declared in the section its image resolves to: the one its art
/// metadata and Buildup are read through. None draws it from its SHP.
pub(crate) fn object_picture(
    art: &ArtRegistry,
    rules: &RuleSet,
    type_id: &str,
) -> Option<PictureSource> {
    let object = rules.object(type_id)?;
    PictureRoutes::declared(art, type_id, &object.image)
}

/// VERA-only, experimental (docs/picture-model-art.md): a building's Buildup
/// picture: `Buildup=` beside its declared `Picture=` names `NAME.png`
/// ([`buildup_shp_candidates`] otherwise).
pub(crate) fn buildup_picture(
    art: &ArtRegistry,
    rules: Option<&RuleSet>,
    building: &str,
) -> Option<PictureSource> {
    let image = rules
        .and_then(|rules| rules.object(building))
        .map_or(building, |object| object.image.as_str());
    let source = PictureRoutes::declared(art, building, image)?;
    let buildup = art.get(&source.section)?.buildup.as_deref()?;
    Some(source.named(buildup))
}

/// VERA-only, experimental (docs/picture-model-art.md): which atlas keys are
/// drawn from a declared picture instead of an SHP, and those pictures' frame
/// counts. Built only when some art section declares `Picture=`; with none
/// (retail data) every key takes its SHP path and no picture is looked up.
struct PictureRoutes<'a> {
    art: &'a ArtRegistry,
    rules: Option<&'a RuleSet>,
    asset_manager: &'a AssetManager,
    theater_name: &'a str,
    /// Upper-case `BibShape=` names of sections that declare `Picture=`,
    /// with the bib picture each names. The first section to name one wins.
    bibs: HashMap<String, PictureSource>,
    /// Upper-case factory door names (`ArtEntry::factory_door`) of sections
    /// that declare `Picture=`, with the picture each names. These are raw
    /// SHP names the building loads (`0x0045F31D..0x0045F706`), not
    /// AnimTypes, so like `BibShape=` they name `NAME.png`. The first section
    /// to name one wins.
    doors: HashMap<String, PictureSource>,
    /// Frame counts read from picture headers in this build.
    counts: HashMap<PictureSource, Option<u16>>,
}

impl<'a> PictureRoutes<'a> {
    fn new(
        art: Option<&'a ArtRegistry>,
        rules: Option<&'a RuleSet>,
        asset_manager: &'a AssetManager,
        theater_name: &'a str,
    ) -> Option<Self> {
        let art = art?;
        // Sections of only VERA's drawing keys declare pictures too, but no
        // BibShape= or factory door (`ArtRegistry::declares_any_picture`).
        let declared = art.declares_any_picture();
        let mut bibs = HashMap::new();
        let mut doors = HashMap::new();
        for (name, entry) in art.iter_entries() {
            if entry.picture.is_none() {
                continue;
            }
            let Some(source) = Self::declared(art, name, "") else {
                continue;
            };
            if let Some(bib) = entry.bib_shape.as_deref() {
                bibs.entry(bib.to_ascii_uppercase())
                    .or_insert_with(|| source.named(bib));
            }
            for door in entry.factory_door.names() {
                doors
                    .entry(door.to_ascii_uppercase())
                    .or_insert_with(|| source.named(door));
            }
        }
        declared.then(|| Self {
            art,
            rules,
            asset_manager,
            theater_name,
            bibs,
            doors,
            counts: HashMap::new(),
        })
    }

    /// The picture declared by the art section `type_id`'s image resolves to
    /// ([`ArtRegistry::resolve_declared_picture`]).
    fn declared(art: &ArtRegistry, type_id: &str, rules_image: &str) -> Option<PictureSource> {
        let (section, decl) = art.resolve_declared_picture(type_id, rules_image)?;
        Some(PictureSource {
            section: section.to_owned(),
            decl: decl.clone(),
        })
    }

    /// An AnimType's picture, declared in its own section.
    fn anim(&self, anim_type: &str) -> Option<PictureSource> {
        Self::declared(self.art, anim_type, "")
    }

    /// A rules object's body picture ([`object_picture`]).
    fn object(&self, type_id: &str) -> Option<PictureSource> {
        object_picture(self.art, self.rules?, type_id)
    }

    /// A building's Buildup picture ([`buildup_picture`]).
    fn buildup(&self, building: &str) -> Option<PictureSource> {
        buildup_picture(self.art, self.rules, building)
    }

    /// A BulletType's picture, declared in the section its image resolves to.
    fn projectile(&self, projectile: &ProjectileType) -> Option<PictureSource> {
        Self::declared(
            self.art,
            &projectile.id,
            projectile.image.as_deref().unwrap_or(""),
        )
    }

    /// The picture `key` is drawn from, resolved as [`find_shp_source`]
    /// resolves its SHP: an anim's own section, a `_MAKE` key's Buildup, a
    /// rules object's image section, else a declared BibShape or factory
    /// door. None draws the key from its SHP.
    fn key(&self, key: &ShpSpriteKey) -> Option<PictureSource> {
        let type_id = key.type_id.as_str();
        match key.palette_context.image_kind() {
            ShpImageKind::Animation => self.anim(type_id),
            // A particle's image is an art section of its own, as an anim's.
            ShpImageKind::Particle => self.anim(type_id),
            ShpImageKind::Projectile => self.projectile(self.rules?.projectile(type_id)?),
            ShpImageKind::Object => {
                if let Some(building) = type_id.strip_suffix("_MAKE") {
                    self.buildup(building)
                } else if self.rules.and_then(|rules| rules.object(type_id)).is_some() {
                    self.object(type_id)
                } else {
                    // An AnimType image drawn as an object's
                    // (`[AudioVisual] WebbedInfantry=`) takes its own
                    // section's picture.
                    let upper = type_id.to_ascii_uppercase();
                    self.bibs
                        .get(&upper)
                        .or_else(|| self.doors.get(&upper))
                        .cloned()
                        .or_else(|| self.anim(type_id))
                }
            }
        }
    }

    /// `source`'s frame count, shadow frames included, from its header
    /// ([`ArtRegistry::picture_header`]); None when it is missing, unreadable
    /// or empty, as a missing SHP.
    fn frame_count(&mut self, source: &PictureSource) -> Option<u16> {
        if let Some(&count) = self.counts.get(source) {
            return count;
        }
        let count = self
            .art
            .picture_header(
                &source.section,
                &source.decl,
                self.asset_manager,
                self.theater_name,
            )
            .and_then(|facts| u16::try_from(facts.frames).ok())
            .filter(|&count| count > 0);
        self.counts.insert(source.clone(), count);
        count
    }

    /// For a rules object whose image declares `Picture=`, its picture's
    /// frame count; None for an object drawn from its SHP.
    fn object_frames(&mut self, type_id: &str) -> Option<Option<u16>> {
        let source = self.object(type_id)?;
        Some(self.frame_count(&source))
    }

    /// For an AnimType that declares `Picture=`, its picture's frame count;
    /// None for an animation drawn from its SHP.
    fn anim_frames(&mut self, anim_type: &str) -> Option<Option<u16>> {
        let source = self.anim(anim_type)?;
        Some(self.frame_count(&source))
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
/// 2. Renders the keys not yet resolved, decoding each type's SHP whole once
///    ([`ShpRasterWork`], which needs no simulation, Rules or asset manager).
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
///
/// This is the synchronous composition of [`prepare_sprite_atlas`], the
/// raster and [`ShpAtlasUpload::finish`]; live growth runs the raster on a
/// worker instead ([`crate::render::entity_atlas_growth`]).
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
    additional_keys: &HashSet<ShpSpriteKey>,
    cell_palette: Option<&Palette>,
    existing: Option<SpriteAtlas>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> Option<SpriteAtlas> {
    let prepared = prepare_sprite_atlas(
        device.limits().max_texture_dimension_2d,
        entities,
        asset_manager,
        palette,
        theater_ext,
        theater_name,
        rules,
        art,
        house_colors,
        extra_building_types,
        anim_remap_keys,
        cell_drawer_type_ids,
        additional_keys,
        cell_palette,
        existing.as_ref(),
        interner,
    );
    let mut atlas = existing;
    match prepared {
        PreparedSpriteAtlas::Unchanged => {}
        PreparedSpriteAtlas::Abandoned(failure) => keep_previous_sprite_atlas(&failure),
        PreparedSpriteAtlas::Raster(work, upload) => {
            upload.finish(device, queue, batch, work.render(), &mut atlas);
        }
    }
    atlas
}

pub(crate) fn prepare_sprite_atlas(
    max_texture_dimension_2d: u32,
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
    additional_keys: &HashSet<ShpSpriteKey>,
    cell_palette: Option<&Palette>,
    existing: Option<&SpriteAtlas>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> PreparedSpriteAtlas {
    let started = Instant::now();
    let previous_atlas = existing;

    // Step 1: object keys, for the (type, house colour) pairs the atlas has
    // not collected — every pair at map load. Structures get facing 0 since
    // buildings don't rotate.
    let collected = |type_id: &str, color| {
        previous_atlas.is_some_and(|atlas| atlas.covers_object(type_id, color))
    };
    let new_objects: HashSet<(String, HouseColorIndex)> =
        object_pairs(entities, house_colors, extra_building_types, interner)
            .filter(|&(type_id, color)| !collected(type_id, color))
            .map(|(type_id, color)| (type_id.to_string(), color))
            .collect();
    let new_remaps: HashSet<AnimRemapKey> = anim_remap_keys
        .iter()
        .filter(|remap| {
            !previous_atlas.is_some_and(|atlas| atlas.covered_anim_remaps.contains(*remap))
        })
        .cloned()
        .collect();
    let mut needed: HashSet<ShpSpriteKey> = HashSet::new();
    // VERA-only, experimental (docs/picture-model-art.md): None unless some
    // art section declares Picture=.
    let mut picture_routes = PictureRoutes::new(art, rules, asset_manager, theater_name);
    let mut pictures = PictureSources::default();
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
        picture_routes.as_mut(),
    );

    // Frame counts and source files registered so far: a refresh extends the
    // atlas's own and replaces them only once it has succeeded.
    let (mut active_anim_frame_counts, mut make_frame_counts, mut shp_files) = previous_atlas
        .map(|atlas| {
            (
                atlas.active_anim_frame_counts.clone(),
                atlas.make_frame_counts.clone(),
                atlas.shp_files.clone(),
            )
        })
        .unwrap_or_default();
    let mut object_shp_frame_counts = previous_atlas
        .map(|atlas| atlas.object_shp_frame_counts.clone())
        .unwrap_or_default();
    for ((kind, type_id), source) in &object_sources {
        if let Some(source) = source
            && let Ok(frame_count) = u16::try_from(source.shp.frames.len())
        {
            let key = type_id.to_ascii_uppercase();
            object_shp_frame_counts.insert(key.clone(), frame_count);
            shp_files.insert((*kind, key), source.found_name.clone());
        }
    }
    // VERA-only, experimental (docs/picture-model-art.md): a picture object's
    // physical frame count is its picture's.
    if let Some(routes) = picture_routes.as_mut() {
        for (type_id, _) in &new_objects {
            if let Some(Some(count)) = routes.object_frames(type_id) {
                object_shp_frame_counts.insert(type_id.to_ascii_uppercase(), count);
            }
        }
    }
    // Frozen DrawAll4D1890 uses a saved body frame and the physical image's
    // second half as its shadow, even when the live Building no longer exists.
    // Reuse the decoded-source owner for both registration and rendering.
    needed.extend(additional_keys.iter().cloned());
    for key in additional_keys {
        // VERA-only, experimental (docs/picture-model-art.md): a picture key
        // counts its picture's frames; its SHP is never opened.
        if let Some(routes) = picture_routes.as_mut()
            && let Some(source) = routes.key(key)
        {
            if key.palette_context.image_kind() == ShpImageKind::Object
                && let Some(count) = routes.frame_count(&source)
            {
                object_shp_frame_counts.insert(key.type_id.to_ascii_uppercase(), count);
                if rules
                    .and_then(|rules| rules.object(&key.type_id))
                    .is_some_and(|object| {
                        object.category == crate::rules::object_type::ObjectCategory::Building
                    })
                    && let Some(shadow) = fogged_shadow_key(key, count)
                {
                    needed.insert(shadow);
                }
            }
            continue;
        }
        let source = object_sources
            .entry((key.palette_context.image_kind(), key.type_id.to_string()))
            .or_insert_with(|| {
                load_shp_source(
                    asset_manager,
                    &key.type_id,
                    None,
                    theater_ext,
                    theater_name,
                    rules,
                    art,
                    key.palette_context.image_kind(),
                )
            });
        let Some(source) = source else { continue };
        let Ok(count) = u16::try_from(source.shp.frames.len()) else {
            continue;
        };
        if key.palette_context.image_kind() == ShpImageKind::Object {
            object_shp_frame_counts.insert(key.type_id.to_ascii_uppercase(), count);
        }
        shp_files.insert(
            (
                key.palette_context.image_kind(),
                key.type_id.to_ascii_uppercase(),
            ),
            source.found_name.clone(),
        );
        if key.palette_context.image_kind() == ShpImageKind::Object
            && rules
                .and_then(|rules| rules.object(&key.type_id))
                .is_some_and(|object| {
                    object.category == crate::rules::object_type::ObjectCategory::Building
                })
            && let Some(shadow) = fogged_shadow_key(key, count)
        {
            needed.insert(shadow);
        }
    }
    let mut projectile_files = previous_atlas
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
        picture_routes.as_mut(),
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
            // Slot 9's row is rules-owned (`0x00464450`), so no art row
            // names a turret animation.
            let turret = rules
                .and_then(|rules| rules.object(type_id))
                .and_then(art_data::shp_turret_anim_config);
            for anim in entry.building_anims.iter().chain(turret.iter()) {
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
                // A turret row shows any entry of the 32-frame table
                // `0x007F4890`, whatever its animation's own LoopEnd.
                let facing_frames = if anim.native_slot == 9 {
                    crate::util::direction_tables::DRAGON_FRAME_TABLE.len() as u16
                } else {
                    0
                };
                for (anim_type, loop_end) in anims {
                    let loop_end = loop_end.max(facing_frames);
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
                        picture_routes.as_mut(),
                    );
                }
            }
            // BibShape: ground-level pad SHP (e.g., refinery dock GAREFNBB).
            // Use bib name directly as atlas key so render can look it up.
            // VERA-only, experimental (docs/picture-model-art.md): a bib named
            // beside a declared Picture= counts its picture's frames; its SHP
            // is never opened.
            if let Some(ref bib) = entry.bib_shape
                && let Some(routes) = picture_routes.as_mut()
                && let Some(source) = routes.bibs.get(&bib.to_ascii_uppercase()).cloned()
            {
                let name = bib.to_ascii_uppercase();
                let count = match object_shp_frame_counts.get(&name) {
                    Some(&count) => Some(count),
                    None => routes.frame_count(&source),
                };
                if let Some(count) = count {
                    object_shp_frame_counts.insert(name.clone(), count);
                    for frame in 0..count {
                        needed.insert(ShpSpriteKey {
                            palette_context: building_art_palette_context(entry),
                            type_id: name.as_str().into(),
                            facing: 0,
                            frame,
                            house_color: *house_color,
                        });
                    }
                }
            } else if let Some(ref bib) = entry.bib_shape {
                let name = bib.to_ascii_uppercase();
                let source = object_sources
                    .entry((ShpImageKind::Object, name.clone()))
                    .or_insert_with(|| {
                        load_shp_source(
                            asset_manager,
                            &name,
                            None,
                            theater_ext,
                            theater_name,
                            rules,
                            art,
                            ShpImageKind::Object,
                        )
                    });
                if let Some(source) = source {
                    let Ok(count) = u16::try_from(source.shp.frames.len()) else {
                        continue;
                    };
                    shp_files.insert(
                        (ShpImageKind::Object, name.clone()),
                        source.found_name.clone(),
                    );
                    object_shp_frame_counts.insert(name.clone(), count);
                    for frame in 0..count {
                        needed.insert(ShpSpriteKey {
                            palette_context: building_art_palette_context(entry),
                            type_id: name.as_str().into(),
                            facing: 0,
                            frame,
                            house_color: *house_color,
                        });
                    }
                }
            }
        }
    }
    for (anim_type, (count, _)) in building_anim_counts {
        active_anim_frame_counts.entry(anim_type).or_insert(count);
    }

    // Step 1c: every frame of each registered building's make (build-up) SHP,
    // a separate file (e.g., GTCNSTMK.SHP) showing the building assembling.
    if let Some(art_reg) = art {
        collect_building_make_asset_requests(
            &mut needed,
            &mut make_frame_counts,
            &mut object_shp_frame_counts,
            &mut shp_files,
            &new_objects,
            asset_manager,
            rules,
            art_reg,
            theater_name,
            picture_routes.as_mut(),
        );
    }

    let mut building_type_bounds = previous_atlas
        .map(|atlas| atlas.building_type_bounds.clone())
        .unwrap_or_default();
    // VERA-only, experimental (docs/picture-model-art.md): the body, Buildup
    // and bib pictures the bounds below fold, decoded side by side.
    if let Some(routes) = picture_routes.as_ref() {
        let sources: Vec<PictureSource> = new_objects
            .iter()
            .filter(|(type_id, _)| !building_type_bounds.contains_key(type_id))
            .filter_map(|(type_id, _)| {
                let body = routes.object(type_id)?;
                let bib = routes
                    .art
                    .get(&body.section)
                    .and_then(|entry| entry.bib_shape.as_deref())
                    .and_then(|bib| routes.bibs.get(&bib.to_ascii_uppercase()))
                    .cloned();
                Some([Some(body), routes.buildup(type_id), bib])
            })
            .flatten()
            .flatten()
            .collect();
        pictures.prefetch(&sources, routes.art, asset_manager, theater_name);
    }
    for (type_id, _) in &new_objects {
        if building_type_bounds.contains_key(type_id) {
            continue;
        }
        let Some(object) = rules
            .and_then(|rules| rules.object(type_id))
            .filter(|object| {
                object.category == crate::rules::object_type::ObjectCategory::Building
            })
        else {
            continue;
        };
        // VERA-only, experimental (docs/picture-model-art.md): the same union
        // over the body, Buildup and bib pictures' drawn frame rectangles.
        if let Some(routes) = picture_routes.as_ref()
            && let Some(body) = routes.object(type_id)
        {
            let buildup = routes.buildup(type_id);
            let bib = routes
                .art
                .get(&body.section)
                .and_then(|entry| entry.bib_shape.as_deref())
                .and_then(|bib| routes.bibs.get(&bib.to_ascii_uppercase()));
            let bounds = pictures.building_bounds(
                routes.art,
                asset_manager,
                theater_name,
                &body,
                buildup.as_ref(),
                bib,
            );
            building_type_bounds.insert(type_id.clone(), bounds);
            continue;
        }
        // Reuse the body/bib bindings already chosen above and the registered
        // make filename; do not resolve another candidate or decode pixels.
        let Some(main) = object_sources.get(&(ShpImageKind::Object, type_id.clone())) else {
            continue;
        };
        let make_file = shp_files.get(&(
            ShpImageKind::Object,
            format!("{type_id}_MAKE").to_ascii_uppercase(),
        ));
        let make = make_file.and_then(|file| asset_manager.get_ref(file));
        if make_file.is_some() && make.is_none() {
            continue;
        }
        let bib = art
            .and_then(|art| art.resolve_metadata_entry(type_id, &object.image))
            .and_then(|entry| entry.bib_shape.as_ref())
            .and_then(|bib| object_sources.get(&(ShpImageKind::Object, bib.to_ascii_uppercase())))
            .and_then(Option::as_ref);
        match building_type_render_bounds(
            main.as_ref().map(|source| source.data.as_ref()),
            make,
            bib.map(|source| source.data.as_ref()),
        ) {
            Ok(bounds) => {
                building_type_bounds.insert(type_id.clone(), bounds);
            }
            Err(error) => log::warn!("Building bounds unavailable for {type_id}: {error}"),
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
                // VERA-only, experimental (docs/picture-model-art.md): a door
                // named beside a declared Picture= counts its picture's
                // frames; its SHP is never opened.
                let picture = picture_routes.as_mut().and_then(|routes| {
                    let source = routes.doors.get(&upper)?.clone();
                    Some(routes.frame_count(&source))
                });
                let count = match (object_shp_frame_counts.get(&upper), picture) {
                    (Some(&count), _) => count,
                    (None, Some(None)) => continue,
                    (None, Some(Some(count))) => {
                        object_shp_frame_counts.insert(upper.clone(), count);
                        count
                    }
                    (None, None) => {
                        let candidates = art_data::theater_shp_names(&upper, theater_name)
                            .map(|name| format!("{name}.SHP"));
                        let Some((file, data)) = find_shp(asset_manager, &candidates) else {
                            continue;
                        };
                        let Ok(count) = ShpFile::frame_count_from_bytes(&data) else {
                            continue;
                        };
                        object_shp_frame_counts.insert(upper.clone(), count);
                        shp_files.insert((ShpImageKind::Object, upper.clone()), file.to_string());
                        count
                    }
                };
                for frame in 0..count {
                    needed.insert(ShpSpriteKey {
                        palette_context,
                        type_id: upper.as_str().into(),
                        facing: 0,
                        frame,
                        house_color: *color,
                    });
                }
            }
        }
    }

    // Step 1c'': Firestorm's `[AudioVisual] WebbedInfantry=` image, every
    // frame, for each infantry type with a `Struggle=` record: Firestorm 2.0
    // `InfantryClass::Get_Image_Data` 0x004D9030 draws a struggling man from
    // it (docs/ts-webs.md). Absent without webs.
    if let Some(webbed) = rules.and_then(|rules| rules.general.webbed_infantry.as_deref()) {
        let upper = webbed.to_ascii_uppercase();
        for (type_id, color) in &new_objects {
            let struggles = rules
                .and_then(|rules| {
                    rules.animation_sequence_in_category(
                        crate::rules::object_type::ObjectCategory::Infantry,
                        type_id,
                    )
                })
                .and_then(|set| set.infantry_action(crate::rules::infantry_sequence::DO_STRUGGLE))
                .is_some();
            if !struggles {
                continue;
            }
            // VERA-only, experimental (docs/picture-model-art.md): a
            // declared picture counts its frames, and the SHP is not opened.
            let picture = object_shp_frame_counts
                .get(&upper)
                .is_none()
                .then(|| picture_routes.as_mut()?.anim_frames(webbed))
                .flatten();
            let count = match (object_shp_frame_counts.get(&upper), picture) {
                (Some(&count), _) => count,
                (None, Some(Some(count))) => {
                    object_shp_frame_counts.insert(upper.clone(), count);
                    count
                }
                (None, Some(None)) => break,
                (None, None) => {
                    let candidates =
                        effect_anim_shp_candidates(webbed, art, theater_ext, theater_name);
                    let Some((file, data)) = find_shp(asset_manager, &candidates) else {
                        break;
                    };
                    let Ok(count) = ShpFile::frame_count_from_bytes(&data) else {
                        break;
                    };
                    object_shp_frame_counts.insert(upper.clone(), count);
                    shp_files.insert((ShpImageKind::Object, upper.clone()), file.to_string());
                    count
                }
            };
            for frame in 0..count {
                needed.insert(ShpSpriteKey {
                    palette_context: ShpPaletteContext::Legacy,
                    type_id: upper.as_str().into(),
                    facing: 0,
                    frame,
                    house_color: *color,
                });
            }
        }
    }

    // Step 1d: Pre-load world effect SHPs (warp animations, explosions, etc.)
    // once, at map load. Names come from rules.ini [General]
    // WarpIn=/WarpOut=/WarpAway= — NOT hardcoded. These use anim.pal (effect
    // palette), not unit.pal — tracked in the effect registry so step 2 can
    // pick the correct palette. Only the SHP header is read here.
    let mut registered_effects = EffectRegistry::default();
    let effect_names: Vec<(String, ShpImageKind)> = match previous_atlas {
        None => rules
            .map(|rules| {
                let mut names: Vec<_> = collect_effect_names(rules)
                    .into_iter()
                    .map(|name| (name, ShpImageKind::Animation))
                    .collect();
                let particles: std::collections::BTreeSet<_> = rules
                    .particle_types_iter()
                    .map(|particle| particle.image.clone())
                    .collect();
                names.extend(
                    particles
                        .into_iter()
                        .map(|name| (name, ShpImageKind::Particle)),
                );
                names
            })
            .unwrap_or_default(),
        Some(_) => Vec::new(),
    };
    for (name, kind) in &effect_names {
        // Use the same resolved Image= and Theater=/NewTheater= filename
        // authority as scheduler binding. Stock cell-drawer rows such as
        // WA01X and TUNTOP01 are theater SHPs (`.TEM` on Temperate maps).
        let candidates = if *kind == ShpImageKind::Animation {
            effect_anim_shp_candidates(name, art, theater_ext, theater_name)
        } else {
            let image = art.map_or_else(
                || name.clone(),
                |art| art.resolve_effective_image_id(name, name),
            );
            art_data::effect_shp_candidates(
                art_data::EffectImageKind::Particle,
                art,
                name,
                &image,
                theater_ext,
                theater_name,
            )
        };
        let required_cell_drawer = *kind == ShpImageKind::Animation
            && cell_drawer_type_ids.contains(&name.to_ascii_uppercase());
        // VERA-only, experimental (docs/picture-model-art.md): a type that
        // declares Picture= counts the frames rules binding read from its
        // picture, and its SHP is never opened.
        let picture = art.is_some_and(|art| art.declares_picture(name));
        let found = if picture && *kind == ShpImageKind::Animation {
            art.and_then(|art| art.anim_runtime_config(name))
                .and_then(|config| config.raw_shp_frame_count)
                .and_then(|count| u16::try_from(count).ok())
                .filter(|&count| count > 0)
                .map(|count| (None, Ok(count)))
        } else if picture {
            // A particle image counts its picture's frames, as the effect
            // catalog does (`EffectAssetCatalog::bind`).
            picture_routes
                .as_mut()
                .and_then(|routes| routes.anim_frames(name).flatten())
                .map(|count| (None, Ok(count)))
        } else {
            find_shp(asset_manager, &candidates)
                .map(|(file, data)| (Some(file), ShpFile::frame_count_from_bytes(&data)))
        };
        match found {
            Some((file, counted)) => match counted {
                Ok(raw_count) => {
                    let name_upper = name.to_ascii_uppercase();
                    let scheduler_owned = *kind == ShpImageKind::Animation
                        && art.is_some_and(|registry| {
                            registry.scheduler_anim_types().contains(&name_upper)
                        });
                    let config = art
                        .filter(|_| *kind == ShpImageKind::Animation)
                        .and_then(|registry| registry.anim_runtime_config(name));
                    let shadow = config.is_some_and(|config| config.shadow);
                    // Keep body timing separate from shadow-frame residency.
                    let count = register_effect_anim_frames(
                        &mut needed,
                        &mut active_anim_frame_counts,
                        name,
                        raw_count,
                        scheduler_owned,
                        shadow,
                        if *kind == ShpImageKind::Animation {
                            anim_frame_context(config, ShpPaletteContext::GlobalAnim)
                        } else {
                            ShpPaletteContext::Particle
                        },
                    );
                    if *kind == ShpImageKind::Animation {
                        registered_effects
                            .frames
                            .insert(name_upper.clone(), (name.clone(), raw_count));
                        registered_effects.type_ids.insert(name.clone());
                    }
                    if let Some(file) = file {
                        shp_files.insert((*kind, name_upper), file.to_string());
                    }
                    log::debug!("Effect anim {}: {} frames from {:?}", name, count, file);
                }
                Err(error) if required_cell_drawer => {
                    return PreparedSpriteAtlas::Abandoned(format!(
                        "bound cell-drawer animation [{name}] failed SHP decode: {error}"
                    ));
                }
                Err(_) => {}
            },
            None if required_cell_drawer => {
                return PreparedSpriteAtlas::Abandoned(format!(
                    "bound cell-drawer animation [{name}] has no SHP asset"
                ));
            }
            None => {}
        }
    }

    // Step 1e: Pre-load the General canopy fallbacks. NPSE Paradrop
    // C2FA75/C2F939 substitutes VehicleParachute for non-infantry.
    // This registers the no-remap GlobalAnim/colour-0 fallback. The live
    // AnimClass remap closure adds SelectedScheme variants for remapped canopies;
    // both use the unit palette here because PARACH sets `AltPalette=`.
    // Configured Ares canopies enter Step 1d through the bound animation roots.
    // That owner supplies their Image=/Theater=/Shadow= bindings.
    let mut parachutes: Vec<&str> = Vec::new();
    if let Some(rules) = rules.filter(|_| previous_atlas.is_none()) {
        parachutes.extend(rules.general.parachute_shp.as_deref());
        if rules.npatch() {
            parachutes.extend(rules.general.vehicle_parachute.as_deref());
        }
    }
    for shp_name in parachutes {
        let candidates = [
            format!("{}.shp", shp_name.to_ascii_lowercase()),
            format!("{}.SHP", shp_name),
        ];
        // VERA-only, experimental (docs/picture-model-art.md): an AnimType
        // that declares Picture= counts its picture's frames and draws from
        // it; its SHP is not opened.
        let found = match picture_routes
            .as_mut()
            .and_then(|routes| routes.anim_frames(shp_name))
        {
            Some(Some(count)) => Some((None, Ok(count))),
            Some(None) => {
                log::warn!("Parachute picture {shp_name} has no frames; chute will not render");
                continue;
            }
            None => find_shp(asset_manager, &candidates)
                .map(|(file, data)| (Some(file), ShpFile::frame_count_from_bytes(&data))),
        };
        match found {
            Some((file, counted)) => match counted {
                Ok(frame_count) => {
                    for frame in 0..frame_count {
                        needed.insert(ShpSpriteKey {
                            palette_context: ShpPaletteContext::GlobalAnim,
                            type_id: shp_name.into(),
                            facing: 0,
                            frame,
                            house_color: HouseColorIndex(0),
                        });
                    }
                    active_anim_frame_counts.insert(shp_name.to_string(), frame_count);
                    if let Some(file) = file {
                        shp_files.insert(
                            (ShpImageKind::Animation, shp_name.to_ascii_uppercase()),
                            file.to_string(),
                        );
                    }
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
            .and_then(|(file, data)| Some((file, ShpFile::frame_count_from_bytes(&data).ok()?)))
        {
            Some((file, frame_count)) if frame_count > INFANTRY_AIR_SHADOW_FRAME => {
                needed.insert(ShpSpriteKey {
                    palette_context: ShpPaletteContext::GlobalAnim,
                    type_id: INFANTRY_AIR_SHADOW_SHP.into(),
                    facing: 0,
                    frame: INFANTRY_AIR_SHADOW_FRAME,
                    house_color: HouseColorIndex(0),
                });
                shp_files.insert(
                    (ShpImageKind::Animation, INFANTRY_AIR_SHADOW_SHP.to_string()),
                    file.to_string(),
                );
            }
            _ => log::warn!("INFSHDW.SHP frame 1 missing: airborne infantry draw no shadow"),
        }
    }

    // Rules and theater bindings are fixed for this atlas's lifetime; a
    // refresh retains its registered files and resident custom gather frames.
    #[cfg(feature = "phobos")]
    if previous_atlas.is_none()
        && let Some(rules) = rules
    {
        register_custom_harvest_frames(
            &mut needed,
            &mut shp_files,
            asset_manager,
            rules,
            art,
            theater_ext,
            theater_name,
            picture_routes.as_mut(),
        );
    }

    let effects = previous_atlas.map_or(&registered_effects, |atlas| &atlas.effects);
    insert_anim_remap_frame_keys(&mut needed, effects, &new_remaps);

    let mut contextual = Vec::new();
    for key in &needed {
        if key.palette_context == ShpPaletteContext::GlobalAnim
            && effects.type_ids.contains(key.type_id.as_str())
        {
            let mut global = key.clone();
            global.house_color = HouseColorIndex(0);
            contextual.push(global.clone());
            if cell_drawer_type_ids.contains(&key.type_id.to_ascii_uppercase()) {
                global.palette_context = ShpPaletteContext::AnimCell;
                contextual.push(global);
            }
        }
    }
    needed.extend(contextual);

    if needed.is_empty() && previous_atlas.is_none() {
        log::info!("No SHP sprite entities found");
        return PreparedSpriteAtlas::Unchanged;
    }

    // Step 2: the keys to render — every one at map load, afterwards only those
    // the atlas neither holds nor has found unrenderable. Sorting groups each
    // type's keys so its SHP is decoded whole once.
    let resolved = |key: &ShpSpriteKey| {
        previous_atlas.is_some_and(|atlas| {
            atlas.entries.contains_key(key) || atlas.unrenderable.contains(key)
        })
    };
    let mut new_keys: Vec<ShpSpriteKey> = needed
        .iter()
        .filter(|key| !resolved(key))
        .cloned()
        .collect();
    new_keys.sort_unstable_by(|a, b| sprite_key_order(a).cmp(&sprite_key_order(b)));
    // VERA-only, experimental (docs/picture-model-art.md): the picture each
    // new key is drawn from, if any; such a key reads no SHP. Its palette
    // choice still decides its house remap (`key_house_remap`).
    let key_pictures: Vec<Option<PictureSource>> =
        picture_routes.as_ref().map_or_else(Vec::new, |routes| {
            new_keys.iter().map(|key| routes.key(key)).collect()
        });
    let picture_of = |index: usize| key_pictures.get(index).and_then(Option::as_ref);
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
    let mut ares_palette_files: HashMap<String, Option<Palette>> = HashMap::new();
    let mut ares_palettes = AresCustomPalettes::default();
    for key in &new_keys {
        ares_palettes.register(key, rules, art, theater_ext, &mut |file| {
            ares_palette_files
                .entry(file.to_owned())
                .or_insert_with(|| {
                    asset_manager
                        .get_ref(file)
                        .and_then(|data| Palette::from_bytes(data).ok())
                })
                .is_some()
        });
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
                &ares_palettes,
            )
        })
        .collect();
    if cell_palette.is_none() && palette_choices.contains(&SpritePaletteChoice::CellIso) {
        return PreparedSpriteAtlas::Abandoned(
            "terrain-attached animations require the active theater ISO palette".to_string(),
        );
    }
    if bullet_palette.is_none() && palette_choices.contains(&SpritePaletteChoice::Bullet) {
        return PreparedSpriteAtlas::Abandoned(
            "visible projectiles require PALETTE.PAL".to_string(),
        );
    }
    if effect_palette.is_none()
        && new_keys.iter().zip(&palette_choices).any(|(key, choice)| {
            *choice != SpritePaletteChoice::AresCustom
                && matches!(
                    key.palette_context,
                    ShpPaletteContext::BulletAnim
                        | ShpPaletteContext::AnimPalette
                        | ShpPaletteContext::Harvest
                )
        })
    {
        return PreparedSpriteAtlas::Abandoned(
            "literal animation palettes require ANIM.PAL".to_string(),
        );
    }

    // What the raster needs for each key, decided here, so its frames can be
    // drawn without the simulation, Rules or the asset manager.
    let mut raster = ShpRasterWork {
        keys: Vec::with_capacity(new_keys.len()),
        sources: Vec::new(),
        render_palettes: Vec::new(),
        // A refresh registers rows in a copy that is installed only with its
        // sprites, so an abandoned refresh leaves the atlas as it was.
        rows: previous_atlas
            .map(|atlas| atlas.palettes.registry())
            .unwrap_or_default(),
        harvest_overlay: None,
        max_rows: max_texture_dimension_2d,
    };
    let mut render_palette_rows: HashMap<
        (SpritePaletteChoice, &str, Option<HouseColorIndex>),
        usize,
    > = HashMap::new();
    let mut prepared_pictures = crate::render::picture_atlas::PreparedPictures::default();
    let mut picture_unrenderable = Vec::new();
    // VERA-only, experimental (docs/picture-model-art.md): each picture key's
    // frame, one the atlas holds or one drawn here, once for every key that
    // draws it; the key's own house colour, offsets and Z shape are its
    // entry's. Its frame is chosen from its picture's frame count as
    // render_shp_frame chooses an SHP's.
    if let (Some(art), Some(routes)) = (art, picture_routes.as_mut()) {
        use crate::render::picture_atlas::{HouseColor, PictureFrameId, PictureKey};
        let mut new_frames: Vec<(PictureFrameId, &PictureSource)> = Vec::new();
        let mut new_frame_ids = HashSet::new();
        for (index, key) in new_keys.iter().enumerate() {
            let Some(source) = picture_of(index) else {
                continue;
            };
            let house = (key_house_remap(key, palette_choices[index])
                && key.house_color != crate::rules::house_colors::NO_REMAP)
                .then_some(key.house_color);
            let infantry = shp_key_is_infantry(key, rules);
            let frame = crate::render::picture_atlas::picture_file(
                source,
                art,
                asset_manager,
                theater_name,
            )
            .and_then(|file| {
                let count = routes.frame_count(source)?;
                Some(PictureFrameId {
                    file,
                    size: source.decl.size,
                    frame: shp_frame_index(key, usize::from(count), infantry)?,
                    deferred: house.is_some(),
                })
            });
            let Some(frame) = frame else {
                picture_unrenderable.push(key.clone());
                continue;
            };
            match previous_atlas.and_then(|atlas| atlas.picture_frames.get(&frame)) {
                Some(None) => {
                    picture_unrenderable.push(key.clone());
                    continue;
                }
                Some(Some(_)) => {}
                None => {
                    if new_frame_ids.insert(frame.clone()) {
                        new_frames.push((frame.clone(), source));
                    }
                }
            }
            let kind = key.palette_context.image_kind();
            let draw_offsets = if kind == ShpImageKind::Projectile
                || key.palette_context == ShpPaletteContext::Harvest
            {
                (0, 0)
            } else {
                art_draw_offsets(Some(art), &key.type_id, kind)
            };
            prepared_pictures.keys.push(PictureKey {
                key: key.clone(),
                frame,
                draw_offsets,
                // Retail building bodies and Buildup frames are stored with
                // the extended format bit, so BUILDNGZ shapes their Z
                // (`0x00490B90`, `0x0043D290`); a building's pictures keep
                // that Z shape (docs/picture-model-art.md).
                extended: shp_key_is_building_body(key, rules),
                house,
            });
        }
        // The new frames' pictures, decoded side by side, then the frames
        // drawn side by side.
        pictures.prefetch(
            new_frames.iter().map(|(_, source)| *source),
            art,
            asset_manager,
            theater_name,
        );
        let drawn = crate::sim::parallel::map(&new_frames, |(frame, source)| {
            let house = if frame.deferred {
                HouseColor::Deferred
            } else {
                HouseColor::Own
            };
            pictures.frame(source, frame.frame, house)
        });
        // VERA-only, experimental (docs/zoom-one-pass.md): what streams each
        // new frame's file at its own resolution, for a camera zoomed in.
        let mut hd_files = HashSet::new();
        for (frame, source) in &new_frames {
            let file = (frame.file.clone(), frame.size);
            if hd_files.insert(file.clone())
                && let Some(hd) = pictures.hd_source(source)
            {
                prepared_pictures.hd_sources.push((file, hd));
            }
        }
        let mut empty = HashSet::new();
        for ((id, _), drawn) in new_frames.into_iter().zip(drawn) {
            match drawn {
                Some(drawn) => prepared_pictures.frames.push((id, drawn)),
                None => {
                    empty.insert(id);
                }
            }
        }
        prepared_pictures.keys.retain(|key| {
            let drawn = !empty.contains(&key.frame);
            if !drawn {
                picture_unrenderable.push(key.key.clone());
            }
            drawn
        });
        prepared_pictures.empty = empty.into_iter().collect();
        if !prepared_pictures.frames.is_empty() {
            prepared_pictures.ramps = crate::render::picture_atlas::ramp_strip(
                rules.map_or(&HouseColorRamps::default(), |rules| {
                    &rules.house_color_ramps
                }),
            );
        }
    }
    // The decoded pictures are done with; freeing a map's worth of them takes
    // a while, so a worker does it.
    if prepared_pictures.frames.is_empty() {
        drop(pictures);
    } else {
        crate::sim::parallel::drop_on_pool(pictures);
    }
    for (index, (key, palette_choice)) in new_keys.into_iter().zip(palette_choices).enumerate() {
        // VERA-only, experimental (docs/picture-model-art.md): a key whose
        // art declares Picture= draws from its picture, never from an SHP.
        if art.is_some() && picture_of(index).is_some() {
            continue;
        }
        let projectile = key.palette_context.is_projectile();
        let kind = key.palette_context.image_kind();
        let same_source = raster.keys.last().is_some_and(|last| {
            last.key.type_id == key.type_id && last.key.palette_context.image_kind() == kind
        });
        if !same_source {
            let file = if projectile {
                projectile_files
                    .get(key.type_id.as_str())
                    .map(|(file, _)| file.as_str())
            } else {
                shp_files
                    .get(&(kind, key.type_id.to_ascii_uppercase()))
                    .map(String::as_str)
            };
            // Object key collection already decoded this exact source for
            // Infantry admission or Structure/Unit shadow-frame registration.
            // Reuse it instead of decoding the same file a second time.
            let mut source = match object_sources.remove(&(kind, key.type_id.to_string())) {
                Some(decoded) => decoded.map(ShpRasterSource::Decoded),
                None => find_shp_source(
                    asset_manager,
                    &key.type_id,
                    file,
                    theater_ext,
                    theater_name,
                    rules,
                    art,
                    kind,
                )
                .map(ShpRasterSource::Found),
            };
            if projectile && let Some(source) = &mut source {
                // BulletDraw468090 has no AnimType/Techno XDrawOffset consumer.
                source.clear_draw_offsets();
            }
            raster.sources.push(source);
        }
        let (pal, extra_name): (&Palette, &str) = match palette_choice {
            SpritePaletteChoice::Anim => (effect_palette.as_ref().unwrap_or(palette), ""),
            SpritePaletteChoice::Unit => (palette, ""),
            SpritePaletteChoice::CellIso => (
                cell_palette.expect("cell-drawer palette was validated before atlas rendering"),
                "",
            ),
            SpritePaletteChoice::Bullet => (
                bullet_palette.as_ref().expect("bullet palette validated"),
                "",
            ),
            SpritePaletteChoice::ExtraBuilding => {
                let name = match key.palette_context {
                    ShpPaletteContext::ExtraScheme(palette) => interner
                        .expect("custom animation palette was resolved through the interner")
                        .resolve(palette),
                    _ => extra_palette_names[key.type_id.to_ascii_uppercase().as_str()],
                };
                let palette = extra_palettes[name]
                    .as_ref()
                    .expect("chosen only when loaded");
                (palette, name)
            }
            SpritePaletteChoice::AresCustom => {
                let name = ares_palettes
                    .file(&key)
                    .expect("chosen only when registered");
                let palette = ares_palette_files[name]
                    .as_ref()
                    .expect("registered only when loaded");
                (palette, name)
            }
        };
        let house_remap = key_house_remap(&key, palette_choice);
        let remap = (house_remap && key.house_color != crate::rules::house_colors::NO_REMAP)
            .then_some(key.house_color);
        let palette = *render_palette_rows
            .entry((palette_choice, extra_name, remap))
            .or_insert_with(|| {
                raster
                    .render_palettes
                    .push(shp_render_palette(pal, house_remap, &key, rules).into_owned());
                raster.render_palettes.len() - 1
            });
        raster.keys.push(ShpRasterKey {
            infantry: shp_key_is_infantry(&key, rules),
            required: cell_drawer_type_ids.contains(&key.type_id.to_ascii_uppercase()),
            source: raster.sources.len() - 1,
            palette,
            key,
        });
    }

    // Step 2b: Render the harvest overlay frames once harvesters exist. Uses
    // anim.pal (effect palette) — no house color remap.
    let load_harvest_overlay = !previous_atlas.is_some_and(|atlas| atlas.harvest_overlay_loaded)
        && entities.values().any(|e| e.miner.is_some());
    if load_harvest_overlay {
        raster.harvest_overlay = harvest_overlay_source(asset_manager);
    }

    PreparedSpriteAtlas::Raster(
        raster,
        ShpAtlasUpload {
            started,
            initial: previous_atlas.is_none(),
            registered_effects,
            make_frame_counts,
            active_anim_frame_counts,
            object_shp_frame_counts,
            building_type_bounds,
            shp_files,
            projectile_files,
            new_objects,
            new_remaps,
            load_harvest_overlay,
            pictures: prepared_pictures,
            picture_unrenderable,
        },
    )
}

/// Whether `key` draws a building's body or its Buildup (`TYPE_MAKE`), the
/// frames the extended walker shapes with BUILDNGZ.
fn shp_key_is_building_body(key: &ShpSpriteKey, rules: Option<&RuleSet>) -> bool {
    let type_id = key.type_id.strip_suffix("_MAKE").unwrap_or(&key.type_id);
    key.palette_context.image_kind() == ShpImageKind::Object
        && rules
            .and_then(|rules| rules.object(type_id))
            .is_some_and(|object| {
                object.category == crate::rules::object_type::ObjectCategory::Building
            })
}

/// Whether `key`'s frames take its house colour through `palette_choice`.
/// 0x00627590 builds each scheme with ColorScheme's constructor (0x0068C710),
/// which writes the colour's ramp over indices 16..31.
fn key_house_remap(key: &ShpSpriteKey, palette_choice: SpritePaletteChoice) -> bool {
    match key.palette_context {
        ShpPaletteContext::Legacy => palette_choice != SpritePaletteChoice::CellIso,
        _ => matches!(
            palette_choice,
            SpritePaletteChoice::Unit | SpritePaletteChoice::ExtraBuilding
        ),
    }
}

/// Prepared draw metadata owns shared physical source files. It borrows no
/// simulation, Rules or process AssetManager; the loading pump can retain it
/// across frames while those existing owners stay in their original phase,
/// and live growth draws its frames on a worker.
pub(crate) enum PreparedSpriteAtlas {
    /// Nothing to draw: no sprite at map load.
    Unchanged,
    /// A required asset or palette is missing, before any frame was drawn:
    /// a map load fails, a refresh keeps its atlas.
    Abandoned(String),
    /// The frames to draw and the bookkeeping published with them.
    Raster(ShpRasterWork, ShpAtlasUpload),
}

impl PreparedSpriteAtlas {
    /// Draw the frames on this thread and complete a map-load build. An
    /// abandoned build is the load's error.
    pub(crate) fn finish(
        self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
    ) -> anyhow::Result<Option<SpriteAtlas>> {
        let (work, upload) = match self {
            Self::Unchanged => return Ok(None),
            Self::Abandoned(failure) => return Err(sprite_atlas_load_error(failure)),
            Self::Raster(work, upload) => (work, upload),
        };
        let drawn = upload
            .drawn(work.render())
            .map_err(sprite_atlas_load_error)?;
        let mut atlas = None;
        upload.finish_drawn(device, queue, batch, drawn, &mut atlas);
        Ok(atlas)
    }
}

/// One key's frame, with everything Rules, ART and the asset manager decided
/// for it.
struct ShpRasterKey {
    key: ShpSpriteKey,
    /// Its type's SHP in [`ShpRasterWork::sources`]; a type's keys are
    /// consecutive and share one.
    source: usize,
    /// The palette its frame resolves through ([`shp_render_palette`]), in
    /// [`ShpRasterWork::render_palettes`].
    palette: usize,
    infantry: bool,
    /// A bound cell-drawer animation: the refresh is abandoned when its frame
    /// cannot be drawn.
    required: bool,
}

/// A type's SHP for the raster: decoded already by key collection, or found
/// and decoded by the raster. None when it is missing or failed to decode.
enum ShpRasterSource {
    Decoded(ShpSource),
    Found(FoundShp),
}

impl ShpRasterSource {
    fn clear_draw_offsets(&mut self) {
        match self {
            Self::Decoded(source) => source.draw_offsets = (0, 0),
            Self::Found(found) => found.draw_offsets = (0, 0),
        }
    }

    fn decode(self) -> Option<ShpSource> {
        match self {
            Self::Decoded(source) => Some(source),
            Self::Found(found) => found.decode(),
        }
    }
}

/// The frames of a build or refresh to draw: decoding each type's SHP whole
/// once and registering the palette rows their indices resolve through. It
/// owns shared source files and plain data only, no simulation, Rules,
/// asset manager or GPU handle, so live growth runs it on a worker.
pub(crate) struct ShpRasterWork {
    keys: Vec<ShpRasterKey>,
    sources: Vec<Option<ShpRasterSource>>,
    render_palettes: Vec<Palette>,
    /// The atlas's row registry, or a new one, to register rows in.
    rows: ShpPalettes,
    /// `anim.pal` and `oregath.shp` when this build draws the harvest overlay.
    harvest_overlay: Option<(Palette, SharedBytes)>,
    /// The device's 2D texture limit, which bounds the palette rows.
    max_rows: u32,
}

/// What [`ShpRasterWork`] drew, or why its refresh must be abandoned.
pub(crate) struct ShpRasterOutput(ShpRaster);

/// A raster's drawn frames, its unrenderable keys and its palette rows.
type DrawnShpSprites = (Vec<RenderedShpSprite>, Vec<ShpSpriteKey>, ShpPalettes);

enum ShpRaster {
    Drawn {
        rendered: Vec<RenderedShpSprite>,
        unrenderable: Vec<ShpSpriteKey>,
        rows: ShpPalettes,
    },
    Abandoned(String),
}

impl ShpRasterWork {
    pub(crate) fn render(self) -> ShpRasterOutput {
        let Self {
            keys,
            mut sources,
            render_palettes,
            mut rows,
            harvest_overlay,
            max_rows,
        } = self;
        let mut rendered: Vec<RenderedShpSprite> = Vec::with_capacity(keys.len());
        let mut unrenderable: Vec<ShpSpriteKey> = Vec::new();
        // Only the current type's decoded file is held.
        let mut decoded: Option<(usize, Option<ShpSource>)> = None;
        for raster in keys {
            if decoded
                .as_ref()
                .is_none_or(|(source, _)| *source != raster.source)
            {
                let source = sources[raster.source]
                    .take()
                    .and_then(ShpRasterSource::decode);
                decoded = Some((raster.source, source));
            }
            let sprite = decoded
                .as_ref()
                .and_then(|(_, source)| source.as_ref())
                .and_then(|source| {
                    render_shp_frame(
                        source,
                        &render_palettes[raster.palette],
                        &raster.key,
                        raster.infantry,
                        &mut rows,
                    )
                });
            match sprite {
                Some(sprite) => rendered.push(sprite),
                None if raster.required => {
                    return ShpRasterOutput(ShpRaster::Abandoned(format!(
                        "bound cell-drawer animation [{}] frame {} failed SHP rendering",
                        raster.key.type_id, raster.key.frame
                    )));
                }
                None => unrenderable.push(raster.key),
            }
        }

        if let Some((palette, data)) = harvest_overlay {
            let sprites = render_harvest_overlay_frames(&palette, data, &mut rows);
            if !sprites.is_empty() {
                log::info!("Rendered {} harvest overlay frames", sprites.len());
                rendered.extend(sprites);
            }
        }

        if rows.colors.len() > max_rows as usize {
            return ShpRasterOutput(ShpRaster::Abandoned(format!(
                "SHP atlas requires {} palette rows; device supports {}",
                rows.colors.len(),
                max_rows,
            )));
        }
        ShpRasterOutput(ShpRaster::Drawn {
            rendered,
            unrenderable,
            rows,
        })
    }

    /// Draw on a worker thread, which sends the output once and ends; the
    /// receiver may be dropped at any time.
    pub(crate) fn spawn(self) -> std::io::Result<std::sync::mpsc::Receiver<ShpRasterOutput>> {
        let (sender, receiver) = std::sync::mpsc::channel();
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        {
            let _ = sender.send(self.render());
        }
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        std::thread::Builder::new()
            .name("shp-atlas-raster".into())
            .spawn(move || {
                let _ = sender.send(self.render());
            })?;
        Ok(receiver)
    }
}

/// The bookkeeping a build or refresh publishes with its frames.
pub(crate) struct ShpAtlasUpload {
    started: Instant,
    /// Prepared without an atlas: a map-load build, which publishes the
    /// effect registry.
    initial: bool,
    registered_effects: EffectRegistry,
    make_frame_counts: HashMap<String, u16>,
    active_anim_frame_counts: HashMap<String, u16>,
    object_shp_frame_counts: HashMap<String, u16>,
    building_type_bounds: HashMap<String, [i32; 4]>,
    shp_files: HashMap<(ShpImageKind, String), String>,
    projectile_files: HashMap<String, (String, u16)>,
    new_objects: HashSet<(String, HouseColorIndex)>,
    new_remaps: HashSet<AnimRemapKey>,
    load_harvest_overlay: bool,
    /// VERA-only, experimental: the frames drawn from declared pictures
    /// ([`SpriteAtlas::append_pictures`]), and their keys with nothing to draw.
    pictures: crate::render::picture_atlas::PreparedPictures,
    picture_unrenderable: Vec<ShpSpriteKey>,
}

impl ShpAtlasUpload {
    /// Main-thread completion shared by map load and live growth: append the
    /// drawn frames to `atlas`, or pack a new one when there is none, and
    /// publish their bookkeeping. `atlas` must be the one this upload was
    /// prepared against. Whether it was published; an abandoned refresh
    /// leaves `atlas` as it was.
    pub(crate) fn finish(
        self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        output: ShpRasterOutput,
        atlas: &mut Option<SpriteAtlas>,
    ) -> bool {
        match self.drawn(output) {
            Ok(drawn) => self.finish_drawn(device, queue, batch, drawn, atlas),
            Err(failure) => {
                keep_previous_sprite_atlas(&failure);
                false
            }
        }
    }

    /// [`Self::finish`] once the frames are drawn.
    fn finish_drawn(
        self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        (rendered, unrenderable, rows): DrawnShpSprites,
        atlas: &mut Option<SpriteAtlas>,
    ) -> bool {
        let mut upload = self;
        let pictures = upload.take_pictures();
        let added = rendered.len() + pictures.keys.len();
        let atlas = match atlas {
            Some(atlas) => {
                atlas.append_sprites(device, queue, batch, rendered, rows);
                atlas
            }
            // Picture pages alone make an atlas too (an art pack that
            // replaces every SHP the world draws).
            None if rendered.is_empty() && pictures.keys.is_empty() => {
                log::warn!("No SHP sprites rendered");
                return false;
            }
            None => atlas.insert(pack_sprites(device, queue, batch, rendered, rows)),
        };
        atlas.append_pictures(device, queue, batch, pictures);
        upload.publish(atlas, added, unrenderable);
        true
    }

    /// The frames drawn from declared pictures, for
    /// [`SpriteAtlas::append_pictures`].
    pub(crate) fn take_pictures(&mut self) -> crate::render::picture_atlas::PreparedPictures {
        std::mem::take(&mut self.pictures)
    }

    /// The drawn frames, unrenderable keys and palette rows, or why the
    /// raster abandoned them.
    fn drawn(&self, output: ShpRasterOutput) -> Result<DrawnShpSprites, String> {
        match output.0 {
            ShpRaster::Drawn {
                rendered,
                unrenderable,
                rows,
            } => Ok((rendered, unrenderable, rows)),
            ShpRaster::Abandoned(failure) => Err(failure),
        }
    }

    /// Publish only a complete incoming atlas; synchronous growth and loading
    /// jobs share all coverage, source identities and failure bookkeeping.
    fn publish(self, atlas: &mut SpriteAtlas, added: usize, unrenderable: Vec<ShpSpriteKey>) {
        let Self {
            started,
            initial,
            registered_effects,
            make_frame_counts,
            active_anim_frame_counts,
            object_shp_frame_counts,
            building_type_bounds,
            shp_files,
            projectile_files,
            new_objects,
            new_remaps,
            load_harvest_overlay,
            pictures: _,
            picture_unrenderable,
        } = self;
        if initial {
            atlas.effects = registered_effects;
        }
        atlas.unrenderable.extend(unrenderable);
        atlas.unrenderable.extend(picture_unrenderable);
        atlas.make_frame_counts = make_frame_counts;
        atlas.active_anim_frame_counts = active_anim_frame_counts;
        atlas.object_shp_frame_counts = object_shp_frame_counts;
        atlas.building_type_bounds = building_type_bounds;
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
    }
}

/// BuildingTypeClass::GetRenderDimensions465570, original gamemd instructions:
/// fold all body frames, all buildup frames, then bib frame0, in that order.
/// SHPStruct::GetFrameBounds69E7E0 reads signed header rectangles; center
/// only by the main canvas.
/// Shared ordered union retains the native strict-maximum-edge +1 behavior.
fn building_type_render_bounds(
    main: Option<&[u8]>,
    buildup: Option<&[u8]>,
    bib: Option<&[u8]>,
) -> Result<[i32; 4], crate::assets::error::AssetError> {
    let Some(main) = main else {
        return Ok([0; 4]);
    };
    let canvas = ShpFile::canvas_size_from_bytes(main)?.map(|word| i32::from(word as i16));
    let mut accumulated = None;
    for source in [Some(main), buildup].into_iter().flatten() {
        let count = ShpFile::frame_count_from_bytes(source)? as i16;
        for frame in 0..count.max(0) {
            crate::util::rect::union_native_draw_bounds(
                &mut accumulated,
                ShpFile::frame_bounds_from_bytes(source, frame as usize)?,
            );
        }
    }
    if let Some(bib) = bib {
        crate::util::rect::union_native_draw_bounds(
            &mut accumulated,
            ShpFile::frame_bounds_from_bytes(bib, 0)?,
        );
    }
    let mut bounds = accumulated.unwrap_or([0; 4]);
    bounds[0] -= canvas[0] / 2;
    bounds[1] -= canvas[1] / 2;
    Ok(bounds)
}

/// One type's decoded SHP and the draw offsets every key of it shares.
struct ShpSource {
    shp: ShpFile,
    /// The file `shp` was decoded from, which its sprites decode frames from.
    data: SharedBytes,
    /// Once per decoded source, shared by every palette/context frame key.
    source_digest: [u8; 32],
    found_name: String,
    /// DrawOffset from art.ini XDrawOffset/YDrawOffset for per-type fine-tuning.
    draw_offsets: (i32, i32),
}

impl ShpSource {
    /// Decode `data` whole, keeping the bytes its sprites decode frames from.
    fn decode(
        data: SharedBytes,
        found_name: String,
        draw_offsets: (i32, i32),
    ) -> Result<Self, crate::assets::error::AssetError> {
        let shp = ShpFile::from_bytes(&data)?;
        let mut digest = crate::util::sha256::Sha256::new();
        digest.update(&data);
        Ok(Self {
            shp,
            data,
            source_digest: digest.finalize(),
            found_name,
            draw_offsets,
        })
    }
}

/// Resolve and decode the SHP a type's keys are drawn from
/// ([`find_shp_source`], then [`FoundShp::decode`]).
fn load_shp_source(
    asset_manager: &AssetManager,
    type_id: &str,
    file: Option<&str>,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
    kind: ShpImageKind,
) -> Option<ShpSource> {
    find_shp_source(
        asset_manager,
        type_id,
        file,
        theater_ext,
        theater_name,
        rules,
        art,
        kind,
    )?
    .decode()
}

/// A type's resolved SHP file and draw offsets, before it is decoded. It
/// holds a share of the file, not the asset manager, so it can be decoded on
/// another thread.
struct FoundShp {
    data: SharedBytes,
    name: String,
    draw_offsets: (i32, i32),
}

impl FoundShp {
    /// Decode the file whole ([`ShpSource::decode`]); None when it fails.
    fn decode(self) -> Option<ShpSource> {
        let source = match ShpSource::decode(self.data, self.name.clone(), self.draw_offsets) {
            Ok(source) => source,
            Err(e) => {
                log::warn!("Failed to parse {}: {}", self.name, e);
                return None;
            }
        };
        let shp = &source.shp;

        if shp.frames.is_empty() {
            log::warn!("{} has no frames", source.found_name);
        }

        log::debug!(
            "{}: {}x{}, {} frames",
            source.found_name,
            shp.width,
            shp.height,
            shp.frames.len()
        );
        Some(source)
    }
}

/// Resolve the SHP a type's keys are drawn from, without decoding it.
///
/// `file` is the file registration counted the type's frames in (animation,
/// make and parachute SHPs), so frames come from the file they were counted
/// in. Building bodies resolve through
/// [`art_data::building_body_shp_candidates`]; other bodies keep the object
/// load requests retained by the Rules/type-load owner. Explicit raw-image
/// callers retain the legacy object naming path.
fn find_shp_source(
    asset_manager: &AssetManager,
    type_id: &str,
    file: Option<&str>,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
    kind: ShpImageKind,
) -> Option<FoundShp> {
    let candidates: Vec<String> = match file {
        Some(file) => vec![file.to_string()],
        None if kind == ShpImageKind::Animation => {
            // AnimTypeLoad428855 and the reached theater postpass own this
            // shape independently of a same-named Techno's Image.
            effect_anim_shp_candidates(type_id, art, theater_ext, theater_name)
        }
        None if kind == ShpImageKind::Particle => {
            let image = art.map_or_else(
                || type_id.to_owned(),
                |art| art.resolve_effective_image_id(type_id, type_id),
            );
            art_data::effect_shp_candidates(
                art_data::EffectImageKind::Particle,
                art,
                type_id,
                &image,
                theater_ext,
                theater_name,
            )
        }
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
            } else if let Some(object) = object {
                // ObjectTypeLoadSHP5F9070 and TechnoRead715820 retain the
                // reached request order; current Image or a fresh ART redirect
                // cannot replace it or manufacture another fallback.
                object
                    .shp_image_loads()
                    .iter()
                    .flat_map(|load| load.shp_names(theater_ext, theater_name))
                    .collect()
            } else if art.is_some_and(|art| {
                art.iter_entries().any(|(_, entry)| {
                    entry
                        .bib_shape
                        .as_deref()
                        .is_some_and(|bib| bib.eq_ignore_ascii_case(base_type_id))
                })
            }) {
                // BuildingTypeClass::LoadVisualAssets @ 0x0045F856..0x0045F922
                // loads BibShape through 5F96B0, then 5F9710 on a miss. The
                // raw bib name needs no ART entry or NewTheater flag of its own.
                art_data::building_shp_names(base_type_id, theater_name).to_vec()
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
    Some(FoundShp {
        data: shp_data,
        name: found_name.to_string(),
        draw_offsets: art_draw_offsets(art, type_id, kind),
    })
}

/// The art XDrawOffset/YDrawOffset a type's `kind` image is drawn at.
fn art_draw_offsets(art: Option<&ArtRegistry>, type_id: &str, kind: ShpImageKind) -> (i32, i32) {
    if kind == ShpImageKind::Animation
        && art
            .and_then(|art| art.anim_runtime_config(type_id))
            .is_some_and(|config| !config.art_body_read)
    {
        // AnimTypeLoad428855 does not read orphan ART offsets. Same-name
        // Techno/Anim atlas identity collisions remain a separate key boundary.
        (0, 0)
    } else {
        art.map(|a| a.draw_offsets(type_id)).unwrap_or((0, 0))
    }
}

/// Load and render a single SHP sprite, with the RGBA its indices resolve to.
#[cfg(test)]
fn render_shp_sprite(
    asset_manager: &AssetManager,
    palette: &Palette,
    house_remap: bool,
    key: &ShpSpriteKey,
    theater_ext: &str,
    theater_name: &str,
    rules: Option<&RuleSet>,
    art: Option<&ArtRegistry>,
) -> Option<(RenderedShpSprite, Vec<u8>)> {
    let source = load_shp_source(
        asset_manager,
        &key.type_id,
        None,
        theater_ext,
        theater_name,
        rules,
        art,
        key.palette_context.image_kind(),
    )?;
    let mut palettes = ShpPalettes::default();
    let sprite = render_shp_key(&source, palette, house_remap, key, rules, &mut palettes)?;
    let rgba = palettes.rgba(&sprite);
    Some((sprite, rgba))
}

/// [`render_shp_frame`] through the palette and Infantry choices key
/// collection makes for `key`.
#[cfg(test)]
fn render_shp_key(
    source: &ShpSource,
    palette: &Palette,
    house_remap: bool,
    key: &ShpSpriteKey,
    rules: Option<&RuleSet>,
    palettes: &mut ShpPalettes,
) -> Option<RenderedShpSprite> {
    render_shp_frame(
        source,
        &shp_render_palette(palette, house_remap, key, rules),
        key,
        shp_key_is_infantry(key, rules),
        palettes,
    )
}

/// Whether `key` draws an Infantry type's own image.
///
/// Source-bound Infantry preparation never registers an invalid frame.
/// [`render_shp_frame`] rejects an out-of-asset key of one too; this
/// adapter's absent sprite does not establish native invalid-index shape
/// access, which the frame selection receipts stop before. Other generic
/// classes retain their existing standing fallback.
fn shp_key_is_infantry(key: &ShpSpriteKey, rules: Option<&RuleSet>) -> bool {
    key.palette_context.image_kind() == ShpImageKind::Object
        && rules
            .and_then(|rules| rules.object(&key.type_id))
            .is_some_and(|object| {
                object.category == crate::rules::object_type::ObjectCategory::Infantry
            })
}

/// The palette `key`'s frame resolves through: `palette`, with the house's
/// `[Colors]` team-color band over indices 16–31 when `house_remap` holds.
/// NO_REMAP owners (civilian/neutral/special) keep the raw theater palette —
/// matching the GPU voxel path (which samples row 0, the unremapped [16,32)
/// range) and the documented NO_REMAP intent.
fn shp_render_palette<'p>(
    palette: &'p Palette,
    house_remap: bool,
    key: &ShpSpriteKey,
    rules: Option<&RuleSet>,
) -> std::borrow::Cow<'p, Palette> {
    if !house_remap || key.house_color == crate::rules::house_colors::NO_REMAP {
        return std::borrow::Cow::Borrowed(palette);
    }
    let default_ramps = HouseColorRamps::default();
    let ramps = rules
        .map(|r| &r.house_color_ramps)
        .unwrap_or(&default_ramps);
    std::borrow::Cow::Owned(palette.with_house_colors(ramps.ramp(key.house_color)))
}

/// Render one key's frame of a decoded SHP through `render_pal`
/// ([`shp_render_palette`]): its palette indices, and the row of `palettes`
/// holding the palette they resolve through.
///
/// Selects the appropriate frame based on facing (8-direction for infantry).
/// An empty (0x0) frame has nothing to draw and returns None, as does a frame
/// whose palette the full rows cannot take, and an out-of-asset frame of a
/// projectile or of an Infantry type ([`shp_key_is_infantry`]).
fn render_shp_frame(
    source: &ShpSource,
    render_pal: &Palette,
    key: &ShpSpriteKey,
    infantry: bool,
    palettes: &mut ShpPalettes,
) -> Option<RenderedShpSprite> {
    let shp = &source.shp;
    let found_name = source.found_name.as_str();

    let frame_idx = shp_frame_index(key, shp.frames.len(), infantry)?;
    let frame = shp.frames.get(frame_idx)?;
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
    // Phobos73D223 calls DrawSHP on AnimType::GetImage, with the unit's
    // gather location. It never enters AnimClass::DrawIt or its art offsets.
    let (xdo, ydo) = if key.palette_context == ShpPaletteContext::Harvest {
        (0, 0)
    } else {
        source.draw_offsets
    };
    let offset_x: f32 = -((full_w / 2) as f32) + xdo as f32;
    let offset_y: f32 = -((full_h / 2) as f32) + ydo as f32;

    Some(RenderedShpSprite {
        key: key.clone(),
        palette: palette_row,
        shp: source.data.clone(),
        source_digest: source.source_digest,
        frame: frame_idx,
        width: fw,
        height: fh,
        offset_x: offset_x + fx as f32,
        offset_y: offset_y + fy as f32,
        canvas_rect: [offset_x, offset_y, full_w as f32, full_h as f32],
        extended: frame.format & 2 != 0,
    })
}

/// The frame `key` draws of an image of `frame_count` frames: its own frame,
/// or past the end the standing frame of its facing (eight frames or more)
/// or frame 0. A projectile or Infantry key past the end draws nothing.
fn shp_frame_index(key: &ShpSpriteKey, frame_count: usize, infantry: bool) -> Option<usize> {
    if (key.palette_context.is_projectile() || infantry) && key.frame as usize >= frame_count {
        return None;
    }
    Some(if (key.frame as usize) < frame_count {
        key.frame as usize
    } else if frame_count >= 8 {
        (INFANTRY_FACING_BUCKETS as usize
            - (canonical_infantry_facing(key.facing) as usize / INFANTRY_FACING_STEP as usize))
            % INFANTRY_FACING_BUCKETS as usize
    } else {
        0
    })
}

/// Canonicalize RA2 facing byte to one of the 8 infantry-facing buckets.
pub fn canonical_infantry_facing(facing: u8) -> u8 {
    (facing / INFANTRY_FACING_STEP) * INFANTRY_FACING_STEP
}

/// Phobos73D223 draws a selected AnimType's loaded Shape at a raw frame.
/// Reuse the animation image request owner, including theater/type-load state,
/// and register every physical frame: the hook does not halve Shadow images or
/// impose OREGATH's 120-frame limit. The normal frame renderer owns decoding,
/// palette rows and caller-context placement; no second image cache is needed.
/// Source: Phobos b90ab4bc, src/Ext/TechnoType/Hooks.cpp
/// (UnitClass_DrawIt_OreGath).
#[cfg(feature = "phobos")]
fn register_custom_harvest_frames(
    needed: &mut HashSet<ShpSpriteKey>,
    files: &mut ShpFiles,
    assets: &AssetManager,
    rules: &RuleSet,
    art: Option<&ArtRegistry>,
    theater_ext: &str,
    theater_name: &str,
    mut pictures: Option<&mut PictureRoutes<'_>>,
) {
    for anim in rules.harvest_overlay_anim_types() {
        // VERA-only, experimental (docs/picture-model-art.md): a declared
        // picture's frames; its SHP is never opened.
        let count = match pictures
            .as_deref_mut()
            .and_then(|routes| routes.anim_frames(anim))
        {
            Some(Some(count)) => count,
            Some(None) => continue,
            None => {
                let candidates = effect_anim_shp_candidates(anim, art, theater_ext, theater_name);
                let Some((file, data)) = find_shp(assets, &candidates) else {
                    log::warn!("gather animation [{anim}] has no loaded SHP asset");
                    continue;
                };
                let Ok(count) = ShpFile::frame_count_from_bytes(&data) else {
                    log::warn!("gather animation [{anim}] has an invalid SHP asset");
                    continue;
                };
                files.insert(
                    (ShpImageKind::Animation, anim.to_ascii_uppercase()),
                    file.to_string(),
                );
                count
            }
        };
        needed.extend((0..count).map(|frame| ShpSpriteKey {
            palette_context: ShpPaletteContext::Harvest,
            type_id: anim.into(),
            facing: 0,
            frame,
            house_color: HouseColorIndex(0),
        }));
    }
}

/// The harvest overlay's effect palette (`anim.pal`) and `oregath.shp`,
/// which [`render_harvest_overlay_frames`] draws; None, with a warning, when
/// either is missing.
fn harvest_overlay_source(asset_manager: &AssetManager) -> Option<(Palette, SharedBytes)> {
    let Some(pal_data) = asset_manager.get_ref("anim.pal") else {
        log::warn!("anim.pal not found — skipping harvest overlay");
        return None;
    };
    let palette = match Palette::from_bytes(pal_data) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to parse anim.pal: {} — skipping harvest overlay", e);
            return None;
        }
    };
    let Some(shp_data) = asset_manager.get_shared("oregath.shp") else {
        log::warn!("oregath.shp not found — skipping harvest overlay");
        return None;
    };
    Some((palette, shp_data))
}

/// Render oregath.shp harvest overlay frames using the effect palette (anim.pal).
///
/// OREGATH uses anim.pal (the same palette used for explosion/effect SHPs),
/// with no house color remap.
///
/// Returns all 120 SHP frames (15 animation frames x 8 facings) as rendered sprites.
/// Each frame is keyed by OREGATH, facing zero and its SHP frame index.
/// At render time, the correct frame is: `facing_index * 15 + anim_frame`.
fn render_harvest_overlay_frames(
    palette: &Palette,
    shp_data: SharedBytes,
    palettes: &mut ShpPalettes,
) -> Vec<RenderedShpSprite> {
    let source = match ShpSource::decode(shp_data, "OREGATH.SHP".to_string(), (0, 0)) {
        Ok(source) => source,
        Err(e) => {
            log::warn!(
                "Failed to parse oregath.shp: {} — skipping harvest overlay",
                e
            );
            return Vec::new();
        }
    };

    let shp = &source.shp;

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

    let Some(palette_row) = palettes.row(palette) else {
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
                type_id: "OREGATH".into(),
                facing: 0,
                frame: frame_idx as u16,
                house_color: hc,
            },
            // The effect palette, with no house color remap.
            palette: palette_row,
            shp: source.data.clone(),
            source_digest: source.source_digest,
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
    sprites: Vec<RenderedShpSprite>,
    palettes: ShpPalettes,
) -> SpriteAtlas {
    let max_texture_dim = device.limits().max_texture_dimension_2d;
    let mut plan = plan_shp_pages(sprites, max_texture_dim);
    let (atlas, indices) = create_packed_shp_atlas(device, queue, batch, &mut plan, palettes);
    for (page, texture) in plan.pages.iter().zip(&indices) {
        upload_shelves(
            device,
            queue,
            texture,
            &plan.sprites,
            &page.placed,
            PACK_BAND_BYTES,
        );
    }
    log_shp_atlas_pack(&atlas, max_texture_dim);
    atlas
}

fn create_packed_shp_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    plan: &mut ShpAtlasPackPlan,
    mut palettes: ShpPalettes,
) -> (SpriteAtlas, Vec<wgpu::Texture>) {
    let palette_rows = palettes.upload(device, queue);
    let mut atlas = SpriteAtlas::new(
        Vec::with_capacity(plan.pages.len()),
        HashMap::with_capacity(plan.sprites.len() + plan.aliases.len()),
        palettes,
    );
    let mut indices = Vec::with_capacity(plan.pages.len());
    atlas.physical_frames.reserve(plan.sprites.len());
    for (page_index, page) in plan.pages.iter().enumerate() {
        let (texture, source_indices) =
            batch.create_indexed_page(device, page.size[0], page.size[1], &palette_rows);
        indices.push(source_indices);
        atlas.pages.push(SpriteAtlasPage { texture });
        for &(idx, origin) in &page.placed {
            atlas.admit_placed_sprite(
                &plan.sprites[idx],
                ShpFramePlacement {
                    origin,
                    page: page_index,
                    page_size: page.size,
                },
            );
        }
    }
    atlas.admit_aliases(std::mem::take(&mut plan.aliases));
    (atlas, indices)
}

fn log_shp_atlas_pack(atlas: &SpriteAtlas, max_texture_dim: u32) {
    // One palette index a texel.
    let texels: u64 = atlas
        .pages
        .iter()
        .map(|page| u64::from(page.texture.width) * u64::from(page.texture.height))
        .sum();
    let sizes: Vec<String> = atlas
        .pages
        .iter()
        .map(|page| format!("{}x{}", page.texture.width, page.texture.height))
        .collect();
    log::info!(
        "Sprite atlas packed into {} page(s) of at most {max_texture_dim} px: {} ({:.0} MiB), {} palette row(s)",
        atlas.pages.len(),
        sizes.join(", "),
        texels as f64 / (1024.0 * 1024.0),
        atlas.palettes.colors.len(),
    );
}

struct ShpPagePlan {
    size: [u32; 2],
    placed: Vec<(usize, [u32; 2])>,
}

/// One CPU-only initial pack, shared by synchronous and staged uploads.
struct ShpAtlasPackPlan {
    sprites: std::sync::Arc<[RenderedShpSprite]>,
    aliases: Vec<RenderedShpSprite>,
    pages: Vec<ShpPagePlan>,
}

fn plan_shp_pages(sprites: Vec<RenderedShpSprite>, max_texture_dim: u32) -> ShpAtlasPackPlan {
    let (sprites, aliases) = separate_physical_frames(sprites, &HashMap::new());
    let sizes: Vec<[u32; 2]> = sprites
        .iter()
        .map(|sprite| [sprite.width, sprite.height])
        .collect();
    let plan = atlas_growth::plan_initial_shelves(
        &sizes,
        atlas_growth::InitialShelfPolicy {
            padding: SPRITE_PADDING,
            max_dim: max_texture_dim,
            split_pages: true,
        },
    );
    let mut pages: Vec<ShpPagePlan> = plan
        .page_heights
        .iter()
        .map(|&height| ShpPagePlan {
            size: [plan.width, height],
            placed: Vec::new(),
        })
        .collect();
    for placed in &plan.placements {
        pages[placed.page]
            .placed
            .push((placed.index, placed.origin));
    }
    ShpAtlasPackPlan {
        sprites: sprites.into(),
        aliases,
        pages,
    }
}

/// CPU-only shelf bands. Borrowed inputs serve the synchronous growth caller;
/// owned/shared inputs can travel with a loading job without retaining GPU
/// handles or the asset manager. Both paths use the same shelf boundaries and
/// single-band decoder, rather than materializing an entire page of pixels.
struct ShpUploadBands<S, P> {
    sprites: S,
    placed: P,
    next: usize,
    row_bytes: u64,
    band_bytes: u64,
}

impl<S: AsRef<[RenderedShpSprite]>, P: AsRef<[(usize, [u32; 2])]>> ShpUploadBands<S, P> {
    fn new(sprites: S, placed: P, width: u32, band_bytes: u64) -> Self {
        debug_assert!(
            placed
                .as_ref()
                .windows(2)
                .all(|pair| pair[0].1[1] <= pair[1].1[1]),
            "placements must run shelf by shelf from the top"
        );
        Self {
            sprites,
            placed,
            next: 0,
            row_bytes: u64::from(width),
            band_bytes,
        }
    }
}

impl<S: AsRef<[RenderedShpSprite]>, P: AsRef<[(usize, [u32; 2])]>> Iterator
    for ShpUploadBands<S, P>
{
    type Item = ([u32; 2], [u32; 2], Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        let rest = &self.placed.as_ref()[self.next..];
        let &(_, [_, top]) = rest.first()?;
        let sprites = self.sprites.as_ref();
        // Whole shelves until this band's rows reach the byte budget. A
        // single taller shelf remains indivisible, as in the original path.
        let (mut len, mut bottom) = (0, top);
        loop {
            let shelf_y = rest[len].1[1];
            while len < rest.len() && rest[len].1[1] == shelf_y {
                let (idx, [_, y]) = rest[len];
                bottom = bottom.max(y + sprites[idx].height);
                len += 1;
            }
            if len == rest.len() || u64::from(bottom - top) * self.row_bytes >= self.band_bytes {
                break;
            }
        }
        self.next += len;
        gather_sprite_band(
            rest[..len]
                .iter()
                .map(|&(idx, origin)| (origin, &sprites[idx])),
        )
    }
}

/// Upload the sprites `placed` on a blank index page (sprite index and origin,
/// shelf by shelf from the top) in bands of whole shelves. A band takes shelves
/// until its rows reach `band_bytes`, so it holds at least one shelf. Bands
/// share no row, as [`atlas_growth::gather_band`] requires. A band decodes its
/// sprites' frames as it is gathered, and its staging copy is retired before
/// the next band is gathered, so the upload holds one band of pixels at a time.
fn upload_shelves(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    indices: &wgpu::Texture,
    sprites: &[RenderedShpSprite],
    placed: &[(usize, [u32; 2])],
    band_bytes: u64,
) {
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    let _ = device;
    for (origin, size, bytes) in ShpUploadBands::new(sprites, placed, indices.width(), band_bytes) {
        atlas_growth::write_texels(queue, indices, origin, size, 1, &bytes);
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        queue.submit(std::iter::empty());
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let submission_index = queue.submit(std::iter::empty());
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        if let Err(error) = device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index),
            timeout: Some(PACK_BAND_TIMEOUT),
        }) {
            log::warn!("Sprite atlas upload could not wait for the GPU: {error}");
        }
    }
}

/// Upload sprites placed on rows of an index page that hold no other sprite,
/// in one [`atlas_growth::gather_band`], decoding each frame as it is gathered.
fn write_sprite_band<'s>(
    queue: &wgpu::Queue,
    indices: &wgpu::Texture,
    sprites: impl Iterator<Item = ([u32; 2], &'s RenderedShpSprite)>,
) {
    if let Some((origin, size, bytes)) = gather_sprite_band(sprites) {
        atlas_growth::write_texels(queue, indices, origin, size, 1, &bytes);
    }
}

fn gather_sprite_band<'s>(
    sprites: impl Iterator<Item = ([u32; 2], &'s RenderedShpSprite)>,
) -> Option<([u32; 2], [u32; 2], Vec<u8>)> {
    let sprites: Vec<_> = sprites.collect();
    let rects: Vec<_> = sprites
        .iter()
        .map(|&(origin, sprite)| (origin, [sprite.width, sprite.height]))
        .collect();
    atlas_growth::gather_band(&rects, 1, |i| sprites[i].1.indices())
}

#[cfg(test)] // shared by test groups
#[path = "sprite_atlas_tests.rs"]
mod tests;

#[cfg(all(test, tests_render_s_z))]
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
